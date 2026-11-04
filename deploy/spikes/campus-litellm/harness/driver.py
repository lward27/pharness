#!/usr/bin/env python3
"""LUC-115 qualification driver. Runs inside the spike namespace (see manifests/50-driver.yaml).

Subcommands print JSON to stdout and never print the master key or any virtual key. Virtual keys
are kept only in the driver pod's emptyDir state files under WORK_DIR.
"""
import argparse
import collections
import hashlib
import csv
import http.client
import io
import json
import os
import socket
import sys
import threading
import time
import urllib.parse

LITELLM_URL = os.environ.get("LITELLM_URL", "http://litellm:4000")
PODS_HOST = os.environ.get("LITELLM_PODS_HOST", "litellm-pods")
UPSTREAM_URL = os.environ.get("UPSTREAM_URL", "http://fake-upstream:8080")
REDIS_HOST = os.environ.get("REDIS_HOST", "redis")
WORK = os.environ.get("WORK_DIR", "/work")
MASTER = os.environ.get("LITELLM_MASTER_KEY", "")
PRICES = {"campus-small": (0.00001, 0.00003), "campus-large": (0.00003, 0.00006)}


def out(payload):
    json.dump(payload, sys.stdout, indent=1, sort_keys=True, default=str)
    sys.stdout.write("\n")


def resolve_target(target):
    if not target or target == "svc":
        return LITELLM_URL
    if target.startswith("pod:"):
        ips = pod_ips()
        return f"http://{ips[int(target[4:]) % len(ips)]}:4000"
    return target


def pod_ips():
    return sorted({info[4][0] for info in socket.getaddrinfo(PODS_HOST, 4000, socket.AF_INET, socket.SOCK_STREAM)})


def call(method, url, body=None, key=None, timeout=60, headers=None):
    parts = urllib.parse.urlsplit(url)
    conn = http.client.HTTPConnection(parts.hostname, parts.port or 80, timeout=timeout)
    path = parts.path + (f"?{parts.query}" if parts.query else "")
    send_headers = {"Content-Type": "application/json"}
    if key:
        send_headers["Authorization"] = f"Bearer {key}"
    send_headers.update(headers or {})
    started = time.time()
    try:
        conn.request(method, path, body=json.dumps(body) if body is not None else None, headers=send_headers)
        resp = conn.getresponse()
        data = resp.read()
        return resp.status, dict(resp.getheaders()), data, time.time() - started
    except Exception as exc:  # network failure is a recorded outcome, not a crash
        return 0, {}, f"{type(exc).__name__}: {exc}".encode(), time.time() - started
    finally:
        conn.close()


def admin(method, path, body=None, target=None):
    status, _, data, _ = call(method, resolve_target(target) + path, body=body, key=MASTER)
    try:
        parsed = json.loads(data or b"{}")
    except ValueError:
        parsed = {"raw": data[:400].decode(errors="replace")}
    return status, parsed


def state_path(scen):
    return os.path.join(WORK, f"{scen}.state.json")


def load_state(scen):
    with open(state_path(scen), encoding="utf-8") as handle:
        state = json.load(handle)
    for key in state["keys"]:
        key["token"] = hashlib.sha256(key["key"].encode()).hexdigest()
    return state


def save_state(scen, state):
    os.makedirs(WORK, exist_ok=True)
    with open(state_path(scen), "w", encoding="utf-8") as handle:
        json.dump(state, handle)


def redis_cmd(*args):
    def encode(parts):
        chunks = [f"*{len(parts)}\r\n".encode()]
        for part in parts:
            raw = str(part).encode()
            chunks.append(f"${len(raw)}\r\n".encode() + raw + b"\r\n")
        return b"".join(chunks)

    sock = socket.create_connection((REDIS_HOST, 6379), timeout=5)
    reader = sock.makefile("rb")

    def parse():
        line = reader.readline().rstrip(b"\r\n")
        kind, rest = line[:1], line[1:]
        if kind == b"+":
            return rest.decode()
        if kind == b"-":
            raise RuntimeError(rest.decode())
        if kind == b":":
            return int(rest)
        if kind == b"$":
            size = int(rest)
            return None if size < 0 else reader.read(size + 2)[:-2].decode()
        if kind == b"*":
            size = int(rest)
            return None if size < 0 else [parse() for _ in range(size)]
        raise RuntimeError(f"bad reply {line!r}")

    try:
        sock.sendall(encode(["AUTH", os.environ.get("REDIS_PASSWORD", "")]) + encode(list(args)))
        parse()
        return parse()
    finally:
        sock.close()


def redis_counters(patterns):
    try:
        found = {}
        for pattern in patterns:
            cursor = "0"
            while True:
                cursor, keys = redis_cmd("SCAN", cursor, "MATCH", pattern, "COUNT", 1000)
                for key in keys:
                    try:
                        found[key] = float(redis_cmd("GET", key))
                    except (TypeError, ValueError, RuntimeError):
                        found[key] = None
                if cursor == "0":
                    break
        return found
    except OSError as exc:
        return {"error": str(exc)}


def cmd_org_probe(args):
    status, body = admin("POST", "/organization/new", {"organization_alias": f"{args.scen}-tenant", "max_budget": 1.0})
    out({"organization_new_status": status, "body": body})


def cmd_setup(args):
    models = args.models.split(",")
    state = {"scen": args.scen, "teams": {}, "users": {}, "keys": [], "caps": {}}
    org_status, org_body = admin("POST", "/organization/new", {"organization_alias": f"{args.scen}-tenant", "max_budget": args.org_budget})
    state["org_probe"] = {"status": org_status, "error": str(org_body)[:300]}
    for course, budget in (("course-a", args.team_a), ("course-b", args.team_b)):
        team_id = f"{args.scen}-{course}"
        body = {"team_id": team_id, "team_alias": team_id, "max_budget": budget, "models": models,
                "metadata": {"pharness_tenant": f"{args.scen}-tenant", "pharness_course": course}}
        if args.budget_duration:
            body["budget_duration"] = args.budget_duration
        status, resp = admin("POST", "/team/new", body)
        if status != 200:
            raise SystemExit(f"team/new {team_id} failed {status}: {str(resp)[:400]}")
        state["teams"][team_id] = {"max_budget": budget}
    for index in range(args.students):
        user_id = f"{args.scen}-stu-{index + 1:02d}"
        team_id = f"{args.scen}-course-a" if index < args.students // 2 else f"{args.scen}-course-b"
        body = {"user_id": user_id, "max_budget": args.user_budget, "user_role": "internal_user", "auto_create_key": False}
        if args.budget_duration:
            body["budget_duration"] = args.budget_duration
        status, resp = admin("POST", "/user/new", body)
        if status != 200:
            raise SystemExit(f"user/new {user_id} failed {status}: {str(resp)[:400]}")
        status, resp = admin("POST", "/team/member_add", {"team_id": team_id, "member": {"role": "user", "user_id": user_id}})
        if status != 200:
            raise SystemExit(f"team/member_add {user_id} failed {status}: {str(resp)[:400]}")
        state["users"][user_id] = {"team_id": team_id, "max_budget": args.user_budget}
    users = sorted(state["users"])
    for index in range(args.keys):
        user_id = users[index % len(users)]
        team_id = state["users"][user_id]["team_id"]
        alias = f"{args.scen}-sess-{index + 1:02d}"
        body = {"user_id": user_id, "team_id": team_id, "max_budget": args.key_budget, "models": models,
                "key_alias": alias, "metadata": {"pharness_session": alias}}
        if args.budget_duration:
            body["budget_duration"] = args.budget_duration
        status, resp = admin("POST", "/key/generate", body)
        if status != 200:
            raise SystemExit(f"key/generate {alias} failed {status}: {str(resp)[:400]}")
        key = resp["key"]
        token = hashlib.sha256(key.encode()).hexdigest()
        state["keys"].append({"idx": index, "key": key, "token": token, "alias": alias, "user_id": user_id,
                              "team_id": team_id, "max_budget": args.key_budget})
    save_state(args.scen, state)
    per_user = collections.Counter(k["user_id"] for k in state["keys"])
    out({"scen": args.scen, "teams": state["teams"], "students": len(state["users"]), "keys": len(state["keys"]),
         "users_with_multiple_keys": sum(1 for v in per_user.values() if v > 1), "org_probe": state["org_probe"],
         "models": models})


def request_body(args, rid, extra_text=""):
    content = f"[[rid:{rid}]] {args.marker} {extra_text} please answer the campus exercise question".strip()
    body = {"model": args.model, "messages": [{"role": "user", "content": content}], "max_tokens": args.max_tokens}
    if args.stream:
        body["stream"] = True
        body["stream_options"] = {"include_usage": True}
    if args.extra:
        body.update(json.loads(args.extra))
    return body


def cmd_burst(args):
    state = load_state(args.scen)
    keys = state["keys"] if not args.key_idx else [state["keys"][int(i)] for i in args.key_idx.split(",")]
    jobs = [(k, n) for n in range(args.per_key) for k in keys]
    barrier = threading.Barrier(len(jobs))
    results = [None] * len(jobs)
    targets = [resolve_target(t) for t in args.targets.split(",")]

    def run(slot, key, n):
        rid = f"{args.scen}:{args.label}:{key['idx']}:{n}"
        url = targets[slot % len(targets)] + "/v1/chat/completions"
        try:
            barrier.wait(timeout=60)
        except threading.BrokenBarrierError:
            pass
        sent = time.time()
        status, headers, data, latency = call("POST", url, request_body(args, rid), key=key["key"], timeout=args.timeout)
        text = data.decode(errors="replace")
        record = {"rid": rid, "idx": key["idx"], "status": status, "latency": round(latency, 3), "sent": sent,
                  "target": url.split("/v1")[0], "cost_header": headers.get("x-litellm-response-cost"),
                  "stream": args.stream, "label": args.label}
        if status == 200 and not args.stream:
            try:
                parsed = json.loads(text)
                record["resp_id"] = parsed.get("id")
                record["usage"] = parsed.get("usage")
            except ValueError:
                record["body_tail"] = text[-200:]
        elif status == 200:
            record["stream_done"] = "[DONE]" in text
            record["stream_error"] = "error" in text[-600:]
            record["body_tail"] = text[-300:]
        else:
            record["error"] = text[:300]
        results[slot] = record

    threads = [threading.Thread(target=run, args=(slot, key, n), daemon=True) for slot, (key, n) in enumerate(jobs)]
    started = time.time()
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    os.makedirs(WORK, exist_ok=True)
    with open(os.path.join(WORK, f"{args.scen}.results.jsonl"), "a", encoding="utf-8") as handle:
        for record in results:
            handle.write(json.dumps(record) + "\n")
    statuses = collections.Counter(r["status"] for r in results)
    reasons = collections.Counter(classify(r) for r in results if r["status"] != 200)
    out({"scen": args.scen, "label": args.label, "sent": len(results), "wall_s": round(time.time() - started, 2),
         "status_counts": dict(statuses), "reject_reasons": dict(reasons),
         "stream_incomplete": sum(1 for r in results if r["status"] == 200 and r.get("stream") and not r.get("stream_done"))})


def classify(record):
    err = (record.get("error") or "").lower()
    if record["status"] == 0:
        return "network:" + err[:60]
    for needle, name in (("budget enforcement unavailable", "fail_closed_503"), ("budget has been exceeded", "budget_exceeded"),
                         ("exceeded budget", "budget_exceeded"), ("exceededbudget", "budget_exceeded"), ("max budget", "budget_exceeded"),
                         ("not allowed to access model", "model_not_allowed"), ("team not allowed", "model_not_allowed"),
                         ("blocked", "key_blocked"), ("invalid proxy server token", "invalid_key"),
                         ("authentication error", "auth_error"), ("ratelimit", "upstream_429"), ("rate limit", "upstream_429"),
                         ("timeout", "timeout"), ("internalservererror", "upstream_5xx"), ("fake upstream failure", "upstream_5xx")):
        if needle in err:
            return name
    return f"http{record['status']}:" + err[:80]


def read_calls():
    status, _, data, _ = call("GET", UPSTREAM_URL + "/calls", timeout=60)
    if status != 200:
        raise SystemExit(f"upstream /calls failed: {status}")
    recv, done = {}, {}
    for line in data.decode().splitlines():
        if not line.strip():
            continue
        rec = json.loads(line)
        if rec.get("ev") == "recv":
            recv[rec["call_id"]] = rec
        elif rec.get("ev") == "done":
            done[rec["call_id"]] = rec
    return recv, done


def read_spendlogs(path):
    if not path:
        return None
    handle = sys.stdin if path == "-" else open(path, encoding="utf-8")
    rows = list(csv.DictReader(io.StringIO(handle.read())))
    return rows


def cmd_report(args):
    state = load_state(args.scen)
    results = []
    results_path = os.path.join(WORK, f"{args.scen}.results.jsonl")
    if os.path.exists(results_path):
        with open(results_path, encoding="utf-8") as handle:
            results = [json.loads(line) for line in handle if line.strip()]
    if args.label:
        results = [r for r in results if r["label"] in args.label.split(",")]
        prefixes = tuple(f"{args.scen}:{label}:" for label in args.label.split(","))
    else:
        prefixes = (f"{args.scen}:",)
    recv, done = read_calls()
    calls = {cid: rec for cid, rec in recv.items() if (rec.get("rid") or "").startswith(prefixes)}
    by_rid = collections.defaultdict(list)
    for cid, rec in calls.items():
        by_rid[rec["rid"]].append(cid)
    keys = {k["idx"]: k for k in state["keys"]}
    truth_key = collections.Counter()
    unresolved = 0
    outcomes = collections.Counter()
    for cid, rec in calls.items():
        idx = int(rec["rid"].split(":")[2])
        fin = done.get(cid)
        if fin is None:
            unresolved += 1
            outcomes["in_flight_or_lost"] += 1
            truth_key[idx] += rec["full_cost"]
        else:
            outcomes[fin["outcome"]] += 1
            truth_key[idx] += fin["billed_usd"]
    truth_user, truth_team = collections.Counter(), collections.Counter()
    for idx, cost in truth_key.items():
        truth_user[keys[idx]["user_id"]] += cost
        truth_team[keys[idx]["team_id"]] += cost

    gw_key, gw_user, gw_team = {}, {}, {}
    if not args.no_gateway:
        for k in state["keys"]:
            _, info = admin("GET", "/key/info?key=" + urllib.parse.quote(k["key"]))
            gw_key[k["idx"]] = (info.get("info") or {}).get("spend")
        for user_id in state["users"]:
            _, info = admin("GET", "/user/info?user_id=" + urllib.parse.quote(user_id))
            gw_user[user_id] = (info.get("user_info") or {}).get("spend")
        for team_id in state["teams"]:
            _, info = admin("GET", "/team/info?team_id=" + urllib.parse.quote(team_id))
            gw_team[team_id] = (info.get("team_info") or {}).get("spend")

    counters = redis_counters(["*spend:*"])
    counter_err = counters.get("error") if isinstance(counters.get("error"), str) else None

    def counter(name):
        if counter_err:
            return None
        return next((v for k, v in counters.items() if k.endswith(name)), None)

    def overs(truth, caps):
        rows = {name: round(truth.get(name, 0.0) - cap, 8) for name, cap in caps.items() if truth.get(name, 0.0) - cap > 1e-9}
        return rows

    key_caps = {k["idx"]: k["max_budget"] for k in state["keys"]}
    user_caps = {u: v["max_budget"] for u, v in state["users"].items()}
    team_caps = {t: v["max_budget"] for t, v in state["teams"].items()}
    key_over, user_over, team_over = overs(truth_key, key_caps), overs(truth_user, user_caps), overs(truth_team, team_caps)

    holds = {}
    for k in state["keys"]:
        value = counter(f"spend:key:{k['token']}")
        if value is not None and gw_key.get(k["idx"]) is not None and value - truth_key.get(k["idx"], 0.0) > 1e-7:
            holds[k["alias"]] = {"counter": round(value, 8), "truth": round(truth_key.get(k["idx"], 0.0), 8),
                                 "db": gw_key.get(k["idx"])}
    team_holds = {}
    for team_id in state["teams"]:
        value = counter(f"spend:team:{team_id}")
        team_holds[team_id] = {"counter": value, "truth": round(truth_team.get(team_id, 0.0), 8), "db": gw_team.get(team_id),
                               "cap": team_caps[team_id]}
    statuses = collections.Counter(r["status"] for r in results)
    reasons = collections.Counter(classify(r) for r in results if r["status"] != 200)
    admitted_rids = {r["rid"] for r in results if r["status"] == 200}
    rejected_rids = {r["rid"] for r in results if r["status"] != 200}
    dup_rids = {rid: len(cids) for rid, cids in by_rid.items() if len(cids) > 1}
    report = {
        "scen": args.scen,
        "labels": args.label,
        "requests_sent": len(results),
        "status_counts": dict(statuses),
        "reject_reasons": dict(reasons),
        "admitted_2xx": len(admitted_rids),
        "upstream_calls": len(calls),
        "upstream_unique_rids": len(by_rid),
        "upstream_outcomes": dict(outcomes),
        "upstream_calls_for_rejected_requests": sum(len(by_rid[r]) for r in rejected_rids if r in by_rid),
        "duplicate_upstream_rids": dup_rids,
        "truth_total_usd": round(sum(truth_key.values()), 8),
        "gateway_key_spend_total_usd": round(sum(v or 0.0 for v in gw_key.values()), 8) if gw_key else None,
        "gateway_user_spend_total_usd": round(sum(v or 0.0 for v in gw_user.values()), 8) if gw_user else None,
        "gateway_team_spend_total_usd": round(sum(v or 0.0 for v in gw_team.values()), 8) if gw_team else None,
        "caps": {"key": state["keys"][0]["max_budget"], "user": next(iter(user_caps.values())), "team": team_caps},
        "overspend": {
            "keys_over_cap": len(key_over), "key_max_over_usd": max(key_over.values(), default=0.0),
            "key_total_over_usd": round(sum(key_over.values()), 8),
            "users_over_cap": len(user_over), "user_max_over_usd": max(user_over.values(), default=0.0),
            "teams_over_cap": team_over,
        },
        "team_counters": team_holds,
        "key_counter_above_truth": holds,
        "redis_error": counter_err,
    }
    spendlogs = read_spendlogs(args.spendlogs)
    if spendlogs is not None:
        token_idx = {k["token"]: k["idx"] for k in state["keys"]}
        upstream_ids = {f"chatcmpl-{cid}" for cid in calls}
        window_start = min((r["sent"] for r in results), default=0) - 1
        rows = [row for row in spendlogs if row["api_key"] in token_idx]
        calls_per_key = collections.Counter(int(rec["rid"].split(":")[2]) for rec in calls.values())
        ok_per_key = collections.Counter(int(calls[cid]["rid"].split(":")[2]) for cid in calls if (done.get(cid) or {}).get("outcome") == "ok")
        success_rows = collections.Counter(token_idx[row["api_key"]] for row in rows if row["status"] == "success")
        report["spendlogs"] = {
            "note": "rows for this scenario's keys (all labels); success rows carry the upstream completion id, failure rows carry the gateway call id",
            "rows_by_status": dict(collections.Counter(row["status"] for row in rows)),
            "settled_usd_all_rows": round(sum(float(row["spend"] or 0) for row in rows), 8),
            "settled_usd_by_status": {s: round(sum(float(r["spend"] or 0) for r in rows if r["status"] == s), 8)
                                      for s in {row["status"] for row in rows}},
            "success_rows_matching_upstream_ids": sum(1 for row in rows if row["request_id"] in upstream_ids),
            "keys_with_more_success_rows_than_ok_upstream_calls": {
                str(i): [success_rows[i], ok_per_key.get(i, 0)] for i in success_rows if success_rows[i] > ok_per_key.get(i, 0)},
            "keys_with_more_rows_than_upstream_calls": {
                str(i): n for i, n in collections.Counter(token_idx[row["api_key"]] for row in rows).items() if n > calls_per_key.get(i, 0)},
            "rows_with_messages_stored": sum(1 for row in rows if int(row.get("messages_len") or 0) > 4),
            "rows_with_response_stored": sum(1 for row in rows if int(row.get("response_len") or 0) > 4),
            "rows_with_proxy_request_stored": sum(1 for row in rows if int(row.get("psr_len") or 0) > 4),
            "end_user_values": sorted({row["end_user"] for row in rows if row.get("end_user")})[:10],
            "user_values": sorted({row["user"] for row in rows if row.get("user")})[:10],
            "team_values": sorted({row["team_id"] for row in rows if row.get("team_id")})[:10],
        }
        report["unsettled_liability_usd"] = round(report["truth_total_usd"] - report["spendlogs"]["settled_usd_all_rows"], 8) if not args.label else None
    out(report)


def cmd_probe(args):
    state = load_state(args.scen)
    key = state["keys"][args.key_idx]
    rid = f"{args.scen}:{args.label}:{key['idx']}:0"
    body = request_body(args, rid)
    status, headers, data, latency = call("POST", resolve_target(args.target) + "/v1/chat/completions", body, key=key["key"],
                                          timeout=args.timeout, headers=json.loads(args.headers) if args.headers else None)
    record = {"rid": rid, "idx": key["idx"], "status": status, "latency": round(latency, 3), "label": args.label,
              "cost_header": headers.get("x-litellm-response-cost"), "stream": args.stream, "sent": time.time() - latency}
    text = data.decode(errors="replace")
    if status != 200:
        record["error"] = text[:400]
    else:
        record["body_tail"] = text[-200:]
    with open(os.path.join(WORK, f"{args.scen}.results.jsonl"), "a", encoding="utf-8") as handle:
        handle.write(json.dumps(record) + "\n")
    out({**record, "class": classify(record) if status != 200 else "ok"})


def cmd_revoke(args):
    state = load_state(args.scen)
    key = state["keys"][args.key_idx]
    targets = [resolve_target(t) for t in args.probe_targets.split(",")]
    probe_args = argparse.Namespace(model="campus-small", marker="", max_tokens=1, stream=False, extra=None)
    for target in targets:
        for n in range(2):
            call("POST", target + "/v1/chat/completions", request_body(probe_args, f"{args.scen}:warm:{key['idx']}:{n}"), key=key["key"])
    log = []
    stop = threading.Event()
    revoked_at = {}

    def prober(target, worker):
        n = 0
        while not stop.is_set():
            n += 1
            rid = f"{args.scen}:{args.label}:{key['idx']}:{worker}-{n}"
            status, _, data, latency = call("POST", target + "/v1/chat/completions", request_body(probe_args, rid), key=key["key"], timeout=20)
            log.append({"t": time.time(), "target": target, "status": status, "rid": rid, "err": data[:120].decode(errors="replace") if status != 200 else ""})
            time.sleep(args.interval)

    threads = [threading.Thread(target=prober, args=(t, i), daemon=True) for i, t in enumerate(targets)]
    for thread in threads:
        thread.start()
    time.sleep(3)
    revoked_at["t"] = time.time()
    if args.mode == "delete":
        status, resp = admin("POST", "/key/delete", {"keys": [key["key"]]}, target=args.admin_target)
    else:
        status, resp = admin("POST", "/key/block", {"key": key["key"]}, target=args.admin_target)
    revoked_at["admin_status"] = status
    revoked_at["admin_done"] = time.time()
    deadline = time.time() + args.max_wait
    while time.time() < deadline:
        time.sleep(0.5)
        done_targets = 0
        for target in targets:
            after = [e for e in log if e["target"] == target and e["t"] > revoked_at["admin_done"]]
            if len(after) >= args.consecutive and all(e["status"] != 200 for e in after[-args.consecutive:]):
                done_targets += 1
        if done_targets == len(targets):
            break
    stop.set()
    for thread in threads:
        thread.join(timeout=25)
    t0 = revoked_at["t"]
    per_target = {}
    for target in targets:
        rows = [e for e in log if e["target"] == target and e["t"] > t0]
        successes = [e for e in rows if e["status"] == 200]
        first_reject = next((e for e in rows if e["status"] != 200), None)
        per_target[target] = {
            "probes_after_revoke": len(rows),
            "successes_after_revoke": len(successes),
            "last_success_after_revoke_s": round(successes[-1]["t"] - t0, 3) if successes else None,
            "first_reject_after_revoke_s": round(first_reject["t"] - t0, 3) if first_reject else None,
            "first_reject_status": first_reject["status"] if first_reject else None,
            "first_reject_err": first_reject["err"][:100] if first_reject else None,
            "rids_admitted_after_revoke": [e["rid"] for e in successes][:5],
        }
    recv, _ = read_calls()
    late_upstream = sum(1 for rec in recv.values() if (rec.get("rid") or "").startswith(f"{args.scen}:{args.label}:") and rec["ts"] > t0)
    out({"scen": args.scen, "mode": args.mode, "admin_status": revoked_at["admin_status"],
         "admin_latency_s": round(revoked_at["admin_done"] - t0, 3), "admin_target": args.admin_target,
         "pods": pod_ips(), "per_target": per_target, "upstream_calls_after_revoke": late_upstream})


IDENTITY_CASES = (
    ("baseline", {}, {}),
    ("body_user_other_student", {"user": "{victim}"}, {}),
    ("metadata_user_id", {"metadata": {"user_id": "{victim}"}}, {}),
    ("metadata_spoof_auth_fields", {"metadata": {"user_api_key_user_id": "{victim}", "user_api_key_team_id": "{victim_team}",
                                                 "user_api_key_alias": "spoofed", "team_id": "{victim_team}", "tags": ["spoof-tag"]}}, {}),
    ("body_team_id", {"team_id": "{victim_team}"}, {}),
    ("litellm_metadata_user", {"litellm_metadata": {"user": "{victim}"}}, {}),
    ("header_end_user_id", {}, {"x-litellm-end-user-id": "{victim}"}),
    ("header_customer_id", {}, {"x-litellm-customer-id": "{victim}"}),
)


def cmd_identity(args):
    state = load_state(args.scen)
    key = state["keys"][0]
    victim = next(u for u, v in state["users"].items() if v["team_id"] != key["team_id"])
    victim_team = state["users"][victim]["team_id"]
    capped = f"{args.scen}-cust-capped"
    cust_status, _ = admin("POST", "/customer/new", {"user_id": capped, "max_budget": args.customer_budget})
    plan = []
    for name, extra, headers in IDENTITY_CASES:
        render = lambda obj: json.loads(json.dumps(obj).replace("{victim}", victim).replace("{victim_team}", victim_team))
        plan.append((name, render(extra), render(headers)))
    plan += [(f"capped_customer_{n}", {}, {"x-litellm-end-user-id": capped}) for n in range(args.capped_requests)]
    plan += [(f"rotated_customer_{n}", {}, {"x-litellm-end-user-id": f"{capped}-rotated-{n}"}) for n in range(3)]
    plan += [(f"rotated_body_user_{n}", {"user": f"{capped}-body-{n}"}, {}) for n in range(2)]
    rows = []
    for n, (name, extra, headers) in enumerate(plan):
        rid = f"{args.scen}:{args.label}:{key['idx']}:{n}-{name}"
        body = {"model": "campus-small", "messages": [{"role": "user", "content": f"[[rid:{rid}]] identity probe"}], "max_tokens": 100}
        body.update(extra)
        status, resp_headers, data, latency = call("POST", LITELLM_URL + "/v1/chat/completions", body, key=key["key"], headers=headers)
        record = {"rid": rid, "idx": key["idx"], "status": status, "latency": round(latency, 3), "label": args.label, "case": name,
                  "sent": time.time() - latency, "stream": False}
        if status != 200:
            record["error"] = data[:300].decode(errors="replace")
        rows.append(record)
        time.sleep(args.gap)
    with open(os.path.join(WORK, f"{args.scen}.results.jsonl"), "a", encoding="utf-8") as handle:
        for record in rows:
            handle.write(json.dumps(record) + "\n")
    out({"scen": args.scen, "key_owner": key["user_id"], "key_team": key["team_id"], "victim": victim, "victim_team": victim_team,
         "customer_new_status": cust_status, "customer_budget": args.customer_budget,
         "cases": [{"case": r["case"], "status": r["status"], "class": classify(r) if r["status"] != 200 else "ok",
                    "error": (r.get("error") or "")[:160]} for r in rows]})


def cmd_identity_report(args):
    state = load_state(args.scen)
    key = state["keys"][0]
    results = []
    with open(os.path.join(WORK, f"{args.scen}.results.jsonl"), encoding="utf-8") as handle:
        results = [json.loads(line) for line in handle if line.strip()]
    results = [r for r in results if r.get("label") == args.label]
    recv, _ = read_calls()
    by_rid = collections.defaultdict(list)
    for cid, rec in recv.items():
        by_rid[rec.get("rid")].append(cid)
    logs = {row["request_id"]: row for row in read_spendlogs(args.spendlogs) or []}
    cases = []
    for record in results:
        cids = by_rid.get(record["rid"], [])
        row = next((logs.get(f"chatcmpl-{cid}") for cid in cids if f"chatcmpl-{cid}" in logs), None)
        cases.append({"case": record["case"], "status": record["status"], "upstream_calls": len(cids),
                      "spendlog_user": row["user"] if row else None, "spendlog_team": row["team_id"] if row else None,
                      "spendlog_end_user": row["end_user"] if row else None,
                      "spendlog_api_key_is_callers": (row["api_key"] == key["token"]) if row else None})
    entities = {}
    victim = next(u for u, v in state["users"].items() if v["team_id"] != key["team_id"])
    for user_id in (key["user_id"], victim):
        _, info = admin("GET", "/user/info?user_id=" + urllib.parse.quote(user_id))
        entities[f"user:{user_id}"] = (info.get("user_info") or {}).get("spend")
    for team_id in state["teams"]:
        _, info = admin("GET", "/team/info?team_id=" + urllib.parse.quote(team_id))
        entities[f"team:{team_id}"] = (info.get("team_info") or {}).get("spend")
    for customer in (victim, f"{args.scen}-cust-capped", f"{args.scen}-cust-capped-rotated-0"):
        status, info = admin("GET", "/customer/info?end_user_id=" + urllib.parse.quote(customer))
        entities[f"customer:{customer}"] = {"status": status, "spend": info.get("spend") if isinstance(info, dict) else None,
                                            "max_budget": ((info.get("litellm_budget_table") or {}) if isinstance(info, dict) else {}).get("max_budget")}
    out({"scen": args.scen, "cases": cases, "entity_spend": entities})

def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("org-probe")
    p.add_argument("scen")
    p = sub.add_parser("setup")
    p.add_argument("scen")
    p.add_argument("--key-budget", type=float, default=0.02)
    p.add_argument("--user-budget", type=float, default=0.03)
    p.add_argument("--team-a", type=float, default=0.25)
    p.add_argument("--team-b", type=float, default=0.25)
    p.add_argument("--org-budget", type=float, default=0.4)
    p.add_argument("--students", type=int, default=20)
    p.add_argument("--keys", type=int, default=30)
    p.add_argument("--models", default="campus-small")
    p.add_argument("--budget-duration", default=None)
    for name in ("burst", "probe"):
        p = sub.add_parser(name)
        p.add_argument("scen")
        p.add_argument("label")
        p.add_argument("--model", default="campus-small")
        p.add_argument("--marker", default="")
        p.add_argument("--max-tokens", type=int, default=100)
        p.add_argument("--stream", action="store_true")
        p.add_argument("--extra", default=None)
        p.add_argument("--timeout", type=float, default=120)
        if name == "burst":
            p.add_argument("--per-key", type=int, default=10)
            p.add_argument("--key-idx", default=None)
            p.add_argument("--targets", default="svc")
        else:
            p.add_argument("--key-idx", type=int, default=0)
            p.add_argument("--target", default="svc")
            p.add_argument("--headers", default=None)
    p = sub.add_parser("report")
    p.add_argument("scen")
    p.add_argument("--label", default=None)
    p.add_argument("--spendlogs", default=None)
    p.add_argument("--no-gateway", action="store_true")
    p = sub.add_parser("revoke")
    p.add_argument("scen")
    p.add_argument("label")
    p.add_argument("--mode", choices=["delete", "block"], default="delete")
    p.add_argument("--key-idx", type=int, default=0)
    p.add_argument("--admin-target", default="pod:0")
    p.add_argument("--probe-targets", default="pod:0")
    p.add_argument("--interval", type=float, default=0.2)
    p.add_argument("--consecutive", type=int, default=10)
    p.add_argument("--max-wait", type=float, default=150)
    p = sub.add_parser("identity")
    p.add_argument("scen")
    p.add_argument("label")
    p.add_argument("--customer-budget", type=float, default=0.005)
    p.add_argument("--capped-requests", type=int, default=4)
    p.add_argument("--gap", type=float, default=0.3)
    p = sub.add_parser("identity-report")
    p.add_argument("scen")
    p.add_argument("label")
    p.add_argument("--spendlogs", default=None)
    p = sub.add_parser("pods")
    args = parser.parse_args()
    handlers = {"org-probe": cmd_org_probe, "setup": cmd_setup, "burst": cmd_burst, "probe": cmd_probe,
                "report": cmd_report, "revoke": cmd_revoke, "identity": cmd_identity,
                "identity-report": cmd_identity_report, "pods": lambda a: out({"pods": pod_ips()})}
    handlers[args.cmd](args)


if __name__ == "__main__":
    main()

"""Budget reset straddle probe (scenario s13b). Run after `driver.py setup s13b --students 4 --keys 4
--key-budget 0.01 --user-budget 0.5 --team-a 0.5 --team-b 0.5 --budget-duration 1m` with
`kubectl -n campus-litellm-spike exec -i deploy/driver -- python3 - < harness/reset_straddle.py`.

Fires delayed requests across the minute budget boundary so some are still in flight when the reset job runs,
then sends a second wave as soon as the reset is visible.
"""
import sys, json, time, threading, urllib.parse, argparse
sys.path.insert(0, "/harness")
import driver as d

SCEN = "s13b"
state = d.load_state(SCEN)
keys = state["keys"]

def info(k):
    i = d.admin("GET", "/key/info?key=" + urllib.parse.quote(k["key"]))[1].get("info") or {}
    return i.get("spend"), i.get("budget_reset_at")

def wave(label, key_idx, delay):
    a = argparse.Namespace(scen=SCEN, label=label, model="campus-small", marker=f"[[delay:{delay}]]" if delay else "",
                           max_tokens=100, stream=False, extra=None, timeout=60, per_key=6, key_idx=str(key_idx), targets="svc")
    d.cmd_burst(a)

log = []
_, reset0 = info(keys[0])
while int(time.time()) % 60 != 0:
    time.sleep(0.2)
boundary = time.time()
threads = []
for n, offset in enumerate((0, 6, 12, 18)):
    def run(n=n, offset=offset):
        time.sleep(offset)
        log.append({"ev": "waveA_fire", "key": n, "t": round(time.time() - boundary, 2)})
        wave("A", n, 7)
        log.append({"ev": "waveA_done", "key": n, "t": round(time.time() - boundary, 2)})
    t = threading.Thread(target=run, daemon=True); t.start(); threads.append(t)
reset_seen = None
while time.time() - boundary < 40:
    _, ra = info(keys[0])
    if ra != reset0:
        reset_seen = round(time.time() - boundary, 2)
        break
    time.sleep(0.3)
log.append({"ev": "reset_seen", "t": reset_seen})
if reset_seen is not None:
    wb = [threading.Thread(target=wave, args=("B", n, 0), daemon=True) for n in range(4)]
    for t in wb: t.start()
    for t in wb: t.join()
    log.append({"ev": "waveB_done", "t": round(time.time() - boundary, 2)})
for t in threads: t.join()
time.sleep(12)
recv, done = d.read_calls()
per_key = {}
for n, k in enumerate(keys):
    rows = []
    for cid, rec in recv.items():
        rid = rec.get("rid") or ""
        if rid.startswith(f"{SCEN}:A:{n}:") or rid.startswith(f"{SCEN}:B:{n}:"):
            fin = done.get(cid) or {}
            rows.append({"wave": rid.split(":")[1], "recv": round(rec["ts"] - boundary, 2),
                         "done": round(fin.get("ts", 0) - boundary, 2) if fin else None, "usd": fin.get("billed_usd", rec.get("full_cost"))})
    spend, ra = info(k)
    after = [r for r in rows if reset_seen is not None and (r["done"] or 1e9) > reset_seen]
    per_key[n] = {"waveA_calls": sum(1 for r in rows if r["wave"] == "A"), "waveB_calls": sum(1 for r in rows if r["wave"] == "B"),
                  "waveA_in_flight_at_reset": sum(1 for r in rows if r["wave"] == "A" and reset_seen is not None and r["recv"] < reset_seen and (r["done"] or 1e9) > reset_seen),
                  "gateway_spend_now": spend, "cap": k["max_budget"],
                  "truth_completed_after_reset_seen_usd": round(sum(r["usd"] for r in after), 6)}
print("RESULT " + json.dumps({"log": log, "per_key": per_key}))

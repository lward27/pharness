"""Campus M1 demonstration probes for a research (Python runner) session.

Runs inside a lab session Pod, either typed into the browser terminal
(``research`` mode) or through an operator's ``kubectl exec`` (``poll``
mode). Usage:

    python3 campus-m1-demo-probe.py research session=<id> api=<ip:port>
    python3 - poll until=<epoch seconds> [fast_after=<epoch seconds>] < campus-m1-demo-probe.py
    python3 campus-m1-demo-probe.py broker session=<id>   # broker checks only

``research`` prints ``PASS <check>: <detail>`` or ``FAIL <check>: <detail>``
per check between a begin and a result marker. ``poll`` asks the broker
about the session's own token until the broker refuses it, printing one
``POLL`` line per change and ``POLL-REFUSED`` at the end.

The broker token is read from its file for every request and is never
printed. Only the standard library is used; the session has no DNS.
"""

import json
import os
import resource
import socket
import sys
import time
import urllib.error
import urllib.request

# Overridable only so scripts/tests can run the probe against a fake broker.
TOKEN_DIR = os.environ.get("CAMPUS_DEMO_BROKER_DIR", "/var/run/campus-broker")
TOKEN_FILE = os.path.join(TOKEN_DIR, "token")
PROXY = ("10.43.130.30", 3128)
ARGS = dict(arg.split("=", 1) for arg in sys.argv[2:] if "=" in arg)
# Built at run time so the echoed command line never contains a marker.
MARK = "DEMO" + "-"
CREDENTIAL_NAMES = ("TOKEN", "SECRET", "PASSWORD", "PASSWD", "API_KEY", "APIKEY", "CREDENTIAL", "PRIVATE_KEY", "AWS_", "OPENAI", "ANTHROPIC", "FIREWORKS")
RESULTS = []


def report(ok, check, detail):
    RESULTS.append(ok)
    print(f"{'PASS' if ok else 'FAIL'} {check}: {str(detail)[:300]}", flush=True)


def broker_url(path):
    return os.environ.get("CAMPUS_BROKER_URL", "").rstrip("/") + path


def read_token():
    with open(TOKEN_FILE, encoding="ascii") as handle:
        return handle.read().strip()


def broker(method, path, body=None, token="file"):
    """(status, decoded JSON or None). The token never leaves this function."""
    headers = {"accept": "application/json"}
    if token == "file":
        headers["authorization"] = "Bearer " + read_token()
    elif token is not None:
        headers["authorization"] = "Bearer " + token
    data = None
    if body is not None:
        data = json.dumps(body).encode()
        headers["content-type"] = "application/json"
    request = urllib.request.Request(broker_url(path), data=data, method=method, headers=headers)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    try:
        with opener.open(request, timeout=10) as response:
            status, raw = response.status, response.read()
    except urllib.error.HTTPError as error:
        status, raw = error.code, error.read()
    try:
        return status, json.loads(raw or b"null")
    except ValueError:
        return status, None


def tcp(host, port, timeout=3.0):
    """'open', 'timeout' or the error name. A NetworkPolicy drop is a timeout."""
    sock = socket.socket(socket.AF_INET6 if ":" in host else socket.AF_INET)
    sock.settimeout(timeout)
    try:
        sock.connect((host, port))
        return "open"
    except socket.timeout:
        return "timeout"
    except OSError as error:
        return type(error).__name__
    finally:
        sock.close()


def blocked(check, host, port):
    outcome = tcp(host, port)
    report(outcome != "open", check, f"{host}:{port} {outcome}")


def code(body):
    return body.get("code") if isinstance(body, dict) else None


def agent_and_broker(session_id):
    import subprocess

    try:
        version = subprocess.run(["codex", "--version"], capture_output=True, text=True, timeout=30)
        line = (version.stdout or version.stderr).strip().splitlines()[:1]
        report(version.returncode == 0 and bool(line) and line[0].startswith("codex-cli"), "agent_cli", line)
    except OSError as error:
        report(False, "agent_cli", type(error).__name__)

    status, body = broker("GET", "/v1/token")
    report(
        status == 200 and body.get("audience") == "inference" and body.get("session_id") == session_id,
        "broker_token_bound_to_session",
        {k: body.get(k) for k in ("audience", "session_id", "lab_generation", "expires_at")} if status == 200 else status,
    )
    request = {"model": "campus-fake", "messages": [{"role": "user", "content": "M1 demo fixture"}]}
    status, body = broker("POST", "/v1/inference/chat/completions", request)
    expected = "PHarness Campus fake inference: received 1 message(s); no model was called."
    content = body["choices"][0]["message"]["content"] if status == 200 else None
    report(
        status == 200 and content == expected and body.get("campus_session_id") == session_id and body.get("model") == "campus-fake",
        "fake_inference_fixture",
        {"status": status, "content": content, "usage": (body or {}).get("usage")},
    )
    status, body = broker("POST", "/v1/inference/chat/completions", dict(request, model="gpt-4o"))
    report(status == 400, "model_outside_envelope_refused", f"model gpt-4o -> {status} {(body or {}).get('error')}")
    for method, path in (("POST", "/v1/tools/call"), ("GET", "/v1/models"), ("POST", "/v1/responses")):
        status, body = broker(method, path, {} if method == "POST" else None)
        report(status in (401, 404), f"broker_route_absent {method} {path}", f"{status} {code(body)}")
    status, body = broker("GET", "/v1/token", token=None)
    report(status == 401 and code(body) == "malformed", "broker_requires_token", f"{status} {code(body)}")
    status, body = broker("GET", "/v1/token", token="pcbt1_" + "A" * 43)
    report(status == 401 and code(body) == "unknown_token", "broker_refuses_forged_token", f"{status} {code(body)}")


def credentials():
    for path in ("/var/run/secrets", "/run/secrets", "/var/run/secrets/kubernetes.io/serviceaccount/token"):
        report(not os.path.exists(path), "service_account_absent", f"{path} exists={os.path.exists(path)}")
    names = sorted(name for name in os.environ if any(part in name.upper() for part in CREDENTIAL_NAMES))
    report(not names, "no_credential_shaped_environment", names or f"{len(os.environ)} variables checked")
    token = read_token()
    leaks = []
    for pid in os.listdir("/proc"):
        if not pid.isdigit():
            continue
        for part in ("environ", "cmdline"):
            try:
                with open(f"/proc/{pid}/{part}", "rb") as handle:
                    if token.encode() in handle.read():
                        leaks.append(f"{pid}/{part}")
            except OSError:
                pass
    report(not leaks, "token_not_in_process_environment_or_arguments", leaks or "checked /proc/*/environ and cmdline")
    del token
    files = sorted(name for name in os.listdir(TOKEN_DIR) if not name.startswith(".."))
    report(set(files) <= {"token", "egress-ca.pem"} and "token" in files, "token_mount_contents", files)
    try:
        with open(TOKEN_FILE, "a", encoding="ascii"):
            pass
        report(False, "token_mount_read_only", "the token file opened for writing")
    except OSError as error:
        report(True, "token_mount_read_only", type(error).__name__)
    uids = set()
    for pid in os.listdir("/proc"):
        if pid.isdigit():
            try:
                uids.add(os.stat(f"/proc/{pid}").st_uid)
            except OSError:
                pass
    report(uids == {os.getuid()}, "processes_are_the_students_own", f"uids {sorted(uids)}, self {os.getuid()}")
    soft, hard = resource.getrlimit(resource.RLIMIT_NPROC)
    report(0 < soft <= 1024 and soft == hard, "process_limit_applied", f"RLIMIT_NPROC {soft}/{hard}")


def network(api):
    try:
        answer = socket.getaddrinfo("example.com", 443)
        report(False, "no_dns", f"example.com resolved: {answer[:1]}")
    except OSError as error:
        report(True, "no_dns", f"example.com: {type(error).__name__}")
    try:
        urllib.request.urlopen("https://example.com/", timeout=5)
        report(False, "https_to_unapproved_host_refused", "https://example.com/ answered")
    except Exception as error:  # noqa: BLE001 - any failure is the expected refusal
        report(True, "https_to_unapproved_host_refused", f"https://example.com/: {type(error).__name__}")
    blocked("raw_ip_https_blocked", "1.1.1.1", 443)
    blocked("raw_ip_https_blocked", "140.82.112.3", 443)
    blocked("egress_proxy_unreachable_without_approval", *PROXY)
    blocked("kubernetes_api_blocked", "10.43.0.1", 443)
    blocked("cloud_metadata_blocked", "169.254.169.254", 80)
    if api:
        host, port = api.rsplit(":", 1)
        blocked("campus_operator_api_blocked", host, int(port))


def research():
    session_id = ARGS.get("session", "")
    print(f"{MARK}BEGIN research session={session_id}", flush=True)
    agent_and_broker(session_id)
    credentials()
    network(ARGS.get("api"))
    failed = RESULTS.count(False)
    print(f"{MARK}RESULT research passed={len(RESULTS) - failed} failed={failed}", flush=True)


def broker_only():
    session_id = ARGS.get("session", "")
    print(f"{MARK}BEGIN broker session={session_id}", flush=True)
    agent_and_broker(session_id)
    failed = RESULTS.count(False)
    print(f"{MARK}RESULT broker passed={len(RESULTS) - failed} failed={failed}", flush=True)


def poll():
    """Ask the broker about this session's token until it is refused."""
    until = float(ARGS.get("until", time.time() + 600))
    fast_after = float(ARGS.get("fast_after", 0))
    last = None
    beat = 0.0
    while time.time() < until:
        try:
            status, body = broker("GET", "/v1/token")
            state = f"{status} {code(body) or '-'}"
        except OSError as error:
            status, state = None, f"error {type(error).__name__}"
        now = time.time()
        if state != last or now - beat >= 10:
            print(f"POLL {int(now * 1000)} {state}", flush=True)
            last, beat = state, now
        if status is not None and status != 200:
            print(f"POLL-REFUSED {int(now * 1000)} {state}", flush=True)
            return
        time.sleep(0.1 if now >= fast_after else 1.0)
    print(f"POLL-TIMEOUT {int(time.time() * 1000)} {last}", flush=True)


if __name__ == "__main__":
    mode = sys.argv[1] if len(sys.argv) > 1 else ""
    if mode == "research":
        research()
    elif mode == "broker":
        broker_only()
    elif mode == "poll":
        poll()
    else:
        sys.exit(__doc__)

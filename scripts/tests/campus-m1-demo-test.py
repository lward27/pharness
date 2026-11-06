"""Offline tests for the LUC-137 M1 demo probes and harness helpers.

Runs both in-sandbox probes against a fake Campus broker on loopback: the
broker checks must pass, the poller must stop at the first refusal, and the
broker token must never appear in their output. Then runs the harness's own
self-test (parsers, evidence directory and secret scan).
"""

import base64
import json
import os
import secrets
import subprocess
import sys
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
PY_PROBE = os.path.join(ROOT, "scripts", "campus-m1-demo-probe.py")
NODE_PROBE = os.path.join(ROOT, "scripts", "campus-m1-demo-probe.mjs")
HARNESS = os.path.join(ROOT, "ui", "scripts", "campus-m1-demo.mjs")
SESSION = "cls_demo_test"
TOKEN = "pcbt1_" + base64.urlsafe_b64encode(secrets.token_bytes(32)).decode().rstrip("=")
FAKE_ANSWER = "PHarness Campus fake inference: received 1 message(s); no model was called."
BROKER_CHECKS = (
    "broker_token_bound_to_session",
    "fake_inference_fixture",
    "model_outside_envelope_refused",
    "broker_route_absent POST /v1/tools/call",
    "broker_route_absent GET /v1/models",
    "broker_requires_token",
    "broker_refuses_forged_token",
)


class Broker(BaseHTTPRequestHandler):
    """The two broker routes, with refusal codes like the real listener."""

    revoke_after = None
    token_requests = 0

    def log_message(self, *args):
        pass

    def answer(self, status, body):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def authorized(self):
        values = self.headers.get_all("authorization") or []
        if len(values) != 1 or not values[0].startswith("Bearer pcbt1_"):
            self.answer(401, {"error": "refused", "code": "malformed"})
            return False
        if values[0] != "Bearer " + TOKEN:
            self.answer(401, {"error": "refused", "code": "unknown_token"})
            return False
        if Broker.revoke_after is not None and Broker.token_requests > Broker.revoke_after:
            self.answer(401, {"error": "refused", "code": "token_revoked"})
            return False
        return True

    def do_GET(self):
        if self.path != "/v1/token":
            self.answer(404, {"error": "no such campus broker route"})
            return
        Broker.token_requests += 1
        if self.authorized():
            self.answer(200, {"token_id": "cbt_1", "audience": "inference", "session_id": SESSION,
                              "lab_generation": 1, "expires_at": 2_000_000_000})

    def do_POST(self):
        length = int(self.headers.get("content-length") or 0)
        body = json.loads(self.rfile.read(length) or b"null")
        if self.path != "/v1/inference/chat/completions":
            self.answer(404, {"error": "no such campus broker route"})
            return
        if not self.authorized():
            return
        if body.get("model") not in (None, "campus-fake"):
            self.answer(400, {"error": "only the campus-fake model is available"})
            return
        self.answer(200, {
            "object": "chat.completion",
            "model": "campus-fake",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": FAKE_ANSWER}}],
            "usage": {"prompt_tokens": 3, "completion_tokens": 11, "total_tokens": 14},
            "campus_session_id": SESSION,
        })


def run(command, env, source=None):
    result = subprocess.run(command, env=env, input=source, capture_output=True, timeout=120)
    output = (result.stdout + result.stderr).decode()
    assert TOKEN not in output, f"{command[0]} printed the broker token"
    assert result.returncode == 0, output
    return output


def check_broker_mode(command, env):
    output = run(command, env)
    begin = output.index("DEMO-BEGIN broker session=" + SESSION)
    lines = output[begin:].splitlines()
    results = [line for line in lines if line.startswith(("PASS ", "FAIL "))]
    # codex is not installed outside the runner images.
    failed = [line for line in results if line.startswith("FAIL ") and not line.startswith("FAIL agent_cli")]
    assert not failed, "\n".join(failed)
    names = {line.split(" ", 1)[1].split(":", 1)[0] for line in results}
    missing = [name for name in BROKER_CHECKS if name not in names]
    assert not missing, f"{missing} missing from {sorted(names)}"
    assert any(line.startswith("DEMO-RESULT broker passed=") for line in lines), output


def check_poll_mode(command, env, probe):
    with open(probe, "rb") as handle:
        source = handle.read()
    Broker.token_requests = 0
    Broker.revoke_after = 3
    try:
        output = run(command, env, source)
    finally:
        Broker.revoke_after = None
    polls = [line.split() for line in output.splitlines() if line.startswith("POLL")]
    assert polls and polls[0][0] == "POLL" and polls[0][2] == "200", polls
    assert polls[-1][0] == "POLL-REFUSED" and polls[-1][2:] == ["401", "token_revoked"], polls


def main():
    server = ThreadingHTTPServer(("127.0.0.1", 0), Broker)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory() as token_dir:
            with open(os.path.join(token_dir, "token"), "w", encoding="ascii") as handle:
                handle.write(TOKEN + "\n")
            env = dict(os.environ, CAMPUS_DEMO_BROKER_DIR=token_dir, PYTHONDONTWRITEBYTECODE="1",
                       CAMPUS_BROKER_URL=f"http://127.0.0.1:{server.server_port}/")
            for name in ("HTTPS_PROXY", "HTTP_PROXY", "https_proxy", "http_proxy"):
                env.pop(name, None)
            check_broker_mode([sys.executable, PY_PROBE, "broker", f"session={SESSION}"], env)
            check_broker_mode(["node", NODE_PROBE, "broker", f"session={SESSION}"], env)
            check_poll_mode([sys.executable, "-", "poll", "until=9999999999"], env, PY_PROBE)
            check_poll_mode(["node", "--input-type=module", "-", "poll", "until=9999999999"], env, NODE_PROBE)
    finally:
        server.shutdown()
    result = subprocess.run(["node", HARNESS, "--self-test"], capture_output=True, text=True, timeout=60)
    assert result.returncode == 0, result.stdout + result.stderr
    print("campus-m1-demo tests: ok")


if __name__ == "__main__":
    main()

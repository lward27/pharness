#!/usr/bin/env python3
"""Deterministic, priced, OpenAI-compatible fake upstream for the LUC-115 LiteLLM spike.

Every call is appended to CALL_LOG (JSON lines, fsync per line) twice: a "recv" record when the
request arrives and a "done" record with the outcome and the billed amount. That log is the ground
truth for upstream calls and provider liability; nothing is ever truncated.

Per-request behaviour is selected by markers in the message text, which LiteLLM forwards verbatim:
  [[rid:ID]]        correlation id chosen by the driver (joins driver requests to upstream calls)
  [[fault:MODE]]    MODE in 429 | 500 | timeout | cut   (cut = stream ends before the usage chunk)
  [[delay:SEC]]     sleep before answering, keeps a request in flight
  [[ctok:N]]        completion tokens to generate (default: max_tokens, else 50)
POST /control {"queue": ["500", ...]} applies faults to the next N calls regardless of markers.

Billing policy (worst-case provider liability): ok, cut and timeout bill the full planned tokens,
because the provider generated them; 429 and 500 bill nothing.
"""
import json
import os
import re
import socket
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

LOG_PATH = os.environ.get("CALL_LOG", "/data/calls.jsonl")
TIMEOUT_SLEEP = float(os.environ.get("TIMEOUT_SLEEP", "30"))
PRICES = {
    "fake-small": (0.00001, 0.00003),
    "fake-large": (0.00003, 0.00006),
    "fake-unpriced": (0.00001, 0.00003),
}
DEFAULT_PRICE = (0.00001, 0.00003)
MARKER = re.compile(r"\[\[(rid|fault|delay|ctok):([^\]]+)\]\]")

_log_lock = threading.Lock()
_control_lock = threading.Lock()
_fault_queue = []


def append_log(record):
    line = json.dumps(record, separators=(",", ":")) + "\n"
    with _log_lock:
        with open(LOG_PATH, "a", encoding="utf-8") as handle:
            handle.write(line)
            handle.flush()
            os.fsync(handle.fileno())


def message_text(body):
    parts = []
    for message in body.get("messages") or []:
        content = message.get("content")
        if isinstance(content, str):
            parts.append(content)
        elif isinstance(content, list):
            parts.extend(str(item.get("text", "")) for item in content if isinstance(item, dict))
    return "\n".join(parts)


def plan(body):
    text = message_text(body)
    markers = {}
    for name, value in MARKER.findall(text):
        markers.setdefault(name, value)
    with _control_lock:
        queued = _fault_queue.pop(0) if _fault_queue else None
    fault = queued or markers.get("fault")
    requested = body.get("max_completion_tokens") or body.get("max_tokens")
    completion = int(markers["ctok"]) if "ctok" in markers else int(requested or 50)
    completion = max(1, min(completion, 2000))
    prompt = 10 + len(text.split())
    model = str(body.get("model", ""))
    price_in, price_out = PRICES.get(model, DEFAULT_PRICE)
    full_cost = prompt * price_in + completion * price_out
    return {
        "rid": markers.get("rid"),
        "fault": fault,
        "fault_source": "control" if queued else ("marker" if "fault" in markers else None),
        "delay": float(markers.get("delay", 0) or 0),
        "prompt_tokens": prompt,
        "completion_tokens": completion,
        "model": model,
        "full_cost": round(full_cost, 10),
    }


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "fake-upstream/1"

    def log_message(self, fmt, *args):
        return

    def _json(self, status, payload, extra_headers=None):
        data = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        for key, value in (extra_headers or {}).items():
            self.send_header(key, value)
        self.end_headers()
        self.wfile.write(data)

    def _body(self):
        length = int(self.headers.get("Content-Length") or 0)
        return json.loads(self.rfile.read(length) or b"{}")

    def do_GET(self):
        if self.path == "/healthz":
            return self._json(200, {"ok": True})
        if self.path.startswith("/v1/models"):
            return self._json(200, {"object": "list", "data": [{"id": m, "object": "model"} for m in PRICES]})
        if self.path.startswith("/calls"):
            data = b""
            if os.path.exists(LOG_PATH):
                with _log_lock, open(LOG_PATH, "rb") as handle:
                    data = handle.read()
            self.send_response(200)
            self.send_header("Content-Type", "application/x-ndjson")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            return None
        if self.path == "/control":
            with _control_lock:
                return self._json(200, {"queue": list(_fault_queue)})
        return self._json(404, {"error": "not found"})

    def do_POST(self):
        if self.path == "/control":
            body = self._body()
            with _control_lock:
                if body.get("clear"):
                    _fault_queue.clear()
                _fault_queue.extend(str(item) for item in body.get("queue") or [])
                queue = list(_fault_queue)
            append_log({"ev": "control", "ts": time.time(), "queue": queue})
            return self._json(200, {"queue": queue})
        if not self.path.rstrip("/").endswith("/chat/completions"):
            return self._json(404, {"error": "not found"})
        body = self._body()
        call = plan(body)
        call_id = uuid.uuid4().hex
        stream = bool(body.get("stream"))
        append_log(
            {
                "ev": "recv",
                "call_id": call_id,
                "ts": time.time(),
                "stream": stream,
                "body_keys": sorted(body.keys()),
                "user_field": body.get("user"),
                "metadata_field": body.get("metadata"),
                **call,
            }
        )
        if call["delay"] > 0:
            time.sleep(call["delay"])
        outcome, billed = self._respond(call_id, call, stream)
        append_log(
            {
                "ev": "done",
                "call_id": call_id,
                "ts": time.time(),
                "rid": call["rid"],
                "outcome": outcome,
                "billed_usd": billed,
                "prompt_tokens": call["prompt_tokens"],
                "completion_tokens": call["completion_tokens"] if billed else 0,
            }
        )
        return None

    def _respond(self, call_id, call, stream):
        fault = call["fault"]
        if fault == "429":
            self._json(429, {"error": {"message": "fake rate limit", "type": "rate_limit_error"}}, {"Retry-After": "1"})
            return "429", 0.0
        if fault == "500":
            self._json(500, {"error": {"message": "fake upstream failure", "type": "server_error"}})
            return "500", 0.0
        if fault == "timeout":
            time.sleep(TIMEOUT_SLEEP)
            try:
                self._json(200, self._completion(call_id, call))
            except OSError:
                pass
            return "timeout", call["full_cost"]
        if stream:
            return self._stream(call_id, call, cut=(fault == "cut"))
        self._json(200, self._completion(call_id, call))
        return "ok", call["full_cost"]

    def _usage(self, call):
        return {
            "prompt_tokens": call["prompt_tokens"],
            "completion_tokens": call["completion_tokens"],
            "total_tokens": call["prompt_tokens"] + call["completion_tokens"],
        }

    def _completion(self, call_id, call):
        return {
            "id": f"chatcmpl-{call_id}",
            "object": "chat.completion",
            "created": int(time.time()),
            "model": call["model"],
            "choices": [
                {
                    "index": 0,
                    "message": {"role": "assistant", "content": " ".join(["tok"] * call["completion_tokens"])},
                    "finish_reason": "length",
                }
            ],
            "usage": self._usage(call),
        }

    def _chunk(self, payload):
        data = f"data: {json.dumps(payload)}\n\n".encode()
        self.wfile.write(f"{len(data):x}\r\n".encode() + data + b"\r\n")
        self.wfile.flush()

    def _stream(self, call_id, call, cut):
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Transfer-Encoding", "chunked")
        self.end_headers()
        base = {"id": f"chatcmpl-{call_id}", "object": "chat.completion.chunk", "created": int(time.time()), "model": call["model"]}
        try:
            self._chunk({**base, "choices": [{"index": 0, "delta": {"role": "assistant", "content": ""}, "finish_reason": None}]})
            pieces = 5
            per_piece = max(1, call["completion_tokens"] // pieces)
            sent = 0
            for index in range(pieces):
                if cut and index == pieces // 2 + 1:
                    time.sleep(0.2)
                    self.connection.shutdown(socket.SHUT_RDWR)
                    self.close_connection = True
                    return "cut", call["full_cost"]
                count = per_piece if index < pieces - 1 else call["completion_tokens"] - sent
                sent += count
                self._chunk({**base, "choices": [{"index": 0, "delta": {"content": " tok" * count}, "finish_reason": None}]})
                time.sleep(0.05)
            self._chunk({**base, "choices": [{"index": 0, "delta": {}, "finish_reason": "length"}]})
            self._chunk({**base, "choices": [], "usage": self._usage(call)})
            data = b"data: [DONE]\n\n"
            self.wfile.write(f"{len(data):x}\r\n".encode() + data + b"\r\n0\r\n\r\n")
            self.wfile.flush()
        except OSError:
            return "client_gone", call["full_cost"]
        return "ok", call["full_cost"]


if __name__ == "__main__":
    os.makedirs(os.path.dirname(LOG_PATH), exist_ok=True)
    server = ThreadingHTTPServer(("0.0.0.0", int(os.environ.get("PORT", "8080"))), Handler)
    server.daemon_threads = True
    server.request_queue_size = 1024
    server.serve_forever()

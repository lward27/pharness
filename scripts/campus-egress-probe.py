"""Campus egress probes, run inside a lab session Pod (LUC-130).

Usage: python3 - <mode> [key=value ...] < campus-egress-probe.py

Every check prints one line, ``PASS <check>: <detail>`` or
``FAIL <check>: <detail>``. Modes:

- ``matrix``: what a session with approved egress must not reach, directly or
  through the proxy, and what the proxy must refuse inside a tunnel.
- ``isolated``: what a session without egress must not reach, the proxy
  included.
- ``functional``: approved retrieval through the proxy (codeload, raw files,
  redirects that leave the approved hosts).

Only the standard library is used; the session has no DNS, so every address
is literal.
"""

import json
import os
import socket
import ssl
import sys
import time
import urllib.error
import urllib.request

PROXY = ("10.43.130.30", 3128)
ARGS = dict(arg.split("=", 1) for arg in sys.argv[2:])
FAILED = 0


def report(ok, check, detail):
    global FAILED
    if not ok:
        FAILED += 1
    print(f"{'PASS' if ok else 'FAIL'} {check}: {str(detail)[:400]}", flush=True)


def tcp(host, port, timeout=3.0):
    """'open', 'timeout' or the error. A NetworkPolicy drop is a timeout."""
    sock = socket.socket(socket.AF_INET6 if ":" in host else socket.AF_INET)
    sock.settimeout(timeout)
    started = time.monotonic()
    try:
        sock.connect((host, port))
        return "open"
    except socket.timeout:
        return "timeout"
    except OSError as error:
        return f"{type(error).__name__}:{error.errno} after {time.monotonic() - started:.1f}s"
    finally:
        sock.close()


def blocked(check, host, port):
    outcome = tcp(host, port)
    report(outcome != "open", check, f"{host}:{port} {outcome}")


def udp_dns(server):
    query = bytes.fromhex("abcd01000001000000000000") + b"\x04pypi\x03org\x00\x00\x01\x00\x01"
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.settimeout(3)
    try:
        sock.sendto(query, (server, 53))
        sock.recvfrom(512)
        return "answered"
    except OSError as error:
        return type(error).__name__
    finally:
        sock.close()


def proxy_head(head, extra=b"", timeout=10):
    """Send a raw request head to the proxy; return its status line or why
    there was none."""
    sock = socket.create_connection(PROXY, timeout=timeout)
    try:
        sock.sendall(head.encode() + extra)
        data = b""
        while b"\r\n" not in data:
            chunk = sock.recv(4096)
            if not chunk:
                break
            data += chunk
        return data.split(b"\r\n")[0].decode(errors="replace") or "closed", data
    except OSError as error:
        return f"error {type(error).__name__}", b""
    finally:
        sock.close()


def connect_refused(check, authority, expect="403"):
    status, data = proxy_head(f"CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n")
    body = data.split(b"\r\n\r\n", 1)[-1].decode(errors="replace").strip()
    report(status.startswith(f"HTTP/1.1 {expect}"), check, f"CONNECT {authority} -> {status} {body}")


def tunnel(host, alpn=("http/1.1",)):
    """An approved TLS tunnel to host through the proxy, verified against the
    session's trust store (the Campus egress CA)."""
    sock = socket.create_connection(PROXY, timeout=15)
    sock.sendall(f"CONNECT {host}:443 HTTP/1.1\r\nHost: {host}:443\r\n\r\n".encode())
    status = sock.recv(4096).split(b"\r\n")[0].decode(errors="replace")
    if not status.startswith("HTTP/1.1 200"):
        sock.close()
        raise RuntimeError(f"tunnel refused: {status}")
    context = ssl.create_default_context()
    context.set_alpn_protocols(list(alpn))
    return context.wrap_socket(sock, server_hostname=host)


def request_in_tunnel(host, raw):
    tls = tunnel(host)
    try:
        tls.sendall(raw.encode())
        data = b""
        while b"\r\n\r\n" not in data:
            chunk = tls.recv(65536)
            if not chunk:
                break
            data += chunk
        head, _, body = data.partition(b"\r\n\r\n")
        return head.split(b"\r\n")[0].decode(errors="replace"), body[:200].decode(errors="replace")
    finally:
        tls.close()


def refused_in_tunnel(check, host, raw, code):
    try:
        status, body = request_in_tunnel(host, raw)
    except Exception as error:  # noqa: BLE001 - every failure is evidence
        report(False, check, f"{type(error).__name__}: {error}")
        return
    report(status.startswith("HTTP/1.1 403") and code in body, check, f"{status} {body.strip()}")


def get(url, **headers):
    opener = urllib.request.build_opener(
        urllib.request.ProxyHandler({"https": f"http://{PROXY[0]}:{PROXY[1]}"})
    )
    request = urllib.request.Request(url, headers=headers)
    with opener.open(request, timeout=30) as response:
        return response.status, response.geturl(), len(response.read())


def common_isolation():
    blocked("cluster API ClusterIP", "10.43.0.1", 443)
    blocked("cluster API endpoint", "192.168.20.192", 6443)
    blocked("kubelet on the lab node", "192.168.20.210", 10250)
    blocked("control-plane node ssh", "192.168.20.192", 22)
    blocked("build node ssh", "192.168.20.175", 22)
    blocked("cloud metadata", "169.254.169.254", 80)
    blocked("pharness-api Service", ARGS.get("api_service", "10.43.244.148"), 4777)
    blocked("pharness-api Pod", ARGS["api_pod"], 4777)
    blocked("pharness-api Pod broker port off the broker Pods", ARGS["api_pod"], 4778)
    blocked("model gateway Service", ARGS.get("gateway_service", "10.43.169.94"), 4780)
    blocked("kube-dns TCP", "10.43.0.10", 53)
    report(udp_dns("10.43.0.10") != "answered", "kube-dns UDP", udp_dns("10.43.0.10"))
    report(udp_dns("8.8.8.8") != "answered", "public DNS UDP", udp_dns("8.8.8.8"))
    try:
        socket.getaddrinfo("pypi.org", 443)
        report(False, "name resolution", "pypi.org resolved")
    except OSError as error:
        report(True, "name resolution", f"pypi.org: {error}")
    blocked("raw public IP 443", "1.1.1.1", 443)
    blocked("raw GitHub IP 443", "140.82.112.3", 443)
    blocked("raw public IP 80", "1.1.1.1", 80)
    blocked("raw public IP 22", "140.82.112.3", 22)
    if ARGS.get("peer"):
        blocked("peer session listener", ARGS["peer"], 8080)


def matrix():
    common_isolation()
    connect_refused("CONNECT raw IPv4", "1.1.1.1:443")
    connect_refused("CONNECT raw IPv6", "[2606:4700:4700::1111]:443")
    connect_refused("CONNECT metadata", "169.254.169.254:80")
    connect_refused("CONNECT cluster API", "10.43.0.1:443")
    connect_refused("CONNECT cluster service name", "kubernetes.default.svc:443")
    connect_refused("CONNECT unapproved host", "example.com:443")
    connect_refused("CONNECT approved-host suffix", "pypi.org.evil.example:443")
    connect_refused("CONNECT trailing dot", "pypi.org.:443")
    connect_refused("CONNECT approved host on port 80", "pypi.org:80")
    connect_refused("CONNECT approved host on port 22", "github.com:22")
    connect_refused("CONNECT with user info", "user@pypi.org:443")
    if ARGS.get("unapproved"):
        connect_refused("CONNECT host approved only for other profiles", f"{ARGS['unapproved']}:443")
    status, _ = proxy_head("GET http://pypi.org/simple/ HTTP/1.1\r\nHost: pypi.org\r\n\r\n")
    report(status.startswith("HTTP/1.1 403"), "plain HTTP forwarding", status)
    status, _ = proxy_head("PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n")
    report(not status.startswith("HTTP/1.1 200"), "HTTP/2 prior knowledge to the proxy", status)
    status, _ = proxy_head(
        "CONNECT pypi.org:443 HTTP/1.1\r\nHost: pypi.org:443\r\n\r\n",
        extra=b"\x16\x03\x01\x02\x00\x01\x00\x01\xfc\x03\x03",
    )
    report(not status.startswith("HTTP/1.1 200"), "data before the tunnel is approved", status)

    # Inside an approved tunnel.
    try:
        tls = tunnel("pypi.org", alpn=("h2",))
        report(False, "ALPN h2 only", f"negotiated {tls.selected_alpn_protocol()}")
        tls.close()
    except (ssl.SSLError, OSError) as error:
        report(True, "ALPN h2 only", f"{type(error).__name__}: {error}")
    tls = tunnel("pypi.org", alpn=("h2", "http/1.1"))
    report(tls.selected_alpn_protocol() == "http/1.1", "ALPN falls back to HTTP/1.1",
           f"{tls.selected_alpn_protocol()} {tls.getpeercert()['issuer']}")
    tls.close()
    refused_in_tunnel("POST to a package index", "pypi.org",
                      "POST /simple/ HTTP/1.1\r\nHost: pypi.org\r\nContent-Length: 0\r\n\r\n", "method")
    refused_in_tunnel("PUT to a package index", "registry.npmjs.org",
                      "PUT /x HTTP/1.1\r\nHost: registry.npmjs.org\r\nContent-Length: 0\r\n\r\n", "method")
    refused_in_tunnel("Host header of another site", "pypi.org",
                      "GET / HTTP/1.1\r\nHost: evil.example\r\n\r\n", "host_mismatch")
    refused_in_tunnel("absolute-form target", "pypi.org",
                      "GET https://evil.example/ HTTP/1.1\r\nHost: pypi.org\r\n\r\n", "target")
    refused_in_tunnel("WebSocket upgrade", "github.com",
                      "GET / HTTP/1.1\r\nHost: github.com\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n"
                      "Sec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n", "upgrade")
    refused_in_tunnel("h2c upgrade", "github.com",
                      "GET / HTTP/1.1\r\nHost: github.com\r\nConnection: Upgrade, HTTP2-Settings\r\n"
                      "Upgrade: h2c\r\nHTTP2-Settings: AAMAAABkAAQAAP__\r\n\r\n", "upgrade")
    refused_in_tunnel("git receive-pack discovery", "github.com",
                      "GET /octocat/Hello-World.git/info/refs?service=git-receive-pack HTTP/1.1\r\n"
                      "Host: github.com\r\n\r\n", "git_push")
    refused_in_tunnel("git receive-pack POST", "github.com",
                      "POST /octocat/Hello-World.git/git-receive-pack HTTP/1.1\r\nHost: github.com\r\n"
                      "Content-Type: application/x-git-receive-pack-request\r\nContent-Length: 0\r\n\r\n", "git_push")
    refused_in_tunnel("encoded receive-pack", "github.com",
                      "GET /octocat/Hello-World.git/info/refs?service=git-%72eceive-pack HTTP/1.1\r\n"
                      "Host: github.com\r\n\r\n", "git_push")
    refused_in_tunnel("upload-pack POST with another content type", "github.com",
                      "POST /octocat/Hello-World.git/git-upload-pack HTTP/1.1\r\nHost: github.com\r\n"
                      "Content-Type: text/plain\r\nContent-Length: 0\r\n\r\n", "method")
    refused_in_tunnel("POST to the GitHub API path", "github.com",
                      "POST /octocat/Hello-World/issues HTTP/1.1\r\nHost: github.com\r\n"
                      "Content-Type: application/json\r\nContent-Length: 2\r\n\r\n{}", "method")
    status, body = request_in_tunnel(
        "github.com",
        "GET /octocat/Hello-World.git/info/refs?service=git-upload-pack HTTP/1.1\r\nHost: github.com\r\n"
        "Connection: close\r\n\r\n",
    )
    report(status.startswith("HTTP/1.1 200"), "git upload-pack discovery", status)
    # github.com/<user>.png redirects to avatars.githubusercontent.com, which
    # is not approved: following it opens a new tunnel that is refused.
    try:
        result = get("https://github.com/octocat.png")
        report(False, "redirect off the approved hosts", f"followed to {result}")
    except (urllib.error.URLError, OSError) as error:
        report("403" in str(error) or "Tunnel" in str(error), "redirect off the approved hosts",
               f"{type(error).__name__}: {error}")


def isolated():
    common_isolation()
    blocked("egress proxy from a session without egress", PROXY[0], PROXY[1])


def functional():
    for url in ARGS["urls"].split(","):
        try:
            status, final, size = get(url)
            report(status == 200 and size > 0, f"GET {url}", f"{status} {final} {size} bytes")
        except Exception as error:  # noqa: BLE001
            report(False, f"GET {url}", f"{type(error).__name__}: {error}")
    print(json.dumps({"env": {key: os.environ.get(key, "") for key in (
        "HTTPS_PROXY", "NO_PROXY", "SSL_CERT_FILE", "PIP_CERT", "CAMPUS_BROKER_URL")}}))


{"matrix": matrix, "isolated": isolated, "functional": functional}[sys.argv[1]]()
print(f"RESULT failed={FAILED}", flush=True)
sys.exit(1 if FAILED else 0)

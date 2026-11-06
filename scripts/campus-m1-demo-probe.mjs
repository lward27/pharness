// Campus M1 demonstration probes for a coding (Node runner) session.
//
// Runs inside a lab session Pod, either typed into the browser terminal
// (`coding` mode) or through an operator's `kubectl exec` (`poll` mode):
//
//   node campus-m1-demo-probe.mjs coding session=<id> api=<ip:port>
//   node --input-type=module - poll until=<epoch seconds> < campus-m1-demo-probe.mjs
//   node campus-m1-demo-probe.mjs broker session=<id>   (broker checks only)
//
// `coding` prints `PASS <check>: <detail>` or `FAIL <check>: <detail>` per
// check between a begin and a result marker. `poll` asks the broker about
// the session's own token until the broker refuses it.
//
// The broker token is read from its file for every request and is never
// printed. Only Node built-ins and git are used; the session has no DNS.

import { spawnSync } from "node:child_process";
import { lookup } from "node:dns/promises";
import { existsSync, mkdtempSync, openSync, closeSync, readFileSync, readdirSync, statSync } from "node:fs";
import http from "node:http";
import net from "node:net";
import tls from "node:tls";

// Overridable only so scripts/tests can run the probe against a fake broker.
const TOKEN_DIR = process.env.CAMPUS_DEMO_BROKER_DIR ?? "/var/run/campus-broker";
const TOKEN_FILE = `${TOKEN_DIR}/token`;
const PROXY = { host: "10.43.130.30", port: 3128 };
const ARGS = Object.fromEntries(process.argv.slice(3).filter(arg => arg.includes("=")).map(arg => {
  const at = arg.indexOf("=");
  return [arg.slice(0, at), arg.slice(at + 1)];
}));
// Built at run time so the echoed command line never contains a marker.
const MARK = "DEMO" + "-";
const CREDENTIAL_NAMES = ["TOKEN", "SECRET", "PASSWORD", "PASSWD", "API_KEY", "APIKEY", "CREDENTIAL", "PRIVATE_KEY", "AWS_", "OPENAI", "ANTHROPIC", "FIREWORKS"];
const results = [];

function report(ok, check, detail) {
  results.push(ok);
  const text = typeof detail === "string" ? detail : JSON.stringify(detail);
  console.log(`${ok ? "PASS" : "FAIL"} ${check}: ${String(text).slice(0, 300)}`);
}

const readToken = () => readFileSync(TOKEN_FILE, "ascii").trim();

/** {status, body}. The token never leaves this function. */
function broker(method, path, body, token = "file") {
  const base = new URL(process.env.CAMPUS_BROKER_URL ?? "");
  const headers = { accept: "application/json" };
  if (token === "file") headers.authorization = `Bearer ${readToken()}`;
  else if (token) headers.authorization = `Bearer ${token}`;
  const payload = body === undefined ? undefined : JSON.stringify(body);
  if (payload) headers["content-type"] = "application/json";
  return new Promise((resolve, reject) => {
    const request = http.request({ host: base.hostname, port: base.port || 80, path, method, headers, timeout: 10_000 }, response => {
      let raw = "";
      response.on("data", chunk => { raw += chunk; });
      response.on("end", () => {
        let parsed = null;
        try { parsed = JSON.parse(raw || "null"); } catch {}
        resolve({ status: response.statusCode, body: parsed });
      });
    });
    request.on("timeout", () => request.destroy(new Error("timeout")));
    request.on("error", reject);
    request.end(payload);
  });
}

/** "open", "timeout" or the error code. A NetworkPolicy drop is a timeout. */
function tcp(host, port, timeout = 3000) {
  return new Promise(resolve => {
    const socket = net.connect({ host, port });
    const done = outcome => { socket.destroy(); resolve(outcome); };
    socket.setTimeout(timeout, () => done("timeout"));
    socket.on("connect", () => done("open"));
    socket.on("error", error => done(error.code ?? error.name));
  });
}

async function blocked(check, host, port) {
  const outcome = await tcp(host, port);
  report(outcome !== "open", check, `${host}:${port} ${outcome}`);
}

/** The proxy's status line for a CONNECT, or why there was none. */
function connectThroughProxy(target) {
  return new Promise(resolve => {
    const socket = net.connect(PROXY);
    let head = "";
    const done = outcome => { socket.destroy(); resolve(outcome); };
    socket.setTimeout(10_000, () => done("timeout"));
    socket.on("connect", () => socket.write(`CONNECT ${target} HTTP/1.1\r\nHost: ${target}\r\n\r\n`));
    socket.on("data", chunk => {
      head += chunk.toString("latin1");
      if (head.includes("\r\n")) done(head.split("\r\n")[0]);
    });
    socket.on("error", error => done(error.code ?? error.name));
    socket.on("end", () => done(head.split("\r\n")[0] || "closed"));
  });
}

/** The status line and body the proxy answers inside an approved tunnel. */
function inTunnel(host, raw) {
  return new Promise(resolve => {
    const socket = net.connect(PROXY);
    let head = "";
    let secure = null;
    const done = outcome => { (secure ?? socket).destroy(); resolve(outcome); };
    socket.setTimeout(15_000, () => done({ status: "timeout", body: "" }));
    socket.on("error", error => done({ status: error.code ?? error.name, body: "" }));
    socket.on("connect", () => socket.write(`CONNECT ${host}:443 HTTP/1.1\r\nHost: ${host}:443\r\n\r\n`));
    const onConnectReply = chunk => {
      head += chunk.toString("latin1");
      if (!head.includes("\r\n\r\n")) return;
      socket.off("data", onConnectReply);
      if (!/^HTTP\/1\.[01] 200/.test(head)) {
        done({ status: `CONNECT ${head.split("\r\n")[0]}`, body: "" });
        return;
      }
      let reply = "";
      secure = tls.connect({ socket, servername: host, ca: readFileSync(process.env.SSL_CERT_FILE ?? "/etc/ssl/certs/ca-certificates.crt"), ALPNProtocols: ["http/1.1"] }, () => secure.write(raw));
      const answer = () => {
        const split = reply.indexOf("\r\n\r\n");
        return { status: reply.split("\r\n")[0] || "closed", body: split < 0 ? "" : reply.slice(split + 4, split + 300) };
      };
      secure.on("data", data => {
        reply += data.toString("latin1");
        if (reply.length > 4096) done(answer());
      });
      secure.on("end", () => done(answer()));
      secure.on("close", () => done(answer()));
      secure.on("error", error => done({ status: `tls ${error.code ?? error.message}`, body: "" }));
    };
    socket.on("data", onConnectReply);
  });
}

const code = body => (body && typeof body === "object" ? body.code ?? null : null);
const run = (command, args, options = {}) => spawnSync(command, args, {
  encoding: "utf8",
  timeout: 60_000,
  env: { ...process.env, GIT_TERMINAL_PROMPT: "0", GIT_ASKPASS: "/bin/false", GIT_CONFIG_NOSYSTEM: "1", GIT_CONFIG_GLOBAL: "/dev/null" },
  ...options,
});

async function agentAndBroker(sessionId) {
  const version = run("codex", ["--version"]);
  const line = `${version.stdout || version.stderr}`.trim().split("\n")[0];
  report(version.status === 0 && line.startsWith("codex-cli"), "agent_cli", line);

  let { status, body } = await broker("GET", "/v1/token");
  report(status === 200 && body.audience === "inference" && body.session_id === sessionId, "broker_token_bound_to_session",
    status === 200 ? { audience: body.audience, session_id: body.session_id, lab_generation: body.lab_generation, expires_at: body.expires_at } : status);
  const request = { model: "campus-fake", messages: [{ role: "user", content: "M1 demo fixture" }] };
  ({ status, body } = await broker("POST", "/v1/inference/chat/completions", request));
  const expected = "PHarness Campus fake inference: received 1 message(s); no model was called.";
  const content = status === 200 ? body.choices[0].message.content : null;
  report(status === 200 && content === expected && body.campus_session_id === sessionId && body.model === "campus-fake",
    "fake_inference_fixture", { status, content, usage: body?.usage });
  ({ status, body } = await broker("POST", "/v1/inference/chat/completions", { ...request, model: "gpt-4o" }));
  report(status === 400, "model_outside_envelope_refused", `model gpt-4o -> ${status} ${body?.error}`);
  for (const [method, path] of [["POST", "/v1/tools/call"], ["GET", "/v1/models"], ["POST", "/v1/responses"]]) {
    ({ status, body } = await broker(method, path, method === "POST" ? {} : undefined));
    report(status === 401 || status === 404, `broker_route_absent ${method} ${path}`, `${status} ${code(body)}`);
  }
  ({ status, body } = await broker("GET", "/v1/token", undefined, null));
  report(status === 401 && code(body) === "malformed", "broker_requires_token", `${status} ${code(body)}`);
  ({ status, body } = await broker("GET", "/v1/token", undefined, `pcbt1_${"A".repeat(43)}`));
  report(status === 401 && code(body) === "unknown_token", "broker_refuses_forged_token", `${status} ${code(body)}`);
}

function credentials() {
  for (const path of ["/var/run/secrets", "/run/secrets", "/var/run/secrets/kubernetes.io/serviceaccount/token"]) {
    report(!existsSync(path), "service_account_absent", `${path} exists=${existsSync(path)}`);
  }
  const names = Object.keys(process.env).filter(name => CREDENTIAL_NAMES.some(part => name.toUpperCase().includes(part))).sort();
  report(names.length === 0, "no_credential_shaped_environment", names.length ? names : `${Object.keys(process.env).length} variables checked`);
  const token = Buffer.from(readToken());
  const leaks = [];
  for (const pid of readdirSync("/proc").filter(name => /^\d+$/.test(name))) {
    for (const part of ["environ", "cmdline"]) {
      try {
        if (readFileSync(`/proc/${pid}/${part}`).includes(token)) leaks.push(`${pid}/${part}`);
      } catch {}
    }
  }
  report(leaks.length === 0, "token_not_in_process_environment_or_arguments", leaks.length ? leaks : "checked /proc/*/environ and cmdline");
  const files = readdirSync(TOKEN_DIR).filter(name => !name.startsWith("..")).sort();
  report(files.includes("token") && files.every(name => name === "token" || name === "egress-ca.pem"), "token_mount_contents", files);
  try {
    closeSync(openSync(TOKEN_FILE, "a"));
    report(false, "token_mount_read_only", "the token file opened for writing");
  } catch (error) {
    report(true, "token_mount_read_only", error.code);
  }
  const uid = process.getuid();
  const uids = new Set();
  for (const pid of readdirSync("/proc").filter(name => /^\d+$/.test(name))) {
    try { uids.add(statSync(`/proc/${pid}`).uid); } catch {}
  }
  report(uids.size === 1 && uids.has(uid), "processes_are_the_students_own", `uids ${[...uids].sort()}, self ${uid}`);
  const limit = run("sh", ["-c", "ulimit -u 2>/dev/null || ulimit -p"]).stdout.trim();
  report(/^\d+$/.test(limit) && Number(limit) <= 1024, "process_limit_applied", `ulimit -u ${limit}`);
}

async function network(api) {
  try {
    const answer = await lookup("example.com");
    report(false, "no_dns", `example.com resolved: ${answer.address}`);
  } catch (error) {
    report(true, "no_dns", `example.com: ${error.code}`);
  }
  await blocked("raw_ip_https_blocked", "1.1.1.1", 443);
  await blocked("raw_ip_https_blocked", "140.82.112.3", 443);
  await blocked("kubernetes_api_blocked", "10.43.0.1", 443);
  await blocked("cloud_metadata_blocked", "169.254.169.254", 80);
  if (api) {
    const at = api.lastIndexOf(":");
    await blocked("campus_operator_api_blocked", api.slice(0, at), Number(api.slice(at + 1)));
  }
  const proxy = process.env.HTTPS_PROXY ?? process.env.https_proxy ?? "";
  report(proxy === `http://${PROXY.host}:${PROXY.port}`, "egress_goes_through_the_campus_proxy", `HTTPS_PROXY=${proxy}`);
  const approved = await connectThroughProxy("pypi.org:443");
  report(/^HTTP\/1\.[01] 200/.test(approved), "proxy_admits_approved_host", `CONNECT pypi.org:443 -> ${approved}`);
  for (const target of ["example.com:443", "93.184.215.14:443", "169.254.169.254:80", "kubernetes.default.svc:443", "pypi.org.evil.example:443"]) {
    const outcome = await connectThroughProxy(target);
    report(/^HTTP\/1\.[01] 403/.test(outcome), "proxy_refuses_unapproved_target", `CONNECT ${target} -> ${outcome}`);
  }
  const git = run("git", ["ls-remote", "https://github.com/octocat/Hello-World.git", "HEAD"]);
  report(git.status === 0 && /^[0-9a-f]{40}\s+HEAD/.test(git.stdout), "git_read_from_approved_host",
    git.status === 0 ? git.stdout.trim().slice(0, 60) : git.stderr.trim().split("\n").slice(-1)[0]);
  const fetchUnapproved = run("git", ["ls-remote", "https://gitlab.com/gitlab-org/gitlab.git", "HEAD"]);
  report(fetchUnapproved.status !== 0, "git_read_from_unapproved_host_refused", fetchUnapproved.stderr.trim().split("\n").slice(-1)[0]);
  const repo = mkdtempSync("/tmp/demo-push-");
  run("git", ["init", "-q", repo]);
  run("git", ["-C", repo, "-c", "user.name=campus-demo", "-c", "user.email=demo@campus-poc.example", "commit", "-q", "--allow-empty", "-m", "demo"]);
  const push = run("git", ["-C", repo, "-c", "credential.helper=", "push", "https://github.com/octocat/Hello-World.git", "HEAD:refs/heads/campus-demo"]);
  report(push.status !== 0 && /403/.test(push.stderr), "git_push_fails", push.stderr.trim().split("\n").slice(-1)[0]);
  const discovery = await inTunnel("github.com", "GET /octocat/Hello-World.git/info/refs?service=git-receive-pack HTTP/1.1\r\nHost: github.com\r\nConnection: close\r\n\r\n");
  report(/^HTTP\/1\.[01] 403/.test(discovery.status) && discovery.body.includes("git_push"), "git_push_refused_by_proxy", `${discovery.status} ${discovery.body.trim()}`);
}

async function coding() {
  const sessionId = ARGS.session ?? "";
  console.log(`${MARK}BEGIN coding session=${sessionId}`);
  await agentAndBroker(sessionId);
  credentials();
  await network(ARGS.api);
  const failed = results.filter(ok => !ok).length;
  console.log(`${MARK}RESULT coding passed=${results.length - failed} failed=${failed}`);
}

async function brokerOnly() {
  const sessionId = ARGS.session ?? "";
  console.log(`${MARK}BEGIN broker session=${sessionId}`);
  await agentAndBroker(sessionId);
  const failed = results.filter(ok => !ok).length;
  console.log(`${MARK}RESULT broker passed=${results.length - failed} failed=${failed}`);
}

async function poll() {
  const until = Number(ARGS.until ?? Date.now() / 1000 + 600);
  const fastAfter = Number(ARGS.fast_after ?? 0);
  let last = null;
  let beat = 0;
  while (Date.now() / 1000 < until) {
    let status = null;
    let state;
    try {
      const answer = await broker("GET", "/v1/token");
      status = answer.status;
      state = `${status} ${code(answer.body) ?? "-"}`;
    } catch (error) {
      state = `error ${error.code ?? error.message}`;
    }
    const now = Date.now();
    if (state !== last || now - beat >= 10_000) {
      console.log(`POLL ${now} ${state}`);
      last = state;
      beat = now;
    }
    if (status !== null && status !== 200) {
      console.log(`POLL-REFUSED ${now} ${state}`);
      return;
    }
    await new Promise(resolve => setTimeout(resolve, now / 1000 >= fastAfter ? 100 : 1000));
  }
  console.log(`POLL-TIMEOUT ${Date.now()} ${last}`);
}

const mode = process.argv[2];
if (mode === "coding") await coding();
else if (mode === "broker") await brokerOnly();
else if (mode === "poll") await poll();
else {
  console.error("usage: node campus-m1-demo-probe.mjs coding session=<id> api=<ip:port> | poll until=<epoch seconds>");
  process.exit(2);
}

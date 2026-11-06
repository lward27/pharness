#!/usr/bin/env node
// LUC-137 M1 demonstration: one governed Campus lab session from login to
// verified teardown, against the deployed Campus PoC (namespace campus-poc).
//
// A real browser (Playwright Chromium) signs the synthetic users in through
// Keycloak, launches labs, drives the browser terminal and stops them. kubectl
// (cluster-admin on the operator's context) records the pinned inputs and
// verifies, independently of the browser, that authority and resources are
// gone. Runbook: planning/operations/campus-m1-demo.md.
//
// Secrets: the synthetic users' passwords are read from the cluster into this
// process and typed into the Keycloak form only. Broker tokens never leave the
// session Pod; the probes read them from the mounted file. Tracing, video and
// HAR recording stay off, and the evidence bundle is scanned for every secret
// value this process held before the run reports success.

import { spawn, spawnSync } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, readdirSync, realpathSync, statSync, writeFileSync } from "node:fs";
import https from "node:https";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(here, "../..");
const PROBE_PY = join(REPO, "scripts/campus-m1-demo-probe.py");
const PROBE_MJS = join(REPO, "scripts/campus-m1-demo-probe.mjs");
const SLOTS = [1, 2, 3, 4, 5].map(n => `campus-lab-slot-${n}`);
const USERS = ["campus-admin", "instructor1", "student1", "student2"];
const TERMINAL_PROTOCOL = "pharness.campus.terminal.v1";
const SESSION_COOKIE = "__Host-pharness-campus";
const LABEL_SESSION = "pharness.dev/campus-session";
const LIVE = new Set(["provisioning", "ready", "stopping", "cleanup_blocked"]);
const REVOKE_LIMIT_MS = 60_000;
const REMOVAL_LIMIT_MS = 300_000;
const TOKEN_PATTERN = /pcbt1_[A-Za-z0-9_-]{43}/;

const USAGE = `Usage: node ui/scripts/campus-m1-demo.mjs [options]

  --base-url <url>        Campus origin (default https://campus.lucas.engineering)
  --idp-url <url>         IdP origin (default https://campus-idp.lucas.engineering)
  --context <ctx>         kubectl context with cluster-admin (default lucas_engineering)
  --port-forward          reach both hostnames through kubectl port-forward to
                          Traefik instead of DNS and the Cloudflare tunnel; TLS,
                          hostnames and Origin stay the public ones
  --traefik <ns/svc:port> Traefik Service for --port-forward (default traefik/traefik:443)
  --out <dir>             evidence directory outside the repository
                          (default /tmp/luc137-demo-<timestamp>)
  --course <id>           course (default cs101)
  --research <id>         research assignment (default research-lab)
  --coding <id>           coding assignment (default coding-lab)
  --timeout <id>          short-lifetime assignment (default research-timeout-demo)
  --skip-coding           skip the coding lab (proxy egress) phase
  --skip-timeout          skip the absolute-expiry phase
  --headed                show the browser
  --self-test             check the parsers and helpers offline and exit
`;

export function parseArgs(argv) {
  const options = {
    baseUrl: "https://campus.lucas.engineering",
    idpUrl: "https://campus-idp.lucas.engineering",
    context: "lucas_engineering",
    portForward: false,
    traefik: "traefik/traefik:443",
    out: null,
    course: "cs101",
    research: "research-lab",
    coding: "coding-lab",
    timeout: "research-timeout-demo",
    skipCoding: false,
    skipTimeout: false,
    headed: false,
    selfTest: false,
  };
  const valued = { "--base-url": "baseUrl", "--idp-url": "idpUrl", "--context": "context", "--traefik": "traefik", "--out": "out", "--course": "course", "--research": "research", "--coding": "coding", "--timeout": "timeout" };
  const flags = { "--port-forward": "portForward", "--skip-coding": "skipCoding", "--skip-timeout": "skipTimeout", "--headed": "headed", "--self-test": "selfTest" };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "-h" || arg === "--help") return { help: true };
    if (valued[arg]) {
      const value = argv[index + 1];
      if (value === undefined || value.startsWith("--")) throw new Error(`${arg} needs a value`);
      options[valued[arg]] = value;
      index += 1;
    } else if (flags[arg]) {
      options[flags[arg]] = true;
    } else {
      throw new Error(`unknown argument ${arg}`);
    }
  }
  for (const key of ["baseUrl", "idpUrl"]) {
    const url = new URL(options[key]);
    if (url.protocol !== "https:" || url.pathname !== "/" || url.search) throw new Error(`${key} must be an https origin`);
    options[key] = url.origin;
  }
  if (!/^[a-z0-9-]+\/[a-z0-9-]+:\d+$/.test(options.traefik)) throw new Error("--traefik must be <namespace>/<service>:<port>");
  return options;
}

/** The evidence directory must not be inside the repository checkout. */
export function outsideRepo(dir, repo = REPO) {
  const target = resolve(dir);
  const rel = relative(realpathSync(repo), existsSync(target) ? realpathSync(target) : target);
  return rel !== "" && (rel.startsWith("..") || isAbsolute(rel));
}

export function stripAnsi(text) {
  return text
    .replace(/\x1b\][^\x07\x1b]*(\x07|\x1b\\)/g, "")
    .replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, "")
    .replace(/\x1b[()][A-Za-z0-9]/g, "")
    .replace(/\x1b[=>78]/g, "")
    .replace(/\r/g, "");
}

/** The PASS/FAIL lines a probe printed between its begin and result markers. */
export function parseProbe(transcript, probe) {
  const begin = transcript.lastIndexOf(`DEMO-BEGIN ${probe} `);
  if (begin < 0) return null;
  const body = transcript.slice(begin);
  const result = body.match(new RegExp(`DEMO-RESULT ${probe} passed=(\\d+) failed=(\\d+)`));
  if (!result) return null;
  const checks = [];
  for (const line of body.slice(0, result.index).split("\n")) {
    const match = line.match(/^(PASS|FAIL) ([^:]+): ?(.*)$/);
    if (match) checks.push({ ok: match[1] === "PASS", check: match[2].trim(), detail: match[3] });
  }
  return { checks, passed: Number(result[1]), failed: Number(result[2]) };
}

/** POLL lines from the broker poller: `POLL <ms> <status> <code>`. */
export function parsePollLine(line) {
  const match = line.match(/^(POLL|POLL-REFUSED|POLL-TIMEOUT) (\d+) (\S+)(?: (\S+))?/);
  if (!match) return null;
  return { kind: match[1], at: Number(match[2]), status: match[3], code: match[4] ?? null };
}

/** Files under dir that contain any of the given values or a broker token. */
export function scanForSecrets(dir, secrets) {
  const hits = [];
  const needles = secrets.filter(value => typeof value === "string" && value.length >= 8).map(value => Buffer.from(value));
  const walk = path => {
    for (const name of readdirSync(path)) {
      const file = join(path, name);
      if (statSync(file).isDirectory()) {
        walk(file);
        continue;
      }
      const data = readFileSync(file);
      if (needles.some(needle => data.includes(needle)) || TOKEN_PATTERN.test(data.toString("latin1"))) hits.push(file);
    }
  };
  walk(dir);
  return hits;
}

const sha256 = value => createHash("sha256").update(value).digest("hex");
const sleep = ms => new Promise(resolvePromise => setTimeout(resolvePromise, ms));
const nowIso = () => new Date().toISOString();

// ---------------------------------------------------------------- run state

const evidence = {
  schema_version: "pharness.dev/campus-m1-demo-evidence/v1",
  story: "LUC-137",
  run_id: null,
  started_at: null,
  finished_at: null,
  options: null,
  pinned: {},
  identities: {},
  assignments: null,
  sessions: {},
  timings: {},
  checks: [],
  screenshots: [],
  errors: [],
};
const heldSecrets = [];
const children = new Set();
let OPTIONS;
let OUT;

function check(phase, name, ok, detail = null) {
  evidence.checks.push({ phase, check: name, ok: Boolean(ok), detail, at: nowIso() });
  console.log(`${ok ? "PASS" : "FAIL"} [${phase}] ${name}${detail === null ? "" : `: ${typeof detail === "string" ? detail : JSON.stringify(detail)}`.slice(0, 400)}`);
  return Boolean(ok);
}

function timing(path, ms) {
  const parts = path.split(".");
  let node = evidence.timings;
  for (const part of parts.slice(0, -1)) node = node[part] ??= {};
  node[parts.at(-1)] = ms;
  console.log(`TIME ${path} ${ms} ms`);
}

function save(name, data) {
  const file = join(OUT, name);
  mkdirSync(dirname(file), { recursive: true });
  writeFileSync(file, typeof data === "string" ? data : `${JSON.stringify(data, null, 2)}\n`);
  return file;
}

// ---------------------------------------------------------------- kubectl

const commandLog = [];

/** kubectl with the operator context. stdout is returned, never logged. */
function kube(args, { input, timeout = 60_000 } = {}) {
  commandLog.push(`${nowIso()} kubectl --context ${OPTIONS.context} ${args.join(" ")}`);
  const result = spawnSync("kubectl", ["--context", OPTIONS.context, ...args], { input, encoding: "utf8", timeout, maxBuffer: 64 * 1024 * 1024 });
  return { status: result.status, stdout: result.stdout ?? "", stderr: (result.stderr ?? "").trim() };
}

function kubeJson(args) {
  const result = kube([...args, "-o", "json"]);
  if (result.status !== 0) return null;
  return JSON.parse(result.stdout);
}

function digestOf(imageId) {
  return imageId?.match(/sha256:[0-9a-f]{64}/)?.[0] ?? null;
}

function podImages(pod) {
  const statuses = [...(pod.status?.initContainerStatuses ?? []), ...(pod.status?.containerStatuses ?? [])];
  return statuses.map(status => ({ container: status.name, image: status.image, image_id: status.imageID, digest: digestOf(status.imageID), ready: status.ready, restarts: status.restartCount }));
}

function readPasswords() {
  const result = kube(["-n", "campus-idp", "get", "secret", "campus-realm-secrets", "-o", "json"]);
  if (result.status !== 0) throw new Error(`cannot read campus-idp/campus-realm-secrets: ${result.stderr}`);
  const data = JSON.parse(result.stdout).data ?? {};
  const passwords = new Map();
  for (const user of USERS) {
    const encoded = data[`${user}-password`];
    if (!encoded) throw new Error(`campus-realm-secrets has no ${user}-password`);
    const value = Buffer.from(encoded, "base64").toString("utf8");
    passwords.set(user, value);
    heldSecrets.push(value);
  }
  return passwords;
}

function gitInfo() {
  const git = args => spawnSync("git", ["-C", REPO, ...args], { encoding: "utf8" }).stdout.trim();
  return { head: git(["rev-parse", "HEAD"]), dirty_files: git(["status", "--porcelain"]).split("\n").filter(Boolean).length };
}

function pinnedInputs() {
  const pinned = { recorded_at: nowIso(), harness: { ...gitInfo(), node: process.version } };
  for (const file of [fileURLToPath(import.meta.url), PROBE_PY, PROBE_MJS]) pinned.harness[relative(REPO, file)] = sha256(readFileSync(file));
  const version = kubeJson(["version"]);
  pinned.kubernetes = { server: version?.serverVersion?.gitVersion ?? null };

  const apps = {};
  for (const name of ["pharness-campus", "campus-lab", "campus-idp"]) {
    const app = kubeJson(["-n", "argocd", "get", "applications.argoproj.io", name]);
    apps[name] = app && { sync: app.status?.sync?.status, health: app.status?.health?.status, revision: app.status?.sync?.revision, repo: app.spec?.source?.repoURL, path: app.spec?.source?.path, target_revision: app.spec?.source?.targetRevision };
  }
  pinned.argo = apps;

  const deployment = kubeJson(["-n", "campus-poc", "get", "deployment", "pharness-campus-api"]);
  const container = deployment?.spec?.template?.spec?.containers?.find(item => item.name === "api");
  const env = Object.fromEntries((container?.env ?? []).filter(item => "value" in item).map(item => [item.name, item.value]));
  pinned.api = {
    image: container?.image ?? null,
    release_revision: env.PHARNESS_BUILD_REVISION ?? null,
    public_origin: env.PHARNESS_CAMPUS_PUBLIC_ORIGIN ?? null,
    lab_provider: env.PHARNESS_CAMPUS_LAB_PROVIDER ?? null,
    terminal: env.PHARNESS_CAMPUS_TERMINAL ?? null,
    pods: (kubeJson(["-n", "campus-poc", "get", "pods", "-l", "app=pharness-campus-api"])?.items ?? []).map(pod => ({ name: pod.metadata.name, node: pod.spec.nodeName, phase: pod.status.phase, started: pod.status.startTime, images: podImages(pod) })),
    service_cluster_ip: kubeJson(["-n", "campus-poc", "get", "service", "pharness-campus-api"])?.spec?.clusterIP ?? null,
  };
  const uiDeployment = kubeJson(["-n", "campus-poc", "get", "deployment", "pharness-campus-ui"]);
  pinned.ui = {
    image: uiDeployment?.spec?.template?.spec?.containers?.[0]?.image ?? null,
    pods: (kubeJson(["-n", "campus-poc", "get", "pods", "-l", "app=pharness-campus-ui"])?.items ?? []).map(pod => ({ name: pod.metadata.name, images: podImages(pod) })),
  };

  const config = kubeJson(["-n", "campus-poc", "get", "configmap", "pharness-campus-config"]);
  pinned.configuration = {};
  for (const key of ["campus-labs.json", "lab-provider.json", "terminal.json"]) {
    const deployed = config?.data?.[key] ?? null;
    const checkout = join(REPO, "deploy/helm/pharness-campus/files", key);
    const local = existsSync(checkout) ? readFileSync(checkout, "utf8") : null;
    const normalize = text => (text === null ? null : JSON.stringify(JSON.parse(text)));
    pinned.configuration[key] = { sha256: deployed && sha256(deployed), canonical_sha256: deployed && sha256(normalize(deployed)), matches_checkout: deployed !== null && local !== null && normalize(deployed) === normalize(local) };
  }
  if (config?.data?.["campus-labs.json"]) {
    const catalog = JSON.parse(config.data["campus-labs.json"]);
    pinned.catalog = {
      assignments: catalog.assignments.map(item => ({ course_id: item.course_id, assignment_id: item.assignment_id, profile_id: item.profile_id, assignment_layer_sha256: item.assignment_layer ? sha256(JSON.stringify(item.assignment_layer)) : null })),
      profiles: catalog.profiles.map(profile => ({ id: profile.id, version: profile.version, image: profile.image, sha256: sha256(JSON.stringify(profile)) })),
      policy_layers: catalog.policy_layers.map(layer => ({ authority: layer.authority, id: layer.id, version: layer.version, sha256: sha256(JSON.stringify(layer)) })),
    };
  }

  pinned.admission = {};
  for (const name of ["pharness-campus-lab-session-guard", "pharness-campus-lab-secret-guard", "pharness-campus-lab-image-allowlist"]) {
    const policy = kubeJson(["get", "validatingadmissionpolicies.admissionregistration.k8s.io", name]);
    pinned.admission[name] = policy && { spec_sha256: sha256(JSON.stringify(policy.spec)), resource_version: policy.metadata.resourceVersion, generation: policy.metadata.generation };
  }
  pinned.slots = {};
  for (const slot of SLOTS) {
    const policies = kubeJson(["-n", slot, "get", "networkpolicies"])?.items ?? [];
    const namespace = kubeJson(["get", "namespace", slot]);
    pinned.slots[slot] = {
      slot_label: namespace?.metadata?.labels?.["pharness.dev/campus-slot"] ?? null,
      network_policies: Object.fromEntries(policies.map(policy => [policy.metadata.name, sha256(JSON.stringify(policy.spec))])),
      controller_objects_at_start: listOwned(slot, null),
    };
  }
  const runtime = kubeJson(["get", "runtimeclass", "gvisor"]);
  pinned.runtime_class = runtime && { handler: runtime.handler, node_selector: runtime.scheduling?.nodeSelector ?? null };
  pinned.egress_proxy = (kubeJson(["-n", "campus-egress", "get", "pods"])?.items ?? []).map(pod => ({ name: pod.metadata.name, images: podImages(pod) }));
  pinned.idp = (kubeJson(["-n", "campus-idp", "get", "pods"])?.items ?? []).map(pod => ({ name: pod.metadata.name, images: podImages(pod) }));
  return pinned;
}

/**
 * Controller-owned objects in a slot (Jobs, Pods, Secrets, ConfigMaps), for
 * one session or all. Only names and labels are printed; Secret data never
 * reaches this process.
 */
function listOwned(slot, sessionId) {
  const selector = sessionId ? `${LABEL_SESSION}=${sessionId}` : "pharness.dev/campus-controller";
  const template = '{range .items[*]}{.kind}{"\\t"}{.metadata.name}{"\\t"}{.metadata.labels.pharness\\.dev/campus-record}{"\\t"}{.metadata.labels.pharness\\.dev/campus-session}{"\\t"}{.metadata.deletionTimestamp}{"\\n"}{end}';
  const result = kube(["-n", slot, "get", "jobs.batch,pods,secrets,configmaps", "-l", selector, "-o", `jsonpath=${template}`]);
  if (result.status !== 0) return { error: result.stderr };
  return result.stdout.split("\n").filter(Boolean).map(line => {
    const [kind, name, record, session, deleting] = line.split("\t");
    return { kind, name, record: record || null, session: session || null, deleting: deleting || null };
  });
}

/** Everything but the occupancy's tombstone counts as a live resource. */
const liveResources = objects => (Array.isArray(objects) ? objects.filter(item => !(item.kind === "ConfigMap" && item.record === "tombstone")) : objects);

async function findSessionSlot(sessionId, timeoutMs = 120_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    for (const slot of SLOTS) {
      const objects = listOwned(slot, sessionId);
      if (Array.isArray(objects) && objects.some(item => item.kind === "Pod")) return slot;
    }
    await sleep(1000);
  }
  return null;
}

function describeWorkload(slot, sessionId) {
  const pods = kubeJson(["-n", slot, "get", "pods", "-l", `${LABEL_SESSION}=${sessionId}`])?.items ?? [];
  const jobs = kubeJson(["-n", slot, "get", "jobs.batch", "-l", `${LABEL_SESSION}=${sessionId}`])?.items ?? [];
  const pod = pods[0];
  const job = jobs[0];
  const spec = pod?.spec ?? {};
  const session = spec.containers?.find(item => item.name === "session");
  return {
    slot,
    job: job && { name: job.metadata.name, active_deadline_seconds: job.spec.activeDeadlineSeconds, backoff_limit: job.spec.backoffLimit, annotations: job.metadata.annotations, labels: job.metadata.labels },
    pod: pod && {
      name: pod.metadata.name,
      node: spec.nodeName,
      phase: pod.status?.phase,
      runtime_class: spec.runtimeClassName,
      service_account: spec.serviceAccountName ?? null,
      automount_service_account_token: spec.automountServiceAccountToken,
      host_network: spec.hostNetwork ?? false,
      share_process_namespace: spec.shareProcessNamespace ?? false,
      containers: spec.containers?.map(item => item.name),
      init_containers: spec.initContainers?.map(item => item.name),
      env: (session?.env ?? []).map(item => ({ name: item.name, literal: "value" in item, value: "value" in item ? item.value : null })),
      env_from: session?.envFrom ?? [],
      volumes: (spec.volumes ?? []).map(volume => ({ name: volume.name, kind: Object.keys(volume).filter(key => key !== "name")[0] })),
      read_only_root: session?.securityContext?.readOnlyRootFilesystem ?? null,
      labels: pod.metadata.labels,
      annotations: pod.metadata.annotations,
      images: podImages(pod),
    },
  };
}

/** The broker poller, run through cluster-admin kubectl exec in the session container. */
function startPoller(slot, pod, runner, until, fastAfter) {
  const script = readFileSync(runner === "node" ? PROBE_MJS : PROBE_PY);
  const command = runner === "node" ? ["node", "--input-type=module", "-", "poll"] : ["python3", "-", "poll"];
  const args = [`until=${until}`, `fast_after=${fastAfter}`];
  commandLog.push(`${nowIso()} kubectl --context ${OPTIONS.context} -n ${slot} exec -i ${pod} -c session -- ${[...command, ...args].join(" ")}`);
  const child = spawn("kubectl", ["--context", OPTIONS.context, "-n", slot, "exec", "-i", pod, "-c", "session", "--", ...command, ...args], { stdio: ["pipe", "pipe", "pipe"] });
  children.add(child);
  const poller = { lines: [], stderr: "", exited: null, waiters: [] };
  let buffer = "";
  child.stdout.on("data", chunk => {
    buffer += chunk.toString();
    let at;
    while ((at = buffer.indexOf("\n")) >= 0) {
      const line = buffer.slice(0, at).trim();
      buffer = buffer.slice(at + 1);
      const parsed = parsePollLine(line);
      if (!parsed) continue;
      const entry = { ...parsed, received: Date.now() };
      poller.lines.push(entry);
      poller.waiters = poller.waiters.filter(waiter => !(waiter.match(entry) && (waiter.resolve(entry), true)));
    }
  });
  child.stderr.on("data", chunk => { poller.stderr += chunk.toString(); });
  child.on("exit", status => {
    poller.exited = status;
    children.delete(child);
    for (const waiter of poller.waiters) waiter.resolve(null);
    poller.waiters = [];
  });
  child.stdin.end(script);
  poller.waitFor = (match, timeoutMs) => {
    const found = poller.lines.find(match);
    if (found) return Promise.resolve(found);
    if (poller.exited !== null) return Promise.resolve(null);
    return new Promise(resolvePromise => {
      const waiter = { match, resolve: resolvePromise };
      poller.waiters.push(waiter);
      setTimeout(() => {
        poller.waiters = poller.waiters.filter(item => item !== waiter);
        resolvePromise(null);
      }, timeoutMs);
    });
  };
  poller.stop = () => child.kill("SIGTERM");
  return poller;
}

function startPortForward(namespace, target, remotePort) {
  return new Promise((resolvePromise, reject) => {
    const child = spawn("kubectl", ["--context", OPTIONS.context, "-n", namespace, "port-forward", "--address", "127.0.0.1", target, `:${remotePort}`], { stdio: ["ignore", "pipe", "pipe"] });
    children.add(child);
    let output = "";
    const timer = setTimeout(() => reject(new Error(`port-forward to ${namespace}/${target} did not start: ${output}`)), 20_000);
    const onData = chunk => {
      output += chunk.toString();
      const match = output.match(/Forwarding from 127\.0\.0\.1:(\d+)/);
      if (match) {
        clearTimeout(timer);
        resolvePromise({ port: Number(match[1]), child });
      }
    };
    child.stdout.on("data", onData);
    child.stderr.on("data", onData);
    child.on("exit", status => {
      children.delete(child);
      clearTimeout(timer);
      reject(new Error(`port-forward exited ${status}: ${output}`));
    });
  });
}

// ---------------------------------------------------------------- browser

let browser;
let connectAddress;
const VIEWPORT = { width: 1440, height: 960 };

async function screenshot(page, name) {
  const file = join(OUT, "screenshots", `${String(evidence.screenshots.length + 1).padStart(2, "0")}-${name}.png`);
  mkdirSync(dirname(file), { recursive: true });
  await page.screenshot({ path: file, fullPage: true });
  evidence.screenshots.push({ name, file: relative(OUT, file), at: nowIso() });
}

/** A same-origin Campus API call from the signed-in page; CSRF for mutations. */
async function api(page, method, path, body) {
  return page.evaluate(async ({ method, path, body }) => {
    const headers = { accept: "application/json" };
    if (method !== "GET") {
      const me = await fetch("/api/campus/me");
      if (me.ok) headers["x-campus-csrf"] = (await me.json()).csrf_token;
      headers["content-type"] = "application/json";
    }
    const response = await fetch(path, { method, headers, body: method === "GET" ? undefined : JSON.stringify(body ?? {}) });
    let data = null;
    try { data = await response.json(); } catch {}
    return { status: response.status, body: data };
  }, { method, path, body });
}

function capture(page) {
  const transcript = { chunks: [], control: [], sockets: 0 };
  page.on("websocket", socket => {
    if (!socket.url().includes("/terminal")) return;
    transcript.sockets += 1;
    socket.on("framereceived", ({ payload }) => {
      if (typeof payload === "string") {
        try { transcript.control.push({ at: Date.now(), ...JSON.parse(payload) }); } catch {}
      } else {
        transcript.chunks.push(Buffer.from(payload));
      }
    });
  });
  transcript.text = () => stripAnsi(Buffer.concat(transcript.chunks).toString("utf8"));
  return transcript;
}

async function waitForText(transcript, pattern, from, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const text = transcript.text().slice(from);
    const match = text.match(pattern);
    if (match) return match;
    await sleep(200);
  }
  return null;
}

async function typeCommand(page, transcript, command) {
  const from = transcript.text().length;
  await page.locator(".xterm-helper-textarea").focus();
  await page.keyboard.insertText(command);
  await page.keyboard.press("Enter");
  return from;
}

async function signIn(user, passwords) {
  const context = await browser.newContext({ viewport: VIEWPORT });
  const page = await context.newPage();
  const transcript = capture(page);
  const started = Date.now();
  await page.goto(`${OPTIONS.baseUrl}/api/campus/auth/login`);
  const idpHost = new URL(OPTIONS.idpUrl).host;
  await page.waitForURL(url => url.host === idpHost && url.pathname.startsWith("/realms/campus/"), { timeout: 30_000 });
  await page.locator('input[name="username"]').fill(user);
  await page.locator('input[name="password"]').fill(passwords.get(user));
  const back = page.waitForURL(url => url.origin === OPTIONS.baseUrl && url.pathname === "/campus.html", { timeout: 30_000 });
  await page.locator('#kc-login, button[type="submit"], input[type="submit"]').first().click();
  await back;
  const ms = Date.now() - started;
  const me = await api(page, "GET", "/api/campus/me");
  const identity = me.body?.identity ?? {};
  const cookies = await context.cookies(OPTIONS.baseUrl);
  for (const cookie of cookies) heldSecrets.push(cookie.value);
  evidence.identities[user] = { issuer: identity.issuer ?? null, subject: identity.subject ?? null, email: identity.email ?? null, login_ms: ms };
  check("login", `${user} signs in through Keycloak`, me.status === 200 && Boolean(identity.subject), { status: me.status, subject: identity.subject, ms });
  timing(`login.${user}`, ms);
  return { user, context, page, transcript, identity };
}

async function sessionView(actor, sessionId) {
  return api(actor.page, "GET", `/api/campus/lab-sessions/${sessionId}`);
}

async function waitForStatus(actor, sessionId, wanted, timeoutMs, history) {
  const deadline = Date.now() + timeoutMs;
  let last = null;
  while (Date.now() < deadline) {
    const view = await sessionView(actor, sessionId);
    const session = view.body?.session;
    const status = session?.status ?? `http_${view.status}`;
    if (status !== last) {
      history?.push({ at: nowIso(), status, stop_requested: session?.stop_requested ?? null, error: session?.error ?? null });
      last = status;
    }
    if (wanted.includes(status)) return { session, at: Date.now() };
    if (!LIVE.has(status) && !wanted.includes(status) && session) return { session, at: Date.now(), unexpected: true };
    await sleep(500);
  }
  return null;
}

async function reclaim(actor) {
  const mine = await api(actor.page, "GET", "/api/campus/lab-sessions");
  for (const session of mine.body?.sessions ?? []) {
    if (!LIVE.has(session.status)) continue;
    console.log(`reclaiming live session ${session.session_id} of ${actor.user} (${session.status})`);
    if (!session.stop_requested) await api(actor.page, "POST", `/api/campus/lab-sessions/${session.session_id}/stop`, {});
    const done = await waitForStatus(actor, session.session_id, ["stopped", "failed"], 300_000);
    check("setup", `${actor.user}'s earlier session ${session.session_id} ends before the demo`, done && !done.unexpected, done?.session?.status ?? "timeout");
  }
}

async function launch(actor, assignment, phase) {
  const key = `luc137-${evidence.run_id}-${phase}`;
  const started = Date.now();
  const launched = await api(actor.page, "POST", `/api/campus/courses/${OPTIONS.course}/assignments/${assignment}/launch`, { idempotency_key: key });
  const session = launched.body?.session;
  check(phase, `${actor.user} launches ${assignment}`, launched.status === 201 && launched.body?.admission === "created", { status: launched.status, admission: launched.body?.admission, code: launched.body?.code, session_id: session?.session_id });
  if (!session) throw new Error(`launch of ${assignment} failed: ${JSON.stringify(launched.body)}`);
  const record = { owner: actor.user, assignment, session_id: session.session_id, launched_at: new Date(started).toISOString(), expires_at: session.expires_at, statuses: [] };
  evidence.sessions[phase] = record;
  return { session, started, record };
}

async function openTerminal(actor, sessionId, phase) {
  await actor.page.goto(`${OPTIONS.baseUrl}/campus.html?session=${encodeURIComponent(sessionId)}`);
  const status = actor.page.getByRole("status");
  await status.filter({ hasText: /Your lab is ready/ }).waitFor({ timeout: 30_000 });
  await screenshot(actor.page, `${phase}-ready`);
  const started = Date.now();
  await actor.page.getByRole("button", { name: "Connect", exact: true }).click();
  await status.filter({ hasText: /Terminal connected/ }).waitFor({ timeout: 30_000 });
  const ms = Date.now() - started;
  check(phase, "the browser terminal connects", true, { ms });
  timing(`${phase}.terminal_connect_ms`, ms);
  await waitForText(actor.transcript, /[$#] $/m, 0, 10_000);
  return status;
}

async function runAgentCli(actor, phase) {
  const from = await typeCommand(actor.page, actor.transcript, "codex --version");
  const match = await waitForText(actor.transcript, /codex-cli \S+/, from, 30_000);
  check(phase, "the agent CLI answers in the terminal (codex --version)", Boolean(match), match?.[0] ?? "no answer");
  await screenshot(actor.page, `${phase}-agent-cli`);
}

async function runProbe(actor, phase, probe, sessionId) {
  const file = probe === "coding" ? PROBE_MJS : PROBE_PY;
  const target = probe === "coding" ? "/tmp/campus-m1-demo-probe.mjs" : "/tmp/campus-m1-demo-probe.py";
  const interpreter = probe === "coding" ? "node" : "python3";
  const encoded = readFileSync(file).toString("base64");
  const apiArg = evidence.pinned.api?.service_cluster_ip ? ` api=${evidence.pinned.api.service_cluster_ip}:4777` : "";
  // Short lines stay under the 4096-byte line limit of a terminal in canonical mode.
  await typeCommand(actor.page, actor.transcript, `: > ${target}.b64`);
  for (let at = 0; at < encoded.length; at += 3000) {
    await typeCommand(actor.page, actor.transcript, `printf %s '${encoded.slice(at, at + 3000)}' >> ${target}.b64`);
    await sleep(300);
  }
  const from = await typeCommand(actor.page, actor.transcript, `base64 -d ${target}.b64 > ${target} && clear && ${interpreter} ${target} ${probe} session=${sessionId}${apiArg}`);
  const done = await waitForText(actor.transcript, new RegExp(`DEMO-RESULT ${probe} passed=\\d+ failed=\\d+`), from, 240_000);
  const parsed = done ? parseProbe(actor.transcript.text(), probe) : null;
  if (!parsed) {
    check(phase, `the ${probe} probe finished in the terminal`, false, "no result marker");
    return;
  }
  for (const item of parsed.checks) check(phase, `in the sandbox: ${item.check}`, item.ok, item.detail);
  check(phase, `the ${probe} probe reports no failure`, parsed.failed === 0 && parsed.checks.length === parsed.passed, { passed: parsed.passed, failed: parsed.failed });
  await screenshot(actor.page, `${phase}-probe`);
}

/** Refusals at the API, terminal ticket and WebSocket handshake for anyone but the owner. */
async function peerChecks(owner, peer, instructor, sessionId, phase) {
  const base = `/api/campus/lab-sessions/${sessionId}`;
  const expect = async (actor, method, path, status, code, label) => {
    const answer = await api(actor.page, method, path, method === "GET" ? undefined : {});
    check(phase, label, answer.status === status && (code === null || answer.body?.code === code), { status: answer.status, code: answer.body?.code ?? null });
  };
  await expect(peer, "GET", base, 404, "session_not_found", `${peer.user} cannot see ${owner.user}'s session`);
  await expect(peer, "POST", `${base}/terminal-ticket`, 404, "session_not_found", `${peer.user} cannot get a terminal ticket for it`);
  await expect(peer, "POST", `${base}/stop`, 404, "session_not_found", `${peer.user} cannot stop it`);
  await expect(peer, "POST", `${base}/reset`, 404, "session_not_found", `${peer.user} cannot reset it`);
  await expect(peer, "GET", `/api/campus/courses/${OPTIONS.course}/lab-sessions`, 403, null, `${peer.user} cannot list the course's sessions`);
  await expect(instructor, "POST", `${base}/terminal-ticket`, 404, "session_not_found", `course staff (${instructor.user}) cannot open a student's terminal`);

  const wsUrl = `wss://${new URL(OPTIONS.baseUrl).host}${base}/terminal`;
  const socketAttempt = page => page.evaluate(({ url, protocol }) => new Promise(resolvePromise => {
    const socket = new WebSocket(url, protocol);
    let opened = false;
    const timer = setTimeout(() => resolvePromise({ opened, code: "timeout" }), 15_000);
    socket.onopen = () => { opened = true; socket.close(1000); };
    socket.onclose = event => { clearTimeout(timer); resolvePromise({ opened, code: event.code }); };
  }), { url: wsUrl, protocol: TERMINAL_PROTOCOL });
  const peerSocket = await socketAttempt(peer.page);
  check(phase, `${peer.user}'s browser cannot open ${owner.user}'s terminal WebSocket`, !peerSocket.opened, peerSocket);

  const anonymous = await browser.newContext({ viewport: VIEWPORT });
  const anonymousPage = await anonymous.newPage();
  await anonymousPage.goto(`${OPTIONS.baseUrl}/campus.html`);
  const anonymousView = await api(anonymousPage, "GET", base);
  check(phase, "an unauthenticated browser cannot read the session", anonymousView.status === 401, { status: anonymousView.status });
  const anonymousSocket = await socketAttempt(anonymousPage);
  check(phase, "an unauthenticated browser cannot open the terminal WebSocket", !anonymousSocket.opened, anonymousSocket);
  await anonymous.close();

  // Positive control first: the same raw handshake with the public Origin is upgraded.
  const cookie = (await owner.context.cookies(OPTIONS.baseUrl)).find(item => item.name === SESSION_COOKIE);
  const own = await rawUpgrade(`${base}/terminal`, OPTIONS.baseUrl, cookie?.value);
  const foreign = await rawUpgrade(`${base}/terminal`, "https://evil.example", cookie?.value);
  check(phase, "the owner's handshake with the Campus Origin is upgraded (control)", own === 101, { status: own });
  check(phase, "the owner's cookie from a foreign Origin is refused before the upgrade", foreign !== 101, { status: foreign });

  // A ticket works once: the second redemption closes with ticket_rejected.
  const second = await owner.context.newPage();
  await second.goto(`${OPTIONS.baseUrl}/campus.html`);
  const replay = await second.evaluate(async ({ sessionId, url, protocol }) => {
    const me = await (await fetch("/api/campus/me")).json();
    const issued = await fetch(`/api/campus/lab-sessions/${sessionId}/terminal-ticket`, {
      method: "POST",
      headers: { "content-type": "application/json", "x-campus-csrf": me.csrf_token },
      body: "{}",
    });
    const ticket = await issued.json();
    const attempt = () => new Promise(resolvePromise => {
      const socket = new WebSocket(url, protocol);
      const seen = [];
      const timer = setTimeout(() => { socket.close(); resolvePromise({ seen, code: "timeout" }); }, 15_000);
      socket.onopen = () => socket.send(JSON.stringify({ type: "auth", ticket: ticket.ticket, cols: 80, rows: 24 }));
      socket.onmessage = event => {
        if (typeof event.data !== "string") return;
        const message = JSON.parse(event.data);
        seen.push(message.reason ? `${message.type}:${message.reason}` : message.type);
        if (message.type === "ready") socket.close(1000);
      };
      socket.onclose = event => { clearTimeout(timer); resolvePromise({ seen, code: event.code }); };
    });
    const first = await attempt();
    const again = await attempt();
    return { issued: issued.status, protocol: ticket.protocol, first, again };
  }, { sessionId, url: wsUrl, protocol: TERMINAL_PROTOCOL });
  await second.close();
  check(phase, "a terminal ticket opens the terminal once", replay.issued === 201 && replay.protocol === TERMINAL_PROTOCOL && replay.first.seen.includes("ready"), replay.first);
  check(phase, "the same ticket is refused on reuse (4001 ticket_rejected)", replay.again.seen.includes("closed:ticket_rejected") && replay.again.code === 4001, replay.again);

  await peer.page.goto(`${OPTIONS.baseUrl}/campus.html?session=${encodeURIComponent(sessionId)}`);
  await peer.page.getByRole("status").filter({ hasText: /not found/ }).waitFor({ timeout: 15_000 }).catch(() => {});
  check(phase, `${peer.user}'s Campus page shows the session as not found`, /not found/.test(await peer.page.getByRole("status").innerText()), await peer.page.getByRole("status").innerText());
  await screenshot(peer.page, `${phase}-peer-refused`);
}

/** The status code of a raw WebSocket handshake, without a browser. */
function rawUpgrade(path, origin, cookie) {
  const host = new URL(OPTIONS.baseUrl).host;
  return new Promise(resolvePromise => {
    const request = https.request({
      host: connectAddress?.host ?? host,
      port: connectAddress?.port ?? 443,
      servername: host,
      path,
      method: "GET",
      headers: {
        host,
        origin,
        cookie: cookie ? `${SESSION_COOKIE}=${cookie}` : "",
        connection: "Upgrade",
        upgrade: "websocket",
        "sec-websocket-version": "13",
        "sec-websocket-key": randomBytes(16).toString("base64"),
        "sec-websocket-protocol": TERMINAL_PROTOCOL,
      },
      timeout: 15_000,
    });
    request.on("upgrade", (response, socket) => { socket.destroy(); resolvePromise(response.statusCode); });
    request.on("response", response => { response.resume(); resolvePromise(response.statusCode); });
    request.on("timeout", () => { request.destroy(); resolvePromise("timeout"); });
    request.on("error", error => resolvePromise(`error ${error.code ?? error.message}`));
    request.end();
  });
}

async function waitForRemoval(slot, sessionId, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  let objects;
  while (Date.now() < deadline) {
    objects = listOwned(slot, sessionId);
    if (Array.isArray(objects) && liveResources(objects).length === 0) return { at: Date.now(), remaining: objects };
    await sleep(1000);
  }
  return { at: null, remaining: objects };
}

/** Revocation, terminal close, status and removal after a stop or an expiry. */
async function verifyEnd(actor, phase, record, workload, poller, origin, closeText, expected) {
  const [closed, refused, stopped, removed] = await Promise.all([
    actor.page.getByRole("status").filter({ hasText: closeText }).waitFor({ timeout: REMOVAL_LIMIT_MS }).then(() => Date.now(), () => null),
    poller.waitFor(line => line.kind !== "POLL", REMOVAL_LIMIT_MS),
    waitForStatus(actor, record.session_id, ["stopped"], REMOVAL_LIMIT_MS, record.statuses),
    waitForRemoval(workload.slot, record.session_id, REMOVAL_LIMIT_MS),
  ]);
  const label = expected.label;
  check(phase, `${label}: the open terminal closes (${expected.reason})`, closed !== null && actor.transcript.control.some(item => item.type === "closed" && item.reason === expected.reason), actor.transcript.control.filter(item => item.type === "closed").map(item => item.reason));
  if (closed !== null) timing(`${phase}.${expected.prefix}_terminal_closed_ms`, closed - origin);
  check(phase, `${label}: the broker refuses the session's token`, refused?.kind === "POLL-REFUSED" && expected.codes.includes(refused.code), refused);
  if (refused) {
    timing(`${phase}.${expected.prefix}_token_refused_ms`, refused.received - origin);
    timing(`${phase}.${expected.prefix}_token_refused_pod_clock_ms`, refused.at - origin);
    check(phase, `${label}: revocation within ${REVOKE_LIMIT_MS / 1000} s`, refused.received - origin <= REVOKE_LIMIT_MS, `${refused.received - origin} ms`);
  }
  check(phase, `${label}: the session reaches stopped`, stopped && !stopped.unexpected, stopped?.session?.status ?? "timeout");
  if (stopped) timing(`${phase}.${expected.prefix}_status_stopped_ms`, stopped.at - origin);
  check(phase, `${label}: the Job, Pod, token Secret and anchor are gone from ${workload.slot}`, removed.at !== null, removed.remaining);
  if (removed.at) {
    timing(`${phase}.${expected.prefix}_resources_gone_ms`, removed.at - origin);
    check(phase, `${label}: removal within ${REMOVAL_LIMIT_MS / 1000} s`, removed.at - origin <= REMOVAL_LIMIT_MS, `${removed.at - origin} ms`);
  }
  record.remaining_after_end = removed.remaining;
  record.ended = { origin: new Date(origin).toISOString(), final: stopped?.session ?? null, poller: poller.lines.slice(-5), poller_exit: poller.exited };
  const ticket = await api(actor.page, "POST", `/api/campus/lab-sessions/${record.session_id}/terminal-ticket`, {});
  check(phase, `${label}: a new terminal ticket is refused`, ticket.status === 409 && expected.ticketCodes.includes(ticket.body?.code), { status: ticket.status, code: ticket.body?.code });
  await screenshot(actor.page, `${phase}-ended`);
  poller.stop();
}

async function labPhase(owner, peer, instructor, phase, assignment, probe, runner) {
  console.log(`== ${phase}: ${owner.user} runs ${assignment}`);
  const { session, started, record } = await launch(owner, assignment, phase);
  const ready = await waitForStatus(owner, session.session_id, ["ready"], 300_000, record.statuses);
  check(phase, "the session reaches ready", ready && !ready.unexpected, ready?.session?.status ?? "timeout");
  if (!ready || ready.unexpected) throw new Error(`${phase} session did not become ready`);
  timing(`${phase}.launch_to_ready_ms`, ready.at - started);

  const again = await api(owner.page, "POST", `/api/campus/courses/${OPTIONS.course}/assignments/${OPTIONS.research === assignment ? OPTIONS.coding : OPTIONS.research}/launch`, {});
  check(phase, "a second live session for the same student is refused (409 active_session_exists)", again.status === 409 && again.body?.code === "active_session_exists" && again.body?.session_id === session.session_id, { status: again.status, code: again.body?.code });

  const slot = await findSessionSlot(session.session_id);
  check(phase, "kubectl finds the session's Pod in exactly one lab slot", Boolean(slot), slot);
  if (!slot) throw new Error(`${phase}: no slot holds the session`);
  const workload = describeWorkload(slot, session.session_id);
  record.workload = workload;
  const pod = workload.pod;
  check(phase, "the Pod runs under gVisor with no service-account token and literal environment only",
    pod?.runtime_class === "gvisor" && pod.automount_service_account_token === false && pod.env.every(item => item.literal) && pod.env_from.length === 0 && pod.host_network === false && pod.share_process_namespace === false && pod.read_only_root === true,
    { runtime_class: pod?.runtime_class, automount: pod?.automount_service_account_token, env: pod?.env?.map(item => item.name), volumes: pod?.volumes });
  check(phase, "the Pod's images are pinned by digest", pod?.images?.every(item => item.digest), pod?.images?.map(item => `${item.container} ${item.digest}`));
  const profileId = evidence.assignments.find?.(item => item.assignment_id === assignment)?.profile?.id;
  const profileImage = evidence.pinned.catalog?.profiles?.find(item => item.id === profileId)?.image ?? null;
  const sessionDigest = pod?.images?.find(image => image.container === "session")?.digest ?? null;
  check(phase, "the session image is the catalog profile's digest", Boolean(profileImage && sessionDigest && profileImage.endsWith(`@${sessionDigest}`)), { profile: profileId, catalog: profileImage, running: sessionDigest });
  check(phase, "the Job's deadline is at most the session lifetime", workload.job && workload.job.active_deadline_seconds <= session.expires_at - Math.floor(started / 1000) + 5, { active_deadline_seconds: workload.job?.active_deadline_seconds, expires_at: session.expires_at });

  const listing = await api(instructor.page, "GET", `/api/campus/courses/${OPTIONS.course}/lab-sessions`);
  const listed = listing.body?.sessions?.find(item => item.session_id === session.session_id);
  check(phase, `${instructor.user} sees the live session and its owner in the course view`, listing.status === 200 && listed?.owner?.subject === owner.identity.subject, { status: listing.status, status_listed: listed?.status, owner: listed?.owner?.subject });

  await openTerminal(owner, session.session_id, phase);
  await runAgentCli(owner, phase);
  await runProbe(owner, phase, probe, session.session_id);

  const poller = startPoller(slot, pod.name, runner, session.expires_at + 120, 0);
  const live = await poller.waitFor(() => true, 30_000);
  check(phase, "independently (kubectl exec): the broker accepts the session's token while it runs", live?.kind === "POLL" && live.status === "200", live ?? poller.stderr.slice(0, 300));

  if (peer) await peerChecks(owner, peer, instructor, session.session_id, phase);

  console.log(`== ${phase}: ${owner.user} stops the lab from the Campus page`);
  const stopAt = Date.now();
  await owner.page.getByRole("button", { name: "Stop lab" }).click();
  await verifyEnd(owner, phase, record, workload, poller, stopAt, /The lab was stopped/, {
    label: "after Stop lab",
    prefix: "stop_to",
    reason: "session_stopped",
    codes: ["token_revoked", "session_inactive"],
    ticketCodes: ["session_stopped"],
  });
  record.transcript = save(`transcripts/${phase}.txt`, owner.transcript.text());
  owner.transcript.chunks = [];
  owner.transcript.control = [];
  return record;
}

async function startTimeoutSession(student) {
  const phase = "timeout";
  console.log(`== ${phase}: ${student.user} launches ${OPTIONS.timeout}`);
  const { session, started, record } = await launch(student, OPTIONS.timeout, phase);
  const lifetime = session.expires_at - Math.floor(started / 1000);
  check(phase, "the session's absolute expiry is fixed at admission (300 s)", lifetime >= 295 && lifetime <= 305, { expires_at: session.expires_at, lifetime_s: lifetime });
  const ready = await waitForStatus(student, session.session_id, ["ready"], 240_000, record.statuses);
  check(phase, "the session reaches ready", ready && !ready.unexpected, ready?.session?.status ?? "timeout");
  if (!ready || ready.unexpected) throw new Error("timeout session did not become ready");
  timing(`${phase}.launch_to_ready_ms`, ready.at - started);
  const slot = await findSessionSlot(session.session_id);
  if (!slot) throw new Error("timeout: no slot holds the session");
  const workload = describeWorkload(slot, session.session_id);
  record.workload = workload;
  check(phase, "the Job's activeDeadlineSeconds is the remaining lifetime", workload.job && workload.job.active_deadline_seconds <= lifetime, workload.job?.active_deadline_seconds);
  await openTerminal(student, session.session_id, phase);
  await runAgentCli(student, phase);
  const poller = startPoller(slot, workload.pod.name, "python", session.expires_at + 180, session.expires_at - 20);
  const live = await poller.waitFor(() => true, 30_000);
  check(phase, "independently (kubectl exec): the broker accepts the token before expiry", live?.kind === "POLL" && live.status === "200", live ?? poller.stderr.slice(0, 300));
  return { record, workload, poller, session };
}

async function finishTimeoutSession(student, pending) {
  const phase = "timeout";
  const { record, workload, poller, session } = pending;
  const expiry = session.expires_at * 1000;
  console.log(`== ${phase}: the absolute expiry is in ${Math.max(0, Math.round((expiry - Date.now()) / 1000))} s`);
  await sleep(Math.max(0, expiry - 10_000 - Date.now()));
  const statusText = await student.page.getByRole("status").innerText();
  check(phase, "the terminal is still open 10 s before expiry", Date.now() < expiry && /Terminal connected/.test(statusText), statusText);
  await verifyEnd(student, phase, record, workload, poller, expiry, /reached its time limit/, {
    label: "at the absolute expiry",
    prefix: "expiry_to",
    reason: "session_expired",
    codes: ["session_expired", "token_revoked", "token_expired", "session_inactive"],
    ticketCodes: ["session_expired", "session_stopped"],
  });
  record.transcript = save(`transcripts/${phase}.txt`, student.transcript.text());
}

async function stopLeftovers(actors) {
  for (const [phase, record] of Object.entries(evidence.sessions)) {
    const actor = actors.get(record.owner);
    if (!actor) continue;
    try {
      const view = await sessionView(actor, record.session_id);
      if (!LIVE.has(view.body?.session?.status)) continue;
      console.log(`cleanup: stopping ${phase} session ${record.session_id}`);
      await api(actor.page, "POST", `/api/campus/lab-sessions/${record.session_id}/stop`, {});
      const done = await waitForStatus(actor, record.session_id, ["stopped", "failed"], 300_000);
      record.cleanup_stop = done?.session?.status ?? "timeout";
    } catch (error) {
      evidence.errors.push(`cleanup of ${phase}: ${error.message}`);
    }
  }
}

function apiLogs(since) {
  const result = kube(["-n", "campus-poc", "logs", "deployment/pharness-campus-api", `--since-time=${since}`], { timeout: 60_000 });
  const ids = Object.values(evidence.sessions).map(record => record.session_id);
  const lines = result.stdout.split("\n").filter(line => ids.some(id => line.includes(id)));
  return stripAnsi(lines.join("\n"));
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help) {
    console.log(USAGE);
    return 0;
  }
  if (options.selfTest) return selfTest();
  OPTIONS = options;
  evidence.run_id = new Date().toISOString().replace(/[-:]/g, "").replace(/\..*/, "");
  OUT = resolve(options.out ?? `/tmp/luc137-demo-${evidence.run_id}`);
  if (!outsideRepo(OUT)) throw new Error(`the evidence directory ${OUT} must be outside ${REPO}`);
  if (existsSync(OUT) && readdirSync(OUT).length > 0) throw new Error(`the evidence directory ${OUT} is not empty`);
  mkdirSync(OUT, { recursive: true });
  evidence.started_at = nowIso();
  evidence.options = { ...options };
  console.log(`evidence: ${OUT}`);

  const actors = new Map();
  let exitCode = 1;
  try {
    console.log("== Pinned inputs and preconditions");
    evidence.pinned = pinnedInputs();
    const pinned = evidence.pinned;
    const app = pinned.argo["pharness-campus"];
    check("preflight", "Argo pharness-campus is Synced and Healthy", app?.sync === "Synced" && app?.health === "Healthy", app);
    const apiPod = pinned.api.pods[0];
    const apiDigest = pinned.api.image?.match(/sha256:[0-9a-f]{64}/)?.[0];
    check("preflight", "one Campus API Pod runs the pinned image digest", pinned.api.pods.length === 1 && apiPod?.images?.[0]?.digest === apiDigest && apiPod?.images?.[0]?.ready, { image: pinned.api.image, pods: pinned.api.pods.map(pod => pod.images.map(item => item.digest)) });
    check("preflight", "the Campus API runs the Kubernetes lab provider and the kubectl terminal", pinned.api.lab_provider === "kubernetes" && pinned.api.terminal === "kubectl", { provider: pinned.api.lab_provider, terminal: pinned.api.terminal });
    check("preflight", "the Campus API's public origin is the base URL", pinned.api.public_origin === options.baseUrl, pinned.api.public_origin);
    check("preflight", "the deployed catalog equals this checkout's chart file", pinned.configuration["campus-labs.json"]?.matches_checkout, pinned.configuration["campus-labs.json"]);
    for (const [slot, state] of Object.entries(pinned.slots)) {
      check("preflight", `${slot} is labelled as its own slot and has its NetworkPolicies`, state.slot_label === slot && state.network_policies["campus-lab-default-deny"] && state.network_policies["campus-lab-session-broker"], { label: state.slot_label, policies: Object.keys(state.network_policies) });
    }
    for (const [name, policy] of Object.entries(pinned.admission)) check("preflight", `admission policy ${name} is installed`, Boolean(policy), policy);
    save("pinned-inputs.json", pinned);

    const passwords = readPasswords();
    if (options.portForward) {
      const [namespace, rest] = options.traefik.split("/");
      const [service, port] = rest.split(":");
      const forward = await startPortForward(namespace, `svc/${service}`, Number(port));
      connectAddress = { host: "127.0.0.1", port: forward.port };
      console.log(`port-forward: ${options.traefik} on 127.0.0.1:${forward.port}`);
    }
    const { chromium } = await import("@playwright/test");
    const hosts = [new URL(options.baseUrl).hostname, new URL(options.idpUrl).hostname];
    const args = connectAddress ? [`--host-resolver-rules=${hosts.map(host => `MAP ${host} 127.0.0.1:${connectAddress.port}`).join(",")}`] : [];
    browser = await chromium.launch({ headless: !options.headed, args });
    evidence.pinned.harness.chromium = browser.version();

    console.log("== Sign-in through Keycloak");
    for (const user of USERS) actors.set(user, await signIn(user, passwords));
    passwords.clear();
    const [admin, instructor, student1, student2] = USERS.map(user => actors.get(user));
    await screenshot(student1.page, "student1-signed-in");

    console.log("== Course membership (tenant admin)");
    for (const [actor, role] of [[instructor, "instructor"], [student1, "student"], [student2, "student"]]) {
      const answer = await api(admin.page, "PUT", `/api/campus/courses/${options.course}/members`, { issuer: actor.identity.issuer, subject: actor.identity.subject, role });
      check("setup", `${admin.user} enrolls ${actor.user} in ${options.course} as ${role}`, answer.status >= 200 && answer.status < 300, { status: answer.status });
    }
    await reclaim(student1);
    await reclaim(student2);
    const assignments = await api(student1.page, "GET", `/api/campus/courses/${options.course}/assignments`);
    evidence.assignments = assignments.body?.assignments?.map(item => ({ assignment_id: item.assignment_id, profile: item.profile, session_seconds: item.session_seconds, launchable: item.launchable })) ?? assignments;
    const named = id => evidence.assignments.find?.(item => item.assignment_id === id);
    check("setup", `${options.research} is launchable`, named(options.research)?.launchable, named(options.research));
    if (!options.skipCoding) check("setup", `${options.coding} is launchable`, named(options.coding)?.launchable, named(options.coding));
    if (!options.skipTimeout) check("setup", `${options.timeout} is launchable with a 300 s lifetime`, named(options.timeout)?.launchable && named(options.timeout)?.session_seconds === 300, named(options.timeout));

    console.log("== Launch requests outside the envelope (API enforcement)");
    for (const body of [{ image: "docker.io/library/alpine:latest" }, { namespace: "kube-system" }, { resources: { cpu_millicores: 64000 } }, { expires_at: 4102444800 }, { profile_id: "campus-coding" }]) {
      const answer = await api(student1.page, "POST", `/api/campus/courses/${options.course}/assignments/${options.research}/launch`, body);
      check("envelope", `a launch naming ${Object.keys(body)[0]} is refused (400 invalid_request)`, answer.status === 400 && answer.body?.code === "invalid_request", { status: answer.status, code: answer.body?.code });
    }
    const outside = await api(student1.page, "POST", `/api/campus/courses/cs999/assignments/${options.research}/launch`, {});
    check("envelope", "a launch in a course the student is not in is refused", outside.status === 403 || outside.status === 404, { status: outside.status, code: outside.body?.code });

    await labPhase(student1, student2, instructor, "research", options.research, "research", "python");

    // The expiry watch runs alongside the coding phase, so its timings start at
    // the real expiry however long the coding phase takes.
    let timeoutWatch = null;
    if (!options.skipTimeout) {
      const pending = await startTimeoutSession(student2);
      timeoutWatch = finishTimeoutSession(student2, pending).then(() => null, error => error);
    }
    let codingError = null;
    if (!options.skipCoding) await labPhase(student1, null, instructor, "coding", options.coding, "coding", "node").catch(error => { codingError = error; });
    const timeoutError = timeoutWatch ? await timeoutWatch : null;
    if (codingError || timeoutError) throw codingError ?? timeoutError;

    console.log("== Sign-out and final state");
    await student1.page.getByRole("button", { name: "Sign out" }).click().catch(() => {});
    await student1.page.getByRole("status").filter({ hasText: /signed out/ }).waitFor({ timeout: 15_000 }).catch(() => {});
    const afterLogout = await api(student1.page, "GET", "/api/campus/me");
    check("final", "after Sign out the Campus session is gone (401)", afterLogout.status === 401, { status: afterLogout.status });
    await screenshot(student1.page, "student1-signed-out");
    for (const [phase, record] of Object.entries(evidence.sessions)) {
      const leftovers = SLOTS.flatMap(slot => (listOwned(slot, record.session_id) || []).map(item => ({ slot, ...item })));
      record.final_objects = leftovers;
      check("final", `no live resource of the ${phase} session remains in any slot`, liveResources(leftovers).length === 0, leftovers);
    }
    exitCode = 0;
  } catch (error) {
    evidence.errors.push(error.stack ?? String(error));
    console.error(`ERROR ${error.stack ?? error}`);
  } finally {
    await stopLeftovers(actors).catch(error => evidence.errors.push(String(error)));
    try {
      save("api-log.txt", apiLogs(evidence.started_at));
    } catch (error) {
      evidence.errors.push(`api log: ${error.message}`);
    }
    await browser?.close().catch(() => {});
    for (const child of children) child.kill("SIGTERM");
    evidence.finished_at = nowIso();
    const failed = evidence.checks.filter(item => !item.ok);
    evidence.summary = { passed: evidence.checks.length - failed.length, failed: failed.length, errors: evidence.errors.length };
    save("kubectl-commands.log", `${commandLog.join("\n")}\n`);
    save("evidence.json", evidence);
    const leaks = scanForSecrets(OUT, heldSecrets);
    heldSecrets.length = 0;
    if (leaks.length) {
      console.error(`SECRET FOUND in the evidence bundle, delete before sharing: ${leaks.join(", ")}`);
      exitCode = 1;
    }
    console.log(`== Result: ${evidence.summary.passed} passed, ${evidence.summary.failed} failed, ${evidence.errors.length} errors`);
    for (const item of failed) console.log(`  FAIL [${item.phase}] ${item.check}`);
    console.log(`evidence: ${join(OUT, "evidence.json")}`);
    if (failed.length || evidence.errors.length) exitCode = 1;
  }
  return exitCode;
}

function selfTest() {
  const assert = (condition, message) => {
    if (!condition) throw new Error(`self-test: ${message}`);
  };
  const sample = "\x1b[?2004h$ python3 /tmp/x research\r\nDEMO-BEGIN research session=s1\r\nPASS agent_cli: ['codex-cli 0.1']\r\n\x1b[32mFAIL\x1b[0m no_dns: example.com resolved\r\nPASS broker_route_absent POST /v1/tools/call: 404 None\r\nDEMO-RESULT research passed=2 failed=1\r\n$ ";
  const parsed = parseProbe(stripAnsi(sample), "research");
  assert(parsed && parsed.passed === 2 && parsed.failed === 1, "result marker");
  assert(parsed.checks.length === 3 && parsed.checks[1].ok === false && parsed.checks[1].check === "no_dns", "check lines");
  assert(parsed.checks[2].check === "broker_route_absent POST /v1/tools/call", "check names with spaces");
  assert(parseProbe("DEMO-BEGIN research session=s1\nPASS a: b\n", "research") === null, "missing result");
  assert(parseProbe(stripAnsi(sample), "coding") === null, "other probe");
  const poll = parsePollLine("POLL-REFUSED 1700000000123 401 token_revoked");
  assert(poll.kind === "POLL-REFUSED" && poll.at === 1700000000123 && poll.status === "401" && poll.code === "token_revoked", "poll line");
  assert(parsePollLine("POLL 1 200 -").code === "-" && parsePollLine("noise") === null, "poll noise");
  assert(stripAnsi("\x1b]0;title\x07a\x1b[1;32mb\x1b[0m\r\n") === "ab\n", "ansi");
  assert(outsideRepo("/tmp/luc137-demo-x") && !outsideRepo(join(REPO, "out")) && !outsideRepo(REPO), "evidence directory");
  assert(parseArgs(["--port-forward", "--out", "/tmp/x"]).portForward === true, "flags");
  let refused = false;
  try { parseArgs(["--base-url", "http://campus.example"]); } catch { refused = true; }
  assert(refused, "plain http base url");
  const dir = join("/tmp", `luc137-selftest-${process.pid}`);
  mkdirSync(join(dir, "nested"), { recursive: true });
  writeFileSync(join(dir, "clean.json"), "{}");
  writeFileSync(join(dir, "nested", "leak.txt"), `token pcbt1_${"A".repeat(43)}`);
  writeFileSync(join(dir, "password.txt"), "the value hunter2-synthetic appears");
  const hits = scanForSecrets(dir, ["hunter2-synthetic"]).map(file => relative(dir, file)).sort();
  assert(JSON.stringify(hits) === JSON.stringify(["nested/leak.txt", "password.txt"]), `secret scan ${hits}`);
  spawnSync("rm", ["-rf", dir]);
  for (const probe of [PROBE_PY, PROBE_MJS]) assert(!readFileSync(probe, "utf8").includes("DEMO-RESULT") && !readFileSync(probe, "utf8").includes("DEMO-BEGIN"), `${probe} builds its markers at run time`);
  console.log("campus-m1-demo self-test: ok");
  return 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().then(code => process.exit(code), error => {
    console.error(error.stack ?? String(error));
    process.exit(1);
  });
}

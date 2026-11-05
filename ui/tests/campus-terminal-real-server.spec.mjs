import AxeBuilder from "@axe-core/playwright";
import react from "@vitejs/plugin-react";
import { expect, test } from "@playwright/test";
import { spawn, spawnSync } from "node:child_process";
import { generateKeyPairSync, randomBytes, sign } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { createServer as createNetServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createServer as createViteServer } from "vite";

// A real PHarness API with Campus, the fake lab provider and the fake
// terminal behind a real Vite dev server, signed in through a local OIDC
// provider. Nothing leaves loopback and no workload runs.

const here = dirname(fileURLToPath(import.meta.url));
const workspace = resolve(here, "../..");
const fixtures = join(workspace, "crates/pharness-core/src/campus_profile/fixtures");
const issuerSubject = "student-e2e";
const fakeAgentVersion = "codex-cli 0.0.0-campus-fake";
const closeBoundMs = 2_000;

let apiProcess;
let apiLog = "";
let idpServer;
let vite;
let fixtureRoot;
let origin;

async function freePort() {
  const server = createNetServer();
  await new Promise(resolvePromise => server.listen(0, "127.0.0.1", resolvePromise));
  const { port } = server.address();
  await new Promise(resolvePromise => server.close(resolvePromise));
  return port;
}

function b64url(value) {
  return Buffer.from(value).toString("base64url");
}

function layer(authority) {
  return {
    schema_version: "pharness.dev/campus-policy-layer/v1alpha1",
    authority,
    id: `${authority}-policy`,
    version: "v1",
    constraints: {},
    mandatory_denies: { tools: [], egress_hosts: [], model_aliases: [], approval_kinds: [] },
  };
}

function catalog() {
  return {
    schema_version: "pharness.dev/campus-lab-catalog/v1alpha1",
    lab_slots: 2,
    profiles: [JSON.parse(readFileSync(join(fixtures, "coding.json"), "utf8"))],
    policy_layers: [layer("platform"), layer("institution")],
    assignments: [{ course_id: "cs101", assignment_id: "lab-1", title: "Lab 1", profile_id: "campus-coding" }],
  };
}

/** Authorize redirects straight back; the token endpoint signs an ES256 id_token for the nonce it saw. */
async function startIdp(clientId) {
  const { privateKey, publicKey } = generateKeyPairSync("ec", { namedCurve: "P-256" });
  const jwk = { ...publicKey.export({ format: "jwk" }), kid: "e2e-key", alg: "ES256", use: "sig" };
  const codes = new Map();
  let issuer;
  const server = createServer((request, response) => {
    const url = new URL(request.url, issuer);
    if (url.pathname === "/jwks") {
      response.setHeader("content-type", "application/json");
      response.end(JSON.stringify({ keys: [jwk] }));
      return;
    }
    if (url.pathname === "/authorize") {
      const code = randomBytes(16).toString("hex");
      codes.set(code, url.searchParams.get("nonce"));
      const back = new URL(url.searchParams.get("redirect_uri"));
      back.searchParams.set("code", code);
      back.searchParams.set("state", url.searchParams.get("state"));
      response.statusCode = 302;
      response.setHeader("location", back.toString());
      response.end();
      return;
    }
    if (url.pathname === "/token" && request.method === "POST") {
      let body = "";
      request.on("data", chunk => { body += chunk; });
      request.on("end", () => {
        const code = new URLSearchParams(body).get("code");
        const nonce = codes.get(code);
        codes.delete(code);
        response.setHeader("content-type", "application/json");
        if (!nonce) {
          response.statusCode = 400;
          response.end(JSON.stringify({ error: "invalid_grant" }));
          return;
        }
        const now = Math.floor(Date.now() / 1000);
        const header = b64url(JSON.stringify({ alg: "ES256", typ: "JWT", kid: jwk.kid }));
        const claims = b64url(JSON.stringify({
          iss: issuer, aud: clientId, sub: issuerSubject, iat: now, exp: now + 300, nonce,
          email: "student-e2e@campus.test", name: "Campus Student",
        }));
        const signature = sign("sha256", Buffer.from(`${header}.${claims}`), { key: privateKey, dsaEncoding: "ieee-p1363" });
        response.end(JSON.stringify({ id_token: `${header}.${claims}.${b64url(signature)}`, token_type: "Bearer" }));
      });
      return;
    }
    response.statusCode = 404;
    response.end();
  });
  await new Promise(resolvePromise => server.listen(0, "127.0.0.1", resolvePromise));
  issuer = `http://127.0.0.1:${server.address().port}`;
  return { server, issuer };
}

async function waitForHealth(url) {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    try {
      if ((await fetch(url)).ok) return;
    } catch {}
    await new Promise(resolvePromise => setTimeout(resolvePromise, 100));
  }
  throw new Error(`the PHarness API fixture did not become ready:\n${apiLog}`);
}

test.beforeAll(async ({}, workerInfo) => {
  if (workerInfo.project.name !== "desktop") return;
  test.setTimeout(180_000);
  fixtureRoot = mkdtempSync(join(tmpdir(), "pharness-campus-terminal-e2e-"));
  const clientId = "pharness-campus-e2e";
  const idp = await startIdp(clientId);
  idpServer = idp.server;
  const [apiPort, vitePort] = [await freePort(), await freePort()];
  origin = `http://localhost:${vitePort}`;
  writeFileSync(join(fixtureRoot, "client-secret"), "e2e-client-secret");
  writeFileSync(join(fixtureRoot, "login-cookie-key"), randomBytes(32).toString("base64"));
  writeFileSync(join(fixtureRoot, "labs.json"), JSON.stringify(catalog()));

  const build = spawnSync("cargo", ["build", "-p", "pharness-api"], {
    cwd: workspace,
    env: { ...process.env, CARGO_TARGET_DIR: join(workspace, "target") },
    encoding: "utf8",
  });
  if (build.status !== 0) throw new Error(`failed to build the API fixture:\n${build.stdout}\n${build.stderr}`);
  apiProcess = spawn(join(workspace, "target/debug/pharness-api"), [], {
    cwd: fixtureRoot,
    env: {
      ...process.env,
      PHARNESS_BIND: `127.0.0.1:${apiPort}`,
      PHARNESS_DB_PATH: join(fixtureRoot, "pharness.db"),
      PHARNESS_WORKSPACE_ROOT: join(fixtureRoot, "workspaces"),
      PHARNESS_OPERATOR_TOKENS: "e2e=e2e-operator-token",
      PHARNESS_CAMPUS_ENABLED: "true",
      PHARNESS_CAMPUS_DEV_ALLOW_INSECURE_LOOPBACK_HTTP: "true",
      PHARNESS_CAMPUS_TENANT_ID: "tenant-e2e",
      PHARNESS_CAMPUS_PUBLIC_ORIGIN: origin,
      PHARNESS_CAMPUS_OIDC_ISSUER: idp.issuer,
      PHARNESS_CAMPUS_OIDC_CLIENT_ID: clientId,
      PHARNESS_CAMPUS_OIDC_CLIENT_SECRET_FILE: join(fixtureRoot, "client-secret"),
      PHARNESS_CAMPUS_LOGIN_COOKIE_KEY_FILE: join(fixtureRoot, "login-cookie-key"),
      PHARNESS_CAMPUS_OIDC_AUTHORIZATION_ENDPOINT: `${idp.issuer}/authorize`,
      PHARNESS_CAMPUS_OIDC_TOKEN_ENDPOINT: `${idp.issuer}/token`,
      PHARNESS_CAMPUS_OIDC_JWKS_URI: `${idp.issuer}/jwks`,
      PHARNESS_CAMPUS_OIDC_REDIRECT_URI: `${origin}/api/campus/auth/callback`,
      PHARNESS_CAMPUS_OIDC_ALLOWED_ALGS: "ES256",
      PHARNESS_CAMPUS_BOOTSTRAP_ADMINS: `${idp.issuer}|${issuerSubject}`,
      PHARNESS_CAMPUS_POST_LOGIN_PATH: "/campus.html",
      PHARNESS_CAMPUS_LABS_FILE: join(fixtureRoot, "labs.json"),
      PHARNESS_CAMPUS_LAB_PROVIDER: "fake",
      PHARNESS_CAMPUS_TERMINAL: "fake",
      RUST_LOG: "info",
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  apiProcess.stdout.on("data", chunk => { apiLog += chunk.toString(); });
  apiProcess.stderr.on("data", chunk => { apiLog += chunk.toString(); });
  await waitForHealth(`http://127.0.0.1:${apiPort}/health`);

  vite = await createViteServer({
    configFile: false,
    root: join(workspace, "ui"),
    logLevel: "error",
    plugins: [react()],
    server: {
      host: "localhost",
      port: vitePort,
      strictPort: true,
      proxy: { "/api": { target: `http://127.0.0.1:${apiPort}`, changeOrigin: true, ws: true } },
    },
  });
  await vite.listen();
});

test.afterAll(async () => {
  await vite?.close();
  apiProcess?.kill("SIGTERM");
  if (idpServer) await new Promise(resolvePromise => idpServer.close(resolvePromise));
  if (fixtureRoot) rmSync(fixtureRoot, { recursive: true, force: true });
});

/** Enroll the signed-in student, launch lab-1 and wait until it is ready. */
async function launchReadyLab(page) {
  return page.evaluate(async () => {
    const me = await (await fetch("/api/campus/me")).json();
    const headers = { "content-type": "application/json", "x-campus-csrf": me.csrf_token };
    await fetch("/api/campus/courses/cs101/members", {
      method: "PUT",
      headers,
      body: JSON.stringify({ issuer: me.identity.issuer, subject: me.identity.subject, role: "student" }),
    });
    let launched;
    for (let attempt = 0; attempt < 100; attempt += 1) {
      const response = await fetch("/api/campus/courses/cs101/assignments/lab-1/launch", { method: "POST", headers, body: "{}" });
      if (response.status === 201 || response.status === 200) {
        launched = (await response.json()).session.session_id;
        break;
      }
      await new Promise(resolvePromise => setTimeout(resolvePromise, 100));
    }
    if (!launched) throw new Error("the lab could not be launched");
    for (let attempt = 0; attempt < 100; attempt += 1) {
      const view = await (await fetch(`/api/campus/lab-sessions/${launched}`)).json();
      if (view.session.status === "ready") return launched;
      await new Promise(resolvePromise => setTimeout(resolvePromise, 100));
    }
    throw new Error("the lab did not become ready");
  });
}

async function accessible(page) {
  const results = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"]).analyze();
  expect(results.violations).toEqual([]);
}

async function openTerminal(page, sessionId) {
  await page.goto(`${origin}/campus.html#/lab-sessions/${sessionId}`);
  const status = page.getByRole("status");
  await expect(status).toHaveText(/Your lab is ready/);
  await accessible(page);
  await page.getByRole("button", { name: "Connect", exact: true }).focus();
  await page.keyboard.press("Enter");
  await expect(status).toHaveText(/Terminal connected/);
  await expect(page.locator(".xterm-helper-textarea")).toBeFocused();
  await expect(page.locator(".xterm-rows")).toContainText("student@campus-lab:~$");
  return status;
}

test("a student signs in, runs the agent CLI in the lab terminal and stop or sign-out closes it", async ({ page }, testInfo) => {
  test.skip(testInfo.project.name !== "desktop", "One real server serves the desktop project.");
  test.setTimeout(120_000);
  await page.goto(`${origin}/api/campus/auth/login`);
  await expect(page).toHaveURL(`${origin}/campus.html`);
  const first = await launchReadyLab(page);

  const status = await openTerminal(page, first);
  await page.keyboard.type("codex --version");
  await page.keyboard.press("Enter");
  await expect(page.locator(".xterm-rows")).toContainText(fakeAgentVersion);
  await page.keyboard.press("Tab");
  await expect(page.locator(".xterm-helper-textarea")).toBeFocused();
  await page.keyboard.press("Control+BracketRight");
  await expect(page.getByRole("button", { name: "Disconnect", exact: true })).toBeFocused();
  await accessible(page);
  await page.screenshot({ path: testInfo.outputPath("campus-terminal-connected.png"), fullPage: true });

  const stopping = Date.now();
  await page.getByRole("button", { name: "Stop lab" }).click();
  await expect(status).toHaveText(/The lab was stopped/);
  const stopLatency = Date.now() - stopping;
  expect(stopLatency).toBeLessThan(closeBoundMs);
  await expect(status).toBeFocused();
  await expect(page.getByRole("button", { name: "Reconnect", exact: true })).toHaveCount(0);
  await accessible(page);
  const revived = await page.evaluate(async id => {
    const me = await (await fetch("/api/campus/me")).json();
    const response = await fetch(`/api/campus/lab-sessions/${id}/terminal-ticket`, {
      method: "POST",
      headers: { "content-type": "application/json", "x-campus-csrf": me.csrf_token },
      body: "{}",
    });
    return { status: response.status, code: (await response.json()).code };
  }, first);
  expect(revived).toEqual({ status: 409, code: "session_stopped" });

  const second = await launchReadyLab(page);
  await openTerminal(page, second);
  await page.keyboard.press("Control+BracketRight");
  const signingOut = Date.now();
  await page.getByRole("button", { name: "Sign out" }).click();
  await expect(page.getByRole("status")).toHaveText(/You were signed out/);
  const logoutLatency = Date.now() - signingOut;
  expect(logoutLatency).toBeLessThan(closeBoundMs);
  await expect(page.getByRole("link", { name: "Sign in" })).toBeVisible();
  await accessible(page);
  testInfo.annotations.push({ type: "close-latency-ms", description: `stop=${stopLatency} logout=${logoutLatency}` });
  console.log(`campus terminal browser close latency: stop ${stopLatency} ms, logout ${logoutLatency} ms`);
  expect(apiLog).not.toContain("e2e-client-secret");
});

import { expect, test } from "@playwright/test";
import { spawn, spawnSync } from "node:child_process";
import { createHash, createHmac } from "node:crypto";
import { createServer } from "node:http";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const sourceSha = "a".repeat(40);
const headSha = "b".repeat(40);
const mergeSha = "c".repeat(40);
const runnerDigest = `registry.test/pharness-runner@sha256:${"d".repeat(64)}`;
const repositoryUrl = "https://github.com/example/repo-mode-ui-e2e";
const canonicalRepositoryUrl = `${repositoryUrl}.git`;
const workerToken = "repo-mode-ui-e2e-worker";
const here = dirname(fileURLToPath(import.meta.url));
const workspace = resolve(here, "../..");
const laminaEnabled = process.env.PHARNESS_UI_TEST_LAMINA === "true";

let apiProcess;
let githubServer;
let fixtureRoot;

function profileRegistry() {
  return JSON.stringify([{
    id: "python-3.11",
    active: true,
    image: runnerDigest,
    revision: sourceSha,
    platform: "linux/amd64",
    required_executables: ["pharness-worker", "git", "python", "pip"],
    preparation_strategy: "python_hashed_requirements",
    service_account: "pharness-runner-python",
    repository_allowlist: [canonicalRepositoryUrl],
    limits: { cpu: "1", memory: "1Gi", ephemeral_storage: "2Gi" },
  }]);
}

async function listen(server) {
  await new Promise((resolvePromise, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolvePromise);
  });
  return server.address().port;
}

async function waitForApi() {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    try {
      const response = await fetch("http://127.0.0.1:4788/health");
      if (response.ok) return;
    } catch {}
    await new Promise(resolvePromise => setTimeout(resolvePromise, 100));
  }
  throw new Error("real PHarness API fixture did not become ready");
}

async function api(path, { method = "GET", body, worker = false } = {}) {
  const response = await fetch(`http://127.0.0.1:4788${path}`, {
    method,
    headers: {
      ...(body ? { "content-type": "application/json" } : {}),
      ...(worker ? { authorization: `Bearer ${workerToken}` } : {}),
    },
    body: body ? JSON.stringify(body) : undefined,
  });
  const text = await response.text();
  if (!response.ok) throw new Error(`${method} ${path} returned ${response.status}: ${text}`);
  const value = text ? JSON.parse(text) : null;
  return value;
}

async function confirmAction(page, name, reason) {
  await page.getByRole("button", { name }).click();
  const dialog = page.getByRole("dialog", { name: new RegExp(name.replaceAll("_", " "), "i") });
  await dialog.getByLabel("Reason").fill(reason);
  await dialog.getByRole("button", { name: "Confirm and apply" }).click();
  await expect(dialog).toHaveCount(0);
}

async function stabilizeCompletedJourneySnapshot(page) {
  await page.locator(".repo-bindings > div").filter({ has:page.getByText("Intent", { exact:true }) }).locator("dd").evaluateAll(nodes => {
    nodes.forEach(node => { node.textContent = "source-delivery-intent"; });
  });
  await page.locator(".repo-freshness small").evaluateAll(nodes => {
    nodes.forEach(node => { node.textContent = "Fresh authoritative observation window"; });
  });
  await page.locator(".repo-evidence-links small").evaluateAll((nodes) => {
    nodes.forEach((node, index) => { node.textContent = `provider-check-observation-${index + 1}`; });
  });
  await page.locator(".repo-sidebar footer > small").evaluateAll(nodes => {
    nodes.forEach(node => { node.textContent = "fixture-as-of"; });
  });
}

function sha256(value) {
  return `sha256:${createHash("sha256").update(value).digest("hex")}`;
}

function sha256Hex(value) {
  return createHash("sha256").update(value).digest("hex");
}

function rustJson(value) {
  if (Array.isArray(value)) return `[${value.map(rustJson).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map(key => `${JSON.stringify(key)}:${rustJson(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function signSnapshot(snapshot) {
  return `hmac-sha256:${createHmac("sha256", workerToken).update(rustJson(snapshot)).digest("hex")}`;
}

function completedOutcome(run, summary = "Deterministic test adapter completed the run", workspaceEvidence) {
  const current = run.budget_consumption || {};
  const source = run.execution_target?.workspace_source || run.execution_target_json?.workspace_source;
  return {
    status: "completed",
    turns: 1,
    summary,
    error: null,
    approval: null,
    workspace_evidence: workspaceEvidence || (source ? {
      workspace_id: source.workspace_id,
      base_commit: source.immutable_commit || source.resolved_commit,
      branch: source.branch,
      status: "",
      diff: "",
      changed_paths: [],
    } : null),
    budget_extension: null,
    consumption: {
      allowed_turns: current.allowed_turns,
      allowed_tokens: current.allowed_tokens,
      turns_used: Math.max(1, current.turns_used || 0),
      tokens_used: Math.max(1000, current.tokens_used || 0),
      active_execution_seconds_used: Math.max(1, current.active_execution_seconds_used || 0),
      extensions: current.extensions || 0,
    },
  };
}

async function submitRunDocument(run, kind, document) {
  await api(`/api/internal/runs/${run.id}/mark-running`, { method: "POST", body: {}, worker: true });
  await api(`/api/internal/runs/${run.id}/events`, {
    method: "POST",
    worker: true,
    body: { events: [{
      event_id: `evt_${kind}_${Date.now()}`,
      session_id: run.session_id,
      run_id: run.id,
      seq: 100,
      type: "tool.finished",
      payload: { status: "ok", content: { structured_submission: true, kind, document } },
    }] },
  });
  return api(`/api/internal/runs/${run.id}/outcome`, { method: "POST", body: completedOutcome(run), worker: true });
}

async function attemptRun(runId) {
  const attempt = await api(`/api/internal/runs/${runId}/attempt-context`, { worker: true });
  return {
    id: attempt.run.run_id,
    session_id: attempt.run.session_id,
    execution_target: attempt.run.execution_target_json,
    budget_consumption: attempt.run.budget_consumption,
  };
}

test.beforeAll(async ({}, workerInfo) => {
  if (workerInfo.project.name !== "desktop") return;
  test.setTimeout(120_000);
  fixtureRoot = mkdtempSync(join(tmpdir(), "pharness-repo-ui-e2e-"));
  githubServer = createServer((request, response) => {
    response.setHeader("content-type", "application/json");
    if (request.url === "/repos/example/repo-mode-ui-e2e") {
      response.end(JSON.stringify({ id: 4242, full_name: "example/repo-mode-ui-e2e", html_url: repositoryUrl, default_branch: "main" }));
      return;
    }
    if (request.url === `/repos/example/repo-mode-ui-e2e/commits/${sourceSha}`) {
      response.end(JSON.stringify({ sha: sourceSha }));
      return;
    }
    response.statusCode = 404;
    response.end(JSON.stringify({ message: "fixture route not found" }));
  });
  const githubPort = await listen(githubServer);
  const build = spawnSync("cargo", ["build", "-p", "pharness-api", "--features", "ui-e2e"], {
    cwd: workspace,
    env: { ...process.env, CARGO_TARGET_DIR: join(workspace, "target") },
    encoding: "utf8",
  });
  if (build.status !== 0) throw new Error(`failed to build real API fixture:\n${build.stdout}\n${build.stderr}`);
  apiProcess = spawn(join(workspace, "target/debug/pharness-api"), [], {
    cwd: workspace,
    env: {
      ...process.env,
      PHARNESS_BIND: "127.0.0.1:4788",
      PHARNESS_DB_PATH: join(fixtureRoot, "pharness.db"),
      PHARNESS_WORKSPACE_ROOT: join(fixtureRoot, "workspaces"),
      PHARNESS_WORKSPACE_ALLOWED_REMOTE_REPOS: canonicalRepositoryUrl,
      PHARNESS_KUBECTL_BIN: join(workspace, "ui/tests/fixtures/fake-kubectl.mjs"),
      PHARNESS_WORKER_MODE: "kubernetes_job",
      PHARNESS_WORKER_K8S_IMAGE: `registry.test/pharness-runtime@sha256:${"c".repeat(64)}`,
      PHARNESS_WORKER_K8S_API_URL: "http://127.0.0.1:4788",
      PHARNESS_WORKER_TOKEN: workerToken,
      PHARNESS_REPO_MODE_V1_ENABLED: "true",
      PHARNESS_REPO_MODE_V1_UI_ENABLED: "true",
      PHARNESS_REPO_MODE_V1_DESIGN_OVERHAUL_ENABLED: String(laminaEnabled),
      PHARNESS_ORGANIZATION_ID: "org_repo_mode_ui_e2e",
      PHARNESS_ORGANIZATION_KEY: "repo-mode-ui-e2e",
      PHARNESS_ORGANIZATION_NAME: "Repo Mode UI E2E",
      PHARNESS_SOURCE_READER_ENABLED: "true",
      PHARNESS_SOURCE_READER_ALLOWED_REPOS: canonicalRepositoryUrl,
      PHARNESS_GIT_WRITER_ENABLED: "true",
      PHARNESS_GIT_WRITER_TOKEN_SECRET: "fixture-writer",
      PHARNESS_GIT_WRITER_ALLOWED_REPOS: canonicalRepositoryUrl,
      PHARNESS_GIT_OBSERVER_ENABLED: "true",
      PHARNESS_GIT_OBSERVER_TOKEN_SECRET: "fixture-observer",
      PHARNESS_GIT_OBSERVER_ALLOWED_REPOS: canonicalRepositoryUrl,
      PHARNESS_ENVIRONMENT_PROFILES_JSON: profileRegistry(),
      PHARNESS_UI_E2E_GITHUB_API_URL: `http://127.0.0.1:${githubPort}`,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stderr = "";
  apiProcess.stderr.on("data", chunk => { stderr += chunk.toString(); });
  apiProcess.once("exit", code => {
    if (code && code !== 0) process.stderr.write(`PHarness API E2E fixture exited ${code}:\n${stderr}`);
  });
  await waitForApi();
});

test.afterAll(async () => {
  apiProcess?.kill("SIGTERM");
  if (githubServer) await new Promise(resolvePromise => githubServer.close(resolvePromise));
  if (fixtureRoot) rmSync(fixtureRoot, { recursive: true, force: true });
});

test("real UI and controller complete Repo Mode from Product creation through source merge closure", async ({ page }, testInfo) => {
  test.setTimeout(120_000);
  test.skip(testInfo.project.name !== "desktop", "The full state catalog separately proves phone layouts.");
  await page.goto("/#/products");
  await expect(page.getByRole("heading", { name: "Products", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "New Product" }).click();
  await page.getByLabel("Name").fill("Repository Experience Fixture");
  await page.getByLabel("Description").fill("Real controller and temporary SQLite browser acceptance.");
  await page.locator("form").getByRole("button", { name: "Create Product" }).click();
  await expect(page).toHaveURL(/#\/products\/prod_[^/]+\/work-items$/);

  await page.getByRole("button", { name: "Repositories", exact: true }).click();
  await page.getByRole("button", { name: "Register Repository" }).click();
  await page.getByLabel("GitHub HTTPS URL").fill(repositoryUrl);
  await page.getByLabel("Full commit SHA").fill(sourceSha);
  await page.getByRole("button", { name: "Run preflight" }).click();
  await expect(page.getByRole("heading", { name: "Review registration" })).toBeVisible();
  await expect(page.getByText(canonicalRepositoryUrl)).toBeVisible();
  await page.getByRole("button", { name: "Confirm registration" }).click();
  await expect(page).toHaveURL(/#\/repository-onboardings\/ronb_[^/]+$/);
  await expect(page.getByText("Repository onboarding", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "start discovery" })).toBeVisible();

  const onboardingId = page.url().split("/").at(-1);
  await confirmAction(page, "start discovery", "Run isolated deterministic discovery");
  let flow = await api(`/api/repository-onboardings/${onboardingId}/flow`);
  const discovery = {
    schema_version: "pharness.dev/repository-discovery/v1alpha1",
    repository: { provider: "github", canonical_url: canonicalRepositoryUrl, default_branch: "main", registered_commit: sourceSha, resolved_commit: sourceSha },
    files: [
      { path: ".pharness/project.yaml", kind: "file", size_bytes: 400, inspected: true, content_sha256: sha256("legacy-contract") },
      { path: "requirements.lock", kind: "file", size_bytes: 200, inspected: true, content_sha256: sha256("locked-dependencies") },
      { path: "src/app.py", kind: "file", size_bytes: 100, inspected: true, content_sha256: sha256("source") },
      { path: "tests/test_app.py", kind: "file", size_bytes: 100, inspected: true, content_sha256: sha256("tests") },
    ],
    symlinks: [],
    submodules: [],
    contract: { canonical_present: false, canonical_sha256: null, alias_present: true, alias_sha256: sha256("legacy-contract"), status: "alias_only" },
    language_indicators: { python: 3 },
    dependency_candidates: [{ kind: "pip_requirements", path: "requirements.lock", content_sha256: sha256("locked-dependencies") }],
    command_candidates: [{ command: "python -m unittest discover -s tests -v", source_path: "readme.md", source_line: 10 }],
    root_candidates: ["src", "tests"],
    automation_references: [],
    conflicts: [],
    blockers: [],
    inspected_text_bytes: 700,
    limits: { max_entries: 20000, max_inspected_text_bytes: 33554432, max_text_file_bytes: 262144 },
    content_hash: "",
  };
  discovery.content_hash = sha256(JSON.stringify(discovery));
  await api(`/api/internal/repository-discoveries/${flow.discovery.id}/outcome`, { method: "POST", worker: true, body: { status: "succeeded", discovery } });
  await page.reload();
  await expect(page.getByText("succeeded", { exact: true }).first()).toBeVisible();

  await confirmAction(page, "start proposer", "Generate one bounded onboarding proposal");
  flow = await api(`/api/repository-onboardings/${onboardingId}/flow`);
  const contract = {
    api_version: "pharness.dev/v1alpha1",
    environment_profile: "python-3.11",
    dependency_lock: { kind: "pip_requirements", path: "requirements.lock", sha256: sha256Hex("locked-dependencies") },
    writable_paths: ["src/**", "tests/**", "readme.md"],
    acceptance_commands: [
      { name: "unit", command: "python -m unittest discover -s tests -v" },
      { name: "compile", command: "python -m compileall -q src tests" },
    ],
    roots: { source: ["src"], tests: ["tests"], documentation: ["readme.md"] },
    agent_network: "denied",
    package_installation: "preparation_only",
  };
  const proposal = {
    schema_version: "pharness.dev/repository-onboarding-proposal/v1alpha1",
    discovery_id: flow.discovery.id,
    discovery_hash: flow.discovery.content_hash,
    candidate_contract: contract,
    instructions: "Read the canonical RepositoryContract before planning.",
    service_proposals: [],
    binding_proposals: [{ service_keys: [], scopes: ["**"] }],
    assumptions: ["The immutable Python lock is authoritative."],
    conflicts: [],
    blockers: [],
    readiness_forecast: { contract: "ready_after_merge", coding: "ready_after_preparation" },
  };
  await submitRunDocument(flow.proposer_run, "repository_onboarding_proposal", proposal);
  await page.reload();
  await expect(page.getByRole("button", { name: "Edit proposal revision" })).toBeVisible();
  await page.getByRole("button", { name: "Edit proposal revision" }).click();
  await page.getByLabel("Repository instructions").fill("Read the reviewed RepositoryContract and bounded instructions before planning.");
  await page.getByRole("button", { name: "Save new proposal revision" }).click();
  await expect(page.getByRole("button", { name: "Close proposal editor" })).toHaveCount(0);
  await page.reload();
  await expect(page.getByRole("button", { name: "approve proposal" })).toBeVisible();

  await confirmAction(page, "approve proposal", "Approve the exact revised onboarding proposal");
  await confirmAction(page, "prepare onboarding patch", "Materialize only the reviewed Repository contract change");
  flow = await api(`/api/repository-onboardings/${onboardingId}/flow`);
  const patch = [
    "diff --git a/.pharness/project.yaml b/.pharness/repository.yaml",
    "similarity index 100%",
    "rename from .pharness/project.yaml",
    "rename to .pharness/repository.yaml",
    "",
  ].join("\n");
  await api(`/api/internal/repository-onboardings/${onboardingId}/patch-outcome`, {
    method: "POST",
    worker: true,
    body: {
      status: "succeeded",
      patch,
      patch_hash: sha256(patch),
      changed_paths: [".pharness/project.yaml", ".pharness/repository.yaml"],
    },
  });
  await page.reload();
  await page.getByText("Review the exact patch authorized for one source PR").click();
  await expect(page.getByText("rename to .pharness/repository.yaml")).toBeVisible();

  await confirmAction(page, "authorize onboarding source delivery", "Create the exact onboarding pull request");
  flow = await api(`/api/repository-onboardings/${onboardingId}/flow`);
  let intent = flow.source_delivery_intent;
  await api(`/api/internal/source-delivery-intents/${intent.id}/writer-outcome`, {
    method: "POST",
    worker: true,
    body: {
      execution_id: intent.writer_execution_id,
      status: "completed",
      branch: intent.head_branch,
      commit_sha: headSha,
      pull_request_url: `${repositoryUrl}/pull/7`,
      pull_request_number: 7,
    },
  });
  await page.reload();
  await confirmAction(page, "observe onboarding source delivery", "Observe the exact onboarding pull request and required checks");
  flow = await api(`/api/repository-onboardings/${onboardingId}/flow`);
  intent = flow.source_delivery_intent;
  await api(`/api/internal/source-delivery-intents/${intent.id}/observation-outcome`, {
    method: "POST",
    worker: true,
    body: {
      execution_id: intent.observer_execution_id,
      status: "observed",
      pull_request_state: "open",
      merged: false,
      head_branch: intent.head_branch,
      head_commit_sha: headSha,
      authoritative_rules_succeeded: true,
      required_checks: [],
      check_runs: [],
      commit_statuses: [],
      provider_check_status: "passing",
    },
  });
  await page.reload();
  await expect(page.getByText("waiting merge", { exact: true })).toBeVisible();
  await confirmAction(page, "observe onboarding source delivery", "Observe the manually merged onboarding pull request");
  flow = await api(`/api/repository-onboardings/${onboardingId}/flow`);
  intent = flow.source_delivery_intent;
  await api(`/api/internal/source-delivery-intents/${intent.id}/observation-outcome`, {
    method: "POST",
    worker: true,
    body: {
      execution_id: intent.observer_execution_id,
      status: "observed",
      pull_request_state: "closed",
      merged: true,
      merge_commit_sha: mergeSha,
      head_branch: intent.head_branch,
      head_commit_sha: headSha,
      authoritative_rules_succeeded: true,
      required_checks: [],
      check_runs: [],
      commit_statuses: [],
      provider_check_status: "passing",
    },
  });
  await page.reload();
  await confirmAction(page, "validate merged contract", "Validate the canonical contract at the observed merge revision");
  flow = await api(`/api/repository-onboardings/${onboardingId}/flow`);
  await api(`/api/internal/repository-onboardings/${onboardingId}/contract-validation-outcome`, {
    method: "POST",
    worker: true,
    body: {
      status: "succeeded",
      contract,
      contract_content_hash: sha256(JSON.stringify(contract)),
      contract_source: "canonical",
      warnings: [],
    },
  });
  flow = await api(`/api/repository-onboardings/${onboardingId}/flow`);
  const repositoryId = flow.onboarding.repository_id;
  await page.goto(`/#/repositories/${repositoryId}/overview`);
  const sourceReaderResponse = page.waitForResponse(response => response.url().includes("/api/system/capabilities/source_reader/preflight") && response.request().method() === "POST");
  await page.getByRole("button", { name: "Verify source reader" }).click();
  expect((await sourceReaderResponse).ok()).toBe(true);
  await expect(page.getByRole("button", { name: "Verify source reader" })).toHaveCount(0);
  const profileVerification = page.getByRole("button", { name: "Verify environment profile:python-3.11" });
  const profileResponse = page.waitForResponse(response => response.url().includes("/api/system/capabilities/environment_profile%3Apython-3.11/preflight") && response.request().method() === "POST");
  await profileVerification.click();
  const profileHttp = await profileResponse;
  const profileBody = await profileHttp.json();
  if (!profileHttp.ok()) throw new Error(`profile verification returned ${profileHttp.status()}: ${JSON.stringify(profileBody)}`);
  expect(profileBody.status).toBe("available");
  await expect(profileVerification).toHaveCount(0);
  await page.goto(`/#/repositories/${repositoryId}/readiness`);
  const readinessResponse = page.waitForResponse(response => response.url().includes(`/api/repositories/${repositoryId}/readiness-assessments`) && response.request().method() === "POST");
  await page.getByRole("button", { name: "Run exact readiness assessment" }).click();
  const readinessHttp = await readinessResponse;
  const readinessText = await readinessHttp.text();
  if (!readinessHttp.ok()) throw new Error(`UI readiness request returned ${readinessHttp.status()}: ${readinessText}`);
  const readinessStart = JSON.parse(readinessText);
  const preparation = readinessStart.preparation;
  const snapshot = {
    source_sha: mergeSha,
    manifest_sha256: sha256(JSON.stringify(contract)),
    dependency_lock_sha256: contract.dependency_lock.sha256,
    runner_image_digest: runnerDigest,
    runner_revision: sourceSha,
    os: "linux",
    architecture: "amd64",
    effective_user: "65532",
    python_version: "Python 3.11.13",
    python_path: "/workspace/.pharness-runtime/venv/bin/python",
    writable_paths: contract.writable_paths,
    unavailable_tools: ["docker", "podman"],
    agent_network: "denied",
    package_installation: "preparation_only",
    acceptance_commands: contract.acceptance_commands,
    preparation_evidence: { checkout: "passed", dependencies: "hashed" },
  };
  await api(`/api/internal/repository-readiness-preparations/${preparation.id}/outcome`, {
    method: "POST",
    worker: true,
    body: {
      status: "succeeded",
      resolved_commit: mergeSha,
      repository_contract: contract,
      repository_contract_hash: snapshot.manifest_sha256,
      environment_snapshot: snapshot,
      snapshot_signature: signSnapshot(snapshot),
      acceptance_results: contract.acceptance_commands.map(command => ({ ...command, status: "passed", exit_code: 0 })),
      logs: [{ step: "checkout", status: "passed" }, { step: "acceptance", status: "passed" }],
    },
  });
  await page.reload();
  await expect(page.getByText("ready", { exact: true }).first()).toBeVisible();
  await expect(page.getByRole("button", { name: "Create WorkItem" })).toBeVisible();

  await page.getByRole("button", { name: "Create WorkItem" }).click();
  await page.getByLabel("Mutable Repository").selectOption(repositoryId);
  await expect(page.getByText(mergeSha, { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Continue" }).click();
  await page.getByLabel("Title").fill("Add a bounded validation helper");
  await page.getByLabel("Bounded intent").fill("Add one pure validation helper, tests, and documentation without changing deployment state.");
  await page.getByLabel("unit", { exact: false }).check();
  await page.getByLabel("compile", { exact: false }).check();
  await page.getByRole("button", { name: "Continue" }).click();
  await page.getByRole("button", { name: "Run preflight" }).click();
  await expect(page.getByRole("heading", { name: "Read-only preflight and final summary" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Later authorization boundaries" })).toBeVisible();
  await expect(page.getByText("PHarness observes but never performs the source merge")).toBeVisible();
  await page.getByRole("button", { name: "Confirm and create WorkItem" }).click();
  await expect(page).toHaveURL(/#\/work-items\/witem_[^/]+\/overview$/);
  const workItemId = page.url().split("/").at(-2);

  await confirmAction(page, "start planner", "Start the bounded Planner from sealed readiness evidence");
  let workFlow = await api(`/api/work-items/${workItemId}/flow`);
  let stageRun = workFlow.repo_mode.stage_executions.find(execution => execution.stage_key === "plan").run_id;
  let run = await attemptRun(stageRun);
  await submitRunDocument(run, "work_plan", {
    title: "Implement and verify the bounded helper",
    summary: "Add one source helper, standard-library tests, and documentation, then execute both declared commands.",
    risk_level: "low",
    steps: [
      { title: "Implement", description: "Add the pure validation helper.", paths: ["src/validator.py"] },
      { title: "Test", description: "Add tests and run declared acceptance.", paths: ["tests/test_validator.py"], acceptance_names: ["unit", "compile"] },
      { title: "Document", description: "Update usage guidance.", paths: ["readme.md"] },
    ],
    assumptions: [],
    risks: [],
  });
  await page.reload();
  await expect(page.getByRole("button", { name: "approve work plan" })).toBeVisible();
  await confirmAction(page, "approve work plan", "Approve the exact Planner WorkPlan revision");
  await confirmAction(page, "authorize stage chain", "Authorize the bounded Builder Tester Verifier chain");

  workFlow = await api(`/api/work-items/${workItemId}/flow`);
  let implementExecution = workFlow.repo_mode.stage_executions.find(execution => execution.stage_key === "implement");
  run = await attemptRun(implementExecution.run_id);
  const source = run.execution_target.workspace_source;
  await api(`/api/internal/runs/${run.id}/workspace-provisioned`, {
    method: "POST",
    worker: true,
    body: { workspace_id: source.workspace_id, resolved_commit: mergeSha, branch: source.branch },
  });
  await api(`/api/internal/runs/${run.id}/environment-preparation`, {
    method: "POST",
    worker: true,
    body: {
      status: "succeeded",
      project_contract: contract,
      project_contract_hash: snapshot.manifest_sha256,
      environment_snapshot: snapshot,
      snapshot_signature: signSnapshot(snapshot),
      logs: [{ step: "checkout", status: "passed" }, { step: "dependencies", status: "passed" }],
    },
  });
  await api(`/api/internal/runs/${run.id}/mark-running`, { method: "POST", body: {}, worker: true });
  const builderDiff = [
    "diff --git a/src/validator.py b/src/validator.py",
    "new file mode 100644",
    "--- /dev/null",
    "+++ b/src/validator.py",
    "@@ -0,0 +1 @@",
    "+def validate(value): return value",
    "",
  ].join("\n");
  await api(`/api/internal/runs/${run.id}/outcome`, {
    method: "POST",
    worker: true,
    body: completedOutcome(run, "Implemented the bounded validation helper", {
      workspace_id: source.workspace_id,
      base_commit: mergeSha,
      branch: source.branch,
      status: " M readme.md\n M src/validator.py\n M tests/test_validator.py",
      diff: builderDiff,
      changed_paths: ["readme.md", "src/validator.py", "tests/test_validator.py"],
    }),
  });

  workFlow = await api(`/api/work-items/${workItemId}/flow`);
  let testExecution = workFlow.repo_mode.stage_executions.find(execution => execution.stage_key === "test");
  run = await attemptRun(testExecution.run_id);
  await api(`/api/internal/runs/${run.id}/mark-running`, { method: "POST", body: {}, worker: true });
  await api(`/api/internal/runs/${run.id}/events`, {
    method: "POST",
    worker: true,
    body: { events: contract.acceptance_commands.map((command, index) => ({
      event_id: `evt_acceptance_${index}_${Date.now()}`,
      session_id: run.session_id,
      run_id: run.id,
      seq: 10 + index,
      type: "tool.finished",
      payload: { status: "ok", summary: `${command.name} passed`, content: { acceptance_command: true, name: command.name, command: command.command, exit_code: 0, duration_ms: 25 } },
    })) },
  });
  await api(`/api/internal/runs/${run.id}/events`, {
    method: "POST",
    worker: true,
    body: { events: [{
      event_id: `evt_test_outcome_${Date.now()}`,
      session_id: run.session_id,
      run_id: run.id,
      seq: 100,
      type: "tool.finished",
      payload: { status: "ok", content: { structured_submission: true, kind: "test_outcome", document: { summary: "Both declared commands passed.", acceptance_names: ["unit", "compile"], claims: [], risks: [] } } },
    }] },
  });
  await api(`/api/internal/runs/${run.id}/outcome`, { method: "POST", worker: true, body: completedOutcome(run, "Tester completed declared acceptance") });

  workFlow = await api(`/api/work-items/${workItemId}/flow`);
  let verifyExecution = workFlow.repo_mode.stage_executions.find(execution => execution.stage_key === "verify");
  run = await attemptRun(verifyExecution.run_id);
  await submitRunDocument(run, "verification", {
    decision: "approved",
    summary: "The reviewed diff and both controller-sealed acceptance results satisfy the WorkPlan.",
    evidence_refs: [implementExecution.id, testExecution.id],
    contradictions: [],
    risks: [],
  });
  await page.reload();
  await expect(page.getByRole("button", { name: "approve change set" })).toBeVisible();
  await confirmAction(page, "approve change set", "Approve the exact controller-derived ChangeSet");
  await confirmAction(page, "authorize source delivery", "Create one exact source pull request from the approved ChangeSet");

  workFlow = await api(`/api/work-items/${workItemId}/flow`);
  intent = workFlow.repo_mode.source_delivery_intent;
  await api(`/api/internal/source-delivery-intents/${intent.id}/writer-outcome`, {
    method: "POST",
    worker: true,
    body: { execution_id: intent.writer_execution_id, status: "completed", branch: intent.head_branch, commit_sha: headSha, pull_request_url: `${repositoryUrl}/pull/8`, pull_request_number: 8 },
  });
  await page.reload();
  await confirmAction(page, "observe source delivery", "Observe the exact source pull request and required checks");
  workFlow = await api(`/api/work-items/${workItemId}/flow`);
  intent = workFlow.repo_mode.source_delivery_intent;
  await api(`/api/internal/source-delivery-intents/${intent.id}/observation-outcome`, {
    method: "POST",
    worker: true,
    body: { execution_id:intent.observer_execution_id,status:"observed",pull_request_state:"open",merged:false,head_branch:intent.head_branch,head_commit_sha:headSha,authoritative_rules_succeeded:true,required_checks:[],check_runs:[],commit_statuses:[],provider_check_status:"passing" },
  });
  await page.reload();
  await confirmAction(page, "observe source delivery", "Observe the exact manually merged source pull request");
  workFlow = await api(`/api/work-items/${workItemId}/flow`);
  intent = workFlow.repo_mode.source_delivery_intent;
  await api(`/api/internal/source-delivery-intents/${intent.id}/observation-outcome`, {
    method: "POST",
    worker: true,
    body: { execution_id:intent.observer_execution_id,status:"observed",pull_request_state:"closed",merged:true,merge_commit_sha:"e".repeat(40),head_branch:intent.head_branch,head_commit_sha:headSha,authoritative_rules_succeeded:true,required_checks:[],check_runs:[],commit_statuses:[],provider_check_status:"passing" },
  });
  await page.goto(`/#/work-items/${workItemId}/delivery`);
  await page.reload();
  await expect(page.getByText("Source Delivery succeeded", { exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Release" }).locator("..").getByText("inapplicable", { exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Observe" }).locator("..").getByText("inapplicable", { exact: true })).toBeVisible();
  await stabilizeCompletedJourneySnapshot(page);
  await expect(page).toHaveScreenshot(`${laminaEnabled ? "lamina" : "repo-mode"}-real-completed-desktop.png`, { fullPage:true });

  await page.setViewportSize({ width:390, height:844 });
  await page.goto(`/#/work-items/${workItemId}/delivery`);
  await expect(page.getByText("Source Delivery succeeded", { exact:true })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);
  await stabilizeCompletedJourneySnapshot(page);
  await expect(page).toHaveScreenshot(`${laminaEnabled ? "lamina" : "repo-mode"}-real-completed-mobile.png`, { fullPage:true });
  const finalFlow = await api(`/api/work-items/${workItemId}/flow`);
  const repeatedFlow = await api(`/api/work-items/${workItemId}/flow`);
  expect(finalFlow.repo_mode.state_hash).toBe(repeatedFlow.repo_mode.state_hash);
  expect(finalFlow.repo_mode.history).toEqual(repeatedFlow.repo_mode.history);
  expect(finalFlow.action_rail).toEqual(repeatedFlow.action_rail);
  const timeline = finalFlow.repo_mode.lifecycle_timeline;
  expect(timeline.intervals).toEqual(repeatedFlow.repo_mode.lifecycle_timeline.intervals);
  expect(timeline.intervals.some(interval => interval.kind === "delivery_wait" && interval.started_at && interval.finished_at)).toBe(true);
  if (laminaEnabled) {
    await page.goto(`/#/work-items/${workItemId}/overview`);
    await expect(page.getByRole("heading", {name:"WorkItem activity"})).toBeVisible();
    await expect(page.locator(".lamina-lane")).toHaveCount(6);
  }
});

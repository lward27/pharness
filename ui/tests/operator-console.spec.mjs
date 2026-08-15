import { expect, test } from "@playwright/test";

function emptyPayload(pathname) {
  if (pathname === "/health") return { status: "ok" };
  if (pathname === "/api/config/effective") return { operator: { name: "lucas" } };
  if (pathname === "/api/runs") return { runs: [] };
  if (pathname === "/api/runs/summary") return { summary: { by_status: [] } };
  if (pathname === "/api/approvals") return { approvals: [] };
  if (pathname === "/api/approval-gates") return { approval_gates: [] };
  if (pathname === "/api/audit-events") return { events: [] };
  if (pathname === "/api/work-plans") return { work_plans: [] };
  if (pathname === "/api/change-sets") return { change_sets: [] };
  if (pathname === "/api/incidents") return { incidents: [] };
  if (pathname === "/api/remediation-plans") return { remediation_plans: [] };
  if (pathname === "/api/observations") return { observations: [] };
  if (pathname === "/api/work-items") return { work_items: [], operator_state: {} };
  if (pathname === "/api/scopes/options") return { environments: [], namespaces: [], repositories: [], branches: [], actors: [], origins: ["legacy"] };
  if (pathname === "/api/triage") return { items: [], summary: {} };
  if (pathname === "/api/triage/summary") return {};
  return {};
}

async function mockApi(page, overrides = {}) {
  await page.clock.install({ time: new Date("2026-08-06T12:00:00Z") });
  await page.route("**/*", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (!path.startsWith("/api/")) {
      await route.fallback();
      return;
    }
    const override = overrides[path];
    const payload = typeof override === "function" ? await override(route, path) : override ?? emptyPayload(path);
    await route.fulfill({ contentType: "application/json", body: JSON.stringify(payload) });
  });
  await page.route("**/health", async (route) => {
    await route.fulfill({ contentType: "application/json", body: JSON.stringify(overrides["/health"] ?? emptyPayload("/health")) });
  });
}

function workItemFixture(id = "witem_preview_1234567890") {
  return {
    id, status: "blocked", title: "Repair finance price cache", intent: "Restore a test-backed price cache.",
    acceptance_criteria: ["tests pass"], source_repo: "https://github.com/lward27/yfinance_wrapper.git", source_ref: "main",
    target_environment: "dev", production_impacting: false, max_attempts: 3, attempt_count: 1, created_by: "lucas", origin: "operator",
    created_at: "1760000000000", updated_at: "1760000100000", status_changed_at: "1760000100000", status_reason: "Git writer authorization is missing.",
  };
}

test("empty triage is honest and free of inventory placeholders", async ({ page }) => {
  await mockApi(page);
  await page.goto("/#/triage");
  await expect(page.getByRole("heading", { name: "Triage" })).toBeVisible();
  await expect(page.getByText("Nothing needs attention")).toBeVisible();
  await expect(page.getByText("No live data")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Refresh", exact: true })).toBeVisible();
  await expect(page).toHaveScreenshot("triage-empty.png", { fullPage: true });
});

test("blocked WorkItems retain a visible boundary and origin", async ({ page }) => {
  const workItem = workItemFixture("witem_blocked_1234567890");
  await mockApi(page, {
    "/api/work-items": { work_items: [workItem], operator_state: { [workItem.id]: { current_boundary: "git_delivery", attention_reason: "Git writer authorization is missing.", attempts_remaining: 2 } } },
    "/api/scopes/options": { environments: ["dev"], namespaces: [], repositories: [workItem.source_repo], branches: ["main"], actors: ["lucas"], origins: ["operator"] },
  });
  await page.goto("/#/work-items");
  await expect(page.getByRole("heading", { name: "WorkItems" })).toBeVisible();
  await expect(page.getByText("Repair finance price cache")).toBeVisible();
  await expect(page.getByText("Git writer authorization is missing.")).toBeVisible();
  await expect(page.getByText("2 attempts left")).toBeVisible();
  await expect(page).toHaveScreenshot("work-items-blocked.png", { fullPage: true });
});

test("WorkItem filters and pages are server-backed with exact totals", async ({ page }) => {
  const workItem = workItemFixture("witem_server_1234567890");
  const requests = [];
  await mockApi(page, {
    "/api/work-items": async (route) => {
      const query = Object.fromEntries(new URL(route.request().url()).searchParams.entries());
      requests.push(query);
      return { work_items: [workItem], operator_state: { [workItem.id]: { current_boundary: "git_delivery", attempts_remaining: 2 } }, count: 30, limit: Number(query.limit ?? 25), offset: Number(query.offset ?? 0) };
    },
    "/api/scopes/options": { environments: ["dev"], namespaces: [], repositories: [workItem.source_repo], branches: ["main"], actors: ["lucas"], origins: ["operator"] },
  });
  await page.goto("/#/work-items");
  await page.getByLabel("Actor").selectOption("lucas");
  await expect.poll(() => requests.some((query) => query.actor === "lucas" && query.include === "operator_state")).toBe(true);
  await page.getByRole("button", { name: "Next" }).click();
  await expect.poll(() => requests.some((query) => query.actor === "lucas" && query.offset === "25")).toBe(true);
});

test("opening a blocked WorkItem only performs a read-only reconcile preview", async ({ page }) => {
  const workItem = workItemFixture();
  const reconcileCalls = [];
  await mockApi(page, {
    [`/api/work-items/${workItem.id}`]: workItem,
    [`/api/work-items/${workItem.id}/flow`]: { work_item: workItem, workspaces: [], controller_waits: [], delivery_segments: [] },
    [`/api/work-items/${workItem.id}/reconcile`]: async (route) => {
      const request = route.request().postDataJSON();
      reconcileCalls.push(request);
      return { can_apply: false, boundary: "git_delivery", effect_summary: "No Git writer will be dispatched.", blockers: [{ summary: "Git writer authorization is missing." }] };
    },
  });
  await page.goto(`/#/work-items/${workItem.id}`);
  await expect(page.getByRole("heading", { name: workItem.title })).toBeVisible();
  await expect(page.locator(".reconcile-panel button.primary-action")).toBeDisabled();
  await expect(page.getByText("Git writer authorization is missing.")).toBeVisible();
  expect(reconcileCalls.length).toBeGreaterThan(0);
  expect(reconcileCalls.every((request) => request.apply === false)).toBe(true);
  await expect(page).toHaveScreenshot("work-item-preview-blocked.png", { fullPage: true });
});

test("an apply-ready WorkItem requires confirmation and dispatches one controller action", async ({ page }) => {
  const workItem = workItemFixture("witem_apply_1234567890");
  const applyCalls = [];
  await mockApi(page, {
    [`/api/work-items/${workItem.id}`]: { ...workItem, status: "executing" },
    [`/api/work-items/${workItem.id}/flow`]: { work_item: workItem, workspaces: [], controller_waits: [], delivery_segments: [] },
    [`/api/work-items/${workItem.id}/reconcile`]: async (route) => {
      const request = route.request().postDataJSON();
      if (request.apply) {
        applyCalls.push(request);
        return { status: "accepted" };
      }
      return { can_apply: true, action: "execute coding attempt", boundary: "coding", effect_summary: "Dispatch one bounded coding attempt.", blockers: [] };
    },
  });
  await page.goto(`/#/work-items/${workItem.id}`);
  await page.getByRole("button", { name: "Execute Coding Attempt" }).click();
  await expect(page.getByText("Confirm controller action")).toBeVisible();
  await page.getByLabel("Reason").fill("reviewed bounded coding attempt");
  await page.getByRole("button", { name: "Confirm and apply" }).click();
  await expect.poll(() => applyCalls.length).toBe(1);
  expect(applyCalls[0]).toMatchObject({ apply: true, actor: "lucas", reason: "reviewed bounded coding attempt" });
});

test("WorkItem detail exposes active wait timing and advisory attempt history", async ({ page }) => {
  const workItem = workItemFixture("witem_history_1234567890");
  const wait = { id: "wait_123", status: "active", wait_kind: "pipeline_execution", check_count: 2, max_checks: 6, next_check_at: "1786032060000", deadline_at: "1786035600000" };
  await mockApi(page, {
    [`/api/work-items/${workItem.id}`]: workItem,
    [`/api/work-items/${workItem.id}/reconcile`]: { can_apply: false, boundary: "pipeline_execution", effect_summary: "Observe the declared PipelineRun.", controller_wait: wait, blockers: [] },
    [`/api/work-items/${workItem.id}/flow`]: {
      work_item: workItem, workspaces: [], controller_waits: [wait], delivery_segments: [],
      audit_events: [{ id: "aud_attempt", kind: "work_item.attempt_finished", run_id: "run_attempt_1234567890", created_at: "1786032000000", payload: { outcome: { status: "failed", turns: 8 }, classification: { code: "policy_denied", recommended_action: "revise_plan_or_authorization" } } }],
    },
  });
  await page.goto(`/#/work-items/${workItem.id}`);
  await expect(page.getByText("Next observation")).toBeVisible();
  await expect(page.getByText("Policy Denied")).toBeVisible();
  await expect(page.getByText(/Recommended action: Revise Plan Or Authorization/)).toBeVisible();
  await expect(page.getByText(/advisory evidence/)).toBeVisible();
});

test("batch gate decisions require an operator reason before any mutation", async ({ page }) => {
  const gate = {
    id: "gate_batch_1234567890", status: "pending", origin: "operator", gate_kind: "release",
    gate_order: 1, title: "Review disposable release", summary: "Review evidence before delivery.", risk_level: "medium",
    remediation_plan_id: "rplan_1234567890", incident_id: null, resource_kind: "release", resource_name: "finance-api",
    created_at: "1760000000000", gate_json: {},
  };
  const batchCalls = [];
  await mockApi(page, {
    "/api/approval-gates": { approval_gates: [gate], groups: [] },
    "/api/scopes/options": { environments: [], namespaces: [], repositories: [], branches: [], actors: ["lucas"], origins: ["operator"] },
    "/api/approval-gates/batch-decide": async (route) => {
      batchCalls.push(route.request().postDataJSON());
      return { approval_gates: [{ ...gate, status: "satisfied" }] };
    },
  });
  await page.goto("/#/gates");
  await page.getByRole("checkbox", { name: /Select Review disposable release/ }).check();
  await expect(page.getByRole("button", { name: "Apply to selected" })).toBeDisabled();
  await page.getByLabel("Reason").fill("reviewed as one bounded disposable release");
  await expect(page.getByRole("button", { name: "Apply to selected" })).toBeEnabled();
  await page.getByRole("button", { name: "Apply to selected" }).click();
  await expect.poll(() => batchCalls.length).toBe(1);
  expect(batchCalls[0]).toMatchObject({ gate_ids: [gate.id], decision: "satisfied", decided_by: "lucas", reason: "reviewed as one bounded disposable release" });
});

test("a single gate decision requires attributable rationale", async ({ page }) => {
  const gate = {
    id: "gate_single_1234567890", status: "pending", origin: "operator", gate_kind: "release", gate_order: 1,
    title: "Review one release", summary: "Review durable evidence.", risk_level: "medium", remediation_plan_id: "rplan_1234567890",
    incident_id: null, resource_kind: "release", resource_name: "finance-api", created_at: "1760000000000", gate_json: {},
  };
  const calls = [];
  await mockApi(page, {
    "/api/approval-gates": { approval_gates: [gate], groups: [], count: 1, limit: 25, offset: 0 },
    [`/api/approval-gates/${gate.id}/waive`]: async (route) => { calls.push(route.request().postDataJSON()); return { approval_gate: { ...gate, status: "waived" } }; },
  });
  await page.goto("/#/gates");
  await expect(page.getByRole("button", { name: "Waive" })).toBeDisabled();
  await page.getByLabel("Rationale").fill("reviewed bounded release evidence");
  await page.getByRole("button", { name: "Waive" }).click();
  await expect.poll(() => calls.length).toBe(1);
  expect(calls[0]).toMatchObject({ decided_by: "lucas", reason: "reviewed bounded release evidence" });
});

test("approval filters and pages are server-backed", async ({ page }) => {
  const approval = { id: "appr_server_1234567890", run_id: "run_server_1234567890", status: "pending", kind: "file_write", summary: "Write a bounded finance note", risk_level: "medium", origin: "operator", created_by: "lucas", requested_at: "1760000000000" };
  const requests = [];
  await mockApi(page, {
    "/api/approvals": async (route) => {
      const query = Object.fromEntries(new URL(route.request().url()).searchParams.entries());
      requests.push(query);
      return { approvals: [approval], groups: [], count: 30, limit: Number(query.limit ?? 25), offset: Number(query.offset ?? 0) };
    },
    "/api/scopes/options": { environments: [], namespaces: [], repositories: [], branches: [], actors: ["lucas"], origins: ["operator"] },
  });
  await page.goto("/#/approvals");
  await expect(page.getByText("Write a bounded finance note").first()).toBeVisible();
  await page.getByLabel("Actor").selectOption("lucas");
  await expect.poll(() => requests.some((query) => query.actor === "lucas" && query.status === "pending")).toBe(true);
  await page.getByRole("button", { name: "Next" }).click();
  await expect.poll(() => requests.some((query) => query.actor === "lucas" && query.offset === "25")).toBe(true);
});

test("run filters, search, and pages are server-backed", async ({ page }) => {
  const run = { id: "run_server_1234567890", status: "running", task: "Inspect finance deployment", origin: "operator", created_by: "lucas", started_at: "1760000000000", scope: { namespace: "apps-dev", repo: "team/finance-api", branch: "main" } };
  const requests = [];
  await mockApi(page, {
    "/api/runs": async (route) => {
      const query = Object.fromEntries(new URL(route.request().url()).searchParams.entries());
      requests.push(query);
      return { runs: [run], groups: [], count: 30, limit: Number(query.limit ?? 25), offset: Number(query.offset ?? 0) };
    },
    "/api/scopes/options": { environments: [], namespaces: ["apps-dev"], repositories: ["team/finance-api"], branches: ["main"], actors: ["lucas"], origins: ["operator"] },
  });
  await page.goto("/#/queue");
  await page.getByLabel("Actor").selectOption("lucas");
  await page.getByLabel("Origin").selectOption("operator");
  await page.getByLabel("Search", { exact: true }).fill("finance");
  await expect.poll(() => requests.some((query) => query.actor === "lucas" && query.origin === "operator" && query.search === "finance")).toBe(true);
  await page.getByRole("button", { name: "Next" }).click();
  await expect.poll(() => requests.some((query) => query.search === "finance" && query.offset === "25")).toBe(true);
});

test("WorkPlan provenance filters use API-returned actor and origin", async ({ page }) => {
  const plan = {
    id: "wplan_operator_1234567890", status: "approved", title: "Review finance cache delivery", summary: "A bounded operator-owned plan.",
    risk_level: "medium", requires_approval: true, revision: 1, work_item_id: "witem_operator_1234567890",
    resource_kind: "application", resource_name: "finance-api", created_by: "lucas", origin: "operator", created_at: "1760000000000",
  };
  await mockApi(page, {
    "/api/work-plans": { work_plans: [plan], groups: [] },
    "/api/scopes/options": { environments: [], namespaces: [], repositories: [], branches: [], actors: ["lucas"], origins: ["legacy", "operator"] },
    [`/api/work-plans/${plan.id}/flow`]: { work_plan: plan, readiness: { ready: true, blockers: [], warnings: [] } },
  });
  await page.goto("/#/workplans");
  await expect(page.getByRole("heading", { name: plan.title })).toBeVisible();
  await page.getByLabel("Origin").selectOption("legacy");
  await expect(page.getByText("No WorkPlans match these filters")).toBeVisible();
  await page.getByLabel("Origin").selectOption("operator");
  await expect(page.getByRole("heading", { name: plan.title })).toBeVisible();
  await page.getByLabel("Actor").selectOption("lucas");
  await expect(page.getByRole("heading", { name: plan.title })).toBeVisible();
});

test("Flow contains only actionable evidence and timeline controls", async ({ page }) => {
  const changeSet = { id: "cset_flow_1234567890", status: "approved", title: "Finance cache change", summary: "A bounded source change.", risk_level: "medium", resource_kind: "application", resource_name: "finance-api", created_at: "1760000000000" };
  const flow = {
    resource_kind: "change_set", resource_id: changeSet.id, change_set: changeSet, readiness: { summary: "Ready for review", blockers: [], warnings: [] }, audit_events: [],
    delivery_segments: [
      { key: "source", label: "Source", status: "complete", summary: "Immutable source evidence is recorded.", resources: [{ kind: "change_set", id: changeSet.id, label: "ChangeSet", summary: changeSet.summary }] },
      { key: "build", label: "Build", status: "active", summary: "Awaiting PipelineIntent.", resources: [] },
      { key: "gitops", label: "GitOps", status: "unreached", summary: "Awaiting build evidence.", resources: [] },
      { key: "deploy", label: "Deploy", status: "unreached", summary: "Awaiting GitOps merge.", resources: [] },
      { key: "verify", label: "Verify", status: "unreached", summary: "Awaiting deployment.", resources: [] },
    ],
  };
  await mockApi(page, {
    "/api/change-sets": { change_sets: [changeSet] },
    [`/api/change-sets/${changeSet.id}/flow`]: flow,
  });
  await page.goto(`/#/flow/change_set/${changeSet.id}`);
  await expect(page.getByRole("heading", { name: /Change Set Flow/ })).toBeVisible();
  await expect(page.getByLabel("WorkItem delivery chain").getByText("Source", { exact: true })).toBeVisible();
  await expect(page.getByLabel("WorkItem delivery chain").getByText("GitOps", { exact: true })).toBeVisible();
  await expect(page.getByText("Unreached", { exact: true }).first()).toBeVisible();
  await expect(page.locator(".topology")).toHaveCount(0);
  await page.getByRole("button", { name: /ChangeSet/ }).click();
  await expect(page.getByRole("complementary", { name: "Flow resource detail" })).toBeVisible();
  await page.getByRole("button", { name: "Close flow resource detail" }).click();
  await expect(page.getByRole("complementary", { name: "Flow resource detail" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Export evidence" })).toHaveCount(0);
  await expect(page.locator(".timeline-wrap input[type='checkbox']")).toHaveCount(0);
});

test("Run Detail renders only durable execution evidence", async ({ page }) => {
  const runId = "run_durable_1234567890";
  await mockApi(page, {
    [`/api/runs/${runId}`]: {
      id: runId, status: "completed", task: "Inspect a completed delivery", max_turns: 8,
      started_at: "1760000000000", finished_at: "1760000010000", origin: "operator",
      scope: { namespace: "apps-dev", repo: "team/finance-api", branch: "main" },
      result: { status: "completed", turns: 2, summary: "Captured bounded deployment evidence." },
    },
    [`/api/runs/${runId}/events`]: { events: [{ event_id: "evt_1", seq: 1, type: "tool.finished", payload: { summary: "Read deployment status" } }] },
    [`/api/runs/${runId}/diff`]: { run_id: runId, changes: [{ id: "chg_1", path: "README.md", diff: "+evidence", created_at: "1760000005000" }], diff: "+evidence" },
    [`/api/runs/${runId}/artifacts`]: { artifacts: [{ id: "art_1", kind: "observation", label: "Deployment", content_text: "Deployment is healthy." }] },
  });
  await page.goto(`/#/runs/${runId}`);
  await expect(page.getByRole("heading", { name: "Run Detail" })).toBeVisible();
  await expect(page.getByText("Captured bounded deployment evidence.")).toBeVisible();
  await expect(page.getByText("Read deployment status")).toBeVisible();
  await expect(page.getByText("Deployment is healthy.")).toBeVisible();
  await expect(page).toHaveScreenshot("run-detail-durable.png", { fullPage: true });
});

test("production WorkItem wizard blocks until immutable preflight and warning acknowledgement", async ({ page }) => {
  const sourceSha = "a".repeat(40);
  const submitted = [];
  const pipeline = { id: "pcontract_yfinance", namespace: "tekton-pipelines", pipeline_ref: "pharness-yfinance-build", status: "active" };
  const deployment = { id: "dcontract_yfinance", environment: "production", namespace: "apps-prod", status: "active" };
  await mockApi(page, {
    "/api/pipeline-contracts": { pipeline_contracts: [pipeline] },
    "/api/deployment-contracts": { deployment_contracts: [deployment] },
    "/api/system/readiness": { capabilities: [{ capability: "gitops_writer", status: "available" }] },
    "/api/work-items/preflight": {
      ready: true,
      state_hash: "state-production-1",
      checks: [{ capability: "gitops_writer", status: "available", summary: "Verified with the isolated writer identity." }],
      blockers: [],
      warnings: ["Production effects still require explicit lifecycle approvals."],
      predicted_external_mutations: ["Create a source pull request", "Create a GitOps pull request", "Sync Argo Application yfinance-wrapper"],
    },
    "/api/work-items": async (route) => {
      if (route.request().method() === "POST") {
        submitted.push(route.request().postDataJSON());
        return { id: "witem_production_1234567890" };
      }
      return emptyPayload("/api/work-items");
    },
    "/api/work-items/witem_production_1234567890": workItemFixture("witem_production_1234567890"),
  });

  await page.goto("/#/work-items/new");
  await expect(page.getByRole("heading", { name: "New WorkItem" })).toBeVisible();
  await page.getByLabel("Full source commit SHA").fill(sourceSha);
  await page.getByRole("button", { name: /Continue/ }).click();
  await expect(page.getByRole("textbox", { name: "Acceptance command 1", exact: true })).toHaveValue("python -m unittest discover -s tests -v");
  await page.getByRole("button", { name: /Continue/ }).click();
  await expect(page.getByRole("textbox", { name: "Environment", exact: true })).toHaveValue("production");
  await page.getByRole("button", { name: /Continue/ }).click();
  await page.getByLabel("PipelineContract").selectOption(pipeline.id);
  await page.getByLabel("DeploymentContract").selectOption(deployment.id);
  await page.getByRole("button", { name: "Run read-only preflight" }).click();
  await expect(page.getByText("Submission ready")).toBeVisible();
  await page.getByRole("button", { name: "Review mutations" }).click();
  await expect(page.getByRole("button", { name: "Create supervised WorkItem" })).toBeDisabled();
  await page.getByLabel("I acknowledge the non-blocking preflight warnings.").check();
  await page.getByRole("button", { name: "Create supervised WorkItem" }).click();
  await expect.poll(() => submitted.length).toBe(1);
  expect(submitted[0]).toMatchObject({ source_commit: sourceSha, preflight_state_hash: "state-production-1", pipeline_contract_id: pipeline.id, deployment_contract_id: deployment.id });
});

test("release readiness reports an API and UI revision mismatch without claiming availability", async ({ page }) => {
  await mockApi(page, {
    "/api/system/readiness": {
      api_revision: "b".repeat(40), ui_revision: "b".repeat(40), runtime_image_digest: `sha256:${"c".repeat(64)}`,
      ui_image_digest: `sha256:${"d".repeat(64)}`, platform_versions_match: false,
      capabilities: [{ capability: "gitops_writer", status: "configured_unverified", summary: "Configured but not verified with its isolated identity." }],
      repository_allowlists: { gitops_writer: ["https://github.com/lward27/lucas_engineering.git"] },
      targets: [{ environment: "production", namespace: "apps-prod", application: "yfinance-wrapper" }],
      blockers: ["GitOps writer verification is not fresh."],
    },
  });
  await page.goto("/#/status");
  await expect(page.getByText("Version alignment")).toBeVisible();
  await expect(page.getByText("Mismatch")).toBeVisible();
  await expect(page.getByText("Configured Unverified")).toBeVisible();
  await expect(page.getByText("GitOps writer verification is not fresh.")).toBeVisible();
});

test("production action confirmation is bound to the exact server action and state hash", async ({ page }) => {
  const workItem = { ...workItemFixture("witem_action_1234567890"), status: "executing", target_environment: "production", target_namespace: "apps-prod", production_impacting: true };
  const action = { id: "dispatch_argo_sync", lifecycle_stage: "deployment", resource: "yfinance-wrapper", status: "ready", effect_class: "external", blockers: [], approval_requirements: ["production_deployment"], external_effect_summary: "Sync exact Argo Application yfinance-wrapper in apps-prod.", state_hash: "state-argo-123" };
  const calls = [];
  await mockApi(page, {
    [`/api/work-items/${workItem.id}`]: workItem,
    [`/api/work-items/${workItem.id}/flow`]: { work_item: workItem, workspaces: [], controller_waits: [], delivery_segments: [], action_rail: [action] },
    [`/api/work-items/${workItem.id}/reconcile`]: { can_apply: true, action: "dispatch_argo_sync", boundary: "deployment", effect_summary: action.external_effect_summary, blockers: [] },
    [`/api/work-items/${workItem.id}/actions/${action.id}/execute`]: async (route) => { calls.push(route.request().postDataJSON()); return { status: "accepted" }; },
    [`/api/work-items/${workItem.id}/rollback-intents`]: { content: { status: "ready_for_writer", baseline: { image_digest: `sha256:${"e".repeat(64)}` }, authorization_expires_at: "1786033800000" } },
  });
  await page.goto(`/#/work-items/${workItem.id}`);
  await page.getByRole("button", { name: "Dispatch Argo Sync" }).click();
  await expect(page.locator(".reconcile-confirmation").getByText("Sync exact Argo Application yfinance-wrapper in apps-prod.")).toBeVisible();
  await expect(page).toHaveScreenshot("production-action-confirmation.png", { fullPage: true });
  await page.getByLabel("Reason").fill("reviewed exact production target and digest");
  await page.getByRole("button", { name: "Confirm and apply" }).click();
  await expect.poll(() => calls.length).toBe(1);
  expect(calls[0]).toMatchObject({ reason: "reviewed exact production target and digest", state_hash: action.state_hash });
});

test("rollback rail exposes exact approvals and digest-bound delivery evidence", async ({ page }) => {
  const workItem = { ...workItemFixture("witem_rollback_1234567890"), status: "failed", target_environment: "production", target_namespace: "apps-prod", production_impacting: true };
  const controllerAction = { id: "review_failure", lifecycle_stage: "release", resource: workItem.id, status: "blocked", effect_class: "internal", blockers: [], approval_requirements: [], external_effect_summary: "Review failed production verification.", state_hash: "state-controller-rollback" };
  const rollbackAction = { id: "approve_rollback_argo_sync", lifecycle_stage: "rollback", resource: "rollback_1234567890", status: "ready", effect_class: "approval_boundary", blockers: [], approval_required: true, approval_requirements: ["production_rollback_deployment", "cluster_mutation", "production_impact"], external_effect_summary: "Open a fresh production rollback window before syncing exact Argo Application yfinance-wrapper.", state_hash: "state-rollback-123" };
  const calls = [];
  await mockApi(page, {
    [`/api/work-items/${workItem.id}`]: workItem,
    [`/api/work-items/${workItem.id}/flow`]: {
      work_item: workItem,
      workspaces: [], controller_waits: [], delivery_segments: [],
      action_rail: [controllerAction, rollbackAction],
      delivery_configuration: {
        pipeline_contract_id: "pcontract-yfinance",
        deployment_contract_id: "dcontract-yfinance",
        gitops: { repository: "https://github.com/lward27/lucas_engineering.git", kustomization_path: "charts/yfinance-wrapper/kustomization.yaml", desired_revision: "a".repeat(40) },
        target: { argo_application: "yfinance-wrapper" },
        argo: { sync_status: "Synced", health_status: "Degraded" },
        current_digest: `sha256:${"b".repeat(64)}`,
        desired_digest: `sha256:${"c".repeat(64)}`,
        baseline_digest: `sha256:${"b".repeat(64)}`,
        production_window_expires_at: "1786033800000",
        rollback_owner: "lucas",
        rollback_status: "ready_for_argo_sync",
      },
    },
    [`/api/work-items/${workItem.id}/reconcile`]: { can_apply: false, action: "review_failure", boundary: "release", effect_summary: controllerAction.external_effect_summary, blockers: [{ code: "verification_failed", summary: "Production verification failed." }] },
    [`/api/work-items/${workItem.id}/actions/${rollbackAction.id}/execute`]: async (route) => { calls.push(route.request().postDataJSON()); return { status: "argo_approved" }; },
    [`/api/work-items/${workItem.id}/rollback-intents`]: { content: { rollback_intent_id: rollbackAction.resource, status: "ready_for_argo_sync", baseline: { image_digest: `sha256:${"b".repeat(64)}` } } },
  });
  await page.goto(`/#/work-items/${workItem.id}`);
  await expect(page.getByText("Approvals: Production Rollback Deployment · Cluster Mutation · Production Impact")).toBeVisible();
  await expect(page.getByText(`sha256:${"b".repeat(64)}`, { exact: true }).first()).toBeVisible();
  await page.getByRole("button", { name: "Review exact action" }).click();
  await expect(page.locator(".reconcile-confirmation").getByText("Open a fresh production rollback window")).toBeVisible();
  await page.getByLabel("Reason").fill("reviewed rollback merge, baseline digest, and exact Argo target");
  await page.getByRole("button", { name: "Confirm and apply" }).click();
  await expect.poll(() => calls.length).toBe(1);
  expect(calls[0]).toMatchObject({ state_hash: rollbackAction.state_hash });
});

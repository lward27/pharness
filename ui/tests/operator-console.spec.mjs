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
  if (pathname === "/api/environment-profiles") return { profiles: [] };
  if (pathname === "/api/system/readiness") return { capabilities: [] };
  if (pathname === "/api/scopes/options") return { environments: [], namespaces: [], repositories: [], branches: [], actors: [], origins: ["legacy"] };
  if (pathname === "/api/triage") return { items: [], summary: {} };
  if (pathname === "/api/triage/summary") return {};
  return {};
}

async function mockApi(page, overrides = {}) {
  // Keep Date deterministic while allowing real timers and mocked polling to
  // run. `clock.install({ time })` advances wall time during the test, which
  // made otherwise identical snapshots alternate between 8:00:01 and
  // 8:00:02 depending on worker load.
  await page.clock.setFixedTime(new Date("2026-08-06T12:00:01Z"));
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

function deliveryFlowFixture(workItem, { verified = false } = {}) {
  const sourceMerge = "b".repeat(40);
  const gitopsMerge = "c".repeat(40);
  const desiredDigest = `sha256:${"d".repeat(64)}`;
  const baselineDigest = `sha256:${"e".repeat(64)}`;
  const artifact = (id, kind, content_json) => ({ id, kind, label: id.replaceAll("_", " "), content_json });
  const flow = {
    work_item: workItem,
    workspaces: [], controller_waits: [], action_rail: [], audit_events: [],
    delivery_segments: [
      { key: "source", label: "Source", status: verified ? "active" : "complete", summary: "Source delivery evidence." },
      { key: "build", label: "Build", status: verified ? "unreached" : "complete", summary: "Build evidence." },
      { key: "gitops", label: "GitOps", status: verified ? "unreached" : "active", summary: "GitOps evidence." },
      { key: "deploy", label: "Deploy", status: "unreached", summary: "Deployment evidence." },
      { key: "verify", label: "Verify", status: "unreached", summary: "Verification evidence." },
    ],
    delivery_configuration: {
      pipeline_contract_id: "pcontract-yfinance",
      deployment_contract_id: "dcontract-yfinance",
      desired_digest: desiredDigest,
      current_digest: baselineDigest,
      baseline_digest: baselineDigest,
      rollback_owner: "lucas",
      rollback_status: "prepared",
      production_window_expires_at: "1786033800000",
      gitops: { repository: "https://github.com/lward27/lucas_engineering.git", kustomization_path: "charts/yfinance-wrapper/kustomization.yaml", desired_revision: verified ? gitopsMerge : null },
      target: { environment: "production", namespace: "apps-prod", workload_name: "yfinance-wrapper", argo_application: "yfinance-wrapper" },
      argo: {},
    },
    sdlc_flow: {
      change_set: { id: "cset_delivery", status: "approved", title: "Yfinance source changes", summary: "Reviewed source diff and acceptance evidence." },
      git_delivery: {
        latest_result: artifact("art_source_pr", "git_delivery_result", { details: { pull_request_number: 2, pull_request_url: "https://github.com/lward27/yfinance_wrapper/pull/2" } }),
        latest_observation: artifact("art_source_observe", "git_delivery_pr_observation", { merged: true, pull_request_number: 2, merge_commit_sha: sourceMerge }),
        latest_merge: artifact("art_source_merge", "git_delivery_merge", { pull_request_number: 2, merge_commit_sha: sourceMerge }),
      },
      pipeline_intent: {
        id: "pint_delivery", status: "approved", title: "Build immutable yfinance image",
        execution_state: { state: "pipeline_run_succeeded", pipeline_run_namespace: "tekton-pipelines", pipeline_run_name: "pharness-yfinance-build-2" },
        execution_evidence: { status: "succeeded" },
        intent_json: {
          execution: { pipeline_ref: "pharness-yfinance-build" },
          evidence: { summary: { pipeline_run_reason: "Succeeded" } },
          build_output: { status: "verified", source_commit: sourceMerge, image_digest: desiredDigest, artifact_id: "art_build", image_ref: `registry.lucas.engineering/yfinance_wrapper@${desiredDigest}` },
          execution_history: [{ status: "failed" }],
        },
      },
      gitops_change_set: { id: "gcset_delivery", status: "approved", title: "Pin yfinance digest", gitops_repo: "https://github.com/lward27/lucas_engineering.git", kustomization_path: "charts/yfinance-wrapper/kustomization.yaml" },
      gitops_delivery: {
        latest_result: artifact("art_gitops_pr", "gitops_delivery_result", { details: { pull_request_number: 26, pull_request_url: "https://github.com/lward27/lucas_engineering/pull/26" } }),
        latest_observation: artifact("art_gitops_observe", "gitops_delivery_pr_observation", verified ? { merged: true, pull_request_number: 26, merge_commit_sha: gitopsMerge } : { merged: false, pull_request_number: 26, pull_request_state: "open" }),
        ...(verified ? { latest_merge: artifact("art_gitops_merge", "gitops_delivery_merge", { pull_request_number: 26, merge_commit_sha: gitopsMerge }) } : {}),
      },
      deployment_intent: { id: "dint_delivery", status: "approved", title: "Sync exact yfinance release", argo_application: "yfinance-wrapper" },
    },
  };
  if (verified) {
    flow.audit_events = [];
    flow.sdlc_flow.release = {
      id: "rel_delivery", status: "completed", title: "Verified yfinance release", image_digest: desiredDigest,
      release_json: { post_sync_verification: {
        status: "verified", deployment_contract_id: "dcontract-yfinance", argo_observation_id: "obs_argo", workload_observation_id: "obs_workload",
        observability: { prometheus_inventory: { observation_id: "obs_prometheus", status: "observed" } },
        checks: [
          { code: "completed_argo_sync", passed: true, summary: "The approved Argo sync completed." },
          { code: "argo_application_synced_healthy", passed: true, summary: "The exact Argo Application is Synced and Healthy." },
          { code: "declared_deployment_rollout_healthy", passed: true, summary: "The declared Deployment is ready." },
          { code: "running_image_digest", passed: true, summary: "The running image digest equals the approved build output." },
          { code: "service_healthz", passed: true, summary: "The bounded /healthz check passed." },
          { code: "prometheus_inventory", passed: true, summary: "Prometheus inventory was recorded." },
        ],
      } },
    };
  }
  return flow;
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

test("primary navigation is grouped without introducing another sidebar", async ({ page }, testInfo) => {
  await mockApi(page);
  await page.goto("/#/triage");

  if (testInfo.project.name === "desktop") {
    await expect(page.getByRole("group", { name: "Operate" })).toBeVisible();
    await expect(page.getByRole("group", { name: "Govern" })).toBeVisible();
    await expect(page.getByRole("group", { name: "Investigate" })).toBeVisible();
    await expect(page.getByRole("group", { name: "Platform" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Tool approvals" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Lifecycle gates" })).toBeVisible();
  } else {
    const navigation = page.getByLabel("Primary navigation");
    await expect(navigation).toBeVisible();
    await navigation.selectOption("WorkItems");
    await expect(page).toHaveURL(/#\/work-items$/);
  }
});

test("WorkItem-specific triage signals collapse into one cockpit route", async ({ page }) => {
  const workItem = workItemFixture("witem_triage_1234567890");
  const triageItems = [
    { id: "gate_source", kind: "approval_gate", title: "Approve source mutation", summary: "Source boundary", status: "pending", risk_level: "high", origin: "operator", resource_kind: "approval_gate", resource_id: "gate_source", work_item_id: workItem.id, created_at: "1760000000000" },
    { id: "gate_pipeline", kind: "approval_gate", title: "Approve pipeline mutation", summary: "Pipeline boundary", status: "pending", risk_level: "high", origin: "operator", resource_kind: "approval_gate", resource_id: "gate_pipeline", work_item_id: workItem.id, created_at: "1760000001000" },
    { id: workItem.id, kind: "blocked_work_item", title: workItem.title, summary: workItem.status_reason, status: "blocked", risk_level: "high", origin: "operator", resource_kind: "work_item", resource_id: workItem.id, work_item_id: workItem.id, created_at: "1760000002000" },
  ];
  await mockApi(page, {
    "/api/triage": { items: triageItems, summary: { total: 3, blocked_work_items: 1, pending_approval_gates: 2, pending_tool_approvals: 0 } },
    "/api/triage/summary": { total: 3, blocked_work_items: 1, pending_approval_gates: 2, pending_tool_approvals: 0 },
    "/api/work-items": { work_items: [workItem], operator_state: {} },
    "/api/scopes/options": { environments: ["dev"], namespaces: [], repositories: [workItem.source_repo], branches: ["main"], actors: ["lucas"], origins: ["operator"] },
  });
  await page.goto("/#/triage");

  await expect(page.locator(".triage-row")).toHaveCount(1);
  await expect(page.getByText("Blocked WorkItem", { exact: true })).toBeVisible();
  await expect(page.getByText("2 lifecycle gates", { exact: true })).toBeVisible();
  await expect(page.getByText("Open WorkItem cockpit", { exact: false })).toBeVisible();
  await expect(page).toHaveScreenshot("triage-exceptions.png", { fullPage: true });
  await page.locator(".triage-row").click();
  await expect(page).toHaveURL(new RegExp(`#\\/work-items\\/${workItem.id}$`));
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
  await expect(page.getByLabel("Current lifecycle boundary").locator("button.primary-action")).toBeDisabled();
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
  await expect(page.getByRole("dialog", { name: "Lifecycle review" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Review controller action" })).toBeVisible();
  await page.getByLabel("Decision reason").fill("reviewed bounded coding attempt");
  await page.getByRole("button", { name: "Confirm Execute Coding Attempt" }).click();
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
  await page.getByLabel("WorkItem sections").getByRole("button", { name: /^Attempt/ }).click();
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

test("future production gates remain visible but cannot be selected or decided", async ({ page }) => {
  const gate = {
    id: "gate_future_1234567890", status: "pending", actionable: false, origin: "controller", gate_kind: "cluster_mutation", gate_order: 5,
    title: "Approve exact Argo sync", summary: "Reserved for the deployment boundary.", risk_level: "high", work_item_id: "witem_future_1234567890",
    remediation_plan_id: null, incident_id: null, resource_kind: "Application", resource_name: "yfinance-wrapper", resource_namespace: "argocd",
    created_at: "1760000000000", gate_json: {}, lifecycle_blocker: "The exact GitOps pull request must be observed merged before cluster production gates can be decided.",
  };
  await mockApi(page, {
    "/api/approval-gates": { approval_gates: [gate], groups: [], count: 1, limit: 25, offset: 0 },
  });
  await page.goto("/#/gates");
  await expect(page.getByText("Future lifecycle gate")).toBeVisible();
  await expect(page.getByText(/exact GitOps pull request must be observed merged/)).toBeVisible();
  await expect(page.getByRole("checkbox", { name: /Select Approve exact Argo sync/ })).toBeDisabled();
  await page.getByLabel("Rationale").fill("must still wait for immutable merge provenance");
  await expect(page.getByRole("button", { name: "Satisfy" })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Waive" })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Reject" })).toBeDisabled();
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
    "/api/environment-profiles": { profiles: [{ id: "python-3.11", status: "available", image: `registry.lucas.engineering/pharness-python-runner@sha256:${"e".repeat(64)}` }] },
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
      normalized_submission: { repository_contract: { dependency_lock: { path: "requirements.lock" }, writable_paths: ["src/**", "tests/**", "readme.md"] } },
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
  await page.getByRole("button", { name: "Advanced budgets" }).click();
  await expect(page.getByLabel("Initial turns")).toHaveValue("48");
  await expect(page.getByLabel("Hard turn maximum")).toHaveValue("100");
  await expect(page.getByLabel("Initial tokens")).toHaveValue("400000");
  await expect(page.getByLabel("Hard token maximum")).toHaveValue("1000000");
  await page.getByRole("button", { name: /Continue/ }).click();
  await page.getByLabel("PipelineContract").selectOption(pipeline.id);
  await page.getByLabel("DeploymentContract").selectOption(deployment.id);
  await page.getByRole("button", { name: "Check readiness" }).click();
  await expect(page.getByText("Submission ready")).toBeVisible();
  await page.getByRole("button", { name: "Review mutations" }).click();
  await expect(page.getByRole("button", { name: "Create supervised WorkItem" })).toBeDisabled();
  await page.getByLabel("I acknowledge the non-blocking preflight warnings.").check();
  await page.getByRole("button", { name: "Create supervised WorkItem" }).click();
  await expect.poll(() => submitted.length).toBe(1);
  expect(submitted[0]).toMatchObject({ source_commit: sourceSha, preflight_state_hash: "state-production-1", pipeline_contract_id: pipeline.id, deployment_contract_id: deployment.id, environment_profile_id: "python-3.11", initial_turn_budget: 48, hard_turn_budget: 100, initial_token_budget: 400000, hard_token_budget: 1000000 });
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
  await expect(page.getByLabel("Effect and authorization").getByText("Sync exact Argo Application yfinance-wrapper in apps-prod.")).toBeVisible();
  await expect(page).toHaveScreenshot("production-action-confirmation.png");
  await page.getByLabel("Decision reason").fill("reviewed exact production target and digest");
  await page.getByRole("button", { name: "Confirm Dispatch Argo Sync" }).click();
  await expect.poll(() => calls.length).toBe(1);
  expect(calls[0]).toMatchObject({ reason: "reviewed exact production target and digest", state_hash: action.state_hash });
});

test("PipelineIntent execution authorization is actionable without starting Tekton", async ({ page }) => {
  const workItem = { ...workItemFixture("witem_pipeline_auth_1234567890"), status: "awaiting_approval", target_environment: "production", target_namespace: "apps-prod", production_impacting: true };
  const sourceSha = "d".repeat(40);
  const action = {
    id: "authorize_pipeline_execution",
    lifecycle_stage: "pipeline",
    resource: "pint_pipeline_auth_1234567890",
    status: "ready",
    effect_class: "approval_boundary",
    blockers: [],
    approval_required: true,
    approval_requirements: ["pipeline_execution_authorization", "pipeline_mutation", "production_impact"],
    external_effect_summary: `Authorize one supervised Tekton execution attempt 2 for exact PipelineIntent pint_pipeline_auth_1234567890 using tekton-pipelines/pharness-yfinance-build at immutable source ${sourceSha}. The grant expires within 30 minutes for production. This action does not start Tekton.`,
    state_hash: "state-pipeline-authorization-2",
  };
  const calls = [];
  await mockApi(page, {
    [`/api/work-items/${workItem.id}`]: workItem,
    [`/api/work-items/${workItem.id}/flow`]: { work_item: workItem, workspaces: [], controller_waits: [], delivery_segments: [], action_rail: [action] },
    [`/api/work-items/${workItem.id}/reconcile`]: { can_apply: false, action: "awaiting_pipeline_execution_authorization", boundary: "pipeline", effect_summary: "A fresh attempt-scoped grant is required.", blockers: [{ code: "awaiting_pipeline_execution_authorization", summary: "A fresh attempt-scoped grant is required." }] },
    [`/api/work-items/${workItem.id}/actions/${action.id}/execute`]: async (route) => { calls.push(route.request().postDataJSON()); return { grant: { id: "pgrant_pipeline_attempt_2", status: "active" } }; },
  });

  await page.goto(`/#/work-items/${workItem.id}`);
  await expect(page.getByRole("button", { name: "Authorize Pipeline Execution", exact: true })).toBeEnabled();
  await page.getByRole("button", { name: "Authorize Pipeline Execution", exact: true }).click();
  await expect(page.getByLabel("Effect and authorization").getByText("This action does not start Tekton.")).toBeVisible();
  await page.getByLabel("Decision reason").fill("authorize only supervised PipelineIntent attempt 2");
  await page.getByRole("button", { name: "Confirm Authorize Pipeline Execution" }).click();
  await expect.poll(() => calls.length).toBe(1);
  expect(calls[0]).toMatchObject({ reason: "authorize only supervised PipelineIntent attempt 2", state_hash: action.state_hash });
});

test("delivery workspace keeps manual merges and external evidence in their owning stages", async ({ page }) => {
  const workItem = {
    ...workItemFixture("witem_delivery_wait_1234567890"),
    status: "executing",
    title: "Deliver yfinance validation",
    source_commit: "a".repeat(40),
    target_environment: "production",
    target_namespace: "apps-prod",
    workload_name: "yfinance-wrapper",
    argo_application: "yfinance-wrapper",
    rollback_owner: "lucas",
    production_impacting: true,
  };
  const flow = deliveryFlowFixture(workItem);
  await mockApi(page, {
    [`/api/work-items/${workItem.id}`]: workItem,
    [`/api/work-items/${workItem.id}/flow`]: flow,
    [`/api/work-items/${workItem.id}/reconcile`]: { can_apply: false, action: "awaiting_gitops_merge", boundary: "gitops", effect_summary: "The GitOps pull request must be merged manually.", blockers: [{ code: "awaiting_external_merge", summary: "Merge GitOps PR #26 in GitHub, then refresh evidence." }] },
    [`/api/work-items/${workItem.id}/rollback-intents`]: { content: { status: "prepared", baseline: { image_digest: `sha256:${"e".repeat(64)}` } } },
  });

  await page.goto(`/#/work-items/${workItem.id}`);
  await page.getByLabel("WorkItem sections").getByRole("button", { name: "Delivery" }).click();
  const workspace = page.getByLabel("Delivery and release workspace");
  await expect(workspace.getByText("2/5 stages evidenced")).toBeVisible();
  await expect(page.getByLabel("Production release guardrails").getByText(/Open until/)).toBeVisible();
  await expect(workspace.locator(".release-stage").nth(0).getByText("PR #2 merged manually", { exact: true }).first()).toBeVisible();
  await expect(workspace.locator(".release-stage").nth(1).getByText("succeeded · 1 earlier failure")).toBeVisible();
  await expect(workspace.locator(".release-stage").nth(2).getByText("PR #26 awaits manual merge", { exact: true }).first()).toBeVisible();
  await expect(workspace.locator(".release-stage").nth(2).getByText("Human boundary")).toBeVisible();
  await expect(page.getByLabel("Production release guardrails")).toHaveScreenshot("work-item-delivery-manual-merge-guardrails.png");
  await workspace.locator(".release-stage").nth(2).scrollIntoViewIfNeeded();
  await expect(page).toHaveScreenshot("work-item-delivery-manual-merge.png");
});

test("verified delivery workspace shows Argo, digest equality, and contract checks", async ({ page }) => {
  const workItem = {
    ...workItemFixture("witem_delivery_verified_1234567890"),
    status: "completed",
    status_reason: "Release verification passed every DeploymentContract check.",
    title: "Verified yfinance release",
    source_commit: "a".repeat(40),
    target_environment: "production",
    target_namespace: "apps-prod",
    workload_name: "yfinance-wrapper",
    argo_application: "yfinance-wrapper",
    rollback_owner: "lucas",
    production_impacting: true,
  };
  const flow = deliveryFlowFixture(workItem, { verified: true });
  await mockApi(page, {
    [`/api/work-items/${workItem.id}`]: workItem,
    [`/api/work-items/${workItem.id}/flow`]: flow,
    [`/api/work-items/${workItem.id}/reconcile`]: { can_apply: false, action: "terminal", boundary: "terminal", effect_summary: "No forward action remains.", blockers: [] },
    [`/api/work-items/${workItem.id}/rollback-intents`]: { content: { status: "prepared", baseline: { image_digest: `sha256:${"e".repeat(64)}` } } },
  });

  await page.goto(`/#/work-items/${workItem.id}`);
  await page.getByLabel("WorkItem sections").getByRole("button", { name: "Delivery" }).click();
  const workspace = page.getByLabel("Delivery and release workspace");
  await expect(workspace.getByText("5/5 stages evidenced")).toBeVisible();
  await expect(page.getByLabel("Production release guardrails").getByText("Running digest verified equal")).toBeVisible();
  await expect(workspace.getByText("Argo Synced · Succeeded")).toBeVisible();
  await expect(workspace.getByText("6/6 required checks passed")).toBeVisible();
  await expect(workspace.getByText("Running Image Digest", { exact: true })).toBeVisible();
  await expect(workspace.getByText("Service Healthz", { exact: true })).toBeVisible();
  await expect(workspace.locator(".release-stage").first().getByText("Controller: Active")).toBeVisible();
  await expect(page.getByLabel("Production release guardrails")).toHaveScreenshot("work-item-delivery-verified-guardrails.png");
  await workspace.locator(".release-stage").nth(4).scrollIntoViewIfNeeded();
  await expect(page).toHaveScreenshot("work-item-delivery-verified.png");
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
  await expect(page.getByLabel("Recovery options").getByText("Approvals: Production Rollback Deployment · Cluster Mutation · Production Impact")).toBeVisible();
  await page.getByLabel("WorkItem sections").getByRole("button", { name: "Delivery" }).click();
  await expect(page.getByText(`sha256:${"b".repeat(64)}`, { exact: true }).first()).toBeVisible();
  await page.getByRole("button", { name: "Review exact action" }).click();
  await expect(page.getByRole("heading", { name: "Review recovery action" })).toBeVisible();
  await expect(page.getByLabel("Effect and authorization").getByText("Open a fresh production rollback window")).toBeVisible();
  await page.getByLabel("Decision reason").fill("reviewed rollback merge, baseline digest, and exact Argo target");
  await page.getByRole("button", { name: "Confirm Approve Rollback Argo Sync" }).click();
  await expect.poll(() => calls.length).toBe(1);
  expect(calls[0]).toMatchObject({ state_hash: rollbackAction.state_hash });
});

test("completed WorkItem keeps rollback in recovery instead of forward progress", async ({ page }) => {
  const workItem = {
    ...workItemFixture("witem_completed_1234567890"),
    status: "completed",
    status_reason: "Release verification passed every DeploymentContract check.",
    source_commit: "f".repeat(40),
    environment_profile_id: "python-3.11",
    target_environment: "production",
    target_namespace: "apps-prod",
    production_impacting: true,
  };
  const terminalAction = { id: "terminal", lifecycle_stage: "source", resource: workItem.id, status: "blocked", effect_class: "internal", blockers: [{ code: "terminal", summary: "preview only" }], approval_requirements: [], external_effect_summary: "preview only", state_hash: "state-terminal-complete" };
  const rollbackAction = { id: "execute_rollback_gitops_pr", lifecycle_stage: "rollback", resource: "rollback_completed_123", status: "ready", effect_class: "external_effect", blockers: [], approval_requirements: [], external_effect_summary: "Create the exact digest-only rollback pull request; merge remains manual.", state_hash: "state-completed-rollback" };
  const calls = [];
  await mockApi(page, {
    [`/api/work-items/${workItem.id}`]: workItem,
    [`/api/work-items/${workItem.id}/flow`]: {
      work_item: workItem,
      workspaces: [], controller_waits: [], audit_events: [],
      action_rail: [terminalAction, rollbackAction],
      delivery_segments: [{ key: "source", label: "Source", status: "active", summary: "Historical source evidence is incomplete.", resources: [] }],
      delivery_configuration: { rollback_status: "approved", rollback_owner: "lucas" },
    },
    [`/api/work-items/${workItem.id}/reconcile`]: { can_apply: false, action: "terminal", boundary: "terminal", effect_summary: "preview only", blockers: [{ code: "terminal", summary: "preview only" }], authorization_checks: [] },
    [`/api/work-items/${workItem.id}/actions/${rollbackAction.id}/execute`]: async (route) => { calls.push(route.request().postDataJSON()); return { status: "writer_dispatched" }; },
    [`/api/work-items/${workItem.id}/rollback-intents`]: { content: { status: "approved" } },
  });

  await page.goto(`/#/work-items/${workItem.id}`);
  await expect(page.getByRole("heading", { name: "WorkItem complete" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Rollback is prepared, not recommended" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Execute Rollback Gitops Pr" })).toHaveCount(0);
  await expect(page.getByText("Delivery evidence needs reconciliation")).toBeVisible();
  await expect(page.locator(".reconcile-blockers")).toHaveCount(0);
  await expect(page).toHaveScreenshot("work-item-completed-recovery.png", { fullPage: true });

  await page.getByRole("button", { name: "Review exact action" }).click();
  await expect(page.getByRole("heading", { name: "Review recovery action" })).toBeVisible();
  await page.getByLabel("Decision reason").fill("reviewed completed release baseline and exact rollback target");
  await page.getByRole("button", { name: "Confirm Execute Rollback Gitops Pr" }).click();
  await expect.poll(() => calls.length).toBe(1);
  expect(calls[0]).toMatchObject({ state_hash: rollbackAction.state_hash });
});

test("WorkPlan review is directly actionable from the WorkItem boundary", async ({ page }) => {
  const workItem = { ...workItemFixture("witem_review_1234567890"), status: "awaiting_approval", source_commit: "a".repeat(40), environment_profile_id: "python-3.11" };
  const action = { id: "approve_work_plan", lifecycle_stage: "planning", resource: "wplan_review_1234567890", status: "ready", effect_class: "approval_boundary", blockers: [], approval_required: true, approval_requirements: ["work_plan_review"], external_effect_summary: "Approve the proposed WorkPlan before authorizing one attempt workspace.", state_hash: "state-workplan-review" };
  const rejectAction = { ...action, id: "reject_work_plan", external_effect_summary: "Reject the proposed WorkPlan and return it for revision.", state_hash: "state-workplan-reject" };
  const workPlan = { id: action.resource, status: "proposed", title: "WorkPlan: Repair finance price cache", summary: workItem.intent, risk_level: "high", revision: 2, work_plan_json: { source_repository: { repo: workItem.source_repo, ref: "main" }, target: { environment: "dev", namespace: "finance-dev" }, acceptance_criteria: ["tests pass"], approval_gates: [{ kind: "source_mutation", required_before: "creating a source pull request" }] } };
  const calls = [];
  await mockApi(page, {
    [`/api/work-items/${workItem.id}`]: workItem,
    [`/api/work-items/${workItem.id}/flow`]: { work_item: workItem, workspaces: [{ id: "ws_review", source_repo: workItem.source_repo, source_ref: "main", status: "declared" }], controller_waits: [], delivery_segments: [], action_rail: [action, rejectAction], sdlc_flow: { work_plan: workPlan } },
    [`/api/work-items/${workItem.id}/reconcile`]: { can_apply: false, action: "awaiting_work_plan_approval", boundary: "planning", effect_summary: action.external_effect_summary, blockers: [] },
    [`/api/work-items/${workItem.id}/actions/${action.id}/execute`]: async (route) => { calls.push(route.request().postDataJSON()); return { work_plan: { id: action.resource, status: "approved" } }; },
    [`/api/work-items/${workItem.id}/actions/${rejectAction.id}/execute`]: async (route) => { calls.push(route.request().postDataJSON()); return { work_plan: { id: action.resource, status: "rejected" } }; },
  });
  await page.goto(`/#/work-items/${workItem.id}`);
  await expect(page.getByRole("button", { name: "Approve Work Plan" })).toBeEnabled();
  await expect(page).toHaveScreenshot("work-item-review-required.png", { fullPage: true });
  await page.getByRole("button", { name: "Approve Work Plan" }).click();
  await expect(page.getByRole("heading", { name: "Review WorkPlan" })).toBeVisible();
  await expect(page.getByLabel("Decision evidence").getByText("Acceptance: tests pass")).toBeVisible();
  await expect(page.getByLabel("Effect and authorization").getByText(action.state_hash, { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Reject Work Plan" }).click();
  await page.getByLabel("Decision reason").fill("acceptance evidence needs revision");
  await page.getByRole("button", { name: "Confirm Reject Work Plan" }).click();
  await expect.poll(() => calls.length).toBe(1);
  expect(calls[0]).toMatchObject({ state_hash: rejectAction.state_hash });
});

test("attempt workspace review shows the exact write grant before model execution", async ({ page }) => {
  const workItem = {
    ...workItemFixture("witem_workspace_review_1234567890"),
    status: "awaiting_approval",
    source_commit: "b".repeat(40),
    environment_profile_id: "python-3.11",
    environment_preparation_status: "succeeded",
    max_attempts: 2,
    repository_contract: { writable_paths: ["src/**", "tests/**", "readme.md"], agent_network: "denied" },
    acceptance_criteria: ["python -m unittest discover -s tests -v"],
  };
  const action = { id: "authorize_workspace_and_start", lifecycle_stage: "attempt", resource: "wplan_workspace_review", status: "ready", effect_class: "model_execution", blockers: [], approval_required: true, approval_requirements: ["attempt_workspace_write"], external_effect_summary: "Authorize one coding attempt for the exact repository and declared writable paths, then start model execution.", state_hash: "state-workspace-review" };
  const calls = [];
  await mockApi(page, {
    [`/api/work-items/${workItem.id}`]: workItem,
    [`/api/work-items/${workItem.id}/flow`]: { work_item: workItem, workspaces: [{ id: "ws_workspace_review", source_repo: workItem.source_repo, resolved_commit: workItem.source_commit, branch: "pharness/attempt-2", status: "declared" }], controller_waits: [], delivery_segments: [], action_rail: [action], sdlc_flow: { work_plan: { id: action.resource, status: "approved" } } },
    [`/api/work-items/${workItem.id}/reconcile`]: { can_apply: false, action: "start_coding_attempt", boundary: "attempt", effect_summary: action.external_effect_summary, blockers: [], authorization_checks: [{ kind: "permission_grant", status: "missing", summary: "Attempt workspace grant requires explicit review." }] },
    [`/api/work-items/${workItem.id}/actions/${action.id}/execute`]: async (route) => { calls.push(route.request().postDataJSON()); return { status: "dispatched" }; },
  });

  await page.goto(`/#/work-items/${workItem.id}`);
  await page.getByRole("button", { name: "Authorize Workspace And Start" }).click();
  await expect(page.getByRole("heading", { name: "Review attempt workspace" })).toBeVisible();
  await expect(page.getByLabel("Decision evidence").getByText("Writable: src/**")).toBeVisible();
  await expect(page.getByLabel("Decision evidence").getByText(`lward27/yfinance_wrapper @ ${workItem.source_commit}`)).toBeVisible();
  await expect(page.getByLabel("Effect and authorization").getByText("Attempt Workspace Write")).toBeVisible();
  await expect(page).toHaveScreenshot("work-item-workspace-review.png");
  await page.getByLabel("Decision reason").fill("authorize only the pinned workspace and declared paths");
  await page.getByRole("button", { name: "Confirm Authorize Workspace And Start" }).click();
  await expect.poll(() => calls.length).toBe(1);
  expect(calls[0]).toMatchObject({ state_hash: action.state_hash });
});

test("replan boundary creates a fresh explicit review action", async ({ page }) => {
  const workItem = { ...workItemFixture("witem_replan_1234567890"), status: "blocked", source_commit: "b".repeat(40), environment_profile_id: "python-3.11", status_reason: "Hard run limit requires a revised WorkPlan." };
  const action = { id: "replan_work_item", lifecycle_stage: "planning", resource: workItem.id, status: "ready", effect_class: "internal", blockers: [], approval_required: false, approval_requirements: [], external_effect_summary: "Create a fresh isolated workspace and proposed WorkPlan; no model starts automatically.", state_hash: "state-replan-ready" };
  const calls = [];
  await mockApi(page, {
    [`/api/work-items/${workItem.id}`]: workItem,
    [`/api/work-items/${workItem.id}/flow`]: { work_item: workItem, workspaces: [{ id: "ws_old", source_repo: workItem.source_repo, status: "retained" }], controller_waits: [], delivery_segments: [], action_rail: [action] },
    [`/api/work-items/${workItem.id}/reconcile`]: { can_apply: false, action: "requires_replan", boundary: "planning", effect_summary: action.external_effect_summary, blockers: [{ code: "requires_replan", summary: "A fresh WorkPlan and workspace are required." }] },
    [`/api/work-items/${workItem.id}/actions/${action.id}/execute`]: async (route) => { calls.push(route.request().postDataJSON()); return { work_item: { ...workItem, status: "planning" } }; },
  });
  await page.goto(`/#/work-items/${workItem.id}`);
  await expect(page.getByRole("button", { name: "Replan Work Item", exact: true })).toBeEnabled();
  await expect(page.getByLabel("Controller blockers").getByRole("button", { name: "Review Replan Work Item" })).toBeVisible();
  await expect(page.getByText("Create a fresh isolated workspace and proposed WorkPlan; no model starts automatically.").first()).toBeVisible();
  await expect(page).toHaveScreenshot("work-item-replan-ready.png", { fullPage: true });
  await page.getByLabel("Controller blockers").getByRole("button", { name: "Review Replan Work Item" }).click();
  await expect(page.getByRole("heading", { name: "Review WorkItem replan" })).toBeVisible();
  await page.getByLabel("Decision reason").fill("revise the bounded plan after hard-limit review");
  await page.getByRole("button", { name: "Confirm Replan Work Item" }).click();
  await expect.poll(() => calls.length).toBe(1);
});

test("budget extension resumes the existing workspace from an exact server action", async ({ page }) => {
  const workItem = { ...workItemFixture("witem_budget_1234567890"), status: "executing", source_commit: "c".repeat(40), environment_profile_id: "python-3.11", current_run_id: "run_budget_1234567890" };
  const action = { id: "approve_budget_extension", lifecycle_stage: "attempt", resource: "budget_1234567890", status: "ready", effect_class: "approval_boundary", blockers: [], approval_required: true, approval_requirements: ["budget_extension"], external_effect_summary: "Resume the existing workspace with exactly 20 additional turns and 200000 additional tokens.", state_hash: "state-budget-extension" };
  const calls = [];
  await mockApi(page, {
    [`/api/work-items/${workItem.id}`]: workItem,
    [`/api/work-items/${workItem.id}/flow`]: { work_item: workItem, workspaces: [{ id: "ws_budget", run_id: workItem.current_run_id, source_repo: workItem.source_repo, source_ref: "main", resolved_commit: workItem.source_commit, status: "active" }], controller_waits: [], delivery_segments: [], action_rail: [action] },
    [`/api/work-items/${workItem.id}/reconcile`]: { can_apply: false, action: "budget_extension_required", boundary: "attempt", effect_summary: action.external_effect_summary, blockers: [] },
    [`/api/work-items/${workItem.id}/actions/${action.id}/execute`]: async (route) => { calls.push(route.request().postDataJSON()); return { id: action.resource, status: "approved" }; },
    [`/api/runs/${workItem.current_run_id}`]: { id: workItem.current_run_id, status: "budget_extension_required", task: workItem.intent, run_budget: { initial_turns: 48, hard_turns: 100, initial_tokens: 400000, hard_tokens: 1000000, active_execution_seconds: 3600 }, budget_consumption: { allowed_turns: 48, allowed_tokens: 400000, turns_used: 48, tokens_used: 365000, active_execution_seconds_used: 1200 }, result: { status: "budget_extension_required", turns: 48 } },
    [`/api/runs/${workItem.current_run_id}/events`]: { events: [] },
    [`/api/runs/${workItem.current_run_id}/diff`]: { run_id: workItem.current_run_id, changes: [], diff: "" },
    [`/api/runs/${workItem.current_run_id}/artifacts`]: { artifacts: [] },
    [`/api/runs/${workItem.current_run_id}/operator-summary`]: { run_id: workItem.current_run_id, actual_total_tokens: 365000, budget_extensions: 0, stop_reason: "soft_turn_budget_exhausted", pending_approvals: [] },
  });
  await page.goto(`/#/work-items/${workItem.id}`);
  await expect(page.getByLabel("WorkItem sections").getByRole("button", { name: /^Attempt/ })).toHaveAttribute("aria-current", "page");
  await expect(page.getByRole("heading", { name: "Workspace retained for in-place extension" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Approve Budget Extension" }).first()).toBeEnabled();
  await expect(page.getByText("48 used · 0 remaining")).toBeVisible();
  await expect(page).toHaveScreenshot("work-item-budget-extension.png", { fullPage: true });
  await page.getByRole("button", { name: "Approve Budget Extension" }).first().click();
  await expect(page.getByRole("heading", { name: "Review run budget" })).toBeVisible();
  await page.getByLabel("Decision reason").fill("preserve the active workspace and finish verification");
  await page.getByRole("button", { name: "Confirm Approve Budget Extension" }).click();
  await expect.poll(() => calls.length).toBe(1);
});

test("active coding displays the verified environment snapshot and live budget", async ({ page }) => {
  const runId = "run_active_environment_1234567890";
  await page.addInitScript(() => {
    window.EventSource = class {
      addEventListener() {}
      close() {}
    };
  });
  await mockApi(page, {
    [`/api/runs/${runId}`]: { id: runId, status: "running", task: "Add pure yfinance validation", started_at: "1786032000000", scope: { namespace: "apps-prod", repo: "lward27/yfinance_wrapper", branch: "pharness/attempt-1" }, run_budget: { initial_turns: 48, hard_turns: 100, initial_tokens: 400000, hard_tokens: 1000000, active_execution_seconds: 3600 }, budget_consumption: { allowed_turns: 48, allowed_tokens: 400000, turns_used: 7, tokens_used: 52000, active_execution_seconds_used: 240 }, result: {} },
    [`/api/runs/${runId}/events`]: { events: [{ event_id: "evt_model", seq: 1, type: "model.request_started", payload: { summary: "Turn 7 started" } }, { event_id: "evt_active", seq: 2, type: "tool.finished", payload: { summary: "Patched src/yfinance_wrapper/validation.py", status: "ok" } }] },
    [`/api/runs/${runId}/diff`]: { run_id: runId, changes: [{ id: "chg_active", path: "src/yfinance_wrapper/validation.py", diff: "+def normalize_ticker(value):", created_at: "1786032010000" }], diff: "+def normalize_ticker(value):" },
    [`/api/runs/${runId}/artifacts`]: { artifacts: [] },
    [`/api/runs/${runId}/operator-summary`]: { run_id: runId, turns: 7, actual_total_tokens: 52000, tools_completed: 4, tools_failed: 0, changed_paths: ["src/yfinance_wrapper/validation.py"], test_results: [{ command: "python -m unittest discover -s tests -v", passed: true, result: { status: "ok" } }], acceptance_evidence: [{ command: "python -m unittest discover -s tests -v", passed: true }], environment_discovery_turns: 0, approval_count: 0, approval_wait_ms: 0, preparation_duration_ms: 84000, budget_extensions: 0, pending_approvals: [] },
    [`/api/runs/${runId}/environment-preparation`]: { id: "envprep_active", status: "succeeded", environment_profile_id: "python-3.11", source_commit: "d".repeat(40), environment_snapshot: { runner_image_digest: `registry.lucas.engineering/pharness-python-runner@sha256:${"e".repeat(64)}`, python_version: "Python 3.11.13", python_path: "/workspace/.pharness-runtime/venv/bin/python", writable_paths: ["src/**", "tests/**", "readme.md"], unavailable_tools: ["docker", "podman", "apt", "apk"], agent_network_policy: "denied" }, logs: [{ step: "dependencies", status: "succeeded" }] },
  });
  await page.goto(`/#/runs/${runId}`);
  await expect(page.getByText("python-3.11 · Python 3.11.13")).toBeVisible();
  await page.getByText("python-3.11 · Python 3.11.13").click();
  await expect(page.getByText("Python 3.11.13 · /workspace/.pharness-runtime/venv/bin/python")).toBeVisible();
  await page.getByText("python-3.11 · Python 3.11.13").click();
  await expect(page.getByRole("heading", { name: "Tool and model stream" })).toBeVisible();
  await expect(page.getByLabel("Acceptance evidence").getByText("python -m unittest discover -s tests -v")).toBeVisible();
  await expect(page.getByLabel("Workspace changes").getByRole("listitem").getByText("src/yfinance_wrapper/validation.py")).toBeVisible();
  await expect(page.getByText("Environment probes")).toBeVisible();
  await expect(page.getByText("7 used · 41 remaining")).toBeVisible();
  await expect(page).toHaveScreenshot("run-active-coding.png", { fullPage: true });
});

test("active WorkItem opens directly into the consolidated attempt workspace", async ({ page }) => {
  const runId = "run_active_workitem_1234567890";
  const workItem = { ...workItemFixture("witem_active_attempt_1234567890"), status: "executing", source_commit: "a".repeat(40), environment_profile_id: "python-3.11", current_run_id: runId, target_environment: "production", target_namespace: "apps-prod", run_budget: { initial_turns: 48, hard_turns: 100, initial_tokens: 400000, hard_tokens: 1000000 } };
  await page.addInitScript(() => {
    window.EventSource = class {
      addEventListener() {}
      close() {}
    };
  });
  await mockApi(page, {
    [`/api/work-items/${workItem.id}`]: workItem,
    [`/api/work-items/${workItem.id}/reconcile`]: { can_apply: false, action: "coding_attempt_running", boundary: "attempt", effect_summary: "Observe the active isolated coding attempt.", blockers: [] },
    [`/api/work-items/${workItem.id}/flow`]: { work_item: workItem, workspaces: [{ id: "ws_active", run_id: runId, source_repo: workItem.source_repo, resolved_commit: workItem.source_commit, branch: "pharness/attempt-1", status: "active" }], controller_waits: [], delivery_segments: [], action_rail: [] },
    [`/api/runs/${runId}`]: { id: runId, status: "running", task: "Repair finance price cache", started_at: "1786032000000", scope: { namespace: "apps-prod", repo: "lward27/yfinance_wrapper", branch: "pharness/attempt-1" }, run_budget: { initial_turns: 48, hard_turns: 100, initial_tokens: 400000, hard_tokens: 1000000, active_execution_seconds: 3600 }, budget_consumption: { allowed_turns: 48, allowed_tokens: 400000, turns_used: 12, tokens_used: 98000, active_execution_seconds_used: 420 }, result: {} },
    [`/api/runs/${runId}/events`]: { events: [{ event_id: "evt_tool_started", seq: 1, type: "tool.started", payload: { summary: "Run declared unit tests" } }, { event_id: "evt_tool_finished", seq: 2, type: "tool.finished", payload: { summary: "Patched tests/test_cache.py", status: "ok" } }] },
    [`/api/runs/${runId}/diff`]: { run_id: runId, changes: [{ id: "chg_cache", path: "tests/test_cache.py", diff: "+class CacheTests", created_at: "1786032010000" }], diff: "+class CacheTests" },
    [`/api/runs/${runId}/artifacts`]: { artifacts: [] },
    [`/api/runs/${runId}/operator-summary`]: { run_id: runId, turns: 12, actual_total_tokens: 98000, tools_completed: 6, tools_failed: 0, changed_paths: ["tests/test_cache.py"], test_results: [{ command: "tests pass", passed: true, result: { status: "ok" } }], environment_discovery_turns: 0, approval_count: 0, approval_wait_ms: 0, preparation_duration_ms: 72000, budget_extensions: 0, pending_approvals: [] },
    [`/api/runs/${runId}/environment-preparation`]: { id: "envprep_workitem", status: "succeeded", environment_profile_id: "python-3.11", source_commit: workItem.source_commit, environment_snapshot: { runner_image_digest: `registry.lucas.engineering/pharness-python-runner@sha256:${"b".repeat(64)}`, python_version: "Python 3.11.13", python_path: "/workspace/.pharness-runtime/venv/bin/python", writable_paths: ["src/**", "tests/**"], unavailable_tools: ["docker", "apt"], agent_network_policy: "denied" }, logs: [{ step: "checkout", status: "succeeded" }, { step: "dependencies", status: "succeeded" }] },
  });

  await page.goto(`/#/work-items/${workItem.id}`);
  await expect(page.getByLabel("WorkItem sections").getByRole("button", { name: /^Attempt/ })).toHaveAttribute("aria-current", "page");
  await expect(page.getByRole("heading", { name: "Agent workspace" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Tool and model stream" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Contracts, target, and rollout" })).toHaveCount(0);
  await expect(page.getByText("12 used · 36 remaining")).toBeVisible();
  await page.getByRole("button", { name: "Delivery" }).click();
  await expect(page.getByRole("heading", { name: "External delivery and release evidence" })).toBeVisible();
  await page.getByLabel("WorkItem sections").getByRole("button", { name: /^Attempt/ }).click();
  await expect(page).toHaveScreenshot("work-item-active-attempt.png", { fullPage: true });
});

test("wizard preserves a blocked preparation preflight without creating durable work", async ({ page }) => {
  const sourceSha = "f".repeat(40);
  await mockApi(page, {
    "/api/environment-profiles": { profiles: [{ id: "python-3.11", status: "configured_unverified", image: `registry.lucas.engineering/pharness-python-runner@sha256:${"a".repeat(64)}` }] },
    "/api/pipeline-contracts": { pipeline_contracts: [{ id: "pcontract_yfinance", namespace: "tekton-pipelines", pipeline_ref: "pharness-yfinance-build" }] },
    "/api/deployment-contracts": { deployment_contracts: [{ id: "dcontract_yfinance", target_environment: "production", target_namespace: "apps-prod" }] },
    "/api/system/readiness": { capabilities: [{ capability: "environment_profile:python-3.11", status: "configured_unverified" }] },
    "/api/work-items/preflight": { ready: false, checks: [{ capability: "environment_profile:python-3.11", status: "unavailable", summary: "The isolated runner could not create a Python venv." }], blockers: ["Runner preparation capability has not passed."], warnings: [], predicted_external_mutations: [] },
  });
  await page.goto("/#/work-items/new");
  await page.getByLabel("Full source commit SHA").fill(sourceSha);
  await page.getByRole("button", { name: /Continue/ }).click();
  await page.getByRole("button", { name: /Continue/ }).click();
  await page.getByRole("button", { name: /Continue/ }).click();
  await page.getByLabel("PipelineContract").selectOption("pcontract_yfinance");
  await page.getByLabel("DeploymentContract").selectOption("dcontract_yfinance");
  await page.getByRole("button", { name: "Check readiness" }).click();
  await expect(page.getByText("1 blocking checks")).toBeVisible();
  await expect(page.getByText("Runner preparation capability has not passed.")).toBeVisible();
  await expect(page).toHaveScreenshot("work-item-preparation-blocked.png", { fullPage: true });
});

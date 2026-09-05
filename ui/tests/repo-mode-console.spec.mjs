import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

const productId = "prod_01jrepoexperience";
const repositoryId = "repo_01jyfinance";
const workItemId = "witem_01jcompleted";
const sourceSha = "a".repeat(40);

const product = {
  id: productId,
  product_key: "yfinance-wrapper",
  display_name: "Yfinance Wrapper",
  description: "Bounded market-data API wrapper",
  owner_principal: "lucas",
  current_model_snapshot_id: "psnap_01jmodel",
  state_version: 4,
  updated_at: "2026-08-25T09:00:00Z",
};

const repository = {
  id: repositoryId,
  provider: "github",
  provider_repository_id: "lward27/yfinance_wrapper",
  external_id: "lward27/yfinance_wrapper",
  canonical_url: "https://github.com/lward27/yfinance_wrapper",
  default_branch: "main",
  registered_commit: sourceSha,
};

const completedWorkItem = {
  id: workItemId,
  mode: "repo",
  product_id: productId,
  repository_id: repositoryId,
  title: "Normalize supported history periods",
  intent: "Validate supported periods before an upstream request.",
  status: "completed",
  status_reason: "Exact approved pull request merge observed.",
  source_commit: sourceSha,
  acceptance_command_names: ["unit", "compile"],
  acceptance_criteria: ["python -m unittest discover -s tests -v", "python -m compileall -q src tests"],
  run_budget: { initial_turns: 48, hard_turns: 100, initial_tokens: 400000, hard_tokens: 1000000, active_execution_seconds: 3600 },
  attempt_count: 1,
  max_attempts: 2,
  updated_at: "2026-08-25T09:30:00Z",
  closed_at: "2026-08-25T09:30:00Z",
  closure_reason: "Source Delivery succeeded after exact merge provenance was observed.",
};

const outcome = (stage, status, conclusion) => ({
  id: `outcome_${stage}`,
  stage_key: stage,
  status,
  content_hash: `sha256:${stage.padEnd(64, "0").slice(0, 64)}`,
  sealed_at: "2026-08-25T09:20:00Z",
  outcome: {
    conclusion,
    verified_facts: [{ statement: `${stage} controller evidence is sealed.` }],
    outputs: [{ kind: "artifact", id: `artifact_${stage}` }],
    acceptance: stage === "test" ? [{ command_name: "unit", status: "passed" }] : [],
    agent_claims: stage === "implement" ? [{ statement: "Implementation is complete." }] : [],
    risks: [],
    contradictions: [],
  },
});

const stageOutcomes = [
  outcome("discover", "succeeded", "Pinned readiness evidence accepted."),
  outcome("plan", "succeeded", "Exact WorkPlan approved."),
  outcome("implement", "succeeded", "Bounded source changes captured."),
  outcome("test", "succeeded", "Declared acceptance commands passed."),
  outcome("verify", "succeeded", "Evidence and diff verified."),
  outcome("source_delivery", "succeeded", "Exact approved pull request merge observed."),
  outcome("release", "inapplicable", "Repo Mode V1 is source-only."),
  outcome("observe", "inapplicable", "Runtime observation is outside Repo Mode V1."),
];

const workItemFlow = {
  work_item: completedWorkItem,
  action_rail: [],
  repo_mode: {
    stage_executions: stageOutcomes.slice(0, 6).map((entry, index) => ({ id: `stage_${index}`, stage_key: entry.stage_key, sequence: 1, status: "completed" })),
    effective_stage_outcomes: stageOutcomes,
    source_delivery_intent: {
      id: "sdi_01jdelivery",
      status: "merged",
      base_commit: sourceSha,
      pull_request: { html_url: "https://github.com/lward27/yfinance_wrapper/pull/42", head_sha: "b".repeat(40) },
      provider_checks: { status: "passing", required_checks: [], expires_at: "2026-08-25T09:35:00Z" },
      merge_provenance: { merge_commit_sha: "c".repeat(40), merged_at: "2026-08-25T09:30:00Z" },
    },
    safe_advance: { eligible: false, blockers: ["work_item_closed"] },
    product_model_snapshot: { id: "psnap_01jmodel" },
    repository_contract_version_id: "rcontract_01jcontract",
    operator_annotations: [],
    operator_annotation_decisions: [],
  },
};

const overview = {
  organization: { id: "org_bootstrap", display_name: "Lucas Engineering" },
  as_of: "2026-08-25T10:00:00Z",
  work_items: { current: 1, waiting: 1, blocked: 0, failed: 0, recently_completed: 1, by_lifecycle_boundary: { source_delivery: 1 }, denominator: 2 },
  product_summaries: [{ ...product, repository_count: 1, current_work_items: 1, actionable_waits: 1 }],
  attention: [{ kind: "external_wait", resource_kind:"work_item", resource_id: "witem_01jwaiting", product_id: productId, repository_id: repositoryId, status: "waiting_external", reason: "Manual source merge is required." }],
  active_agent_runs: [],
  repository_readiness_gaps: [],
  repository_readiness_rate: { ready: 1, total: 1 },
  unassigned_legacy: { count: 1 },
};

const repositoryOverview = {
  repository,
  product_bindings: [{ product }],
  latest_onboarding: { id: "onboard_01jready", status: "ready" },
  canonical_contract: {
    api_version: "pharness.dev/v1alpha1",
    source_commit: sourceSha,
    content_hash: `sha256:${"d".repeat(64)}`,
    contract: {
      environment_profile: "python-3.11",
      writable_paths: ["src/**", "tests/**", "readme.md"],
      acceptance_commands: [
        { name: "unit", command: "python -m unittest discover -s tests -v" },
        { name: "compile", command: "python -m compileall -q src tests" },
      ],
    },
  },
  readiness: { contract_status: "ready", coding_status: "ready", environment_profile_id: "python-3.11", runner_image_digest: `sha256:${"e".repeat(64)}`, dependency_lock_hash: `sha256:${"f".repeat(64)}` },
  readiness_stale_reasons: [],
  capabilities: [
    { capability: "source_reader", status: "available" },
    { capability: "source_writer", status: "available" },
    { capability: "source_observer", status: "available" },
  ],
  trust_policy: { repository_allowlisted: "allowed", agent_network: "denied" },
  authorization: { source_mutation: "requires_explicit_grant" },
  current_work_items: [],
  historical_work_items: [completedWorkItem],
};

function emptyPayload(pathname) {
  if (pathname === "/api/config/effective") return { operator: { name: "lucas" }, features: { repo_mode_v1: { enabled: true, ui_enabled: true } }, workspace: { allowed_repo_count: 1 } };
  if (pathname === "/api/organization/overview") return overview;
  if (pathname === "/api/products") return { products: [product] };
  if (pathname === `/api/products/${productId}/overview`) return { product, services: [], repositories: [repository], current_work_items: [], historical_work_items: [completedWorkItem], active_agent_runs: [], capability_posture: [] };
  if (pathname === "/api/repositories") return { repositories: [{ ...repository, product_bindings: [product], contract_readiness: "ready", coding_readiness: "ready", readiness: repositoryOverview.readiness }], count: 1, limit: 50, offset: 0 };
  if (pathname === `/api/repositories/${repositoryId}/overview`) return repositoryOverview;
  if (pathname === "/api/work-items") return { work_items: [], operator_state: {}, count: 0, limit: 100, offset: 0 };
  if (pathname === `/api/work-items/${workItemId}/flow`) return workItemFlow;
  if (pathname === `/api/work-items/${workItemId}/evidence`) return { work_item_id: workItemId, evidence_validations: [], effective_stage_outcomes: stageOutcomes };
  if (pathname === "/api/runs") return { runs: [], count: 0, limit: 100, offset: 0 };
  if (pathname === "/api/agent-profiles") return { agent_profiles: [{ id: "repo-builder", version: "v1", profile_hash: "profile-hash", allowed_tools: ["write_file", "run_acceptance"] }] };
  if (pathname === "/api/releases") return { releases: [], count: 0 };
  if (pathname === "/api/audit-events") return { audit_events: [] };
  if (pathname === "/api/observations") return { observations: [] };
  if (pathname === "/api/incidents") return { incidents: [] };
  if (pathname === "/api/remediation-plans") return { remediation_plans: [] };
  if (pathname === "/api/environment-profiles") return { profiles: [{ id: "python-3.11", status: "available", platform: "linux/amd64", image: `registry.example/pharness-runner@sha256:${"e".repeat(64)}`, revision: sourceSha, required_executables: ["python", "git"], preparation_strategy: "python_hashed_lock" }] };
  if (pathname === "/api/system/readiness") return { api_revision: sourceSha, ui_revision: sourceSha, runtime_image_digest: `sha256:${"1".repeat(64)}`, ui_image_digest: `sha256:${"2".repeat(64)}`, platform_versions_match: true, capabilities: repositoryOverview.capabilities, repository_allowlists: { source_reader: [repository.canonical_url] } };
  if (pathname === "/api/search") return { results: [{ kind: "work_item", id: workItemId, label: completedWorkItem.title, status: "completed", product_id: productId, repository_id: repositoryId }] };
  return {};
}

async function mockRepoModeApi(page, overrides = {}) {
  await page.clock.setFixedTime(new Date("2026-08-25T10:00:00Z"));
  await page.route("**/*", async route => {
    const url = new URL(route.request().url());
    if (!url.pathname.startsWith("/api/")) {
      await route.fallback();
      return;
    }
    const override = overrides[url.pathname];
    const payload = typeof override === "function" ? await override(route, url) : override ?? emptyPayload(url.pathname);
    if (payload && payload.__status) {
      await route.fulfill({ status: payload.__status, contentType: "application/json", body: JSON.stringify(payload.body) });
      return;
    }
    await route.fulfill({ contentType: "application/json", body: JSON.stringify(payload) });
  });
}

async function assertNoSeriousAccessibilityViolations(page) {
  const results = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"]).analyze();
  const serious = results.violations.filter(violation => ["serious", "critical"].includes(violation.impact));
  expect(serious, serious.map(violation => `${violation.id}: ${violation.help}`).join("\n")).toEqual([]);
}

test("flagged shell uses the approved hierarchy and read-only overview", async ({ page }, testInfo) => {
  await mockRepoModeApi(page);
  await page.goto("/#/overview");
  await expect(page.getByRole("heading", { name: "Lucas Engineering" })).toBeVisible();
  await expect(page.getByRole("navigation", { name: "Primary navigation" })).toContainText("Products");
  await expect(page.getByRole("navigation", { name: "Primary navigation" })).not.toContainText("Triage");
  await expect(page.getByText("Manual source merge is required.")).toBeVisible();
  await expect.poll(() => page.evaluate(() => Math.max(document.documentElement.scrollWidth, document.body.scrollWidth) - window.innerWidth)).toBeLessThanOrEqual(1);
  await assertNoSeriousAccessibilityViolations(page);
  await expect(page).toHaveScreenshot(`repo-mode-overview-${testInfo.project.name}.png`, { fullPage: true });
});

test("overview attention navigates to the exact owning Repository onboarding", async ({ page }) => {
  const onboardingId = "onboard_attention";
  await mockRepoModeApi(page, {
    "/api/organization/overview": {
      ...overview,
      attention:[{kind:"human_action",resource_kind:"repository_onboarding",resource_id:onboardingId,product_id:productId,repository_id:repositoryId,status:"proposal_ready",reason:"Review the exact onboarding proposal",action:{id:"approve_proposal",status:"available"}}],
    },
    [`/api/repository-onboardings/${onboardingId}/flow`]: {onboarding:{id:onboardingId,product_id:productId,repository_id:repositoryId,registered_commit:sourceSha,status:"proposal_ready",actions:[]}},
  });
  await page.goto("/#/overview");
  await page.getByRole("button", {name:/approve proposal/i}).click();
  await expect(page).toHaveURL(new RegExp(`#/repository-onboardings/${onboardingId}$`));
  await expect(page.getByRole("heading", {name:repository.external_id})).toBeVisible();
});

test("completed Repo Mode delivery shows source success and inapplicable downstream stages", async ({ page }, testInfo) => {
  await mockRepoModeApi(page);
  await page.goto(`/#/work-items/${workItemId}/delivery`);
  await expect(page.getByRole("heading", { name: completedWorkItem.title })).toBeVisible();
  await expect(page.getByText("Source Delivery succeeded", { exact: true })).toBeVisible();
  await expect(page.getByText("inapplicable", { exact: true })).toHaveCount(2);
  await expect(page.getByText(/0\/5 stages evidenced/i)).toHaveCount(0);
  await expect(page.getByText(/delivery evidence needs reconciliation/i)).toHaveCount(0);
  await assertNoSeriousAccessibilityViolations(page);
  await expect(page).toHaveScreenshot(`repo-mode-completed-delivery-${testInfo.project.name}.png`, { fullPage: true });
});

test("server search is keyboard accessible and navigates to the owning resource", async ({ page }) => {
  await mockRepoModeApi(page);
  await page.goto("/#/overview");
  await expect(page.getByRole("heading", { name: "Lucas Engineering" })).toBeVisible();
  await page.keyboard.press("Control+K");
  const dialog = page.getByRole("dialog", { name: "Search PHarness" });
  await expect(dialog).toBeVisible();
  await page.getByLabel("Search Products, Repositories, WorkItems, and AgentRuns").fill("period");
  await expect(dialog.getByRole("button", { name: new RegExp(completedWorkItem.title) })).toBeVisible();
  await dialog.getByRole("button", { name: new RegExp(completedWorkItem.title) }).click();
  await expect(page).toHaveURL(new RegExp(`#\/work-items\/${workItemId}\/overview$`));
});

test("AgentRun history pagination remains server-owned", async ({ page }) => {
  const offsets = [];
  await mockRepoModeApi(page, {
    "/api/runs": (_route,url) => {
      const offset = Number(url.searchParams.get("offset") || 0);
      offsets.push(offset);
      return {count:51,limit:50,offset,runs:[{id:offset ? "run_page_two" : "run_page_one",task:"Bounded fixture",status:"completed",max_turns:48,ownership:{product_id:productId,work_item_id:workItemId,stage_execution_id:"stage_fixture",agent_profile_id:"repo-tester"},budget_consumption:{turns_used:4,allowed_turns:48}}]};
    },
  });
  await page.goto("/#/agents");
  await expect(page.getByText("run_page_one")).toBeVisible();
  await page.getByRole("button", {name:"Next"}).click();
  await expect(page.getByText("run_page_two")).toBeVisible();
  expect(offsets).toContain(50);
});

test("a stale state-hashed action shows the blocker, refreshes, and cannot be retried", async ({ page }) => {
  const active = { ...completedWorkItem, id: "witem_01jactive", status: "awaiting_approval", closed_at: null, closure_reason: null };
  let flowReads = 0;
  let actionCalls = 0;
  const action = { id: "approve_work_plan", lifecycle_stage: "plan", resource: "wplan_01jplan", status: "ready", effect_class: "approval_boundary", state_hash: "state-old", external_effect_summary: "Approve the exact proposed WorkPlan.", expected_result: "The Builder chain may be authorized." };
  await mockRepoModeApi(page, {
    [`/api/work-items/${active.id}/flow`]: () => {
      flowReads += 1;
      return { work_item: active, action_rail: [action], repo_mode: { stage_executions: [], effective_stage_outcomes: [], safe_advance: { eligible: false } } };
    },
    [`/api/work-items/${active.id}/actions/${action.id}/execute`]: () => {
      actionCalls += 1;
      return { __status: 409, body: { error: "stale state hash" } };
    },
  });
  await page.goto(`/#/work-items/${active.id}/overview`);
  await page.getByRole("button", { name: "approve work plan" }).click();
  const dialog = page.getByRole("dialog", { name: /approve work plan/i });
  await dialog.getByLabel("Reason").fill("Reviewed exact WorkPlan revision");
  await dialog.getByRole("button", { name: "Confirm and apply" }).click();
  await expect(dialog.getByRole("alert")).toContainText("stale state hash");
  await expect(dialog.getByRole("button", { name: "Refresh required" })).toBeDisabled();
  await expect.poll(() => flowReads).toBeGreaterThan(1);
  expect(actionCalls).toBe(1);
});

test("Product editing and onboarding proposal revision stay state-hash bound", async ({ page }) => {
  const productPatches = [];
  const proposalPuts = [];
  const onboardingId = "onboard_01jproposal";
  const proposal = {
    schema_version: "pharness.dev/repository-onboarding-proposal/v1alpha1",
    discovery_id: "rdisc_01jdiscovery",
    discovery_hash: `sha256:${"3".repeat(64)}`,
    candidate_contract: repositoryOverview.canonical_contract.contract,
    instructions: "Read the Repository contract before planning.",
    service_proposals: [], binding_proposals: [], assumptions: ["Python 3.11 is authoritative."], conflicts: [], blockers: [],
    readiness_forecast: { contract: "ready", coding: "ready" },
  };
  await mockRepoModeApi(page, {
    [`/api/products/${productId}`]: route => route.request().method() === "PATCH"
      ? (productPatches.push(route.request().postDataJSON()), { ...product, state_hash: "product-state-2" })
      : { ...product, state_hash: "product-state-1" },
    [`/api/repository-onboardings/${onboardingId}/flow`]: {
      onboarding: { id:onboardingId, product_id:productId, repository_id:repositoryId, registered_commit:sourceSha, status:"proposal_ready", state_hash:"onboarding-state-1", current_proposal_revision:1, actions:[{ id:"approve_proposal", lifecycle_stage:"proposal", resource:{kind:"repository_onboarding",id:onboardingId}, status:"available", effect_class:"human_review", external_effect_summary:"Approve the exact onboarding proposal revision.", expected_result:"Reviewed configuration becomes authoritative.", state_hash:"onboarding-state-1" }] },
      discovery: { status:"succeeded", inventory_json:{languages:["python"],contract:{canonical_present:false,alias_present:true}} },
      proposal: { id:"rprop_01jproposal", status:"proposed", proposal },
      readiness: null,
    },
    [`/api/repository-onboardings/${onboardingId}/proposal`]: route => (proposalPuts.push(route.request().postDataJSON()), { proposal:{ id:"rprop_01jproposal2" } }),
  });

  await page.goto(`/#/products/${productId}/work-items`);
  await page.getByRole("button", { name:"Edit Product" }).click();
  await page.getByLabel("Description").fill("Updated bounded market-data API wrapper");
  await page.getByRole("button", { name:"Save exact revision" }).click();
  await expect.poll(() => productPatches.length).toBe(1);
  expect(productPatches[0]).toMatchObject({ state_hash:"product-state-1", reason:"Update Product registry metadata" });

  await page.goto(`/#/repository-onboardings/${onboardingId}`);
  await page.getByRole("button", { name:"Edit proposal revision" }).click();
  await page.getByLabel("Repository instructions").fill("Read the exact Repository contract and discovery evidence before planning.");
  await page.getByRole("button", { name:"Save new proposal revision" }).click();
  await expect.poll(() => proposalPuts.length).toBe(1);
  expect(proposalPuts[0]).toMatchObject({ state_hash:"onboarding-state-1", proposal:{ discovery_id:proposal.discovery_id } });
});

test("legacy deep links resolve to their owning WorkItem history", async ({ page }) => {
  await mockRepoModeApi(page, {
    "/api/work-plans/wplan_01jowned": { id:"wplan_01jowned", work_item_id:workItemId },
  });
  await page.goto("/#/workplans/wplan_01jowned");
  await expect(page).toHaveURL(new RegExp(`#\/work-items\/${workItemId}\/history$`));
  await expect(page.getByRole("heading", { name:completedWorkItem.title })).toBeVisible();
});

test("every primary route has a server-backed accessible state", async ({ page }) => {
  await mockRepoModeApi(page);
  for (const route of ["overview", "products", "repositories", "work-items", "agents", "releases", "insights/audit", "settings/platform"]) {
    await page.goto(`/#/${route}`);
    await expect(page.locator("main h1").first()).toBeVisible();
    await assertNoSeriousAccessibilityViolations(page);
  }
});

test("Repo Mode state catalog remains legible", async ({ page }) => {
  const onboardingId = "onboard_01jvisual";
  const visualWorkItemId = "witem_01jvisual";
  const visualRunId = "run_01jvisual";
  const plannerAction = { id:"approve_work_plan", lifecycle_stage:"plan", resource:{kind:"work_plan",id:"wplan_01jvisual"}, status:"ready", effect_class:"approval_boundary", state_hash:"state-visual-plan", external_effect_summary:"Approve the exact proposed WorkPlan.", expected_result:"The bounded stage chain may be authorized." };
  const activeItem = { ...completedWorkItem, id:visualWorkItemId, title:"Validate period boundaries", status:"running", status_reason:"Builder is editing the exact workspace.", closed_at:null, closure_reason:null, current_run_id:visualRunId, current_stage_execution_id:"stage_builder_visual" };
  let organizationState = { ...overview, product_summaries:[], attention:[], repository_readiness_gaps:[], repository_readiness_rate:{ready:0,total:0}, work_items:{current:0,waiting:0,blocked:0,failed:0,recently_completed:0,by_lifecycle_boundary:{},denominator:0}, unassigned_legacy:{count:0} };
  let repositoryState = { ...repositoryOverview, latest_onboarding:null, canonical_contract:null, readiness:null, readiness_stale_reasons:["assessment_missing"], current_work_items:[], historical_work_items:[] };
  let onboardingState = { onboarding:{ id:onboardingId,product_id:productId,repository_id:repositoryId,registered_commit:sourceSha,status:"discovering",state_hash:"state-discovery",actions:[] }, discovery:{status:"running"}, proposal:null, readiness:null };
  let workItemState = { work_item:{...activeItem,status:"awaiting_approval",status_reason:"Exact WorkPlan review is required.",current_run_id:null,current_stage_execution_id:"stage_plan_visual"}, action_rail:[plannerAction], repo_mode:{ stage_executions:[{id:"stage_plan_visual",stage_key:"plan",sequence:1,status:"succeeded"}], effective_stage_outcomes:[outcome("discover","succeeded","Pinned readiness accepted.")], safe_advance:{eligible:false,blockers:["human_review_required"]} } };
  let runState = { id:visualRunId, task:"Implement period validation", status:"approval_required", started_at:"2026-08-25T09:40:00Z", finished_at:null, scope:{repo:"lward27/yfinance_wrapper",branch:"main"}, run_budget:{initial_turns:48,initial_tokens:400000,active_execution_seconds:3600}, budget_consumption:{turns_used:9,allowed_turns:48,tokens_used:72000,allowed_tokens:400000,active_execution_seconds_used:420} };
  let runSummary = { run_id:visualRunId, turns:9, actual_total_tokens:72000, tools_completed:4, tools_failed:0, recoverable_failures:0, retries:0, changed_paths:["src/yfinance_wrapper/validation.py"], test_results:[], acceptance_evidence:[], environment_discovery_turns:0, approval_count:1, approval_wait_ms:12000, budget_extensions:0, pending_approvals:["approval_01jwrite"], stop_reason:"running" };
  const currentRepoItems = [
    {...activeItem,id:"witem_01jexternal",title:"Await source merge",status:"waiting_external",status_reason:"Manual source merge is required.",current_run_id:null,current_stage_execution_id:null},
    {...activeItem,id:"witem_01jblocked",title:"Correct verifier evidence",status:"blocked",status_reason:"Verifier found a contradiction.",current_run_id:null,current_stage_execution_id:null},
  ];
  const historicalRepoItems = [completedWorkItem,{...completedWorkItem,id:"witem_01jfailed",title:"Rejected drifted pull request",status:"failed",status_reason:"Merged head did not match approved provenance.",closure_reason:"Source Delivery failed closed."}];
  const legacyItem = {...completedWorkItem,id:"witem_01jlegacy",mode:null,product_id:null,repository_id:null,title:"Legacy production rollout",status:"blocked",closed_at:null,closure_reason:null};

  await mockRepoModeApi(page, {
    "/api/organization/overview": () => organizationState,
    [`/api/repositories/${repositoryId}/overview`]: () => repositoryState,
    [`/api/repository-onboardings/${onboardingId}/flow`]: () => onboardingState,
    [`/api/work-items/${visualWorkItemId}/flow`]: () => workItemState,
    [`/api/runs/${visualRunId}`]: () => runState,
    [`/api/runs/${visualRunId}/events`]: { events:[{event_id:"event_1",seq:1,type:"model.request_started",payload:{summary:"Builder received the sealed context pack."}},{event_id:"event_2",seq:2,type:"tool.finished",payload:{summary:"Patched src/yfinance_wrapper/validation.py"}}] },
    [`/api/runs/${visualRunId}/diff`]: { run_id:visualRunId, changes:[{id:"change_1",path:"src/yfinance_wrapper/validation.py",diff:"+ def validate_period(period):\n+     return period",created_at:"2026-08-25T09:44:00Z"}], diff:"" },
    [`/api/runs/${visualRunId}/artifacts`]: { artifacts:[] },
    [`/api/runs/${visualRunId}/operator-summary`]: () => runSummary,
    [`/api/runs/${visualRunId}/environment-preparation`]: { id:"prep_01jvisual",status:"succeeded",environment_profile_id:"python-3.11",source_commit:sourceSha,project_contract:{writable_paths:["src/**","tests/**","readme.md"],agent_network:"denied"},environment_snapshot:{runner_image_digest:`sha256:${"e".repeat(64)}`,python_version:"3.11.9",python_path:"/workspace/.pharness-runtime/venv/bin/python",writable_paths:["src/**","tests/**","readme.md"],agent_network_policy:"denied",unavailable_tools:["docker","apt"]},logs:[{step:"checkout",status:"succeeded"},{step:"hashed dependencies",status:"succeeded"}] },
    "/api/work-items": (_route,url) => {
      const mode = url.searchParams.get("mode");
      const lifecycle = url.searchParams.get("lifecycle");
      if(mode === "legacy") return {work_items:[legacyItem],operator_state:{[legacyItem.id]:{current_boundary:"Deployment review required"}},count:1,limit:100,offset:0};
      const items = lifecycle === "history" ? historicalRepoItems : currentRepoItems;
      return {work_items:items,operator_state:Object.fromEntries(items.map(item => [item.id,{current_lifecycle_stage:item.status === "waiting_external" || item.status === "completed" || item.status === "failed" ? "source_delivery" : "verify",current_boundary:item.status_reason}])),count:items.length,limit:100,offset:0};
    },
  });

  await page.goto("/#/overview");
  await expect(page.getByText("No Products registered")).toBeVisible();
  await expect(page).toHaveScreenshot("repo-mode-state-empty-organization.png",{fullPage:true});

  await page.goto(`/#/repositories/${repositoryId}/overview`);
  await expect(page.getByText("Immutable registration")).toBeVisible();
  await expect(page).toHaveScreenshot("repo-mode-state-registered-not-onboarded.png",{fullPage:true});

  await page.goto(`/#/repository-onboardings/${onboardingId}`);
  await expect(page.getByText("Discovery evidence has not been sealed.")).toBeVisible();
  await expect(page).toHaveScreenshot("repo-mode-state-discovery-active.png",{fullPage:true});

  onboardingState = { ...onboardingState, onboarding:{...onboardingState.onboarding,status:"proposal_ready",state_hash:"state-proposal",actions:[{id:"approve_proposal",lifecycle_stage:"proposal",resource:{kind:"repository_onboarding",id:onboardingId},status:"available",effect_class:"human_review",state_hash:"state-proposal",external_effect_summary:"Approve the reviewed Repository contract proposal.",expected_result:"The approved proposal may be materialized as a bounded diff."}]}, discovery:{status:"succeeded",inventory_json:{languages:["python"],contract_files:[".pharness/project.yaml"],dependency_locks:["requirements.lock"]}}, proposal:{status:"proposed",proposal:{schema_version:"pharness.dev/repository-onboarding-proposal/v1alpha1",discovery_id:"rdisc_visual",discovery_hash:`sha256:${"3".repeat(64)}`,candidate_contract:repositoryOverview.canonical_contract.contract,instructions:"Read the contract first.",services:[],bindings:[],assumptions:["Python 3.11 is authoritative."],conflicts:[],blockers:[],readiness_forecast:{contract:"ready",coding:"ready"}}} };
  await page.reload();
  await expect(page.getByRole("button",{name:"approve proposal"})).toBeVisible();
  await expect(page).toHaveScreenshot("repo-mode-state-proposal-review.png",{fullPage:true});

  onboardingState = { ...onboardingState, onboarding:{...onboardingState.onboarding,status:"waiting_merge",actions:[]}, source_delivery_intent:{status:"waiting_merge",pull_request:{html_url:"https://github.com/lward27/yfinance_wrapper/pull/73"},provider_checks:{status:"passing",required_checks:[],expires_at:"2026-08-25T10:15:00Z"},merge_provenance:null} };
  await page.reload();
  await expect(page.getByText("waiting merge",{exact:true})).toBeVisible();
  await expect(page).toHaveScreenshot("repo-mode-state-onboarding-pr-wait.png",{fullPage:true});

  repositoryState = {...repositoryOverview,readiness:{...repositoryOverview.readiness,coding_status:"blocked",blockers:[{code:"runner_verification_stale",summary:"Refresh the isolated runner verification."}]},readiness_stale_reasons:["assessment_expired"],capabilities:[{capability:"source_reader",status:"stale"},{capability:"source_writer",status:"configured_unverified"},{capability:"source_observer",status:"available"}]};
  await page.goto(`/#/repositories/${repositoryId}/readiness`);
  await expect(page.getByText("assessment expired").first()).toBeVisible();
  await expect(page).toHaveScreenshot("repo-mode-state-coding-blocked-stale.png",{fullPage:true});

  repositoryState = repositoryOverview;
  await page.reload();
  await expect(page.getByRole("button",{name:/Create WorkItem/})).toBeEnabled();
  await expect(page).toHaveScreenshot("repo-mode-state-coding-ready.png",{fullPage:true});

  await page.goto(`/#/work-items/${visualWorkItemId}/overview`);
  await expect(page.getByRole("button",{name:"approve work plan"})).toBeVisible();
  await expect(page).toHaveScreenshot("repo-mode-state-planner-review.png",{fullPage:true});

  workItemState = {work_item:activeItem,action_rail:[],repo_mode:{stage_executions:[{id:"stage_builder_visual",stage_key:"implement",sequence:1,status:"running",run_id:visualRunId}],effective_stage_outcomes:[outcome("discover","succeeded","Pinned readiness accepted."),outcome("plan","succeeded","Exact WorkPlan approved.")],safe_advance:{eligible:false,blockers:["model_execution_active"]}}};
  await page.goto(`/#/work-items/${visualWorkItemId}/current-stage`);
  await page.reload();
  await expect(page.getByText("Model action is paused for review")).toBeVisible();
  await expect(page).toHaveScreenshot("repo-mode-state-active-builder-approval.png",{fullPage:true});

  runState = {...runState,status:"budget_extension_required",stop_reason:"soft_turn_budget_exhausted"};
  runSummary = {...runSummary,pending_approvals:[],stop_reason:"soft_turn_budget_exhausted"};
  await page.reload();
  await expect(page.getByText("Workspace retained for in-place extension")).toBeVisible();
  await expect(page).toHaveScreenshot("repo-mode-state-budget-extension.png",{fullPage:true});

  workItemState = {work_item:{...activeItem,status:"running",status_reason:"Verifier is inspecting sealed test evidence.",current_run_id:null,current_stage_execution_id:"stage_verify_visual"},action_rail:[],repo_mode:{stage_executions:[{id:"stage_test_visual",stage_key:"test",sequence:1,status:"succeeded"},{id:"stage_verify_visual",stage_key:"verify",sequence:1,status:"running"}],effective_stage_outcomes:[...stageOutcomes.filter(entry => ["discover","plan","implement","test"].includes(entry.stage_key))],safe_advance:{eligible:false,blockers:["model_execution_active"]}}};
  await page.goto(`/#/work-items/${visualWorkItemId}/stage-outcomes`);
  await page.reload();
  await expect(page.getByLabel("Lifecycle: verify")).toBeVisible();
  await expect(page.getByText("No sealed outcome",{exact:true}).first()).toBeVisible();
  await expect(page).toHaveScreenshot("repo-mode-state-tester-verifier-progress.png",{fullPage:true});

  organizationState = overview;
  await page.goto("/#/work-items");
  await expect(page.getByText("Manual source merge is required.")).toBeVisible();
  await expect(page).toHaveScreenshot("repo-mode-state-external-and-blocked-waits.png",{fullPage:true});
  await page.getByRole("button",{name:"History",exact:true}).click();
  await expect(page.getByText("Rejected drifted pull request")).toBeVisible();
  await expect(page.getByText("Legacy production rollout")).toBeVisible();
  await expect(page).toHaveScreenshot("repo-mode-state-history-and-legacy.png",{fullPage:true});
});

test("initial route failure is explicit rather than empty or stale", async ({ page }) => {
  await mockRepoModeApi(page,{"/api/organization/overview":{__status:503,body:{error:"overview unavailable"}}});
  await page.goto("/#/overview");
  await expect(page.getByRole("alert")).toHaveText("overview unavailable");
  await expect(page.getByText("No Products registered")).toHaveCount(0);
});

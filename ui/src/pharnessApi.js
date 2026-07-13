const JSON_HEADERS = {
  accept: "application/json",
};

let operatorName = "console-operator";

export function setOperatorName(name) {
  if (typeof name === "string" && name.trim() !== "") {
    operatorName = name.trim();
  }
}

export function getOperatorName() {
  return operatorName;
}

const WRITE_HEADERS = {
  ...JSON_HEADERS,
  "content-type": "application/json",
};

const RUN_EVENT_TYPES = [
  "run.queued",
  "run.started",
  "run.finished",
  "run.failed",
  "run.cancelled",
  "run.cancel_requested",
  "run.resumed",
  "model.request_started",
  "model.response_finished",
  "action.proposed",
  "policy.evaluated",
  "tool.started",
  "tool.finished",
  "approval.required",
  "approval.decided",
  "stream.error",
];

async function fetchJson(path, { optional = false } = {}) {
  const response = await fetch(path, { headers: JSON_HEADERS });
  if (optional && response.status === 404) {
    return null;
  }
  if (!response.ok) {
    const text = await response.text();
    throw new Error(`${response.status} ${response.statusText}${text ? `: ${text}` : ""}`);
  }
  return response.json();
}

async function postJson(path, body) {
  const response = await fetch(path, {
    method: "POST",
    headers: WRITE_HEADERS,
    body: JSON.stringify(body),
  });
  if (!response.ok) {
    const text = await response.text();
    throw new Error(`${response.status} ${response.statusText}${text ? `: ${text}` : ""}`);
  }
  return response.json();
}

async function firstListItem(path, key) {
  const payload = await fetchJson(`${path}?limit=1`);
  const items = Array.isArray(payload?.[key]) ? payload[key] : [];
  return items[0] ?? null;
}

function withQuery(path, values) {
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(values)) {
    if (value !== undefined && value !== null && value !== "") {
      params.set(key, String(value));
    }
  }
  const query = params.toString();
  return query ? `${path}?${query}` : path;
}

function executionScopeQuery(scope = {}) {
  return {
    namespace: scope.namespace,
    repo: scope.repo,
    branch: scope.branch,
    production_impacting: scope.productionImpacting,
  };
}

function flowPathForRoot(root) {
  if (root?.kind === "work_plan") {
    return `/api/work-plans/${encodeURIComponent(root.id)}/flow`;
  }
  return `/api/change-sets/${encodeURIComponent(root.id)}/flow`;
}

async function loadFlow(rootOverride) {
  if (rootOverride?.kind && rootOverride?.id) {
    return {
      root: rootOverride,
      flow: await fetchJson(flowPathForRoot(rootOverride)),
    };
  }

  const changeSet = await firstListItem("/api/change-sets", "change_sets");
  if (changeSet?.id) {
    return {
      root: { kind: "change_set", id: changeSet.id },
      flow: await fetchJson(`/api/change-sets/${encodeURIComponent(changeSet.id)}/flow`),
    };
  }

  const workPlan = await firstListItem("/api/work-plans", "work_plans");
  if (workPlan?.id) {
    return {
      root: { kind: "work_plan", id: workPlan.id },
      flow: await fetchJson(`/api/work-plans/${encodeURIComponent(workPlan.id)}/flow`),
    };
  }

  return { root: null, flow: null };
}

export async function loadDashboardData(flowRootOverride, scope = {}) {
  const executionScope = executionScopeQuery(scope);
  const namespaceScope = scope.namespace ? { resource_namespace: scope.namespace } : {};
  const [
    health,
    config,
    runs,
    runsSummary,
    approvals,
    approvalGates,
    auditEvents,
    workPlans,
    changeSets,
    incidents,
    remediationPlans,
    observations,
    flowResult,
  ] = await Promise.all([
    fetchJson("/health"),
    fetchJson("/api/config/effective"),
    fetchJson(withQuery("/api/runs", { limit: 25, ...executionScope })),
    fetchJson(withQuery("/api/runs/summary", executionScope)),
    fetchJson(withQuery("/api/approvals", { limit: 50, ...executionScope })),
    fetchJson(withQuery("/api/approval-gates", { limit: 50, ...namespaceScope })),
    fetchJson(withQuery("/api/audit-events", { limit: 50, ...executionScope })),
    fetchJson(withQuery("/api/work-plans", { limit: 50, ...namespaceScope })),
    fetchJson(withQuery("/api/change-sets", { limit: 25, ...namespaceScope })),
    fetchJson(withQuery("/api/incidents", { limit: 50, ...namespaceScope })),
    fetchJson(withQuery("/api/remediation-plans", { limit: 50, ...namespaceScope })),
    fetchJson(withQuery("/api/observations", { limit: 50, ...namespaceScope })),
    loadFlow(flowRootOverride),
  ]);

  setOperatorName(config?.operator?.name);

  return {
    health,
    config,
    runs: Array.isArray(runs?.runs) ? runs.runs : [],
    runsSummary,
    approvals: Array.isArray(approvals?.approvals) ? approvals.approvals : [],
    approvalGates: Array.isArray(approvalGates?.approval_gates) ? approvalGates.approval_gates : [],
    auditEvents: Array.isArray(auditEvents?.events) ? auditEvents.events : [],
    workPlans: Array.isArray(workPlans?.work_plans) ? workPlans.work_plans : [],
    changeSets: Array.isArray(changeSets?.change_sets) ? changeSets.change_sets : [],
    incidents: Array.isArray(incidents?.incidents) ? incidents.incidents : [],
    remediationPlans: Array.isArray(remediationPlans?.remediation_plans) ? remediationPlans.remediation_plans : [],
    observations: Array.isArray(observations?.observations) ? observations.observations : [],
    flowRoot: flowResult.root,
    flow: flowResult.flow,
    loadedAt: new Date().toLocaleTimeString(),
  };
}

export async function loadAuditEvents(filters = {}, scope = {}) {
  const payload = await fetchJson(withQuery("/api/audit-events", {
    limit: 100,
    kind: filters.kind,
    actor: filters.actor,
    resource_kind: filters.resourceKind,
    resource_id: filters.resourceId,
    run_id: filters.runId,
    search: filters.search,
    ...executionScopeQuery(scope),
  }));
  return Array.isArray(payload?.events) ? payload.events : [];
}

export async function loadWorkPlanFlow(workPlanId) {
  return fetchJson(`/api/work-plans/${encodeURIComponent(workPlanId)}/flow`);
}

export async function decideApproval(approvalId, decision) {
  const endpoint = decision === "approved" ? "approve" : "deny";
  return postJson(`/api/approvals/${encodeURIComponent(approvalId)}/${endpoint}`, {
    decided_by: operatorName,
    reason: `operator ${decision} from pharness ui`,
  });
}

export async function decideApprovalGate(gateId, decision) {
  const endpointByDecision = {
    satisfied: "satisfy",
    waived: "waive",
    rejected: "reject",
  };
  const endpoint = endpointByDecision[decision];
  if (!endpoint) {
    throw new Error(`unsupported approval gate decision: ${decision}`);
  }
  return postJson(`/api/approval-gates/${encodeURIComponent(gateId)}/${endpoint}`, {
    decided_by: operatorName,
    reason: `operator ${decision} from pharness ui`,
  });
}

export async function submitRun({ task, cwd, maxTurns }) {
  return postJson("/api/runs", {
    task,
    cwd: cwd || ".",
    max_turns: maxTurns ? Number(maxTurns) : 40,
  });
}

export async function cancelRun(runId) {
  return postJson(`/api/runs/${encodeURIComponent(runId)}/cancel`, {});
}

const TEKTON_E2E_NAMESPACE = "tekton-pipelines";
const TEKTON_E2E_PIPELINE = "pharness-e2e-noop";
const TEKTON_E2E_CONTRACT = { params: [], workspaces: [] };

function smokeReason(stage) {
  return `console bounded Tekton e2e smoke: ${stage}`;
}

async function transition(path, targetStatus) {
  return postJson(path, {
    target_status: targetStatus,
    actor: operatorName,
    reason: smokeReason(`transition to ${targetStatus}`),
  });
}

async function ensureTektonE2eContract() {
  const payload = await fetchJson(withQuery("/api/pipeline-contracts", {
    namespace: TEKTON_E2E_NAMESPACE,
    pipeline_ref: TEKTON_E2E_PIPELINE,
    status: "active",
    limit: 10,
  }));
  const contracts = Array.isArray(payload?.pipeline_contracts) ? payload.pipeline_contracts : [];
  if (contracts.length === 0) {
    return postJson("/api/pipeline-contracts", {
      namespace: TEKTON_E2E_NAMESPACE,
      pipeline_ref: TEKTON_E2E_PIPELINE,
      version: "e2e-v1",
      contract_json: TEKTON_E2E_CONTRACT,
      actor: operatorName,
      reason: smokeReason("create fixture contract"),
    });
  }
  if (contracts.length !== 1 || JSON.stringify(contracts[0].contract_json) !== JSON.stringify(TEKTON_E2E_CONTRACT)) {
    throw new Error("The active e2e PipelineContract is missing, duplicated, or does not match the fixture's empty inputs.");
  }
  return contracts[0];
}

export async function prepareTektonE2eSmoke() {
  const observation = await postJson("/api/observations", {
    source: "tekton_e2e_smoke",
    kind: "pipeline_execution_request",
    subject: "finance-experiment-safety-check",
    summary: "Bounded execution smoke; finance experiment resources are observation-only and unchanged.",
    resource_namespace: TEKTON_E2E_NAMESPACE,
    resource_kind: "Pipeline",
    resource_name: TEKTON_E2E_PIPELINE,
    resource_ref: {
      apiVersion: "tekton.dev/v1",
      kind: "Pipeline",
      namespace: TEKTON_E2E_NAMESPACE,
      name: TEKTON_E2E_PIPELINE,
    },
    data_json: { fixture: true, application_resources_changed: false },
    actor: operatorName,
    reason: smokeReason("create observation"),
  });
  const incident = await postJson("/api/incidents", {
    observation_id: observation.id,
    severity: "low",
    title: "Validate bounded Tekton execution",
    summary: "Exercise the inert Pharness delivery path without changing an application.",
    data_json: { fixture: true },
    actor: operatorName,
    reason: smokeReason("create incident"),
  });
  const remediationPlan = await postJson("/api/remediation-plans", {
    incident_id: incident.id,
    title: "Execute inert Tekton fixture",
    summary: "Preflight and execute a no-op PipelineRun; retain durable evidence.",
    risk_level: "medium",
    requires_approval: true,
    plan_json: {
      steps: ["verify contract", "dispatch inert PipelineRun", "record terminal evidence"],
      approval_gates: [
        { kind: "pipeline_mutation", required_before: "starting the inert PipelineRun" },
        { kind: "cluster_mutation", required_before: "creating the inert PipelineRun" },
      ],
    },
    actor: operatorName,
    reason: smokeReason("create remediation plan"),
  });
  const gates = await fetchJson(withQuery("/api/approval-gates", {
    remediation_plan_id: remediationPlan.id,
    limit: 10,
  }));
  for (const gate of gates.approval_gates ?? []) {
    if (["pipeline_mutation", "cluster_mutation"].includes(gate.gate_kind)) {
      await postJson(`/api/approval-gates/${encodeURIComponent(gate.id)}/satisfy`, {
        decided_by: operatorName,
        reason: smokeReason("approve bounded execution gate"),
      });
    }
  }
  const workPlanResult = await postJson("/api/work-plans", { remediation_plan_id: remediationPlan.id });
  const workPlan = workPlanResult.work_plan;
  await transition(`/api/work-plans/${encodeURIComponent(workPlan.id)}/transition`, "proposed");
  const approvedWorkPlan = (await transition(`/api/work-plans/${encodeURIComponent(workPlan.id)}/transition`, "approved")).work_plan;
  const changeSetResult = await postJson("/api/change-sets", {
    work_plan_id: approvedWorkPlan.id,
    title: "Bounded Tekton e2e change",
    summary: "No application code or configuration changes.",
    risk_level: "medium",
    change_set_json: { changes: [], fixture: TEKTON_E2E_PIPELINE, application_resources_changed: false },
    actor: operatorName,
    reason: smokeReason("create change set"),
  });
  const changeSet = changeSetResult.change_set;
  await transition(`/api/change-sets/${encodeURIComponent(changeSet.id)}/transition`, "proposed");
  const approvedChangeSet = (await transition(`/api/change-sets/${encodeURIComponent(changeSet.id)}/transition`, "approved")).change_set;
  await postJson(`/api/change-sets/${encodeURIComponent(approvedChangeSet.id)}/trusted-envelope`, {
    created_by: operatorName,
    reason: smokeReason("authorize bounded change set"),
    environment: "homelab",
    namespace: TEKTON_E2E_NAMESPACE,
    production_impacting: false,
  });
  const pipelineIntentResult = await postJson("/api/pipeline-intents/from-change-set", {
    change_set_id: approvedChangeSet.id,
    title: "Execute inert Tekton fixture",
    summary: "No-op Pipeline that only emits a marker.",
    risk_level: "medium",
    intent_kind: "build_test_package",
    intent_json: {
      execution: {
        enabled: true,
        namespace: TEKTON_E2E_NAMESPACE,
        pipeline_ref: TEKTON_E2E_PIPELINE,
        production_impacting: false,
        params: {},
        workspaces: [],
      },
    },
    actor: operatorName,
    reason: smokeReason("create pipeline intent"),
  });
  const pipelineIntent = pipelineIntentResult.pipeline_intent;
  const approvedPipelineIntent = (await transition(`/api/pipeline-intents/${encodeURIComponent(pipelineIntent.id)}/transition`, "approved")).pipeline_intent;
  const pipelineContract = await ensureTektonE2eContract();
  await postJson(`/api/pipeline-intents/${encodeURIComponent(approvedPipelineIntent.id)}/trusted-envelope`, {
    created_by: operatorName,
    reason: smokeReason("authorize only this inert pipeline intent"),
  });
  const preview = await postJson(`/api/pipeline-intents/${encodeURIComponent(approvedPipelineIntent.id)}/execute`, {
    dry_run: true,
    actor: operatorName,
    reason: smokeReason("preflight"),
  });
  if (!preview.ready || preview.status !== "ready" || preview.manifest?.metadata?.namespace !== TEKTON_E2E_NAMESPACE) {
    throw new Error("The bounded execution preflight did not pass. No PipelineRun was created.");
  }
  return { observation, incident, remediationPlan, workPlan: approvedWorkPlan, changeSet: approvedChangeSet, pipelineIntent: approvedPipelineIntent, pipelineContract, preview };
}

export async function dispatchTektonE2eSmoke(pipelineIntentId) {
  return postJson(`/api/pipeline-intents/${encodeURIComponent(pipelineIntentId)}/execute`, {
    dry_run: false,
    actor: operatorName,
    reason: smokeReason("explicit execution"),
  });
}

export async function loadPipelineIntent(pipelineIntentId) {
  return fetchJson(`/api/pipeline-intents/${encodeURIComponent(pipelineIntentId)}`);
}

export async function loadRunDetail(runId) {
  const encodedRunId = encodeURIComponent(runId);
  const [run, events, diff, artifacts] = await Promise.all([
    fetchJson(`/api/runs/${encodedRunId}`),
    fetchJson(`/api/runs/${encodedRunId}/events`),
    fetchJson(`/api/runs/${encodedRunId}/diff`, { optional: true }),
    fetchJson(`/api/runs/${encodedRunId}/artifacts`, { optional: true }),
  ]);

  return {
    run,
    events: Array.isArray(events?.events) ? events.events : [],
    diff: diff ?? { run_id: runId, changes: [], diff: "" },
    artifacts: Array.isArray(artifacts?.artifacts) ? artifacts.artifacts : [],
  };
}

export function subscribeRunEvents(runId, { afterSeq = 0, onEvent, onError }) {
  const params = afterSeq > 0 ? `?after_seq=${encodeURIComponent(String(afterSeq))}` : "";
  const source = new EventSource(`/api/runs/${encodeURIComponent(runId)}/events/stream${params}`);
  const handleEvent = (message) => {
    try {
      onEvent(JSON.parse(message.data));
    } catch (error) {
      onError?.(error instanceof Error ? error : new Error(String(error)));
    }
  };
  const handleStreamError = (message) => {
    try {
      const payload = JSON.parse(message.data);
      onError?.(new Error(payload.error ?? "run event stream failed"));
    } catch (error) {
      onError?.(error instanceof Error ? error : new Error(String(error)));
    }
  };

  for (const eventType of RUN_EVENT_TYPES) {
    source.addEventListener(eventType, eventType === "stream.error" ? handleStreamError : handleEvent);
  }
  source.onerror = () => {
    onError?.(new Error("run event stream disconnected"));
  };

  return () => source.close();
}

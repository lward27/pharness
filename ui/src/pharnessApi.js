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

async function loadFlow() {
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

export async function loadDashboardData() {
  const [health, config, runs, runsSummary, approvals, approvalGates, auditEvents, workPlans, flowResult] = await Promise.all([
    fetchJson("/health"),
    fetchJson("/api/config/effective"),
    fetchJson("/api/runs?limit=25"),
    fetchJson("/api/runs/summary"),
    fetchJson("/api/approvals?limit=25"),
    fetchJson("/api/approval-gates?limit=25"),
    fetchJson("/api/audit-events?limit=50"),
    fetchJson("/api/work-plans?limit=50"),
    loadFlow(),
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
    flowRoot: flowResult.root,
    flow: flowResult.flow,
    loadedAt: new Date().toLocaleTimeString(),
  };
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

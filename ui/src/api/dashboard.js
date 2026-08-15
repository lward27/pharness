import { executionScopeQuery, fetchJson, firstListItem, withQuery } from "./http";
import { setOperatorName } from "./operator";

function flowPathForRoot(root) {
  return root?.kind === "work_plan"
    ? `/api/work-plans/${encodeURIComponent(root.id)}/flow`
    : `/api/change-sets/${encodeURIComponent(root.id)}/flow`;
}

async function loadFlow(rootOverride) {
  if (rootOverride?.kind && rootOverride?.id) return { root: rootOverride, flow: await fetchJson(flowPathForRoot(rootOverride)) };
  const changeSet = await firstListItem("/api/change-sets", "change_sets");
  if (changeSet?.id) return { root: { kind: "change_set", id: changeSet.id }, flow: await fetchJson(`/api/change-sets/${encodeURIComponent(changeSet.id)}/flow`) };
  const workPlan = await firstListItem("/api/work-plans", "work_plans");
  if (workPlan?.id) return { root: { kind: "work_plan", id: workPlan.id }, flow: await fetchJson(`/api/work-plans/${encodeURIComponent(workPlan.id)}/flow`) };
  return { root: null, flow: null };
}

export async function loadDashboardData(flowRootOverride, scope = {}) {
  const executionScope = executionScopeQuery(scope);
  const namespaceScope = scope.namespace ? { resource_namespace: scope.namespace } : {};
  const [health, config, runs, runsSummary, approvals, approvalGates, auditEvents, workPlans, changeSets, incidents, remediationPlans, observations, workItems, scopeOptions, triage, triageSummary, flowResult] = await Promise.all([
    fetchJson("/health"), fetchJson("/api/config/effective"), fetchJson(withQuery("/api/runs", { limit: 25, ...executionScope })),
    fetchJson(withQuery("/api/runs/summary", executionScope)), fetchJson(withQuery("/api/approvals", { limit: 50, ...executionScope })),
    fetchJson(withQuery("/api/approval-gates", { limit: 50, ...namespaceScope })), fetchJson(withQuery("/api/audit-events", { limit: 50, ...executionScope })),
    fetchJson(withQuery("/api/work-plans", { limit: 50, ...namespaceScope })), fetchJson(withQuery("/api/change-sets", { limit: 25, ...namespaceScope })),
    fetchJson(withQuery("/api/incidents", { limit: 50, ...namespaceScope })), fetchJson(withQuery("/api/remediation-plans", { limit: 50, ...namespaceScope })),
    fetchJson(withQuery("/api/observations", { limit: 50, ...namespaceScope })),
    fetchJson(withQuery("/api/work-items", { limit: 100, include: "operator_state", target_environment: scope.environment })),
    fetchJson("/api/scopes/options", { optional: true }), fetchJson("/api/triage", { optional: true }), fetchJson("/api/triage/summary", { optional: true }), loadFlow(flowRootOverride),
  ]);
  setOperatorName(config?.operator?.name);
  return {
    health, config, runs: Array.isArray(runs?.runs) ? runs.runs : [], runGroups: Array.isArray(runs?.groups) ? runs.groups : [], runsSummary,
    approvals: Array.isArray(approvals?.approvals) ? approvals.approvals : [], approvalGroups: Array.isArray(approvals?.groups) ? approvals.groups : [],
    approvalGates: Array.isArray(approvalGates?.approval_gates) ? approvalGates.approval_gates : [], approvalGateGroups: Array.isArray(approvalGates?.groups) ? approvalGates.groups : [],
    auditEvents: Array.isArray(auditEvents?.events) ? auditEvents.events : [], workPlans: Array.isArray(workPlans?.work_plans) ? workPlans.work_plans : [], workPlanGroups: Array.isArray(workPlans?.groups) ? workPlans.groups : [],
    changeSets: Array.isArray(changeSets?.change_sets) ? changeSets.change_sets : [], incidents: Array.isArray(incidents?.incidents) ? incidents.incidents : [],
    remediationPlans: Array.isArray(remediationPlans?.remediation_plans) ? remediationPlans.remediation_plans : [], observations: Array.isArray(observations?.observations) ? observations.observations : [],
    workItems: Array.isArray(workItems?.work_items) ? workItems.work_items : [], workItemOperatorState: workItems?.operator_state ?? {},
    scopeOptions: scopeOptions ?? { environments: [], namespaces: [], repositories: [], branches: [], actors: [], origins: ["legacy"] },
    triage: triage ?? { items: [], summary: triageSummary ?? {} }, triageSummary: triageSummary ?? triage?.summary ?? {}, flowRoot: flowResult.root, flow: flowResult.flow,
    loadedAt: new Date().toLocaleTimeString(), loadedAtAbsolute: new Date().toLocaleString(),
  };
}

export function loadTriage() { return fetchJson("/api/triage"); }

export function loadTriageSummary() { return fetchJson("/api/triage/summary", { optional: true }); }

export async function loadAuditEvents(filters = {}, scope = {}) {
  const payload = await fetchJson(withQuery("/api/audit-events", {
    limit: 100, kind: filters.kind, actor: filters.actor, origin: filters.origin, resource_kind: filters.resourceKind, resource_id: filters.resourceId,
    run_id: filters.runId, search: filters.search, ...executionScopeQuery(scope),
  }));
  return Array.isArray(payload?.events) ? payload.events : [];
}

export function loadWorkPlanFlow(workPlanId) { return fetchJson(`/api/work-plans/${encodeURIComponent(workPlanId)}/flow`); }

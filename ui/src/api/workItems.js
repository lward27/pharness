import { fetchJson, postJson, withQuery } from "./http";

export function loadWorkItems(filters = {}) {
  return fetchJson(withQuery("/api/work-items", {
    status: filters.status,
    target_environment: filters.environment,
    source_repo: filters.repository,
    actor: filters.actor,
    origin: filters.origin,
    include: "operator_state",
    limit: filters.limit,
    offset: filters.offset,
  }));
}

export function loadWorkItem(workItemId) {
  return fetchJson(`/api/work-items/${encodeURIComponent(workItemId)}`);
}

export function loadWorkItemFlow(workItemId) {
  return fetchJson(`/api/work-items/${encodeURIComponent(workItemId)}/flow`);
}

export function previewWorkItemReconcile(workItemId) {
  return postJson(`/api/work-items/${encodeURIComponent(workItemId)}/reconcile`, { apply: false });
}

export function applyWorkItemReconcile(workItemId, { actor, reason }) {
  return postJson(`/api/work-items/${encodeURIComponent(workItemId)}/reconcile`, { apply: true, actor, reason });
}

export function preflightWorkItem(payload) {
  return postJson("/api/work-items/preflight", payload);
}

export function createWorkItem(payload) {
  return postJson("/api/work-items", payload);
}

export function executeWorkItemAction(workItemId, actionId, { actor, reason, stateHash }) {
  return postJson(`/api/work-items/${encodeURIComponent(workItemId)}/actions/${encodeURIComponent(actionId)}/execute`, {
    actor, reason, state_hash: stateHash,
  });
}

export function advanceWorkItem(workItemId, { actor, reason }) {
  return postJson(`/api/work-items/${encodeURIComponent(workItemId)}/advance`, { actor, reason, max_steps: 10 });
}

export function loadSystemReadiness() {
  return fetchJson("/api/system/readiness");
}

export function verifySystemCapability(capability) {
  return postJson(`/api/system/capabilities/${encodeURIComponent(capability)}/preflight`, {});
}

export function loadRollbackIntent(workItemId) {
  return fetchJson(`/api/work-items/${encodeURIComponent(workItemId)}/rollback-intents`);
}

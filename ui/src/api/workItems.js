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

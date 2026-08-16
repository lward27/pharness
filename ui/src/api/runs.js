import { executionScopeQuery, fetchJson, postJson, withQuery } from "./http";

export function loadRuns(filters = {}, scope = {}) {
  return fetchJson(withQuery("/api/runs", {
    search: filters.search,
    status: filters.status,
    actor: filters.actor,
    origin: filters.origin,
    limit: filters.limit,
    offset: filters.offset,
    ...executionScopeQuery(scope),
  }));
}

const RUN_EVENT_TYPES = [
  "run.queued", "run.started", "run.finished", "run.failed", "run.cancelled", "run.cancel_requested", "run.resumed",
  "model.request_started", "model.response_finished", "action.proposed", "policy.evaluated", "tool.started", "tool.finished",
  "approval.required", "approval.decided", "stream.error",
];

export function submitRun({ task, cwd, maxTurns }) {
  return postJson("/api/runs", { task, cwd: cwd || ".", max_turns: maxTurns ? Number(maxTurns) : 40 });
}

export function cancelRun(runId) {
  return postJson(`/api/runs/${encodeURIComponent(runId)}/cancel`, {});
}

export function decideRunApproval(runId, { decision, decidedBy, reason }) {
  return postJson(`/api/runs/${encodeURIComponent(runId)}/approvals`, { decision, decided_by: decidedBy, reason });
}

export async function loadRunDetail(runId) {
  const encodedRunId = encodeURIComponent(runId);
  const [run, events, diff, artifacts, operatorSummary, environmentPreparation] = await Promise.all([
    fetchJson(`/api/runs/${encodedRunId}`),
    fetchJson(`/api/runs/${encodedRunId}/events`),
    fetchJson(`/api/runs/${encodedRunId}/diff`, { optional: true }),
    fetchJson(`/api/runs/${encodedRunId}/artifacts`, { optional: true }),
    fetchJson(`/api/runs/${encodedRunId}/operator-summary`, { optional: true }),
    fetchJson(`/api/runs/${encodedRunId}/environment-preparation`, { optional: true }),
  ]);
  return {
    run,
    events: Array.isArray(events?.events) ? events.events : [],
    diff: diff ?? { run_id: runId, changes: [], diff: "" },
    artifacts: Array.isArray(artifacts?.artifacts) ? artifacts.artifacts : [],
    operatorSummary: operatorSummary?.run_id ? operatorSummary : null,
    environmentPreparation: environmentPreparation?.id ? environmentPreparation : null,
  };
}

export function subscribeRunEvents(runId, { afterSeq = 0, onEvent, onError }) {
  const params = afterSeq > 0 ? `?after_seq=${encodeURIComponent(String(afterSeq))}` : "";
  const source = new EventSource(`/api/runs/${encodeURIComponent(runId)}/events/stream${params}`);
  const handleEvent = (message) => {
    try { onEvent(JSON.parse(message.data)); } catch (error) { onError?.(error instanceof Error ? error : new Error(String(error))); }
  };
  const handleStreamError = (message) => {
    try { onError?.(new Error(JSON.parse(message.data).error ?? "run event stream failed")); } catch (error) { onError?.(error instanceof Error ? error : new Error(String(error))); }
  };
  for (const eventType of RUN_EVENT_TYPES) source.addEventListener(eventType, eventType === "stream.error" ? handleStreamError : handleEvent);
  source.onerror = () => onError?.(new Error("run event stream disconnected"));
  return () => source.close();
}

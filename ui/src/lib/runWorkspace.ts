export type WorkItemSection = "overview" | "attempt" | "delivery" | "evidence";

export type BudgetMetric = {
  used: number;
  limit: number;
  remaining: number;
  percent: number;
  tone: "healthy" | "warning" | "risk";
};

const ACTIVE_ATTEMPT_STATUSES = new Set([
  "queued",
  "preparing",
  "planning",
  "executing",
  "running",
  "approval_required",
  "budget_extension_required",
  "verifying",
]);

const WORKSPACE_EVENT_PREFIXES = ["run.", "model.", "action.", "policy.", "tool.", "approval.", "stream."];

export function defaultWorkItemSection(item: any): WorkItemSection {
  if (!item?.current_run_id) return "overview";
  return ACTIVE_ATTEMPT_STATUSES.has(String(item.status ?? "").toLowerCase()) ? "attempt" : "overview";
}

export function isActiveRun(run: any) {
  return ACTIVE_ATTEMPT_STATUSES.has(String(run?.status ?? "").toLowerCase());
}

export function budgetMetric(usedValue: unknown, limitValue: unknown): BudgetMetric {
  const used = Math.max(0, Number(usedValue) || 0);
  const limit = Math.max(0, Number(limitValue) || 0);
  const remaining = Math.max(0, limit - used);
  const percent = limit > 0 ? Math.min(100, Math.round((used / limit) * 100)) : 0;
  const tone = percent >= 95 ? "risk" : percent >= 75 ? "warning" : "healthy";
  return { used, limit, remaining, percent, tone };
}

export function formatRunDuration(secondsValue: unknown) {
  const seconds = Math.max(0, Math.floor(Number(secondsValue) || 0));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const remainder = seconds % 60;
  if (minutes < 60) return remainder ? `${minutes}m ${remainder}s` : `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  const minuteRemainder = minutes % 60;
  return minuteRemainder ? `${hours}h ${minuteRemainder}m` : `${hours}h`;
}

export function workspaceEvents(events: any[]) {
  return (events ?? []).filter((event) => WORKSPACE_EVENT_PREFIXES.some((prefix) => String(event?.type ?? "").startsWith(prefix)));
}

export function acceptanceRows(operatorSummary: any) {
  const results = Array.isArray(operatorSummary?.test_results) ? operatorSummary.test_results : [];
  return results.map((result: any, index: number) => ({
    id: `${result?.command ?? "acceptance"}-${index}`,
    command: result?.command ?? "Declared acceptance command",
    passed: result?.passed === true,
    result: result?.result,
  }));
}

export function changedPaths(changes: any[], operatorSummary: any) {
  const ordered = [
    ...(changes ?? []).map((change) => change?.path),
    ...(operatorSummary?.changed_paths ?? []),
  ].filter((path): path is string => typeof path === "string" && path.length > 0);
  return [...new Set(ordered)];
}

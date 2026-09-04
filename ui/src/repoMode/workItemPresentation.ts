const terminal = new Set(["completed", "succeeded", "failed", "cancelled", "inapplicable"]);
const stages = new Set(["discover", "plan", "implement", "test", "verify", "source_delivery"]);

/** Durable pointers and outcomes, never the last Run or stage position. */
export function workItemPosition(flow: any) {
  const item = flow?.work_item;
  const repo = flow?.repo_mode;
  const outcomes = repo?.history?.stage_outcomes || repo?.effective_stage_outcomes || [];
  const pinned = repo?.stage_executions?.find((e: any) => e.id === (item?.current_stage_execution_id || repo?.metadata?.current_stage_execution_id));
  const current = !item?.closed_at && pinned && !pinned.finished_at && !terminal.has(pinned.status) && !outcomes.some((o: any) => o.stage_execution_id === pinned.id) ? pinned : undefined;
  const action = flow?.action_rail?.[0];
  const delivery = repo?.effective_stage_outcomes?.find((o: any) => o.stage_key === "source_delivery");
  const liveInterval = repo?.lifecycle_timeline?.intervals?.find((e: any) => e.is_current);
  const boundary = flow?.reconcile_preview?.boundary;
  const stage = current?.stage_key || action?.lifecycle_stage || (delivery ? "source_delivery" : liveInterval?.stage_key) || pinned?.stage_key || (stages.has(boundary) ? boundary : "unavailable");
  return { current, action, stage };
}

export function recordedNumber(value: unknown) {
  return typeof value === "number" && Number.isFinite(value) ? Intl.NumberFormat("en", { notation: "compact", maximumFractionDigits: 1 }).format(value) : "Unavailable";
}
export function recordedPair(value: unknown, maximum: unknown) {
  return `${recordedNumber(value)} / ${recordedNumber(maximum)}`;
}

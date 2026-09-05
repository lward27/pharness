const terminal = new Set(["completed", "succeeded", "failed", "cancelled", "inapplicable"]);
export const hostedStages = ["discover", "plan", "implement", "test", "verify", "source_delivery", "release", "observe"];
const stages = new Set(hostedStages);

export function isHostedWorkItem(item: any) {
  return item?.workflow_kind === "hosted_sdlc";
}

export function workItemCondition(flow: any) {
  const {current, action} = workItemPosition(flow);
  const blockers = (action?.blockers || []).map((blocker: any) => typeof blocker === "string" ? blocker : blocker.summary || blocker.code).filter(Boolean);
  return blockers.join("; ") || flow?.repo_mode?.workflow_control?.reason || flow?.work_item?.status_reason || current?.stop_reason || (flow?.work_item?.closed_at ? "WorkItem closed" : "No wait reason recorded");
}

/** Durable pointers and outcomes, never the last Run or stage position. */
export function workItemPosition(flow: any) {
  const item = flow?.work_item;
  const repo = flow?.repo_mode;
  const outcomes = repo?.history?.stage_outcomes || repo?.effective_stage_outcomes || [];
  const pinned = repo?.stage_executions?.find((e: any) => e.id === (item?.current_stage_execution_id || repo?.metadata?.current_stage_execution_id));
  const current = !item?.closed_at && pinned && !pinned.finished_at && !terminal.has(pinned.status) && !outcomes.some((o: any) => o.stage_execution_id === pinned.id) ? pinned : undefined;
  const controls = (flow?.action_rail || []).filter((action: any) => action.effect_class === "workflow_control");
  const action = flow?.action_rail?.find((action: any) => action.effect_class !== "workflow_control");
  const delivery = repo?.effective_stage_outcomes?.find((o: any) => o.stage_key === "source_delivery");
  const liveInterval = repo?.lifecycle_timeline?.intervals?.find((e: any) => e.is_current);
  const boundary = flow?.reconcile_preview?.boundary;
  const hostedOutcome = isHostedWorkItem(item) ? ["observe", "release"].map(stage => repo?.effective_stage_outcomes?.find((outcome: any) => outcome.stage_key === stage && outcome.status !== "inapplicable")).find(Boolean) : undefined;
  const stage = current?.stage_key || (stages.has(action?.lifecycle_stage) ? action.lifecycle_stage : undefined) || liveInterval?.stage_key || (stages.has(boundary) ? boundary : undefined) || hostedOutcome?.stage_key || (delivery ? "source_delivery" : pinned?.stage_key) || "unavailable";
  return { current, action, controls, stage };
}

export function recordedNumber(value: unknown) {
  return typeof value === "number" && Number.isFinite(value) ? Intl.NumberFormat("en", { notation: "compact", maximumFractionDigits: 1 }).format(value) : "Unavailable";
}
export function recordedPair(value: unknown, maximum: unknown) {
  return `${recordedNumber(value)} / ${recordedNumber(maximum)}`;
}

import { riskTone, statusText } from "./formatters";

export type TriageThread = {
  id: string;
  kind: string;
  title: string;
  detail: string;
  status: string;
  tone: "low" | "medium" | "high";
  origin: string;
  origins: string[];
  createdAt: string | number | null;
  route: [string, string];
  actionLabel: string;
  signalCount: number;
  signals: string[];
  workItemId?: string;
};

type NormalizedSignal = Omit<TriageThread, "actionLabel" | "signalCount" | "signals" | "origins"> & {
  resourceKind?: string;
  workItemId?: string;
};

function resourceLabel(resource: any) {
  return [resource?.resource_kind, resource?.resource_name].filter(Boolean).join("/") || resource?.resource_namespace || "not scoped";
}

function approvalActionName(approval: any) {
  return approval?.action?.action ?? approval?.kind ?? "tool approval";
}

function approvalPreviewPath(approval: any) {
  return approval?.preview?.path ?? approval?.action?.path ?? "no preview path";
}

function routeForSignal(resourceKind: string | undefined, resourceId: string, workItemId?: string): [string, string] {
  if (workItemId || resourceKind === "work_item") return ["WorkItems", workItemId ?? resourceId];
  if (resourceKind === "approval") return ["Approvals", resourceId];
  if (resourceKind === "approval_gate") return ["Approval Gates", resourceId];
  if (resourceKind === "remediation_plan") return ["Remediation Plans", resourceId];
  return ["Audit", resourceId];
}

function normalizeSignals(data: any): NormalizedSignal[] {
  const apiItems = Array.isArray(data?.triage?.items) ? data.triage.items : [];
  if (apiItems.length) {
    return apiItems.map((item: any) => ({
      id: item.id,
      kind: statusText(item.kind),
      title: item.title,
      detail: item.summary,
      status: item.status,
      tone: riskTone(item.risk_level),
      origin: item.origin ?? "legacy",
      createdAt: item.created_at,
      resourceKind: item.resource_kind,
      workItemId: item.work_item_id,
      route: routeForSignal(item.resource_kind, item.resource_id, item.work_item_id),
    }));
  }

  return [
    ...(data?.approvalGates ?? []).filter((gate: any) => gate.status === "pending").map((gate: any) => ({
      id: gate.id,
      kind: "Approval gate",
      title: gate.title ?? gate.gate_kind ?? "Approval required",
      detail: gate.summary ?? resourceLabel(gate),
      status: gate.status,
      tone: riskTone(gate.risk_level),
      origin: gate.origin ?? "legacy",
      createdAt: gate.created_at,
      resourceKind: "approval_gate",
      workItemId: gate.work_item_id,
      route: routeForSignal("approval_gate", gate.id, gate.work_item_id),
    })),
    ...(data?.approvals ?? []).filter((approval: any) => approval.status === "pending").map((approval: any) => ({
      id: approval.id,
      kind: "Tool approval",
      title: approvalActionName(approval),
      detail: approval.summary ?? approvalPreviewPath(approval),
      status: approval.status,
      tone: "medium" as const,
      origin: approval.origin ?? "legacy",
      createdAt: approval.created_at,
      resourceKind: "approval",
      workItemId: approval.work_item_id,
      route: routeForSignal("approval", approval.id, approval.work_item_id),
    })),
    ...(data?.workItems ?? []).filter((item: any) => item.status === "blocked").map((item: any) => ({
      id: item.id,
      kind: "Blocked WorkItem",
      title: item.title,
      detail: item.status_reason ?? item.intent,
      status: item.status,
      tone: "high" as const,
      origin: item.origin ?? "legacy",
      createdAt: item.status_changed_at ?? item.updated_at,
      resourceKind: "work_item",
      workItemId: item.id,
      route: ["WorkItems", item.id] as [string, string],
    })),
  ];
}

function signalLabels(signals: NormalizedSignal[]) {
  const labels: string[] = [];
  const gateCount = signals.filter((signal) => signal.resourceKind === "approval_gate").length;
  const toolCount = signals.filter((signal) => signal.resourceKind === "approval").length;
  const blockedCount = signals.filter((signal) => signal.resourceKind === "work_item" && signal.status === "blocked").length;
  if (blockedCount) labels.push(blockedCount === 1 ? "Blocked WorkItem" : `${blockedCount} blocked WorkItems`);
  if (gateCount) labels.push(`${gateCount} lifecycle gate${gateCount === 1 ? "" : "s"}`);
  if (toolCount) labels.push(`${toolCount} tool approval${toolCount === 1 ? "" : "s"}`);
  const otherCount = signals.length - gateCount - toolCount - blockedCount;
  if (otherCount) labels.push(`${otherCount} controller signal${otherCount === 1 ? "" : "s"}`);
  return labels;
}

export function buildTriageThreads(data: any): TriageThread[] {
  const signals = normalizeSignals(data);
  const workItemTitles = new Map((data?.workItems ?? []).map((item: any) => [String(item.id), item.title]));
  const workItemGroups = new Map<string, NormalizedSignal[]>();
  const standalone: NormalizedSignal[] = [];

  signals.forEach((signal) => {
    if (!signal.workItemId) {
      standalone.push(signal);
      return;
    }
    const group = workItemGroups.get(signal.workItemId) ?? [];
    group.push(signal);
    workItemGroups.set(signal.workItemId, group);
  });

  const threads: TriageThread[] = standalone.map((signal) => ({
    ...signal,
    origins: [signal.origin],
    actionLabel: signal.resourceKind === "approval" ? "Review tool request" : "Inspect exception",
    signalCount: 1,
    signals: signalLabels([signal]),
  }));

  workItemGroups.forEach((group, workItemId) => {
    const blocked = group.find((signal) => signal.resourceKind === "work_item" && signal.status === "blocked");
    const representative = blocked ?? group[0];
    const origins = Array.from(new Set(group.map((signal) => signal.origin)));
    const createdAt = group.reduce<string | number | null>((oldest, signal) => {
      if (oldest == null) return signal.createdAt;
      if (signal.createdAt == null) return oldest;
      return Number(signal.createdAt) < Number(oldest) ? signal.createdAt : oldest;
    }, null);
    threads.push({
      id: `work-item:${workItemId}`,
      kind: group.length > 1 ? "WorkItem exception thread" : representative.kind,
      title: workItemTitles.get(workItemId) ?? representative.title ?? "WorkItem review required",
      detail: blocked?.detail ?? representative.detail,
      status: blocked ? "blocked" : "review_required",
      tone: group.some((signal) => signal.tone === "high") ? "high" : group.some((signal) => signal.tone === "medium") ? "medium" : "low",
      origin: origins.length === 1 ? origins[0] : "mixed",
      origins,
      createdAt,
      route: ["WorkItems", workItemId],
      actionLabel: "Open WorkItem cockpit",
      signalCount: group.length,
      signals: signalLabels(group),
      workItemId,
    });
  });

  return threads.sort((left, right) => Number(left.createdAt ?? 0) - Number(right.createdAt ?? 0));
}

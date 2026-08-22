import { describe, expect, it } from "vitest";
import { buildTriageThreads } from "./triagePresentation";

describe("buildTriageThreads", () => {
  it("bundles WorkItem-specific gates and failures into one cockpit thread", () => {
    const workItemId = "witem_triage_1234567890";
    const threads = buildTriageThreads({
      workItems: [{ id: workItemId, title: "Validate finance inputs" }],
      triage: { items: [
        { id: "gate_source", kind: "approval_gate", title: "Approve source mutation", summary: "Source boundary", status: "pending", risk_level: "high", origin: "operator", resource_kind: "approval_gate", resource_id: "gate_source", work_item_id: workItemId, created_at: "100" },
        { id: "gate_pipeline", kind: "approval_gate", title: "Approve pipeline mutation", summary: "Pipeline boundary", status: "pending", risk_level: "high", origin: "operator", resource_kind: "approval_gate", resource_id: "gate_pipeline", work_item_id: workItemId, created_at: "200" },
        { id: workItemId, kind: "blocked_work_item", title: "Validate finance inputs", summary: "Acceptance command failed", status: "blocked", risk_level: "high", origin: "operator", resource_kind: "work_item", resource_id: workItemId, work_item_id: workItemId, created_at: "300" },
      ] },
    });

    expect(threads).toHaveLength(1);
    expect(threads[0]).toMatchObject({
      title: "Validate finance inputs",
      detail: "Acceptance command failed",
      status: "blocked",
      signalCount: 3,
      signals: ["Blocked WorkItem", "2 lifecycle gates"],
      route: ["WorkItems", workItemId],
      actionLabel: "Open WorkItem cockpit",
    });
  });

  it("keeps standalone tool requests distinct from lifecycle gates", () => {
    const threads = buildTriageThreads({
      triage: { items: [{ id: "approval_1", kind: "tool_approval", title: "Run command", summary: "python -m unittest", status: "pending", risk_level: "medium", origin: "worker", resource_kind: "approval", resource_id: "approval_1", created_at: "100" }] },
    });

    expect(threads).toHaveLength(1);
    expect(threads[0]).toMatchObject({ route: ["Approvals", "approval_1"], actionLabel: "Review tool request", signalCount: 1 });
  });

  it("routes legacy fallback gates through their WorkItem when available", () => {
    const threads = buildTriageThreads({
      approvalGates: [{ id: "gate_legacy", title: "Approve deployment", status: "pending", risk_level: "high", work_item_id: "witem_legacy", created_at: "100" }],
      approvals: [],
      workItems: [],
    });

    expect(threads[0].route).toEqual(["WorkItems", "witem_legacy"]);
  });
});

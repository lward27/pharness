import { describe, expect, it } from "vitest";
import { selectPrimaryWorkItemAction } from "./workItemActions";

describe("WorkItem primary action selection", () => {
  it("prefers an applicable controller transition over an unrelated ready gate", () => {
    const selected = selectPrimaryWorkItemAction([
      { id: "satisfy_approval_gate:source_mutation", status: "ready" },
      { id: "prepare_git_delivery", status: "ready" },
    ], {
      action: "prepare_git_delivery",
      can_apply: true,
    });

    expect(selected?.id).toBe("prepare_git_delivery");
  });

  it("prefers a ready review when the controller is waiting at a boundary", () => {
    const selected = selectPrimaryWorkItemAction([
      { id: "approve_change_set", status: "ready" },
      { id: "awaiting_change_set_approval", status: "blocked" },
    ], {
      action: "awaiting_change_set_approval",
      can_apply: false,
    });

    expect(selected?.id).toBe("approve_change_set");
  });
});

import { describe, expect, it } from "vitest";
import { selectPrimaryWorkItemAction, selectRecoveryActions } from "./workItemActions";

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

  it("does not present rollback as forward progress after completion", () => {
    const actions = [
      { id: "terminal", status: "blocked", lifecycle_stage: "source" },
      { id: "execute_rollback_gitops_pr", status: "ready", lifecycle_stage: "rollback" },
    ];

    expect(selectPrimaryWorkItemAction(actions, { action: "terminal", can_apply: false }, "completed")).toBeUndefined();
    expect(selectRecoveryActions(actions)).toEqual([actions[1]]);
  });

  it("keeps rollback out of the primary selector for active work", () => {
    const actions = [
      { id: "approve_work_plan", status: "blocked", lifecycle_stage: "planning" },
      { id: "prepare_rollback", status: "ready", lifecycle_stage: "rollback" },
    ];

    expect(selectPrimaryWorkItemAction(actions, { action: "approve_work_plan", can_apply: false }, "blocked")).toEqual(actions[0]);
  });
});

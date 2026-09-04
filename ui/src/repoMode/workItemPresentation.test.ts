import { describe, expect, it } from "vitest";
import { recordedNumber, recordedPair, workItemPosition } from "./workItemPresentation";

describe("recorded WorkItem presentation", () => {
  const previous = { id:"old", stage_key:"plan", status:"completed", run_id:"historical-run" };
  it("never uses a historical Run in place of a current execution", () => {
    expect(workItemPosition({work_item:{},repo_mode:{stage_executions:[previous]}}).current).toBeUndefined();
    expect(workItemPosition({work_item:{current_stage_execution_id:"old"},repo_mode:{stage_executions:[previous]}}).current).toBeUndefined();
  });
  it("honors a paused durable execution and excludes a sealed one", () => {
    const flow:any = {work_item:{current_stage_execution_id:"paused"},repo_mode:{stage_executions:[previous,{id:"paused",stage_key:"implement",status:"paused",run_id:"current"}]}};
    expect(workItemPosition(flow).current.run_id).toBe("current");
    flow.repo_mode.effective_stage_outcomes=[{stage_execution_id:"paused",stage_key:"implement",status:"failed"}];
    expect(workItemPosition(flow).current).toBeUndefined();
  });
  it("preserves the first blocked action rather than picking a later ready one", () => {
    const first={id:"review",status:"blocked",lifecycle_stage:"plan"};
    const position=workItemPosition({action_rail:[first,{id:"later",status:"ready"}]});
    expect(position.action).toBe(first); expect(position.stage).toBe("plan");
  });
  it("does not label a terminal coding failure as source delivery", () => {
    const flow={work_item:{closed_at:"now",current_stage_execution_id:"failure"},repo_mode:{stage_executions:[{id:"failure",stage_key:"implement",status:"failed"}]}};
    expect(workItemPosition(flow).stage).toBe("implement");
  });
  it("distinguishes missing values and zero and keeps phone budget labels short", () => {
    expect(recordedNumber(null)).toBe("Unavailable");
    expect(recordedNumber(0)).toBe("0");
    expect(recordedPair(400000,1000000)).toBe("400K / 1M");
  });
});

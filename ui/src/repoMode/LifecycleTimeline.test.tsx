import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { LifecycleTimeline, intervalTimes, timestamp, type TimelineInterval } from "./LifecycleTimeline";
import { Lifecycle } from "./screens/WorkItemsScreen";

const entry:TimelineInterval={id:"repair",stage_key:"implement",sequence:2,resource_kind:"stage_execution",resource_id:"repair",kind:"execution",timing_basis:"recorded_execution",started_at:"2026-09-04T10:00:00Z",finished_at:null,is_current:true,is_effective:false,is_ongoing:true,status:"paused",origin:"agent",correction_of:{outcome_id:"failed"}};
describe("recorded laminae",()=>{
  it("uses observed time for ongoing intervals and never fills missing time",()=>{
    expect(timestamp("not a timestamp")).toBeNull();
    expect(timestamp("1788523200000")).toBe(1788523200000);
    expect(intervalTimes(entry,"2026-09-04T11:00:00Z")!.end-intervalTimes(entry,"2026-09-04T11:00:00Z")!.start).toBe(3600000);
    expect(intervalTimes({...entry,started_at:null},"2026-09-04T11:00:00Z")).toBeNull();
    expect(intervalTimes({...entry,is_ongoing:false},"2026-09-04T11:00:00Z")).toBeNull();
    expect(intervalTimes({...entry,finished_at:"2026-09-04T09:00:00Z"},"2026-09-04T11:00:00Z")).toBeNull();
  });
  it("exposes read-only correction and timing inspection",()=>{
    render(<LifecycleTimeline workItemId="wi" projection={{as_of:"2026-09-04T11:00:00Z",elapsed_includes_waits:true,intervals:[entry]}} />);
    const button=screen.getByRole("button",{name:"Repair 2 · paused · 1h 0m elapsed"});
    button.focus(); fireEvent.click(button);
    const dialog=screen.getByRole("dialog",{name:"Recorded interval"});
    expect(within(dialog).getByText("Correction lineage")).toBeInTheDocument();
    expect(within(dialog).getByText("Active model time")).toBeInTheDocument();
    expect(within(dialog).queryByText("Confirm and apply")).not.toBeInTheDocument();
    fireEvent.keyDown(document,{key:"Escape"}); expect(screen.queryByRole("dialog")).not.toBeInTheDocument(); expect(button).toHaveFocus();
  });
  it("does not paint preceding stages successful without effective outcomes",()=>{
    const {container}=render(<Lifecycle stage="verify" outcomes={[{stage_key:"plan",status:"succeeded"},{stage_key:"test",status:"failed"}]} />);
    expect(container.querySelectorAll(".is-complete")).toHaveLength(1);
    expect(screen.getByLabelText("test: no successful outcome supplied")).not.toHaveClass("is-complete");
  });
});

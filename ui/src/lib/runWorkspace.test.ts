import { describe, expect, it } from "vitest";
import { acceptanceRows, budgetMetric, changedPaths, defaultWorkItemSection, formatRunDuration, workspaceEvents } from "./runWorkspace";

describe("run workspace presentation", () => {
  it("defaults active WorkItems with a durable run to Attempt", () => {
    expect(defaultWorkItemSection({ status: "executing", current_run_id: "run_1" })).toBe("attempt");
    expect(defaultWorkItemSection({ status: "budget_extension_required", current_run_id: "run_1" })).toBe("attempt");
    expect(defaultWorkItemSection({ status: "completed", current_run_id: "run_1" })).toBe("overview");
    expect(defaultWorkItemSection({ status: "executing", current_run_id: null })).toBe("overview");
  });

  it("calculates bounded budget progress and pressure", () => {
    expect(budgetMetric(7, 48)).toEqual({ used: 7, limit: 48, remaining: 41, percent: 15, tone: "healthy" });
    expect(budgetMetric(46, 48).tone).toBe("risk");
    expect(budgetMetric(60, 48).remaining).toBe(0);
  });

  it("keeps only execution events and preserves durable order", () => {
    const events = [
      { seq: 1, type: "run.started" },
      { seq: 2, type: "unrelated.event" },
      { seq: 3, type: "tool.finished" },
    ];
    expect(workspaceEvents(events).map((event) => event.seq)).toEqual([1, 3]);
  });

  it("derives exact acceptance and first-change evidence", () => {
    expect(acceptanceRows({ test_results: [{ command: "python -m unittest", passed: true }] })[0]).toMatchObject({ command: "python -m unittest", passed: true });
    expect(changedPaths([{ path: "src/a.py" }, { path: "src/a.py" }], { changed_paths: ["tests/a.py", "src/a.py"] })).toEqual(["src/a.py", "tests/a.py"]);
  });

  it("formats active time without false precision", () => {
    expect(formatRunDuration(42)).toBe("42s");
    expect(formatRunDuration(125)).toBe("2m 5s");
    expect(formatRunDuration(3720)).toBe("1h 2m");
  });
});

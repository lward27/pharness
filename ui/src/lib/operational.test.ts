import { describe, expect, it } from "vitest";
import { matchesOperationalFilters, operationalFilterOptions, recordActor, recordOrigin } from "./operational";

const records = [
  { id: "run_1", status: "blocked", origin: "operator", created_by: "lucas", task: "repair finance importer" },
  { id: "run_2", status: "completed", origin: "smoke", status_changed_by: "controller", task: "smoke delivery" },
];

describe("operational filters", () => {
  it("uses durable actors and origins when filtering lists", () => {
    expect(recordActor(records[0])).toBe("lucas");
    expect(recordOrigin({})).toBe("legacy");
    expect(matchesOperationalFilters(records[0], { search: "finance", status: "blocked", actor: "lucas", origin: "operator" }, (record) => record.task)).toBe(true);
    expect(matchesOperationalFilters(records[1], { search: "finance", status: "", actor: "", origin: "" }, (record) => record.task)).toBe(false);
  });

  it("merges configured and observed filter options without duplicates", () => {
    expect(operationalFilterOptions(records, { actors: ["lucas", "worker"], origins: ["system"] })).toEqual({
      statuses: ["blocked", "completed"],
      actors: ["controller", "lucas", "worker"],
      origins: ["operator", "smoke", "system"],
    });
  });
});

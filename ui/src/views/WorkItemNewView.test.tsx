import { describe, expect, it } from "vitest";
import { deploymentContractLabel } from "./WorkItemNewView";

describe("WorkItem wizard deployment contracts", () => {
  it("renders the target fields returned by the deployment contract API", () => {
    expect(deploymentContractLabel({
      id: "dcontract_yfinance",
      target_environment: "production",
      target_namespace: "apps-prod",
    })).toBe("production/apps-prod · dcontract_yfinance");
  });
});

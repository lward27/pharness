import { describe, expect, it } from "vitest";
import { parseRoute } from "./routes";

describe("Repo Mode typed hash routes", () => {
  it("parses the approved resource hierarchy", () => {
    expect(parseRoute("#/products/prod_one/history")).toMatchObject({ name:"product", params:{productId:"prod_one"}, section:"history" });
    expect(parseRoute("#/repository-onboardings/ronb_one")).toMatchObject({ name:"onboarding", params:{onboardingId:"ronb_one"} });
    expect(parseRoute("#/agents/runs/run_one")).toMatchObject({ name:"agentRun", params:{runId:"run_one"} });
  });

  it("redirects removed primary destinations to their approved owners", () => {
    expect(parseRoute("#/triage").canonicalHash).toBe("#/overview");
    expect(parseRoute("#/queue").canonicalHash).toBe("#/agents");
    expect(parseRoute("#/runs/run_one").canonicalHash).toBe("#/agents/runs/run_one");
    expect(parseRoute("#/status").canonicalHash).toBe("#/settings/platform");
  });
});

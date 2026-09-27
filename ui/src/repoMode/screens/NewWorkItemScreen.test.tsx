import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { NewWorkItemScreen } from "./WorkItemsScreen";

const source = "a".repeat(40);
const contextSource = "b".repeat(40);

function json(value: unknown, status = 200) {
  return new Response(JSON.stringify(value), {
    status,
    headers: { "content-type": "application/json" },
  });
}

const productOverview = {
  product: { id: "product_finance", display_name: "Finance" },
  repositories: [
    {
      id: "repo_frontend",
      external_id: "lward27/finance-frontend",
      registered_commit: source,
      coding_readiness: "ready",
      freshness: "current",
      work_item_eligibility: {
        mutable: { eligible: true, status: "ready", summary: "Ready" },
        context: { eligible: true, status: "ready", summary: "Ready" },
      },
    },
    {
      id: "repo_database",
      external_id: "lward27/finance_app_database_service",
      registered_commit: contextSource,
      work_item_eligibility: {
        mutable: { eligible: false, status: "stale", summary: "Readiness is stale" },
        context: {
          eligible: false,
          status: "missing",
          summary: "No successful deterministic discovery exists for the registered revision.",
          resolution: {
            label: "Prepare context Repository",
            destination: "repositories/repo_database/overview",
          },
        },
      },
    },
  ],
};

const repositoryOverview = {
  repository: {
    id: "repo_frontend",
    external_id: "lward27/finance-frontend",
    canonical_url: "https://github.com/lward27/finance-frontend.git",
    registered_commit: source,
  },
  readiness: { contract_status: "ready", coding_status: "ready" },
  readiness_stale_reasons: [],
  canonical_contract: {
    contract: {
      environment_profile: "node-24",
      acceptance_commands: [{ name: "test", command: "npm test" }],
    },
  },
};

describe("New WorkItem prerequisite guidance", () => {
  afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

  it("keeps an ineligible context visible with its exact preparation path", async () => {
    vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input);
      if (path.includes("/api/products/product_finance/overview")) return json(productOverview);
      if (path.includes("/api/repositories/repo_frontend/overview")) return json(repositoryOverview);
      if (path.includes("/api/agent-profiles")) return json({ agent_profiles: [] });
      if (path.includes("/api/inference-policies")) return json({ policies: [] });
      if (path.includes("/api/agent-execution-policies")) return json({ policies: [] });
      throw new Error(`Unexpected request ${path}`);
    }));

    render(<NewWorkItemScreen productId="product_finance" operatorName="lucas" />);
    fireEvent.change(await screen.findByLabelText("Mutable Repository"), { target: { value: "repo_frontend" } });

    const context = await screen.findByRole("checkbox", {
      name: /finance_app_database_service/i,
    });
    expect(context).toBeDisabled();
    expect(screen.getByText(/No successful deterministic discovery exists/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Prepare context Repository/ })).toBeInTheDocument();
  });

  it("reviews one server-authored capability step and reruns preflight", async () => {
    let preflightCalls = 0;
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input);
      if (path.includes("/api/products/product_finance/overview")) return json(productOverview);
      if (path.includes("/api/repositories/repo_frontend/overview")) return json(repositoryOverview);
      if (path.includes("/api/agent-profiles")) return json({ agent_profiles: [{ id: "repo-builder", profile_hash: "sha256:profile" }] });
      if (path.includes("/api/inference-policies")) return json({ policies: [] });
      if (path.includes("/api/agent-execution-policies")) return json({ policies: [] });
      if (path.includes("/api/system/capabilities/source_reader/preflight")) return json({ capability: "source_reader", status: "available" });
      if (path.includes("/api/products/product_finance/work-items/preflight")) {
        preflightCalls += 1;
        const common = {
          source_repo: "https://github.com/lward27/finance-frontend.git",
          source_commit: source,
          environment_profile_id: "node-24",
          product_model_snapshot_id: "pmodel_finance",
          planner_inference: { policy: { policy_id: "planner-default" } },
          predicted_mutations: [],
          authorization_boundaries: [],
          warnings: [],
          preflight_hash: `hash-${preflightCalls}`,
        };
        if (preflightCalls === 1) return json({
          ...common,
          blockers: [{ code: "repository_readiness_not_current", summary: "Readiness is stale" }],
          prerequisites: [{
            id: "prerequisite:source_reader_verification_required:repo_frontend",
            code: "source_reader_verification_required",
            status: "stale",
            summary: "Source access must pass a fresh isolated verification for this exact Repository.",
            subject: { repository_id: "repo_frontend", repository_name: "lward27/finance-frontend", revision: source, role: "mutable" },
            details: [],
            resolution: {
              kind: "verify_capability",
              label: "Verify source-reader access",
              effect_class: "controller_internal",
              inline: true,
              confirmation_required: true,
              capability: "source_reader",
              repository_id: "repo_frontend",
              expected_result: "A fresh verification is recorded.",
            },
          }],
          recommended_resolution: { kind: "verify_capability", label: "Verify source-reader access" },
        });
        return json({ ...common, blockers: [], prerequisites: [], recommended_resolution: null, predicted_mutations: ["create_repo_work_item"] });
      }
      throw new Error(`Unexpected request ${path}`);
    });
    vi.stubGlobal("fetch", fetchMock);

    render(<NewWorkItemScreen productId="product_finance" operatorName="lucas" />);
    fireEvent.change(await screen.findByLabelText("Mutable Repository"), { target: { value: "repo_frontend" } });
    fireEvent.click(await screen.findByRole("button", { name: "Continue" }));
    fireEvent.change(screen.getByLabelText("Title"), { target: { value: "Adjust Finance colors" } });
    fireEvent.change(screen.getByLabelText("Bounded intent"), { target: { value: "Change the header accent colors and preserve contrast." } });
    fireEvent.click(screen.getByRole("checkbox", { name: /test/i }));
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    fireEvent.click(screen.getByRole("button", { name: "Check readiness" }));

    fireEvent.click(await screen.findByRole("button", { name: "Verify source-reader access" }));
    expect(await screen.findByRole("dialog", { name: "Verify source-reader access" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Confirm corrective step" }));

    await screen.findByText("Ready to create this WorkItem");
    expect(preflightCalls).toBe(2);
    const capabilityCall = fetchMock.mock.calls.find(([path]) => String(path).includes("/api/system/capabilities/source_reader/preflight"));
    expect(capabilityCall).toBeTruthy();
    expect(JSON.parse(String(capabilityCall?.[1]?.body))).toEqual({
      actor: "lucas",
      reason: "Request a bounded product change",
    });
    await waitFor(() => expect(screen.getByRole("button", { name: "Confirm and create WorkItem" })).toBeEnabled());
  });
});

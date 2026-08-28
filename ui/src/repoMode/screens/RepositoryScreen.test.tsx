import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { RepositoryScreen } from "./RepositoriesScreen";

describe("Repository capability verification", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("binds a source capability preflight to the owning Repository", async () => {
    const overview = {
      repository: {
        id: "repo_two",
        external_id: "example/repository",
        canonical_url: "https://github.com/example/repository.git",
        provider: "github",
        default_branch: "main",
        registered_commit: "a".repeat(40),
        state_version: 1,
      },
      product_bindings: [],
      capabilities: [
        {
          capability: "source_reader",
          status: "configured_unverified",
          summary: "Exact repository reachability is unverified",
        },
      ],
      trust_policy: { source_reader: "configured_policy" },
      authorization: { source_mutation: "onboarding_scoped" },
    };
    const fetchMock = vi.fn(async () =>
      new Response(JSON.stringify(overview), {
        status: 200,
        headers: { "content-type": "application/json" },
      }),
    );
    vi.stubGlobal("fetch", fetchMock);

    render(
      <RepositoryScreen
        repositoryId="repo_two"
        section="overview"
        operatorName="operator"
      />,
    );
    const verify = await screen.findByRole("button", {
      name: "Verify source reader",
    });
    fireEvent.click(verify);

    await waitFor(() =>
      expect(fetchMock).toHaveBeenCalledWith(
        "/api/system/capabilities/source_reader/preflight?repository_id=repo_two",
        expect.objectContaining({ method: "POST" }),
      ),
    );
  });
});

import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SettingsScreen } from "./PlatformScreens";

describe("Environment profile settings", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("renders a Node profile with generic runtime and lock policy", async () => {
    vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.endsWith("/api/environment-profiles")) return json({ profiles: [{
        id: "node-24",
        status: "available",
        platform: "linux/amd64",
        runtime_kind: "node",
        preparation_strategy: "node_npm_ci",
        image: `registry.example/pharness-node-runner@sha256:${"a".repeat(64)}`,
        revision: "b".repeat(40),
        accepted_dependency_lock_kinds: ["npm_package_lock"],
        lifecycle_scripts: "denied",
        required_executables: ["pharness-worker", "git", "node", "npm"],
      }] });
      if (url.endsWith("/api/config/effective")) return json({ features: { repo_mode_v1: { enabled: true, ui_enabled: true } } });
      return json({ capabilities: [], repository_allowlists: {} });
    }));

    render(<SettingsScreen section="profiles" operatorName="lucas" />);

    await waitFor(() => expect(screen.getByRole("heading", { name: "node-24" })).toBeInTheDocument());
    expect(screen.getByText("linux/amd64 · node · node_npm_ci")).toBeInTheDocument();
    expect(screen.getByText("npm_package_lock")).toBeInTheDocument();
    expect(screen.getByText("denied")).toBeInTheDocument();
    expect(screen.queryByText(/Python pending/i)).not.toBeInTheDocument();
  });
});

function json(value: unknown) {
  return new Response(JSON.stringify(value), { status: 200, headers: { "content-type": "application/json" } });
}

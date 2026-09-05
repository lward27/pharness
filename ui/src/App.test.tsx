import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";

vi.mock("./repoMode/RepoModeApp", () => ({RepoModeApp: () => <main>Configured console</main>}));

describe("console configuration availability", () => {
  afterEach(() => {cleanup(); vi.unstubAllGlobals();});
  it("shows an unavailable state and retries without falling into a different console", async () => {
    const fetch = vi.fn()
      .mockResolvedValueOnce(new Response(JSON.stringify({error:"Unavailable"}), {status:503}))
      .mockResolvedValueOnce(new Response(JSON.stringify({features:{repo_mode_v1:{ui_enabled:true}}}), {headers:{"content-type":"application/json"}}));
    vi.stubGlobal("fetch", fetch);
    render(<App />);
    expect(await screen.findByRole("heading", {name:"PHarness is unavailable"})).toBeInTheDocument();
    expect(screen.queryByText("Nothing needs attention")).not.toBeInTheDocument();
    expect(fetch).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", {name:"Retry connection"}));
    expect(await screen.findByText("Configured console")).toBeInTheDocument();
    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(2));
    expect(fetch.mock.calls.every(([path, init]) => path === "/api/config/effective" && !init?.method)).toBe(true);
  });
});

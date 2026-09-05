import { afterEach, describe, expect, it, vi } from "vitest";
import { getJson, sendJson } from "./api";

afterEach(() => vi.unstubAllGlobals());

describe("Lamina API failure messages", () => {
  it.each([
    ['{"error":"Legacy records unavailable"}', "Legacy records unavailable"],
    ['{"message":"Please sign in again"}', "Please sign in again"],
    ["Service temporarily unavailable", "Service temporarily unavailable"],
    ['{"diagnostic":{"upstream":"unreachable"}}', "Request failed (503 Service Unavailable)"],
    ["<html><body>Proxy failure</body></html>", "Request failed (503 Service Unavailable)"],
  ])("shows readable failure details without raw JSON or HTML: %s", async (body, message) => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(body, { status: 503, statusText: "Service Unavailable" })));
    await expect(getJson("/api/work-items")).rejects.toMatchObject({ message, status: 503 });
  });

  it("preserves the conflict status and stale-action reason for mutation recovery", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({ error: "Action state hash is stale; review current state" }), { status: 409 })));
    await expect(sendJson("/api/work-items/wi/actions/approve/execute", "POST", {})).rejects.toMatchObject({ message: "Action state hash is stale; review current state", status: 409 });
  });
});

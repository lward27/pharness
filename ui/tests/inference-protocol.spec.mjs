import { test, expect } from "@playwright/test";
import { protocolPayload, policyId } from "./fixtures/inference-protocol.mjs";

for (const theme of ["dark", "light"]) {
  for (const initialState of ["unverified", "stale", "failed", "passed"]) {
    test(`${theme}: exact policy protocol ${initialState}`, async ({ page }) => {
      let state = initialState;
      const posts = [];
      await page.addInitScript(value => localStorage.setItem("pharness.theme", value), theme);
      await page.route("**/api/**", async route => {
        const request = route.request();
        const url = new URL(request.url());
        if (!url.pathname.startsWith("/api/")) { await route.fallback(); return; }
        if (request.method() !== "GET") {
          posts.push({ path: url.pathname, body: request.postDataJSON() });
          expect(url.pathname).toBe("/api/inference-targets/fireworks-kimi-k3/revisions/v1/preflight");
          state = "passed";
          await route.fulfill({ json: { status: "passed" } });
          return;
        }
        await route.fulfill({ json: protocolPayload(url.pathname, url.searchParams, state) });
      });
      await page.goto("/#/settings/inference");
      const policy = page.locator("article").filter({ has: page.getByRole("heading", { name: "Planner Kimi K3", exact: true }) });
      await expect(policy).toBeVisible();
      await expect(page.locator("html")).toHaveCSS("color-scheme", theme);
      const qualify = policy.getByRole("button", { name: "Run qualification" });
      if (initialState === "passed") await expect(qualify).toBeEnabled();
      else await expect(qualify).toBeDisabled();
      if (initialState === "failed") await expect(policy.getByText(/Protocol response timed out/)).toBeVisible();
      expect(posts).toEqual([]); // Opening and inspecting settings is read-only.
      await expect(page.getByRole("button", { name: "Verify target", exact: true })).toHaveCount(0);
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 2)).toBe(true);
      if (initialState === "unverified") await page.screenshot({ path: test.info().outputPath(`ASTRA-policy-${theme}-${test.info().project.name}.png`), fullPage: true });
      const verify = policy.getByRole("button", { name: "Check protocol" });
      await verify.focus();
      await page.keyboard.press("Enter");
      await expect(qualify).toBeEnabled();
      expect(posts).toHaveLength(1);
      expect(posts[0].body.policy).toEqual({ policy_id: policyId, revision: "v1" });
      expect(posts[0].body.config_hash).toBe("fixture-registry");
    });
  }
}

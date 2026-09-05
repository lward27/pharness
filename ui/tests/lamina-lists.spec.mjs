import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { asOf, item, payload } from "./fixtures/lamina.mjs";

async function accessible(page) {
  const result = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa"]).analyze();
  expect(result.violations.filter(value => ["serious", "critical"].includes(value.impact))).toEqual([]);
  expect(await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)).toBeLessThanOrEqual(1);
}

for (const theme of ["dark", "light"]) {
  test(`Lamina ${theme} search keeps keyboard focus through loading and filtered emptiness`, async ({ page }, testInfo) => {
    await page.clock.setFixedTime(new Date(asOf));
    await page.emulateMedia({ reducedMotion: "reduce" });
    let releaseSearch;
    const pendingSearch = new Promise(resolve => { releaseSearch = resolve; });
    const writes = [];
    await page.route("**/api/**", async route => {
      const url = new URL(route.request().url());
      if (!url.pathname.startsWith("/api/")) return route.continue();
      if (route.request().method() !== "GET") writes.push(url.pathname);
      let body = payload(url.pathname, url.searchParams);
      if (url.pathname === "/api/repositories" && url.searchParams.get("search")) {
        await pendingSearch;
        body = { repositories: [], count: 0 };
      }
      await route.fulfill({ contentType: "application/json", body: JSON.stringify(body) });
    });
    await page.goto("/#/repositories");
    await expect(page.getByRole("button", { name: "Open Repository lward27/finance-frontend", exact: true })).toBeVisible();
    if (theme === "light") await page.getByRole("button", { name: "Use light theme" }).click();
    const input = page.getByRole("textbox", { name: "Search Repositories" });
    await input.focus();
    await page.keyboard.type("missing");
    await expect(input).toHaveValue("missing");
    await expect(input).toBeFocused();
    await expect(page.getByText("Loading current state…")).toBeVisible();
    releaseSearch();
    await expect(page.getByRole("heading", { name: "No matching Repositories" })).toBeVisible();
    await expect(input).toBeFocused();
    await expect(page.getByText("0 matching", { exact: true })).toBeVisible();
    await accessible(page);
    await expect(page).toHaveScreenshot(`lamina-list-empty-${theme}-${testInfo.project.name}.png`, { fullPage: true });
    expect(writes).toEqual([]);
  });

  test(`Lamina ${theme} pagination recovers from removed results and names unavailable legacy records`, async ({ page }, testInfo) => {
    await page.clock.setFixedTime(new Date(asOf));
    await page.emulateMedia({ reducedMotion: "reduce" });
    let shrunk = false;
    const writes = [];
    await page.route("**/api/**", async route => {
      const url = new URL(route.request().url());
      if (!url.pathname.startsWith("/api/")) return route.continue();
      if (route.request().method() !== "GET") writes.push(url.pathname);
      if (url.pathname === "/api/work-items" && url.searchParams.get("mode") === "legacy") {
        return route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ error: "Legacy records are temporarily unavailable" }) });
      }
      let body = payload(url.pathname, url.searchParams);
      if (url.pathname === "/api/work-items") {
        if (Number(url.searchParams.get("offset")) > 0) shrunk = true;
        body = { ...body, work_items: Number(url.searchParams.get("offset")) > 0 ? [] : [item], count: shrunk ? 2 : 51 };
      }
      await route.fulfill({ contentType: "application/json", body: JSON.stringify(body) });
    });
    await page.goto("/#/work-items");
    await expect(page.getByRole("heading", { name: item.title })).toBeVisible();
    if (theme === "light") await page.getByRole("button", { name: "Use light theme" }).click();
    const pages = page.getByRole("navigation", { name: "WorkItem pages", exact: true });
    await pages.getByRole("button", { name: "Next" }).click();
    await expect(page.getByRole("heading", { name: "No results on this page" })).toBeVisible();
    await expect(pages.getByText("No results on this page · 2 matching")).toBeVisible();
    await expect(pages.getByRole("button", { name: "Next" })).toBeDisabled();
    await expect(page.getByRole("alert")).toContainText("Legacy records are temporarily unavailable");
    await accessible(page);
    await expect(page).toHaveScreenshot(`lamina-list-unavailable-${theme}-${testInfo.project.name}.png`, { fullPage: true });
    await pages.getByRole("button", { name: "Previous" }).focus();
    await page.keyboard.press("Enter");
    await expect(page.getByRole("heading", { name: item.title })).toBeVisible();
    await expect(pages.getByText("1–1 of 2")).toBeVisible();
    expect(writes).toEqual([]);
  });
}

import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { asOf, item, payload } from "./fixtures/lamina.mjs";

async function fixture(page, state="waiting") {
  await page.clock.setFixedTime(new Date(asOf));
  await page.emulateMedia({reducedMotion:"reduce"});
  const writes=[];
  await page.route("**/api/**", async route=> {
    const url=new URL(route.request().url());
    if (!url.pathname.startsWith("/api/")) return route.continue();
    if(route.request().method()!=="GET") writes.push(url.pathname);
    await route.fulfill({contentType:"application/json",body:JSON.stringify(payload(url.pathname,url.searchParams,state))});
  });
  return writes;
}
async function accessible(page) {
  const result=await new AxeBuilder({page}).withTags(["wcag2a","wcag2aa","wcag21aa"]).analyze();
  expect(result.violations.filter(v=>["serious","critical"].includes(v.impact)),JSON.stringify(result.violations.map(v=>({id:v.id,nodes:v.nodes.map(n=>n.target)})))).toEqual([]);
  expect(await page.evaluate(()=>document.documentElement.scrollWidth-window.innerWidth)).toBeLessThanOrEqual(1);
}
test("lamina default shows recorded lanes, repair lineage, and read-only inspection",async({page},testInfo)=>{
  const writes=await fixture(page);
  await page.goto(`/#/work-items/${item.id}/overview`);
  await expect(page.getByRole("heading",{name:"WorkItem activity"})).toBeVisible();
  await expect(page.locator(".lamina-lane")).toHaveCount(6);
  await expect(page.getByRole("button",{name:/Repair 2 · succeeded/}).first()).toBeVisible();
  await accessible(page);
  await expect(page).toHaveScreenshot(`lamina-workitem-${testInfo.project.name}.png`,{fullPage:true});
  const repair=page.getByRole("button",{name:/Repair 2 · succeeded/}).first();
  await repair.focus(); await page.keyboard.press("Enter");
  const inspector=page.getByRole("dialog",{name:"Recorded interval"});
  await expect(inspector.getByText("Correction lineage",{exact:true})).toBeVisible();
  await accessible(page);
  await page.keyboard.press("Escape");
  await expect(inspector).toHaveCount(0); await expect(repair).toBeFocused();
  expect(writes).toEqual([]);
});
test("all eight sections remain owned, responsive, and read-only",async({page},testInfo)=>{
  const writes=await fixture(page);
  for(const route of ["overview","products","repositories","work-items","agents","releases","insights/audit","settings/platform"]){
    await page.goto(`/#/${route}`);
    await expect(page.locator(".lamina-topbar")).toBeVisible();
    await expect(page.locator(".repo-state").filter({hasText:"Loading current state"})).toHaveCount(0);
    if(route==="repositories") {
      await expect(page.getByRole("button",{name:"Open Repository lward27/finance-frontend",exact:true})).toBeVisible();
      await expect(page.getByRole("button",{name:"Open Repository lward27/lucas_engineering",exact:true})).toBeVisible();
      await expect(page.locator(".repo-table-row")).toHaveCount(5);
      await expect(page.locator(".repo-table-row").first()).toContainText("Finance");
    }
    await accessible(page);
  }
  await page.goto("/#/overview"); await expect(page.getByRole("heading",{name:"Lucas Engineering"})).toBeVisible();
  await expect(page).toHaveScreenshot(`lamina-overview-${testInfo.project.name}.png`,{fullPage:true});
  if(testInfo.project.name==="mobile") { await page.getByRole("button",{name:"Open navigation"}).click(); await expect(page.getByRole("dialog",{name:"Navigation"}).getByRole("button")).toHaveCount(9); await page.keyboard.press("Escape"); await expect(page.getByRole("button",{name:"Open navigation"})).toBeFocused(); }
  expect(writes).toEqual([]);
});
test("light theme tablet and unavailable host are honest",async({page})=>{
  await page.setViewportSize({width:768,height:1024}); await fixture(page,"paused");
  await page.goto(`/#/work-items/${item.id}/overview`);
  await page.getByRole("button",{name:"Use light theme"}).click();
  await expect(page.locator(".lamina-app")).toHaveClass(/theme-light/);
  await expect(page.getByText(/agent_host_unavailable: waiting/).first()).toBeVisible();
  await accessible(page);
  await expect(page).toHaveScreenshot("lamina-paused-light-tablet.png",{fullPage:true});
});
test("completed source delivery does not invent a deployment",async({page},testInfo)=>{
  await fixture(page,"completed"); await page.goto(`/#/work-items/${item.id}/delivery`);
  await expect(page.getByText("Source Delivery succeeded",{exact:true})).toBeVisible();
  await expect(page.getByText("inapplicable",{exact:true})).toHaveCount(2);
  await expect(page.getByText(/delivery evidence needs reconciliation|0\/5 stages/)).toHaveCount(0);
  await accessible(page);
  await expect(page).toHaveScreenshot(`lamina-source-completed-${testInfo.project.name}.png`,{fullPage:true});
});
test("proposal review and exact state-hashed confirmation",async({page},testInfo)=>{
  await fixture(page,"review"); await page.goto("/#/repository-onboardings/onboard_frontend");
  await expect(page.getByRole("heading",{name:"Agent proposal",exact:true})).toBeVisible();
  await accessible(page);
  await expect(page).toHaveScreenshot(`lamina-onboarding-${testInfo.project.name}.png`,{fullPage:true});
  await page.getByRole("button",{name:"approve proposal",exact:true}).click();
  const dialog=page.getByRole("dialog"); await expect(dialog.getByText("Exact action target")).toBeVisible(); await expect(dialog.getByText("State hash",{exact:true})).toBeVisible(); await accessible(page);
});

test("paused Codex and compacted Run history preserve provenance and unknown usage",async({page},testInfo)=>{
  const writes=await fixture(page);
  const streams=[]; page.on("request",request=>{if(request.url().includes("/events/stream"))streams.push(request.url());});
  for (const id of ["run_codex","run_compacted"]) {
    await page.goto(`/#/agents/runs/${id}`);
    await expect(page.getByRole("heading",{name:"Codex Builder · gpt-5.6-sol"})).toBeVisible();
    await expect(page.getByText("lucas-desktop · unavailable")).toBeVisible();
    await expect(page.getByText("Usage unavailable")).toBeVisible();
    if(id==="run_compacted") await expect(page.getByRole("heading",{name:"Raw Run payload intentionally expired"})).toBeVisible();
    await accessible(page);
    await expect(page).toHaveScreenshot(`lamina-${id}-${testInfo.project.name}.png`,{fullPage:true});
  }
  expect(streams).toEqual([]); expect(writes).toEqual([]);
});

test("stale review requires a new review and never resubmits automatically",async({page})=>{
  await fixture(page,"review");
  let submitted=0;
  await page.route("**/api/work-items/wi_market/actions/approve_work_plan/execute",async route=>{
    submitted++;
    await route.fulfill({status:409,contentType:"application/json",body:JSON.stringify({error:"Action state hash is stale; review current state"})});
  });
  await page.goto(`/#/work-items/${item.id}/overview`);
  const review=page.getByRole("button",{name:"approve work plan",exact:true});
  await review.click();
  const dialog=page.getByRole("dialog");
  await dialog.getByLabel("Reason").fill("Review the exact Plan");
  await dialog.getByRole("button",{name:"Confirm and apply"}).click();
  await expect(dialog.getByRole("button",{name:"Refresh required"})).toBeDisabled();
  await expect(dialog.getByRole("alert")).toContainText("stale");
  expect(submitted).toBe(1);
  await page.keyboard.press("Escape"); await expect(review).toBeFocused();
});

test("shared overview, search, and navigation remain read-only",async({page})=>{
  const writes=await fixture(page); let overviewReads=0;
  // The development StrictMode probe cancels its first request. Count delivered
  // responses, then prove shell and Overview share one subsequent polling cycle.
  page.on("response",response=>{if(new URL(response.url()).pathname==="/api/organization/overview")overviewReads++;});
  await page.goto("/#/overview");
  await expect(page.getByRole("heading",{name:"Lucas Engineering"})).toBeVisible();
  expect(overviewReads).toBe(1);
  await page.clock.fastForward(20_001);
  await expect.poll(()=>overviewReads).toBe(2);
  const search=page.getByRole("button",{name:"Search PHarness",exact:true}); await search.click();
  const dialog=page.getByRole("dialog",{name:"Search PHarness"});
  await dialog.getByRole("textbox").fill("Market");
  await dialog.getByRole("button",{name:/Market overview for Finance/}).click();
  await expect(page).toHaveURL(/work-items\/wi_market\/overview/);
  expect(writes).toEqual([]);
});

test("Finance topology preserves source repositories and scoped shared GitOps",async({page})=>{
  const writes=await fixture(page);
  await page.goto("/#/products/prod_finance/services-repositories");
  await expect(page.getByRole("heading",{name:"Product topology revision"})).toBeVisible();
  const scopes=page.getByLabel("Repository-relative scope",{exact:true});
  await expect(scopes).toHaveCount(18);
  expect(await scopes.evaluateAll(inputs=>inputs.map(input=>input.value))).toContain("charts/rabbitmq/**");
  expect(await scopes.evaluateAll(inputs=>inputs.map(input=>input.value))).toContain("charts/postgresql/**");
  await expect(page.getByRole("button",{name:"New WorkItem",exact:true})).toBeEnabled();
  await accessible(page);
  expect(writes).toEqual([]);
});

import AxeBuilder from "@axe-core/playwright";
import {expect, test} from "@playwright/test";
import {asOf, flow, item, payload} from "./fixtures/lamina.mjs";

// Explicit presentation fixtures; no model, cluster, approval, or release is exercised.
function hostedFlow() {
  const value = flow("completed");
  value.work_item = {...value.work_item,workflow_kind:"hosted_sdlc",status:"blocked",closed_at:null,closure_reason:null,status_reason:"Build evidence is missing. Production is not eligible."};
  value.reconcile_preview = {boundary:"release"};
  value.repo_mode.effective_stage_outcomes = value.repo_mode.effective_stage_outcomes.filter(outcome => outcome.status !== "inapplicable");
  value.repo_mode.lifecycle_timeline.intervals = value.repo_mode.lifecycle_timeline.intervals.map(interval => ({...interval,is_current:false,is_ongoing:false,finished_at:interval.finished_at || asOf}));
  value.delivery_configuration = {kind:"hosted_sdlc",release:{required:true,steps:[{key:"build",pipeline_contract_id:"pipeline_finance"},{key:"staging",deployment_contract_id:"staging_finance"},{key:"production",deployment_contract_id:"production_finance",approval_boundary:"before_gitops_merge"}]},observe:{required:true},required_evidence:["source_merge","image_digest","staging_verification","production_approval","runtime_verification"]};
  return value;
}
async function fixture(page, overrides={}) {
  await page.clock.setFixedTime(new Date(asOf));
  await page.emulateMedia({reducedMotion:"reduce"});
  const writes=[];
  await page.route("**/api/**", async route => {
    const request=route.request(), url=new URL(request.url());
    if(!url.pathname.startsWith("/api/")) return route.continue();
    if(request.method()!=="GET") writes.push(url.pathname);
    const handler=overrides[url.pathname];
    if(handler) return handler(route);
    const data=url.pathname===`/api/work-items/${item.id}/flow` ? hostedFlow() : payload(url.pathname,url.searchParams,"waiting");
    await route.fulfill({contentType:"application/json",body:JSON.stringify(data)});
  });
  return writes;
}
async function accessible(page) {
  expect((await new AxeBuilder({page}).withTags(["wcag2a","wcag2aa"]).analyze()).violations).toEqual([]);
  expect(await page.evaluate(() => document.documentElement.scrollWidth-window.innerWidth)).toBeLessThanOrEqual(1);
}

test("hosted condition precedes timing and release remains unevidenced in both themes",async({page},testInfo)=>{
  const writes=await fixture(page);
  await page.goto(`/#/work-items/${item.id}/overview`);
  const condition=page.getByRole("heading",{name:"What PHarness is doing"});
  await expect(condition).toBeVisible();
  const timing=page.getByRole("heading",{name:"WorkItem activity"});
  expect((await condition.boundingBox()).y).toBeLessThan((await timing.boundingBox()).y);
  await expect(page.locator(".lamina-lane")).toHaveCount(8);
  await expect(page.getByText("inapplicable",{exact:true})).toHaveCount(0);
  await expect(page.getByText(/No release outcome is recorded/)).toBeVisible();
  await accessible(page);
  await page.screenshot({path:testInfo.outputPath("ASTRA-hosted-overview-dark.png"),fullPage:true});
  await page.getByRole("button",{name:"Use light theme"}).click();
  await accessible(page);
  await page.screenshot({path:testInfo.outputPath("ASTRA-hosted-overview-light.png"),fullPage:true});
  await page.getByRole("button",{name:"delivery",exact:true}).click();
  await expect(page.getByRole("heading",{name:"Required release evidence"})).toBeVisible();
  await expect(page.getByText(/Production approval must precede/)).toBeVisible();
  await expect(page.getByText("Source Delivery succeeded",{exact:true})).toHaveCount(0);
  await accessible(page);
  await page.screenshot({path:testInfo.outputPath("ASTRA-hosted-delivery-light.png"),fullPage:true});
  expect(writes).toEqual([]);
});

test("failed configuration offers a keyboard retry without opening the legacy console",async({page})=>{
  let failed=true;
  const writes=await fixture(page,{"/api/config/effective":async route => route.fulfill({status:failed?503:200,contentType:"application/json",body:JSON.stringify(failed?{error:"Unavailable"}:payload("/api/config/effective"))})});
  await page.goto("/#/overview");
  await expect(page.getByRole("heading",{name:"PHarness is unavailable"})).toBeVisible();
  await expect(page.getByText("Nothing needs attention")).toHaveCount(0);
  const retry=page.getByRole("button",{name:"Retry connection"});
  await retry.focus();failed=false;await page.keyboard.press("Enter");
  await expect(page.locator(".lamina-topbar")).toBeVisible();
  expect(writes).toEqual([]);
});

test("an unavailable refresh retains the same record and disables its stale action",async({page},testInfo)=>{
  let fail=false;
  const initial=hostedFlow();
  initial.repo_mode.workflow_control={control:"active",condition:"blocked",reason:"Build evidence is missing. Production is not eligible.",as_of:asOf};
  initial.action_rail=["pause_workflow","cancel_workflow"].map(id=>({id,status:"ready",lifecycle_stage:"workflow",external_effect_summary:"Control new work; observation continues",state_hash:"fixture-state",effect_class:"workflow_control"}));
  const writes=await fixture(page,{[`/api/work-items/${item.id}/flow`]:async route=>route.fulfill({status:fail?503:200,contentType:"application/json",body:JSON.stringify(fail?{error:"Unavailable"}:initial)})});
  await page.goto(`/#/work-items/${item.id}/overview`);
  const pause=page.getByRole("button",{name:"pause workflow",exact:true});
  await expect(pause).toBeEnabled();
  await expect(page.getByRole("button",{name:"cancel workflow",exact:true})).toBeEnabled();
  fail=true;await page.clock.fastForward(10_001);
  await expect(page.getByText(/Showing retained data/)).toBeVisible();
  await expect(page.getByRole("heading",{name:item.title})).toBeVisible();
  await expect(pause).toBeDisabled();
  await expect(page.getByRole("button",{name:"cancel workflow",exact:true})).toBeDisabled();
  await accessible(page);
  await page.screenshot({path:testInfo.outputPath("ASTRA-hosted-stale-controls.png"),fullPage:true});
  expect(writes).toEqual([]);
});

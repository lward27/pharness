import AxeBuilder from "@axe-core/playwright";
import {expect,test} from "@playwright/test";
import {asOf,payload} from "./fixtures/lamina.mjs";

// Presentation fixtures only. No source, model, cluster, or approval is exercised.
test("blocked onboarding retains missing facts in both themes and supports keyboard editing",async({page},testInfo)=>{
  await page.clock.setFixedTime(new Date(asOf));
  await page.emulateMedia({reducedMotion:"reduce"});
  const original=payload("/api/repository-onboardings/onboard_frontend/flow");
  const flow=structuredClone(original);
  flow.onboarding.status="proposal_blocked";
  flow.onboarding.actions=[{id:"refresh_onboarding",status:"blocked",state_hash:"blocked-fixture",blockers:["An immutable dependency lock is missing. Revise incorrect proposal facts or refresh onboarding after the source is corrected."]}];
  flow.proposal.proposal={...flow.proposal.proposal,schema_version:"pharness.dev/repository-onboarding-proposal/v1alpha2",candidate_contract:null,blockers:["immutable_dependency_lock_missing"],instructions:"The dependency lock must exist before an executable contract can be proposed."};
  const writes=[];
  await page.route("**/api/**",async route=>{
    const request=route.request(),url=new URL(request.url());
    if(!url.pathname.startsWith("/api/")) return route.continue();
    if(request.method()!=="GET") writes.push({path:url.pathname,method:request.method(),body:request.postDataJSON()});
    const data=url.pathname==="/api/repository-onboardings/onboard_frontend/flow"?flow:payload(url.pathname,url.searchParams);
    await route.fulfill({contentType:"application/json",body:JSON.stringify(data)});
  });
  await page.goto("/#/repository-onboardings/onboard_frontend");
  await expect(page.getByText(/No executable contract proposed/)).toBeVisible();
  await expect(page.getByRole("button",{name:"approve proposal",exact:true})).toHaveCount(0);
  await expect(page.getByRole("button",{name:"retry proposer",exact:true})).toHaveCount(0);
  for(const theme of ["dark","light"]) {
    if(theme==="light") await page.getByRole("button",{name:"Use light theme"}).click();
    expect((await new AxeBuilder({page}).withTags(["wcag2a","wcag2aa"]).analyze()).violations).toEqual([]);
    expect(await page.evaluate(()=>document.documentElement.scrollWidth-window.innerWidth)).toBeLessThanOrEqual(1);
    await page.screenshot({path:testInfo.outputPath(`ASTRA-onboarding-blocked-${theme}.png`),fullPage:true});
  }
  expect(writes).toEqual([]);
  const edit=page.getByRole("button",{name:"Edit proposal revision"});
  await edit.focus();await page.keyboard.press("Enter");
  await expect(page.getByLabel("Executable contract JSON")).toHaveValue("null");
  const save=page.getByRole("button",{name:"Save new proposal revision"});
  await save.focus();await page.keyboard.press("Enter");
  await expect.poll(()=>writes.length).toBe(1);
  expect(writes[0].method).toBe("PUT");
  expect(writes[0].body.proposal.candidate_contract).toBeNull();
  expect(writes[0].body.proposal.blockers).toEqual(["immutable_dependency_lock_missing"]);
});

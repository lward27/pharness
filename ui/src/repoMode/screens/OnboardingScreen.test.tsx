import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { OnboardingScreen } from "./RepositoriesScreen";

describe("Repository onboarding", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("presents discovery, proposal, source delivery, and readiness with progressive disclosure", async () => {
    const flow = {
      onboarding:{id:"ronb_one",repository_id:"repo_one",product_id:"prod_one",registered_commit:"a".repeat(40),status:"ready",actions:[]},
      discovery:{status:"completed",inventory_json:{repository:{resolved_commit:"a".repeat(40)},files:[{path:"src/app.py",inspected:true}],inspected_text_bytes:1024,contract:{status:"legacy_alias"},language_indicators:{python:1},root_candidates:["src"],dependency_candidates:[{path:"requirements.lock",kind:"pip_requirements"}],command_candidates:[{command:"python -m unittest",source_path:".pharness/project.yaml"}],conflicts:[],blockers:[]}},
      proposal:{status:"approved",proposal:{discovery_id:"rdisc_one",discovery_hash:"sha256:discovery",candidate_contract:{api_version:"pharness.dev/v1alpha1",environment_profile:"python-3.11",agent_network:"denied",package_installation:"preparation_only",acceptance_commands:[{name:"unit",command:"python -m unittest"}],writable_paths:["src/**"],dependency_lock:{path:"requirements.lock",sha256:"abc"}},instructions:"Use the durable environment snapshot.",service_proposals:[],binding_proposals:[],assumptions:["Python comes from the runner."],conflicts:[],blockers:[],readiness_forecast:{coding_status:"ready"}}},
      source_delivery_intent:{id:"src_one",status:"merged",base_commit:"a".repeat(40),pull_request:{number:4,url:"https://github.com/example/repo/pull/4",head_sha:"b".repeat(40)},provider_checks:{status:"passing",observation_id:"obs_one"},merge_provenance:{status:"succeeded",merge_commit_sha:"c".repeat(40)}},
      readiness:{contract_status:"ready",coding_status:"ready",assessed_at:"1787654551238",environment_profile_id:"python-3.11",checks:[{key:"exact_checkout",status:"passed"}],warnings:[],blockers:[],evidence_refs:[{kind:"repository_contract_version",id:"contract_one"}]},
    };
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify(flow), {status:200,headers:{"content-type":"application/json"}})));
    render(<OnboardingScreen onboardingId="ronb_one" operatorName="operator" />);
    await waitFor(() => expect(screen.getByText("1 entries · 1 inspected")).toBeInTheDocument());
    expect(screen.getByRole("heading", {name:"Acceptance command candidates"})).toBeInTheDocument();
    expect(screen.getByText("Python comes from the runner.")).toBeInTheDocument();
    expect(screen.getByRole("link", {name:/PR #4/})).toHaveAttribute("href", "https://github.com/example/repo/pull/4");
    expect(screen.getByText("exact_checkout")).toBeInTheDocument();
    const rawDiscovery = screen.getByText("Raw discovery inventory · 1 entries").closest("details");
    expect(rawDiscovery).toBeInTheDocument();
    expect(rawDiscovery).not.toHaveAttribute("open");
  });
});

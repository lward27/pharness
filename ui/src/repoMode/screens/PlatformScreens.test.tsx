import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SettingsScreen } from "./PlatformScreens";

describe("Environment profile settings", () => {
  afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

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
      if (url.endsWith("/api/config/effective")) return json({ features: { repo_mode_v1: { enabled: true, ui_enabled: true }, coding_reliability_v2:{enabled:true} } });
      return json({ capabilities: [], repository_allowlists: {} });
    }));

    render(<SettingsScreen section="profiles" operatorName="lucas" />);

    await waitFor(() => expect(screen.getByRole("heading", { name: "node-24" })).toBeInTheDocument());
    expect(screen.getByText("linux/amd64 · node · node_npm_ci")).toBeInTheDocument();
    expect(screen.getByText("npm_package_lock")).toBeInTheDocument();
    expect(screen.getByText("denied")).toBeInTheDocument();
    expect(screen.queryByText(/Python pending/i)).not.toBeInTheDocument();
  });

  it("separates gateway alignment, target verification, and policy qualification", async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      if (url.endsWith("/api/inference-targets") && !init?.method) return json({
        gateway_enabled:true, registry_hash:"registry-one",
        gateway:{status:"available",registry_aligned:true,api_registry_hash:"registry-one",gateway_registry_hash:"registry-one",direct_fireworks_enabled:true},
        targets:[{target_id:"fireworks-kimi-k2p6",revision:"v1",display_name:"Fireworks Kimi K2.6",backend_kind:"fireworks",upstream_model:"accounts/fireworks/models/kimi-k2p6",allowed_stages:["plan","implement"],transport:{scheme:"https",private_network:false},authentication_configured:true,context_limit_tokens:262144,output_limit_tokens:16384,selectable:true,config_hash:"target-one",latest_verification:{status:"passed",expires_at:String(Math.floor(Date.now()/1000)+900)}}],
      });
      if (url.endsWith("/api/inference-policies")) return json({registry_hash:"registry-one",policies:[
        {policy_id:"fireworks-legacy-v1",revision:"v1",display_name:"Fireworks legacy behavior",eligible_stages:["plan","implement"],eligible_profiles:["repo-planner","repo-builder"],target:{target_id:"fireworks-kimi-k2p6"},reasoning:{context_mode:"provider_default"},tool_choice:"required",context_assembly_limit:16000,temperature:0.1,maximum_output_tokens:4096,qualified:true,is_default:true,qualification_status:"accepted_legacy_baseline",policy_hash:"policy-one"},
        {policy_id:"planner-kimi-k2p6-high-v1",revision:"v1",display_name:"Planner Kimi K2.6 high",eligible_stages:["plan"],eligible_profiles:["repo-planner"],reliability_v2_default_for_profiles:["repo-planner"],target:{target_id:"fireworks-kimi-k2p6"},reasoning:{effort:"high",context_mode:"current_turn"},tool_choice:"auto",context_assembly_limit:64000,temperature:0.1,maximum_output_tokens:8192,qualified:false,is_default:false,qualification_status:"not_qualified",policy_hash:"policy-two",qualification_contract:{suite_id:"planner-v1",agent_profile_id:"repo-planner",agent_profile_hash:"profile-two"}},
      ]});
      if (url.includes("/preflight") && init?.method === "POST") return json({status:"passed"});
      if (url.includes("/qualifications") && init?.method === "POST") return json({id:"infeval-one",status:"running",job_name:"pharness-inference-eval-one"});
      if (url.endsWith("/api/environment-profiles")) return json({profiles:[]});
      if (url.endsWith("/api/config/effective")) return json({features:{repo_mode_v1:{enabled:true,ui_enabled:true},coding_reliability_v2:{enabled:true}}});
      return json({capabilities:[],repository_allowlists:{},inference:{status:"available",registry_aligned:true}});
    });
    vi.stubGlobal("fetch",fetchMock);

    render(<SettingsScreen section="inference" operatorName="lucas" />);

    await waitFor(() => expect(screen.getByText("Gateway for new bound Runs")).toBeInTheDocument());
    expect(screen.getByRole("heading",{name:"Fireworks Kimi K2.6"})).toBeInTheDocument();
    expect(screen.getByText("accepted legacy baseline")).toBeInTheDocument();
    expect(screen.getByText(/Protocol compatibility alone is insufficient/i)).toBeInTheDocument();
    expect(screen.getByText("auto · parallel disabled")).toBeInTheDocument();
    expect(screen.getAllByText("repo-planner")).not.toHaveLength(0);
    expect(screen.getByRole("button",{name:"Run qualification"})).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button",{name:"Verify target"}));
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith(
      "/api/inference-targets/fireworks-kimi-k2p6/revisions/v1/preflight",
      expect.objectContaining({method:"POST",body:JSON.stringify({actor:"lucas",reason:"Verify inference target protocol and isolated connectivity",config_hash:"registry-one"})}),
    ));

    fireEvent.click(screen.getByRole("button",{name:"Run qualification"}));
    const qualificationButton = screen.getByRole("button",{name:"Run two-attempt qualification"});
    fireEvent.click(qualificationButton);
    fireEvent.click(qualificationButton);
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith(
      "/api/inference-policies/planner-kimi-k2p6-high-v1/revisions/v1/qualifications",
      expect.objectContaining({method:"POST",body:JSON.stringify({actor:"lucas",reason:"Run controlled qualification for Planner Kimi K2.6 high",config_hash:"registry-one",attempts:2})}),
    ));
    expect(fetchMock.mock.calls.filter(([input,init]) => String(input).includes("/qualifications") && (init as RequestInit | undefined)?.method === "POST")).toHaveLength(1);
    expect(await screen.findByText("infeval-one")).toBeInTheDocument();
  });
});

function json(value: unknown) {
  return new Response(JSON.stringify(value), { status: 200, headers: { "content-type": "application/json" } });
}

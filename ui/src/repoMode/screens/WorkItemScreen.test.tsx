import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WorkItemsScreen, WorkItemScreen } from "./WorkItemsScreen";

const completedFlow = {
  work_item: {
    id:"witem_repo_complete", mode:"repo", status:"completed", title:"Normalize periods", intent:"Validate periods before upstream calls", product_id:"prod_yfinance", repository_id:"repo_yfinance", source_commit:"a".repeat(40), acceptance_command_names:["unit"], acceptance_criteria:["python -m unittest"], run_budget:{}, attempt_count:1, max_attempts:2, closed_at:"123", closure_reason:"exact merge observed",
  },
  action_rail:[],
  repo_mode:{
    ownership:{product:{display_name:"Market Data"},repository:{external_id:"example/repo"},services:[{display_name:"History API"}]},
    stage_executions:[],
    effective_stage_outcomes:[
      { id:"out_delivery", stage_key:"source_delivery", status:"succeeded", content_hash:"sha256:delivery", sealed_at:"123", outcome:{ conclusion:"Exact approved head merged", stop_reason:"fresh checks and merge provenance matched" } },
      { id:"out_release", stage_key:"release", status:"inapplicable", content_hash:"sha256:release", sealed_at:"123", outcome:{ conclusion:"Repo Mode is source-only" } },
      { id:"out_observe", stage_key:"observe", status:"inapplicable", content_hash:"sha256:observe", sealed_at:"123", outcome:{ conclusion:"Runtime observation is outside Repo Mode" } },
    ],
    source_delivery_intent:{ id:"sdint_one", status:"merged", base_commit:"a".repeat(40), pull_request:{html_url:"https://github.com/example/repo/pull/1",head_sha:"b".repeat(40)}, provider_checks:{status:"passing",expires_at:"999"}, merge_provenance:{merge_commit_sha:"c".repeat(40)} },
    history:{stage_outcomes:[],work_plans:[{id:"plan_one",status:"approved",revision:1}],change_sets:[{id:"change_one",status:"approved",revision:1}],runs:[{id:"run_one",status:"completed"}],workspaces:[{id:"ws_one",status:"retained",source_commit:"a".repeat(40)}],audit_events:[{id:"audit_one",action:"approve_work_plan",actor:"operator",reason:"Reviewed plan",result:"recorded"}]},
  },
};

describe("completed Repo Mode delivery", () => {
  afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

  it("shows successful Source Delivery and controller-recorded inapplicable stages", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify(completedFlow), { status:200, headers:{"content-type":"application/json"} })));
    render(<WorkItemScreen workItemId="witem_repo_complete" section="delivery" operatorName="operator" />);
    await waitFor(() => expect(screen.getByText("Source Delivery succeeded")).toBeInTheDocument());
    expect(screen.getByRole("heading",{name:"Release"})).toBeInTheDocument();
    expect(screen.getByRole("heading",{name:"Observe"})).toBeInTheDocument();
    expect(screen.getAllByText("inapplicable")).toHaveLength(2);
    expect(screen.queryByText(/0\/5/)).not.toBeInTheDocument();
    expect(screen.queryByText(/delivery evidence needs reconciliation/i)).not.toBeInTheDocument();
  });

  it("keeps current ownership separate from structured immutable history", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify(completedFlow), { status:200, headers:{"content-type":"application/json"} })));
    const { rerender } = render(<WorkItemScreen workItemId="witem_repo_complete" section="overview" operatorName="operator" />);
    await waitFor(() => expect(screen.getByText("Market Data")).toBeInTheDocument());
    expect(screen.getByText("History API")).toBeInTheDocument();
    rerender(<WorkItemScreen workItemId="witem_repo_complete" section="history" operatorName="operator" />);
    await waitFor(() => expect(screen.getByText("Plans, changes, and Runs")).toBeInTheDocument());
    expect(screen.getByText("WorkPlans")).toBeInTheDocument();
    expect(screen.getByText("ChangeSets")).toBeInTheDocument();
    expect(screen.getByText("AgentRuns")).toBeInTheDocument();
    expect(screen.getByText("Reviewed plan", {exact:false})).toBeInTheDocument();
  });
});

describe("Repo Mode WorkItem rollup", () => {
  afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

  it("shows the effective outcome, active AgentRun, exact boundary, and age source", async () => {
    vi.stubGlobal("fetch", vi.fn(async (input:RequestInfo | URL) => {
      const url = String(input);
      const body = url.includes("mode=legacy")
        ? {work_items:[],operator_state:{},count:0}
        : url.includes("/api/organization/overview")
          ? {product_summaries:[{id:"prod_one",display_name:"Payments"}]}
          : {
              work_items:[{id:"witem_current",mode:"repo",product_id:"prod_one",repository_id:"repo_one",title:"Validate payment input",intent:"Reject malformed input",status:"running",source_commit:"a".repeat(40),updated_at:"2026-08-26T10:00:00Z"}],
              operator_state:{witem_current:{current_lifecycle_stage:"test",exact_wait_or_blocker:"Tester is executing declared acceptance",effective_stage_outcome:{status:"succeeded",outcome:{conclusion:"Implementation evidence sealed"}},active_agent_run:{id:"run_current",profile_id:"repo-tester",status:"running"}}},
              count:1,
            };
      return new Response(JSON.stringify(body), {status:200,headers:{"content-type":"application/json"}});
    }));
    render(<WorkItemsScreen />);
    await waitFor(() => expect(screen.getByText("Tester is executing declared acceptance")).toBeInTheDocument());
    expect(screen.getByText("Effective outcome", {exact:false}).parentElement).toHaveTextContent("Implementation evidence sealed");
    expect(screen.getByText("Active AgentRun", {exact:false}).parentElement).toHaveTextContent("repo-tester · run_current");
    expect(screen.getByText("Updated", {exact:false}).parentElement).toHaveAttribute("title");
  });
});

describe("stage inference authorization", () => {
  afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

  it("binds independently qualified Builder, Tester, and Verifier policies", async () => {
    const requests: Array<{url:string;init?:RequestInit}> = [];
    const flow = {
      ...completedFlow,
      work_item:{...completedFlow.work_item,id:"witem_chain",status:"waiting",closed_at:null,closure_reason:null},
      action_rail:[{id:"authorize_stage_chain",status:"ready",effect_class:"model_execution",state_hash:"state-chain",external_effect_summary:"Authorize the exact bounded Builder, Tester, and Verifier chain."}],
      repo_mode:{...completedFlow.repo_mode,effective_stage_outcomes:[],source_delivery_intent:null},
    };
    const policy = (stage:string, profile:string, id:string) => ({policy_id:id,revision:"v1",display_name:id,eligible_stages:[stage],eligible_profiles:[profile],selectable:true,qualified:true,is_default:true});
    const sharedPolicy = {policy_id:"legacy-policy",revision:"v1",display_name:"legacy-policy",eligible_stages:["implement","test","verify"],eligible_profiles:["repo-builder","repo-tester","repo-verifier"],selectable:true,qualified:true,is_default:false};
    vi.stubGlobal("fetch", vi.fn(async (input:RequestInfo | URL, init?:RequestInit) => {
      const url = String(input); requests.push({url,init});
      if(url.endsWith("/api/inference-policies")) return json({policies:[policy("implement","repo-builder","builder-policy"),policy("test","repo-tester","tester-policy"),policy("verify","repo-verifier","verifier-policy"),sharedPolicy]});
      return json(flow);
    }));
    render(<WorkItemScreen workItemId="witem_chain" section="overview" operatorName="lucas" />);
    fireEvent.click(await screen.findByRole("button",{name:"authorize stage chain"}));
    await waitFor(() => expect(screen.getByLabelText("Builder policy")).toHaveValue("builder-policy@v1"));
    expect(screen.getByLabelText("Tester policy")).toHaveValue("tester-policy@v1");
    expect(screen.getByLabelText("Verifier policy")).toHaveValue("verifier-policy@v1");
    fireEvent.change(screen.getByLabelText("Apply one policy to remaining compatible stages"),{target:{value:"legacy-policy@v1"}});
    fireEvent.click(screen.getByRole("button",{name:"Apply to compatible stages"}));
    expect(screen.getByLabelText("Builder policy")).toHaveValue("legacy-policy@v1");
    expect(screen.getByLabelText("Tester policy")).toHaveValue("legacy-policy@v1");
    expect(screen.getByLabelText("Verifier policy")).toHaveValue("legacy-policy@v1");
    fireEvent.change(screen.getByLabelText("Builder policy"),{target:{value:"builder-policy@v1"}});
    fireEvent.change(screen.getByLabelText("Tester policy"),{target:{value:"tester-policy@v1"}});
    fireEvent.change(screen.getByLabelText("Verifier policy"),{target:{value:"verifier-policy@v1"}});
    fireEvent.change(screen.getByLabelText("Reason"),{target:{value:"Reviewed the exact stage policies"}});
    fireEvent.click(screen.getByRole("button",{name:"Confirm and apply"}));
    await waitFor(() => expect(requests.some(request => request.init?.method === "POST")).toBe(true));
    const submitted = requests.find(request => request.init?.method === "POST");
    expect(JSON.parse(String(submitted?.init?.body))).toMatchObject({
      actor:"lucas",reason:"Reviewed the exact stage policies",state_hash:"state-chain",
      inference_policies:{implement:{policy_id:"builder-policy",revision:"v1"},test:{policy_id:"tester-policy",revision:"v1"},verify:{policy_id:"verifier-policy",revision:"v1"}},
    });
  });

  it("binds Builder, Repair, optional Test diagnosis, and Verifier for reliability V2", async () => {
    const requests: Array<{url:string;init?:RequestInit}> = [];
    const flow = {
      ...completedFlow,
      work_item:{...completedFlow.work_item,id:"witem_chain_v2",status:"waiting",closed_at:null,closure_reason:null},
      action_rail:[{id:"authorize_stage_chain",status:"ready",effect_class:"model_execution",state_hash:"state-chain-v2",external_effect_summary:"Authorize deterministic Test and one bounded correction."}],
      repo_mode:{
        ...completedFlow.repo_mode,
        effective_stage_outcomes:[],
        source_delivery_intent:null,
        coding_reliability:{enabled:true,deterministic_test:true,max_internal_corrections:1,internal_corrections_used:0,correction_lineage:[]},
      },
    };
    const policy = (stage:string, profile:string, id:string) => ({
      policy_id:id,revision:"v2",display_name:id,eligible_stages:[stage],eligible_profiles:[profile],
      reliability_v2_default_for_profiles:[profile],selectable:true,qualified:true,is_default:false,
    });
    vi.stubGlobal("fetch", vi.fn(async (input:RequestInfo | URL, init?:RequestInit) => {
      const url = String(input); requests.push({url,init});
      if(url.endsWith("/api/inference-policies")) return json({policies:[
        policy("implement","repo-builder","builder-v2"),
        policy("implement","repo-repair","repair-v2"),
        policy("test","repo-test-diagnoser","diagnoser-v2"),
        policy("verify","repo-verifier","verifier-v2"),
      ]});
      return json(flow);
    }));

    render(<WorkItemScreen workItemId="witem_chain_v2" section="overview" operatorName="lucas" />);
    fireEvent.click(await screen.findByRole("button",{name:"authorize stage chain"}));
    await waitFor(() => expect(screen.getByLabelText("Builder policy")).toHaveValue("builder-v2@v2"));
    expect(screen.getByLabelText("Repair policy")).toHaveValue("repair-v2@v2");
    expect(screen.getByLabelText("Test diagnosis policy (optional)")).toHaveValue("diagnoser-v2@v2");
    expect(screen.getByLabelText("Verifier policy")).toHaveValue("verifier-v2@v2");
    expect(screen.getAllByText(/Deterministic Test/)).not.toHaveLength(0);
    expect(screen.getByText(/One Repair execution/)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Reason"),{target:{value:"Reviewed deterministic Test and correction policy"}});
    fireEvent.click(screen.getByRole("button",{name:"Confirm and apply"}));

    await waitFor(() => expect(requests.some(request => request.init?.method === "POST")).toBe(true));
    const submitted = requests.find(request => request.init?.method === "POST");
    expect(JSON.parse(String(submitted?.init?.body))).toMatchObject({
      actor:"lucas",reason:"Reviewed deterministic Test and correction policy",state_hash:"state-chain-v2",
      inference_policies:{
        implement:{policy_id:"builder-v2",revision:"v2"},
        repair:{policy_id:"repair-v2",revision:"v2"},
        test_diagnosis:{policy_id:"diagnoser-v2",revision:"v2"},
        verify:{policy_id:"verifier-v2",revision:"v2"},
      },
    });
  });

  it("shows deterministic Test origin and repair lineage at the current stage", async () => {
    const flow = {
      ...completedFlow,
      work_item:{...completedFlow.work_item,id:"witem_repair",status:"running",closed_at:null,closure_reason:null,current_stage_execution_id:"stage_repair"},
      repo_mode:{
        ...completedFlow.repo_mode,
        stage_executions:[{id:"stage_repair",stage_key:"implement",status:"running",origin:"agent",agent_profile_id:"repo-repair",sequence:4,created_at:"123"}],
        coding_reliability:{enabled:true,deterministic_test:true,max_internal_corrections:1,internal_corrections_used:1,correction_lineage:[{stage_execution_id:"stage_repair",correction_of:{outcome_id:"out_test_failed"}}]},
      },
    };
    vi.stubGlobal("fetch", vi.fn(async () => json(flow)));
    render(<WorkItemScreen workItemId="witem_repair" section="current-stage" operatorName="lucas" />);
    await waitFor(() => expect(screen.getByText("Deterministic Test enabled",{exact:false})).toBeInTheDocument());
    expect(screen.getByText(/correction allowance 1\/1/)).toBeInTheDocument();
    expect(screen.getByText(/repairs out_test_failed/)).toBeInTheDocument();
    expect(screen.getByText("repo-repair")).toBeInTheDocument();
    expect(screen.getByText("agent")).toBeInTheDocument();
  });
});

function json(value:any) {
  return new Response(JSON.stringify(value),{status:200,headers:{"content-type":"application/json"}});
}

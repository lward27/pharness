import { fireEvent, render, screen, waitFor } from "@testing-library/react";
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
  afterEach(() => vi.unstubAllGlobals());

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
  afterEach(() => vi.unstubAllGlobals());

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
  afterEach(() => vi.unstubAllGlobals());

  it("binds independently qualified Builder, Tester, and Verifier policies", async () => {
    const requests: Array<{url:string;init?:RequestInit}> = [];
    const flow = {
      ...completedFlow,
      work_item:{...completedFlow.work_item,id:"witem_chain",status:"waiting",closed_at:null,closure_reason:null},
      action_rail:[{id:"authorize_stage_chain",status:"ready",effect_class:"model_execution",state_hash:"state-chain",external_effect_summary:"Authorize the exact bounded Builder, Tester, and Verifier chain."}],
      repo_mode:{...completedFlow.repo_mode,effective_stage_outcomes:[],source_delivery_intent:null},
    };
    const policy = (stage:string, id:string) => ({policy_id:id,revision:"v1",display_name:id,eligible_stages:[stage],selectable:true,qualified:true,is_default:true});
    const sharedPolicy = {policy_id:"legacy-policy",revision:"v1",display_name:"legacy-policy",eligible_stages:["implement","test","verify"],selectable:true,qualified:true,is_default:false};
    vi.stubGlobal("fetch", vi.fn(async (input:RequestInfo | URL, init?:RequestInit) => {
      const url = String(input); requests.push({url,init});
      if(url.endsWith("/api/inference-policies")) return json({policies:[policy("implement","builder-policy"),policy("test","tester-policy"),policy("verify","verifier-policy"),sharedPolicy]});
      return json(flow);
    }));
    render(<WorkItemScreen workItemId="witem_chain" section="overview" operatorName="lucas" />);
    fireEvent.click(await screen.findByRole("button",{name:"authorize stage chain"}));
    await waitFor(() => expect(screen.getByLabelText("implement policy")).toHaveValue("builder-policy@v1"));
    expect(screen.getByLabelText("test policy")).toHaveValue("tester-policy@v1");
    expect(screen.getByLabelText("verify policy")).toHaveValue("verifier-policy@v1");
    fireEvent.change(screen.getByLabelText("Apply one policy to remaining compatible stages"),{target:{value:"legacy-policy@v1"}});
    fireEvent.click(screen.getByRole("button",{name:"Apply to compatible stages"}));
    expect(screen.getByLabelText("implement policy")).toHaveValue("legacy-policy@v1");
    expect(screen.getByLabelText("test policy")).toHaveValue("legacy-policy@v1");
    expect(screen.getByLabelText("verify policy")).toHaveValue("legacy-policy@v1");
    fireEvent.change(screen.getByLabelText("implement policy"),{target:{value:"builder-policy@v1"}});
    fireEvent.change(screen.getByLabelText("test policy"),{target:{value:"tester-policy@v1"}});
    fireEvent.change(screen.getByLabelText("verify policy"),{target:{value:"verifier-policy@v1"}});
    fireEvent.change(screen.getByLabelText("Reason"),{target:{value:"Reviewed the exact stage policies"}});
    fireEvent.click(screen.getByRole("button",{name:"Confirm and apply"}));
    await waitFor(() => expect(requests.some(request => request.init?.method === "POST")).toBe(true));
    const submitted = requests.find(request => request.init?.method === "POST");
    expect(JSON.parse(String(submitted?.init?.body))).toMatchObject({
      actor:"lucas",reason:"Reviewed the exact stage policies",state_hash:"state-chain",
      inference_policies:{implement:{policy_id:"builder-policy",revision:"v1"},test:{policy_id:"tester-policy",revision:"v1"},verify:{policy_id:"verifier-policy",revision:"v1"}},
    });
  });
});

function json(value:any) {
  return new Response(JSON.stringify(value),{status:200,headers:{"content-type":"application/json"}});
}

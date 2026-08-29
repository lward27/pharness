import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { RunDetailView } from "./RunDetailView";

describe("Run retention presentation", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("labels compacted raw payload as intentional expiry and preserves the sealed summary", async () => {
    const inference = {backend_kind:"fireworks",model:"accounts/fireworks/models/kimi-k2p6",target_id:"fireworks-kimi-k2p6",target_revision:"v1",policy_id:"planner-kimi-k2p6-high-v1",policy_revision:"v1",reasoning:{effort:"high",context_mode:"current_turn"},temperature:0.1,maximum_output_tokens:8192,binding_hash:"binding-hash",policy_hash:"policy-hash"};
    const summary = {
      run_id:"run_compacted",turns:12,recoverable_failures:1,retries:1,estimated_context_tokens:1200,
      actual_prompt_tokens:1000,actual_completion_tokens:200,actual_total_tokens:1200,compactions:1,
      truncated_tool_results:0,tools_started:5,tools_completed:5,tools_failed:1,changed_paths:["src/app.py"],
      diff_reference:"/api/runs/run_compacted/diff",test_commands:["python -m unittest"],
      test_results:[{command:"python -m unittest",passed:true}],acceptance_evidence:[{command:"python -m unittest",passed:true}],
      pending_approvals:[],environment_discovery_turns:0,approval_count:2,approval_wait_ms:2000,
      preparation_duration_ms:500,budget_extensions:0,stop_reason:"completed",actual_reasoning_tokens:50,cached_tokens:100,normalized_cost:0.002,inference_binding:inference,
    };
    vi.stubGlobal("fetch", vi.fn(async (input:RequestInfo | URL) => {
      const url = String(input);
      if(url.endsWith("/operator-summary")) return json(summary);
      if(url.endsWith("/events")) return json({events:[]});
      if(url.endsWith("/diff")) return json({run_id:"run_compacted",changes:[{id:"change",path:"src/app.py",diff:"[purged by retention policy]"}],diff:"[purged by retention policy]"});
      if(url.endsWith("/artifacts")) return json({artifacts:[]});
      if(url.endsWith("/environment-preparation")) return new Response("",{status:404});
      return json({id:"run_compacted",status:"completed",task:"Historical coding run",started_at:"1",finished_at:"2",run_budget:{initial_turns:48,initial_tokens:400000,active_execution_seconds:3600},budget_consumption:{turns_used:12,tokens_used:1200,active_execution_seconds_used:120,allowed_turns:48,allowed_tokens:400000},retention_state:"compacted",sealed_summary:summary,inference_binding:inference});
    }));
    render(<RunDetailView runId="run_compacted" onOpenQueue={() => {}} operatorName="lucas"/>);
    await waitFor(() => expect(screen.getByRole("heading",{name:"Raw Run payload intentionally expired"})).toBeInTheDocument());
    expect(screen.getByText(/immutable Run identity, sealed summary/i)).toBeInTheDocument();
    expect(screen.queryByRole("heading",{name:"Tool and model stream"})).not.toBeInTheDocument();
    expect(screen.getByText("python -m unittest")).toBeInTheDocument();
    expect(screen.getAllByText("src/app.py")).not.toHaveLength(0);
    expect(screen.queryByText(/application error/i)).not.toBeInTheDocument();
    expect(screen.getByRole("heading",{name:"planner-kimi-k2p6-high-v1@v1"})).toBeInTheDocument();
    expect(screen.getByText("fireworks · accounts/fireworks/models/kimi-k2p6")).toBeInTheDocument();
    expect(screen.getByText("high · current_turn")).toBeInTheDocument();
  });
});

function json(value:any) {
  return new Response(JSON.stringify(value),{status:200,headers:{"content-type":"application/json"}});
}

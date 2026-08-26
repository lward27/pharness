import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WorkItemScreen } from "./WorkItemsScreen";

const completedFlow = {
  work_item: {
    id:"witem_repo_complete", mode:"repo", status:"completed", title:"Normalize periods", intent:"Validate periods before upstream calls", product_id:"prod_yfinance", repository_id:"repo_yfinance", source_commit:"a".repeat(40), acceptance_command_names:["unit"], acceptance_criteria:["python -m unittest"], run_budget:{}, attempt_count:1, max_attempts:2, closed_at:"123", closure_reason:"exact merge observed",
  },
  action_rail:[],
  repo_mode:{
    stage_executions:[],
    effective_stage_outcomes:[
      { id:"out_delivery", stage_key:"source_delivery", status:"succeeded", content_hash:"sha256:delivery", sealed_at:"123", outcome:{ conclusion:"Exact approved head merged", stop_reason:"fresh checks and merge provenance matched" } },
      { id:"out_release", stage_key:"release", status:"inapplicable", content_hash:"sha256:release", sealed_at:"123", outcome:{ conclusion:"Repo Mode is source-only" } },
      { id:"out_observe", stage_key:"observe", status:"inapplicable", content_hash:"sha256:observe", sealed_at:"123", outcome:{ conclusion:"Runtime observation is outside Repo Mode" } },
    ],
    source_delivery_intent:{ id:"sdint_one", status:"merged", base_commit:"a".repeat(40), pull_request:{html_url:"https://github.com/example/repo/pull/1",head_sha:"b".repeat(40)}, provider_checks:{status:"passing",expires_at:"999"}, merge_provenance:{merge_commit_sha:"c".repeat(40)} },
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
});

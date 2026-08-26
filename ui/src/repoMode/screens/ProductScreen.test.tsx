import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ProductScreen } from "./ProductsScreen";

const product = { id:"prod_one", display_name:"Payments", description:"Payment services", owner_principal:"operator", state_hash:"sha256:product" };

function stubOverview(value:any) {
  vi.stubGlobal("fetch", vi.fn(async (input:RequestInfo | URL) => {
    const url = String(input);
    const body = url.endsWith("/api/products/prod_one") ? product : value;
    return new Response(JSON.stringify(body), { status:200, headers:{"content-type":"application/json"} });
  }));
}

describe("Product detail", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("blocks WorkItem creation with an exact Repository readiness path", async () => {
    stubOverview({ product, repositories:[{id:"repo_one",external_id:"example/payments",contract_readiness:"ready",coding_readiness:"blocked"}], current_work_items:[] });
    render(<ProductScreen productId="prod_one" section="work-items" operatorName="operator" />);
    await waitFor(() => expect(screen.getByText("WorkItem creation is unavailable")).toBeInTheDocument());
    expect(screen.getByRole("button", {name:"New WorkItem"})).toBeDisabled();
    expect(screen.getByRole("button", {name:"Resolve readiness"})).toBeInTheDocument();
  });

  it("renders Product evidence and audit summaries instead of raw JSON", async () => {
    stubOverview({
      product,
      repositories:[{id:"repo_one",external_id:"example/payments",contract_readiness:"ready",coding_readiness:"ready"}],
      evidence_summary:{validation_count:3,work_items_with_validations:1,work_item_denominator:2,latest_validated_at:"1787654551238",validators:[{name:"declared_acceptance",count:2}]},
      audit_events:[{id:"audit_one",action:"approve_work_plan",actor:"operator",reason:"Reviewed exact plan",created_at:"1787654551238",result:"recorded"}],
      capability_posture:[],
    });
    render(<ProductScreen productId="prod_one" section="evidence-audit" operatorName="operator" />);
    await waitFor(() => expect(screen.getByRole("heading", {name:"Evidence coverage"})).toBeInTheDocument());
    expect(screen.getByText("3")).toBeInTheDocument();
    expect(screen.getByText("1/2")).toBeInTheDocument();
    expect(screen.getByText("declared_acceptance")).toBeInTheDocument();
    expect(screen.getByText("Reviewed exact plan", {exact:false})).toBeInTheDocument();
    expect(screen.queryByText(/\{"validation_count"/)).not.toBeInTheDocument();
  });

  it("shows only genuine Product-connected Release records", async () => {
    stubOverview({product,repositories:[],connected_release_data:{available:true,releases:[{id:"release_one",title:"Payment release",status:"completed",work_item_id:"witem_one",source_revision:"b".repeat(40)}]}});
    render(<ProductScreen productId="prod_one" section="releases" operatorName="operator" />);
    await waitFor(() => expect(screen.getByRole("heading", {name:"Payment release"})).toBeInTheDocument());
    expect(screen.getByText("witem_one")).toBeInTheDocument();
    expect(screen.queryByText(/synthetic health/i)).not.toBeInTheDocument();
  });

  it("renders the current immutable RepositoryBinding revision returned by the API", async () => {
    stubOverview({
      product,
      services:[{id:"svc_history",display_name:"History API",status:"active"}],
      repositories:[{id:"repo_one",external_id:"example/payments",registered_commit:"a".repeat(40),coding_readiness:"ready"}],
      repository_bindings:[{
        binding:{id:"binding_one",repository_id:"repo_one",current_revision_id:"binding_revision_two",status:"active"},
        current_revision:{id:"binding_revision_two",revision:2,service_ids:["svc_history"]},
      }],
    });
    render(<ProductScreen productId="prod_one" section="services-repositories" operatorName="operator" />);
    await waitFor(() => expect(screen.getByText("Binding revision 2 · 1 Service mappings")).toBeInTheDocument());
    expect(screen.getByText("History API")).toBeInTheDocument();
  });
});

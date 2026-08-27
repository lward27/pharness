import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DataLifecycleScreen } from "./DataLifecycleScreen";

describe("Data lifecycle settings", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("shows the active generation, policy inventory, and exact archive deletion boundary", async () => {
    const requests:any[] = [];
    const archive = {
      id:"archive_one", archived_generation_id:"dbgen_old", database_claim:"pharness-api-data",
      archive_claim:"pharness-archive-one", deletion_eligible_at:"1787654551238", status:"retained",
      deletion_action:{status:"ready",state_hash:"sha256:archive-state",confirmation:"DELETE ARCHIVE archive_one",external_effect_summary:"Delete the two exact retained PVCs",blockers:[]},
    };
    vi.stubGlobal("fetch", vi.fn(async (input:RequestInfo | URL, init?:RequestInit) => {
      const url = String(input); requests.push({url,init});
      if(init?.method === "POST") return new Response(JSON.stringify({archive:{...archive,status:"deleted"}}),{status:200,headers:{"content-type":"application/json"}});
      if(url.endsWith("/api/system/data-inventory")) return json({inventory:{database_generation:{id:"dbgen_clean",schema_version:"0049",purpose:"clean finance generation",initializing_revision:"a".repeat(40)},retained_bytes:{messages:10,events:20},active_holds:0,table_counts:{work_items:0,runs:0}},policy:{workspace_days:7,raw_run_payload_days:30,evidence_retention:"indefinite",automatic_execution:false},holds:[]});
      if(url.endsWith("/api/system/archives")) return json({archives:[archive],count:1});
      if(url.endsWith("/api/system/retention/previews")) return json({previews:[],count:0});
      return json({receipts:[],count:0});
    }));
    render(<DataLifecycleScreen operatorName="lucas"/>);
    await waitFor(() => expect(screen.getByText("dbgen_clean")).toBeInTheDocument());
    expect(screen.getByText("7 days")).toBeInTheDocument();
    expect(screen.getByText("Database claim pharness-api-data · archive claim pharness-archive-one")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button",{name:"Review deletion"}));
    const confirmation = screen.getByLabelText("Archive deletion confirmation");
    fireEvent.change(confirmation,{target:{value:"DELETE ARCHIVE archive_one"}});
    fireEvent.click(screen.getByRole("button",{name:"Delete exact retained PVCs"}));
    await waitFor(() => expect(requests.some(request => request.url.endsWith("/api/system/archives/archive_one/delete") && request.init?.method === "POST")).toBe(true));
    const body = JSON.parse(requests.find(request => request.url.endsWith("/delete"))!.init.body);
    expect(body).toMatchObject({actor:"lucas",state_hash:"sha256:archive-state",confirmation:"DELETE ARCHIVE archive_one"});
  });
});

function json(value:any) {
  return new Response(JSON.stringify(value),{status:200,headers:{"content-type":"application/json"}});
}

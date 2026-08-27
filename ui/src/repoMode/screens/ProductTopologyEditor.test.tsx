import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ProductTopologyEditor } from "./ProductTopologyEditor";

describe("Product topology editor", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("previews and applies one exact typed-scope snapshot", async () => {
    const requests:any[] = [];
    vi.stubGlobal("fetch", vi.fn(async (input:RequestInfo | URL, init?:RequestInit) => {
      const url = String(input); requests.push({url,init});
      if(url.endsWith("/model-changes/preflight")) return json({
        state_hash:"sha256:product", preflight_hash:"sha256:preflight", resulting_snapshot_hash:"sha256:snapshot",
        normalized_change:{services:[{id:"svc_web",service_key:"finance-web",display_name:"Finance Web",description:"Browser frontend",status:"active"}],bindings:[{repository_id:"repo_frontend",status:"active",scopes:[{path_glob:"**",role:"source",service_id:"svc_web"}]}]},
        resulting_snapshot:{schema_version:"pharness.dev/product-model/v1alpha2",services:[{service_key:"finance-web"}]},
      });
      if(url.endsWith("/model-changes")) return json({product:{id:"prod_finance"},snapshot:{id:"pmodel_two"}});
      return json({
        product:{id:"prod_finance",state_hash:"sha256:product"},
        services:[{id:"svc_web",service_key:"finance-web",display_name:"Finance Web",description:"Browser frontend",status:"active"}],
        repositories:[{id:"repo_frontend",external_id:"lward27/finance-frontend",registered_commit:"a".repeat(40)}],
        bindings:[{repository_id:"repo_frontend",status:"active",typed_scopes:[{path_glob:"**",role:"source",service_id:"svc_web"}]}],
      });
    }));
    const applied = vi.fn();
    render(<ProductTopologyEditor productId="prod_finance" operatorName="lucas" onApplied={applied}/>);
    await waitFor(() => expect(screen.getByDisplayValue("finance-web")).toBeInTheDocument());
    expect(screen.getByDisplayValue("**")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button",{name:"Preview immutable snapshot"}));
    await waitFor(() => expect(screen.getByText("sha256:snapshot")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button",{name:"Confirm and apply exact revision"}));
    await waitFor(() => expect(applied).toHaveBeenCalled());
    const applyRequest = requests.find(request => request.url.endsWith("/model-changes") && request.init?.method === "POST");
    const body = JSON.parse(applyRequest.init.body);
    expect(body).toMatchObject({state_hash:"sha256:product",preflight_hash:"sha256:preflight",actor:"lucas"});
    expect(body.normalized_change.bindings[0].scopes[0]).toEqual({path_glob:"**",role:"source",service_id:"svc_web"});
  });
});

function json(value:any) {
  return new Response(JSON.stringify(value),{status:200,headers:{"content-type":"application/json"}});
}

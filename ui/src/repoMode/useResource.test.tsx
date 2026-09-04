import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useResource } from "./useResource";

const response = (value:unknown) => new Response(JSON.stringify(value), {headers:{"content-type":"application/json"}});
describe("route-owned resources", () => {
  afterEach(() => vi.unstubAllGlobals());
  it("does not show old resource data or accept a late aborted response", async () => {
    const resolvers: Array<(response:Response)=>void> = [];
    vi.stubGlobal("fetch",vi.fn(() => new Promise<Response>(resolve => resolvers.push(resolve))));
    const hook=renderHook(({path})=>useResource(path),{initialProps:{path:"/api/products/a"}});
    hook.rerender({path:"/api/products/b"});
    await act(async()=>resolvers[1](response({id:"b"})));
    await waitFor(()=>expect(hook.result.current.data).toEqual({id:"b"}));
    await act(async()=>resolvers[0](response({id:"a"})));
    expect(hook.result.current.data).toEqual({id:"b"});
    hook.rerender({path:"/api/products/c"});
    expect(hook.result.current.data).toBeNull();
  });
  it("retains only same-resource stale data with an error", async () => {
    vi.stubGlobal("fetch",vi.fn().mockResolvedValueOnce(response({id:"a"})).mockRejectedValueOnce(new Error("offline")));
    const hook=renderHook(()=>useResource("/api/products/a"));
    await waitFor(()=>expect(hook.result.current.status).toBe("ready"));
    await act(async()=>hook.result.current.refresh());
    expect(hook.result.current.data).toEqual({id:"a"});
    expect(hook.result.current.error).toBe("offline");
    expect(hook.result.current.updatedAt).toBeInstanceOf(Date);
  });
  it("disabled resources do not fetch or retain previous data", async()=>{
    const fetch=vi.fn().mockResolvedValue(response({id:"a"})); vi.stubGlobal("fetch",fetch);
    const hook=renderHook(({enabled})=>useResource("/api/a",{enabled}),{initialProps:{enabled:true}});
    await waitFor(()=>expect(hook.result.current.data).toEqual({id:"a"}));
    hook.rerender({enabled:false});
    expect(hook.result.current.data).toBeNull(); expect(fetch).toHaveBeenCalledTimes(1);
  });
});

import { useEffect, useMemo, useState } from "react";
import { GitBranch, Plus, Trash } from "@phosphor-icons/react";
import { sendJson } from "../api";
import { ResourceState, Status } from "../components";
import { useResource } from "../useResource";
import { repositoryLabel } from "../presentation";

type ServiceDraft = {id?:string;service_key:string;display_name:string;description:string;status:string};
type ScopeDraft = {path_glob:string;role:string;service_key:string};
type BindingDraft = {repository_id:string;status:string;scopes:ScopeDraft[]};

export function ProductTopologyEditor({ productId,operatorName,onApplied }: {productId:string;operatorName:string;onApplied:()=>void}) {
  const model = useResource<any>(`/api/products/${encodeURIComponent(productId)}/model`);
  const [services,setServices] = useState<ServiceDraft[]>([]);
  const [bindings,setBindings] = useState<BindingDraft[]>([]);
  const [reason,setReason] = useState("Review Finance Product Service and Repository scope topology");
  const [preview,setPreview] = useState<any>(null);
  const [busy,setBusy] = useState(false);
  const [error,setError] = useState("");
  useEffect(() => {
    if(!model.data || preview) return;
    const nextServices = (model.data.services || []).map((service:any) => ({id:service.id,service_key:service.service_key,display_name:service.display_name,description:service.description,status:service.status || "active"}));
    setServices(nextServices);
    setBindings((model.data.bindings || []).map((entry:any) => {
      const scopes = entry.typed_scopes?.length ? entry.typed_scopes : (entry.current_revision?.scopes || ["**"]).map((path_glob:string) => ({path_glob,role:"source",service_id:null}));
      return {repository_id:entry.repository_id,status:entry.status || "active",scopes:scopes.map((scope:any) => ({path_glob:scope.path_glob || scope,role:scope.role || "source",service_key:nextServices.find((service:ServiceDraft) => service.id === scope.service_id)?.service_key || ""}))};
    }));
  },[model.data,preview]);
  const repositories = useMemo(() => new Map((model.data?.repositories || []).map((repository:any) => [repository.id,repository])),[model.data]);
  const updateService = (index:number,field:keyof ServiceDraft,value:string) => { setPreview(null); setServices(items => items.map((item,position) => position === index ? {...item,[field]:value}:item)); };
  const updateBinding = (bindingIndex:number,scopeIndex:number,field:keyof ScopeDraft,value:string) => { setPreview(null); setBindings(items => items.map((binding,index) => index === bindingIndex ? {...binding,scopes:binding.scopes.map((scope,position) => position === scopeIndex ? {...scope,[field]:value}:scope)}:binding)); };
  const preflight = async () => {
    setBusy(true);setError("");
    try {
      const value = await sendJson(`/api/products/${encodeURIComponent(productId)}/model-changes/preflight`,"POST",{
        services:services.map(service => ({...service,id:service.id || undefined})),
        bindings:bindings.map(binding => ({...binding,scopes:binding.scopes.map(scope => ({path_glob:scope.path_glob,role:scope.role,service_key:scope.service_key || undefined}))})),
        actor:operatorName,reason,
      });
      setPreview(value);
    } catch(caught) { setError(caught instanceof Error ? caught.message : String(caught)); }
    finally { setBusy(false); }
  };
  const apply = async () => {
    if(!preview) return;
    setBusy(true);setError("");
    try {
      await sendJson(`/api/products/${encodeURIComponent(productId)}/model-changes`,"POST",{
        normalized_change:preview.normalized_change,state_hash:preview.state_hash,preflight_hash:preview.preflight_hash,actor:operatorName,reason,
      });
      setPreview(null);model.refresh();onApplied();
    } catch(caught) {
      const value = caught as Error & {status?:number};
      if(value.status === 409) { setPreview(null);model.refresh(); }
      setError(value.message);
    } finally { setBusy(false); }
  };
  return <ResourceState status={model.status} error={model.error}><section className="repo-panel repo-span-2"><header><div><span className="repo-eyebrow">pharness.dev/product-model/v1alpha2</span><h2>Product topology revision</h2></div><Status value={preview ? "review required" : "editable"}/></header><p>Services describe the Product. Typed repository-relative scopes describe where each Service is implemented or delivered without granting ownership of an entire shared Repository.</p><h3>Services</h3><div className="repo-topology-editor">{services.map((service,index) => <div className="repo-topology-service" key={service.id || index}><label>Key<input value={service.service_key} onChange={event => updateService(index,"service_key",event.target.value)}/></label><label>Name<input value={service.display_name} onChange={event => updateService(index,"display_name",event.target.value)}/></label><label>Description<input value={service.description} onChange={event => updateService(index,"description",event.target.value)}/></label><label>Status<select value={service.status} onChange={event => updateService(index,"status",event.target.value)}><option value="active">Active</option><option value="retired">Retired</option></select></label></div>)}<button type="button" onClick={() => {setPreview(null);setServices(items => [...items,{service_key:"",display_name:"",description:"",status:"active"}]);}}><Plus size={16}/>Add Service</button></div><h3>Repository scopes</h3><div className="repo-topology-editor">{bindings.map((binding,bindingIndex) => { const repository:any = repositories.get(binding.repository_id); return <article className="repo-topology-binding" key={binding.repository_id}><header><GitBranch size={18}/><div><strong>{repositoryLabel(repository,binding.repository_id)}</strong><small className="repo-mono">{repository?.registered_commit}</small></div></header>{binding.scopes.map((scope,scopeIndex) => <div className="repo-topology-scope" key={`${binding.repository_id}-${scopeIndex}`}><label>Repository-relative scope<input value={scope.path_glob} onChange={event => updateBinding(bindingIndex,scopeIndex,"path_glob",event.target.value)}/></label><label>Role<select value={scope.role} onChange={event => updateBinding(bindingIndex,scopeIndex,"role",event.target.value)}><option value="source">Source</option><option value="delivery">Delivery</option><option value="automation">Automation</option><option value="product_integration">Product integration</option><option value="documentation">Documentation</option></select></label><label>Service<select value={scope.service_key} onChange={event => updateBinding(bindingIndex,scopeIndex,"service_key",event.target.value)}><option value="">Product level</option>{services.filter(service => service.status === "active").map(service => <option value={service.service_key} key={service.id || service.service_key}>{service.display_name || service.service_key}</option>)}</select></label><button type="button" aria-label="Remove scope" disabled={binding.scopes.length === 1} onClick={() => {setPreview(null);setBindings(items => items.map((item,index) => index === bindingIndex ? {...item,scopes:item.scopes.filter((_,position) => position !== scopeIndex)}:item));}}><Trash size={16}/></button></div>)}<button type="button" onClick={() => {setPreview(null);setBindings(items => items.map((item,index) => index === bindingIndex ? {...item,scopes:[...item.scopes,{path_glob:"",role:"source",service_key:""}]}:item));}}><Plus size={16}/>Add scope</button></article>;})}</div><div className="repo-inline-form"><label className="repo-span-2">Operator reason<input value={reason} onChange={event => setReason(event.target.value)}/></label><button className="repo-primary" type="button" disabled={busy || !reason.trim()} onClick={preflight}>Preview immutable snapshot</button>{preview ? <button type="button" disabled={busy} onClick={apply}>Confirm and apply exact revision</button> : null}</div>{preview ? <div className="repo-preview"><strong>Resulting snapshot</strong><span className="repo-mono">{preview.resulting_snapshot_hash}</span><pre className="repo-code">{JSON.stringify(preview.resulting_snapshot,null,2)}</pre></div> : null}{error ? <div className="repo-error" role="alert">{error}</div> : null}</section></ResourceState>;
}

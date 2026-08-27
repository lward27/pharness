import { useEffect, useState } from "react";
import { ClockCounterClockwise, Cube, FileText, GitBranch, Plus, Stack, WarningCircle } from "@phosphor-icons/react";
import { sendJson } from "../api";
import { Empty, LinkButton, ResourceState, SectionHeader, Status } from "../components";
import { navigate } from "../routes";
import { useResource } from "../useResource";
import { FactGrid, formatMoment, humanize, RecordList } from "../presentation";
import { ProductTopologyEditor } from "./ProductTopologyEditor";

export function ProductsScreen({ operatorName }: { operatorName?: string }) {
  const resource = useResource<any>("/api/products");
  const overview = useResource<any>("/api/organization/overview");
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState("");
  const [form, setForm] = useState({ display_name: "", description: "", owner_principal: operatorName || "operator", reason: "Create Product for Repo Mode" });
  const submit = async (event: React.FormEvent) => {
    event.preventDefault(); setError("");
    try {
      const product = await sendJson("/api/products", "POST", { ...form, actor: operatorName || form.owner_principal });
      navigate(`products/${product.id}/work-items`);
    } catch (caught) { setError(caught instanceof Error ? caught.message : String(caught)); }
  };
  return <ResourceState status={resource.status} error={resource.error}>
    <SectionHeader eyebrow="Registry" title="Products" summary="The durable owner of Repository bindings, WorkItems, evidence, and connected delivery state." action={<button className="repo-primary" type="button" onClick={() => setCreating(value => !value)}><Plus size={17} />New Product</button>} />
    {creating ? <form className="repo-inline-form" onSubmit={submit}>
      <label>Name<input required value={form.display_name} onChange={event => setForm(value => ({ ...value, display_name: event.target.value }))} /></label>
      <label>Description<textarea required rows={2} value={form.description} onChange={event => setForm(value => ({ ...value, description: event.target.value }))} /></label>
      <label>Owner<input required value={form.owner_principal} onChange={event => setForm(value => ({ ...value, owner_principal: event.target.value }))} /></label>
      <label>Reason<input required value={form.reason} onChange={event => setForm(value => ({ ...value, reason: event.target.value }))} /></label>
      {error ? <div className="repo-error" role="alert">{error}</div> : null}<button className="repo-primary" type="submit">Create Product</button>
    </form> : null}
    <div className="repo-card-grid repo-three">
      {(resource.data?.products || []).map((product: any) => { const summary = overview.data?.product_summaries?.find((entry:any) => entry.id === product.id); return <article className="repo-resource-card" key={product.id}>
        <header><div className="repo-resource-icon"><Stack size={22} /></div>{summary?.actionable_waits ? <Status value="waiting" /> : null}</header>
        <h2>{product.display_name}</h2><p>{product.description}</p>
        <dl><div><dt>Owner</dt><dd>{product.owner_principal}</dd></div><div><dt>Repositories</dt><dd>{summary?.repository_count ?? "—"}</dd></div><div><dt>Current WorkItems</dt><dd>{summary?.current_work_items ?? "—"}</dd></div><div><dt>Actionable waits</dt><dd>{summary?.actionable_waits ?? "—"}</dd></div><div><dt>Evidence freshness</dt><dd>{summary?.evidence_freshness?.latest_work_item_update || "No WorkItem evidence"}</dd></div><div><dt>Capabilities</dt><dd>{summary?.capability_posture?.map((entry:any) => `${entry.capability}: ${entry.status}`).join(" · ") || "Unavailable"}</dd></div></dl>
        <LinkButton to={`products/${product.id}/work-items`}>Open Product</LinkButton>
      </article>; })}
    </div>
    {!resource.data?.products?.length ? <Empty title="No Products yet" message="Create the first Product, then register a Repository at an immutable revision." action={<button className="repo-primary" type="button" onClick={() => setCreating(true)}><Cube size={17} />Create Product</button>} /> : null}
  </ResourceState>;
}

const sections = ["work-items", "services-repositories", "agents", "releases", "evidence-audit", "history"];

export function ProductScreen({ productId, section, operatorName }: { productId: string; section: string; operatorName?: string }) {
  const resource = useResource<any>(`/api/products/${encodeURIComponent(productId)}/overview`, { pollMs: 15_000 });
  const editable = useResource<any>(`/api/products/${encodeURIComponent(productId)}`);
  const [editing,setEditing] = useState(false);
  const [editError,setEditError] = useState("");
  const [draft,setDraft] = useState({ display_name:"", description:"", owner_principal:"", actor:operatorName || "operator", reason:"Update Product registry metadata" });
  const data = resource.data;
  const product = data?.product;
  const readyRepositories = (data?.repositories || []).filter((repository:any) => repository.contract_readiness === "ready" && repository.coding_readiness === "ready");
  const firstBlockedRepository = (data?.repositories || []).find((repository:any) => repository.contract_readiness !== "ready" || repository.coding_readiness !== "ready");
  const workItemBlocker = !data?.repositories?.length
    ? "Register and onboard a Repository before creating work."
    : !readyRepositories.length
      ? `No Repository is coding-ready. ${firstBlockedRepository?.external_id || "The first Repository"} is ${humanize(firstBlockedRepository?.contract_readiness || "contract unavailable")} / ${humanize(firstBlockedRepository?.coding_readiness || "coding unavailable")}.`
      : "";
  useEffect(() => { if(editable.data && !editing) setDraft(current => ({...current,display_name:editable.data.display_name || "",description:editable.data.description || "",owner_principal:editable.data.owner_principal || ""})); },[editable.data,editing]);
  const save = async (event:React.FormEvent) => {
    event.preventDefault(); setEditError("");
    try {
      await sendJson(`/api/products/${encodeURIComponent(productId)}`,"PATCH",{...draft,state_hash:editable.data.state_hash});
      setEditing(false); editable.refresh(); resource.refresh();
    } catch(caught) {
      const value = caught as Error & { status?:number };
      if(value.status === 409) { setEditing(false); editable.refresh(); resource.refresh(); return; }
      setEditError(value.message); editable.refresh();
    }
  };
  return <ResourceState status={resource.status} error={resource.error}>
    <SectionHeader eyebrow="Product" title={product?.display_name || productId} summary={product?.description} action={<div className="repo-header-actions"><button type="button" onClick={() => setEditing(value => !value)}>Edit Product</button>{readyRepositories.length ? <LinkButton className="is-primary" to={`products/${productId}/work-items/new`}>New WorkItem</LinkButton> : <button type="button" disabled title={workItemBlocker}>New WorkItem</button>}</div>} />
    {workItemBlocker ? <div className="repo-corrective-path" role="status"><WarningCircle size={18} /><div><strong>WorkItem creation is unavailable</strong><span>{workItemBlocker}</span></div>{firstBlockedRepository ? <LinkButton to={`repositories/${firstBlockedRepository.id}/readiness`}>Resolve readiness</LinkButton> : <LinkButton to="repositories">Register Repository</LinkButton>}</div> : null}
    {editing ? <form className="repo-inline-form" onSubmit={save}><label>Name<input required value={draft.display_name} onChange={event => setDraft(value => ({...value,display_name:event.target.value}))} /></label><label>Description<textarea required rows={2} value={draft.description} onChange={event => setDraft(value => ({...value,description:event.target.value}))} /></label><label>Owner<input required value={draft.owner_principal} onChange={event => setDraft(value => ({...value,owner_principal:event.target.value}))} /></label><label>Operator<input required value={draft.actor} onChange={event => setDraft(value => ({...value,actor:event.target.value}))} /></label><label className="repo-span-2">Reason<input required value={draft.reason} onChange={event => setDraft(value => ({...value,reason:event.target.value}))} /></label>{editError ? <div className="repo-error" role="alert">{editError}</div> : null}<footer><button className="repo-primary" type="submit" disabled={!editable.data?.state_hash}>Save exact revision</button><button type="button" onClick={() => setEditing(false)}>Cancel</button></footer></form> : null}
    <nav className="repo-tabs" aria-label="Product sections">{sections.map(value => <button type="button" className={section === value ? "is-active" : ""} key={value} onClick={() => navigate(`products/${productId}/${value}`)}>{value.replaceAll("-", " ")}</button>)}</nav>
    {section === "work-items" ? <WorkItemRollup title="Current WorkItems" items={data?.current_work_items || []} /> : null}
    {section === "services-repositories" ? <ServicesAndRepositories data={data} productId={productId} operatorName={operatorName || "operator"} onApplied={() => resource.refresh()} /> : null}
    {section === "agents" ? <ResourceCollection title="Active AgentRuns" icon={<Stack size={19} />} items={(data?.active_agent_runs || []).map((item: any) => ({ id:item.id, title:item.profile_id || item.id, status:item.status, detail:item.work_item_id, href:`agents/runs/${item.id}` }))} /> : null}
    {section === "releases" ? <ProductReleases data={data?.connected_release_data} /> : null}
    {section === "evidence-audit" ? <ProductEvidenceAudit data={data} /> : null}
    {section === "history" ? <WorkItemRollup title="History" items={data?.historical_work_items || []} /> : null}
  </ResourceState>;
}

function ServicesAndRepositories({ data,productId,operatorName,onApplied }: { data:any;productId:string;operatorName:string;onApplied:()=>void }) {
  return <div className="repo-two-columns">
    <ResourceCollection title="Services" icon={<Cube size={19} />} items={(data?.services || []).map((item: any) => ({ id:item.id, title:item.display_name, status:item.status, detail:item.description }))} />
    <section className="repo-panel"><header><span><Stack size={19} /><h2>Repository bindings</h2></span><span className="repo-count">{data?.repositories?.length || 0}</span></header><div className="repo-list">{(data?.repositories || []).map((repository:any) => {
      const bindingSummary = (data?.repository_bindings || []).find((entry:any) => {
        const binding = entry.binding || entry;
        return binding.repository_id === repository.id || entry.repository?.id === repository.id;
      });
      const binding = bindingSummary?.binding || bindingSummary;
      const revision = bindingSummary?.current_revision || bindingSummary?.binding_revision;
      return <button className="repo-list-row" type="button" key={repository.id} onClick={() => navigate(`repositories/${repository.id}/overview`)}><GitBranch size={18} /><div><strong>{repository.external_id}</strong><span>Binding revision {revision?.revision ?? binding?.revision ?? "unavailable"} · {(revision?.service_ids || binding?.service_ids || []).length} Service mappings</span><small className="repo-mono">{repository.registered_commit}</small></div><Status value={repository.coding_readiness || "registered"} /></button>;
    })}{!data?.repositories?.length ? <p className="repo-muted">No durable Repository bindings.</p> : null}</div></section>
    <ProductTopologyEditor productId={productId} operatorName={operatorName} onApplied={onApplied}/>
  </div>;
}

function ProductReleases({ data }: { data:any }) {
  const releases = data?.releases || [];
  if (!releases.length) return <Empty title="No connected Releases" message="Repo Mode V1 is source-only. This Product has no connected Release data, and PHarness will not manufacture deployment health." />;
  return <div className="repo-card-grid repo-three">{releases.map((release:any) => <article className="repo-resource-card" key={release.id}><header><GitBranch size={20} /><Status value={release.status} /></header><h2>{release.title || release.id}</h2><p>{release.summary || "Connected durable Release"}</p><FactGrid facts={[{label:"WorkItem",value:release.work_item_id,mono:true},{label:"Source revision",value:release.source_revision || release.source_commit,mono:true},{label:"Updated",value:formatMoment(release.updated_at)}]} /></article>)}</div>;
}

function ProductEvidenceAudit({ data }: { data:any }) {
  const summary = data?.evidence_summary || {};
  return <div className="repo-two-columns">
    <section className="repo-panel"><header><div><span className="repo-eyebrow">Immutable validation</span><h2>Evidence coverage</h2></div><FileText size={20} /></header><FactGrid facts={[{label:"Validations",value:summary.validation_count ?? 0},{label:"WorkItems with evidence",value:`${summary.work_items_with_validations ?? 0}/${summary.work_item_denominator ?? data?.evidence_denominators?.work_items ?? 0}`},{label:"Latest validation",value:formatMoment(summary.latest_validated_at)}]} /><RecordList values={summary.validators || []} empty="No typed evidence validations have been sealed for this Product." /></section>
    <section className="repo-panel"><header><div><span className="repo-eyebrow">Operator trace</span><h2>Recent audit</h2></div><ClockCounterClockwise size={20} /></header><div className="repo-list">{(data?.audit_events || []).map((event:any) => <article className="repo-list-row" key={event.id}><div><strong>{humanize(event.action || event.kind)}</strong><span>{event.actor || "controller"} · {event.reason || event.summary || "Durable transition"}</span><small>{formatMoment(event.created_at)}</small></div><Status value={event.result || event.status || "recorded"} /></article>)}{!data?.audit_events?.length ? <p className="repo-muted">No Product-owned audit events are available.</p> : null}</div></section>
    <section className="repo-panel repo-span-2"><header><h2>Capability posture</h2></header><div className="repo-list repo-columns">{(data?.capability_posture || []).map((item: any) => <div className="repo-list-row" key={item.capability}><div><strong>{humanize(item.capability)}</strong><span>{item.summary}</span></div><Status value={item.status} /></div>)}</div></section>
  </div>;
}

function WorkItemRollup({ title, items }: { title: string; items: any[] }) {
  return <section className="repo-panel"><header><h2>{title}</h2><span className="repo-count">{items.length}</span></header><div className="repo-list">{items.map(item => <button className="repo-list-row" type="button" key={item.id} onClick={() => navigate(`work-items/${item.id}/overview`)}><div><strong>{item.title}</strong><span>{item.current_stage} · {item.status_reason || item.intent}</span></div><Status value={item.status} /></button>)}{!items.length ? <p className="repo-muted">No records in this lifecycle partition.</p> : null}</div></section>;
}

function ResourceCollection({ title, icon, items }: { title: string; icon: React.ReactNode; items: Array<{ id:string; title:string; status:string; detail?:string; href?:string }> }) {
  return <section className="repo-panel"><header><span>{icon}<h2>{title}</h2></span><span className="repo-count">{items.length}</span></header><div className="repo-list">{items.map(item => <button className="repo-list-row" type="button" key={item.id} disabled={!item.href} onClick={() => item.href && navigate(item.href)}><div><strong>{item.title}</strong><span className="repo-mono">{item.detail}</span></div><Status value={item.status} /></button>)}{!items.length ? <p className="repo-muted">No durable records.</p> : null}</div></section>;
}

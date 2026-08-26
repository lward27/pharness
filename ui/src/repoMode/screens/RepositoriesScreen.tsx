import { useMemo, useState } from "react";
import { CheckCircle, GitBranch, MagnifyingGlass, Plus, WarningCircle } from "@phosphor-icons/react";
import { query, sendJson } from "../api";
import { ActionDialog, Empty, LinkButton, ResourceState, SectionHeader, Status, type ServerAction } from "../components";
import { navigate } from "../routes";
import { useResource } from "../useResource";

export function RepositoriesScreen({ operatorName }: { operatorName?: string }) {
  const [search, setSearch] = useState("");
  const resource = useResource<any>(query("/api/repositories", { search }));
  const products = useResource<any>("/api/products");
  const [registering, setRegistering] = useState(false);
  return <ResourceState status={resource.status} error={resource.error}>
    <SectionHeader eyebrow="Source registry" title="Repositories" summary="Registration, contract readiness, coding readiness, and capability posture remain separate facts." action={<button className="repo-primary" type="button" onClick={() => setRegistering(value => !value)}><Plus size={17} />Register Repository</button>} />
    {registering ? <RepositoryRegistration products={products.data?.products || []} operatorName={operatorName} onCancel={() => setRegistering(false)} /> : null}
    <div className="repo-filterbar"><MagnifyingGlass size={18} /><input aria-label="Search Repositories" placeholder="Search registered repositories" value={search} onChange={event => setSearch(event.target.value)} /></div>
    <div className="repo-table" aria-label="Repositories">
      <div className="repo-table-head" aria-hidden="true"><span>Repository</span><span>Registration</span><span>Contract</span><span>Coding</span><span>Freshness</span></div>
      {(resource.data?.repositories || []).map((repository: any) => <button type="button" className="repo-table-row" key={repository.id} aria-label={`Open Repository ${repository.provider_repository_id}`} onClick={() => navigate(`repositories/${repository.id}/overview`)}>
        <span><strong>{repository.provider_repository_id}</strong><small>{repository.product_bindings?.map((binding: any) => binding.display_name).join(" · ") || "Unbound"}</small></span>
        <span><Status value="registered" /><small className="repo-mono">{repository.registered_commit?.slice(0, 12)}</small></span>
        <span><Status value={repository.contract_readiness} /></span>
        <span><Status value={repository.coding_readiness} /></span>
        <span><Status value={repository.freshness || "unavailable"} /><small>{repository.stale_reasons?.join(" · ")}</small></span>
      </button>)}
    </div>
    {!resource.data?.repositories?.length ? <Empty title="No Repositories registered" message="Select a Product, then register a GitHub HTTPS Repository at a full immutable commit SHA." /> : null}
  </ResourceState>;
}

function RepositoryRegistration({ products, operatorName, onCancel }: { products:any[]; operatorName?:string; onCancel:()=>void }) {
  const [form, setForm] = useState({ product_id:products[0]?.id || "", repository_url:"", source_commit:"", actor:operatorName || "operator", reason:"Register Repository for Repo Mode" });
  const [preflight, setPreflight] = useState<any>(null);
  const [error, setError] = useState("");
  const submit = async () => {
    setError("");
    try {
      if (!preflight) {
        setPreflight(await sendJson(`/api/products/${encodeURIComponent(form.product_id)}/repositories/preflight`, "POST", { repository_url:form.repository_url, source_commit:form.source_commit }));
      } else {
        const repository = await sendJson(`/api/products/${encodeURIComponent(form.product_id)}/repositories`, "POST", { repository_url:form.repository_url, source_commit:form.source_commit, preflight_hash:preflight.preflight_hash, actor:form.actor, reason:form.reason });
        navigate(`repository-onboardings/${repository.onboarding_id}`);
      }
    } catch (caught) { setError(caught instanceof Error ? caught.message : String(caught)); setPreflight(null); }
  };
  return <section className="repo-registration repo-panel"><header><div><span className="repo-eyebrow">Immutable registration</span><h2>{preflight ? "Review registration" : "Register Repository"}</h2></div></header>
    <div className="repo-form-grid"><label>Product<select value={form.product_id} disabled={Boolean(preflight)} onChange={event => setForm(value => ({...value,product_id:event.target.value}))}>{products.map(product => <option value={product.id} key={product.id}>{product.display_name}</option>)}</select></label><label>GitHub HTTPS URL<input value={form.repository_url} disabled={Boolean(preflight)} onChange={event => setForm(value => ({...value,repository_url:event.target.value}))} placeholder="https://github.com/owner/repository" /></label><label className="repo-span-2">Full commit SHA<input className="repo-mono" value={form.source_commit} disabled={Boolean(preflight)} onChange={event => setForm(value => ({...value,source_commit:event.target.value}))} /></label></div>
    {preflight ? <div className="repo-preflight"><h3>Server preflight</h3><dl><div><dt>Canonical Repository</dt><dd>{preflight.canonical_url}</dd></div><div><dt>Default branch</dt><dd>{preflight.default_branch}</dd></div><div><dt>Immutable revision</dt><dd className="repo-mono">{preflight.source_commit}</dd></div><div><dt>Provider commit</dt><dd><Status value={preflight.commit_verified ? "verified" : "blocked"} /></dd></div></dl>{preflight.predicted_mutations?.map((item:string) => <p key={item}><CheckCircle size={16} />{item.replaceAll("_"," ")}</p>)}{preflight.blockers?.map((item:string) => <p className="repo-error" key={item}>{item}</p>)}</div> : null}
    {preflight ? <div className="repo-form-grid"><label>Operator<input value={form.actor} onChange={event => setForm(value => ({...value,actor:event.target.value}))} /></label><label>Reason<input value={form.reason} onChange={event => setForm(value => ({...value,reason:event.target.value}))} /></label></div> : null}
    {error ? <div className="repo-error" role="alert">{error}</div> : null}<footer><button className="repo-primary" type="button" disabled={!form.product_id || !form.repository_url || form.source_commit.length !== 40 || Boolean(preflight?.blockers?.length)} onClick={submit}>{preflight ? "Confirm registration" : "Run preflight"}</button><button type="button" onClick={preflight ? () => setPreflight(null) : onCancel}>{preflight ? "Edit" : "Cancel"}</button></footer>
  </section>;
}

const repositorySections = ["overview", "readiness", "work-items", "history"];

export function RepositoryScreen({ repositoryId, section, operatorName }: { repositoryId:string; section:string; operatorName?:string }) {
  const resource = useResource<any>(`/api/repositories/${encodeURIComponent(repositoryId)}/overview`, { pollMs:15_000 });
  const data = resource.data;
  const repository = data?.repository;
  const onboarding = data?.latest_onboarding;
  return <ResourceState status={resource.status} error={resource.error}>
    <SectionHeader eyebrow="Repository" title={repository?.external_id || repositoryId} summary={repository?.canonical_url} action={onboarding ? <LinkButton to={`repository-onboardings/${onboarding.id}`}>Open onboarding</LinkButton> : undefined} />
    <nav className="repo-tabs" aria-label="Repository sections">{repositorySections.map(value => <button type="button" className={section === value ? "is-active" : ""} key={value} onClick={() => navigate(`repositories/${repositoryId}/${value}`)}>{value.replaceAll("-"," ")}</button>)}</nav>
    {section === "overview" ? <div className="repo-two-columns"><section className="repo-panel"><h2>Immutable registration</h2><dl className="repo-bindings"><div><dt>Provider</dt><dd>{repository?.provider}</dd></div><div><dt>Default branch</dt><dd>{repository?.default_branch}</dd></div><div><dt>Registered revision</dt><dd className="repo-mono">{repository?.registered_commit}</dd></div><div><dt>Products</dt><dd>{data?.product_bindings?.map((entry:any) => entry.product.display_name).join(" · ") || "None"}</dd></div></dl></section><CapabilityAxes capabilities={data?.capabilities || []} trust={data?.trust_policy || {}} authorization={data?.authorization || {}} onVerified={resource.refresh} /></div> : null}
    {section === "readiness" ? <Readiness data={data} repositoryId={repositoryId} operatorName={operatorName} onRefresh={resource.refresh} /> : null}
    {section === "work-items" ? <WorkItems items={data?.current_work_items || []} /> : null}
    {section === "history" ? <WorkItems items={data?.historical_work_items || []} /> : null}
  </ResourceState>;
}

function CapabilityAxes({ capabilities, trust, authorization, onVerified }: any) {
  const [pending,setPending] = useState("");
  const [error,setError] = useState("");
  const verify = async (capability:string) => {
    setPending(capability); setError("");
    try {
      await sendJson(`/api/system/capabilities/${encodeURIComponent(capability)}/preflight`, "POST");
      onVerified();
    } catch(caught) { setError(caught instanceof Error ? caught.message : String(caught)); }
    finally { setPending(""); }
  };
  return <section className="repo-panel"><h2>Capability, trust, authorization</h2><div className="repo-axis-grid"><div><h3>Availability</h3>{capabilities.map((item:any) => <div className="repo-capability-row" key={item.capability}><span>{item.capability}</span><Status value={item.status} />{item.status !== "available" ? <button type="button" disabled={Boolean(pending)} onClick={() => verify(item.capability)}>{pending === item.capability ? "Verifying…" : `Verify ${item.capability.replaceAll("_"," ")}`}</button> : null}</div>)}</div><div><h3>Trust policy</h3>{Object.entries(trust).map(([key,value]) => <p key={key}><span>{key}</span><Status value={String(value)} /></p>)}</div><div><h3>Authorization</h3>{Object.entries(authorization).map(([key,value]) => <p key={key}><span>{key}</span><small>{String(value).replaceAll("_"," ")}</small></p>)}</div></div>{error ? <div className="repo-error" role="alert">{error}</div> : null}</section>;
}

function Readiness({ data, repositoryId, operatorName, onRefresh }: any) {
  const readiness = data?.readiness;
  const [preparing,setPreparing] = useState(false);
  const [error,setError] = useState("");
  const runReadiness = async () => {
    setPreparing(true); setError("");
    try {
      await sendJson(`/api/repositories/${encodeURIComponent(repositoryId)}/readiness-assessments`, "POST", {source_commit:data.repository.registered_commit,actor:operatorName || "operator",reason:"Refresh exact Repository coding readiness"});
      onRefresh();
    } catch(caught) { setError(caught instanceof Error ? caught.message : String(caught)); }
    finally { setPreparing(false); }
  };
  const preparation = data?.readiness_preparation;
  const assessmentRunning = ["queued", "running"].includes(preparation?.status) || readiness?.status === "assessment_running";
  const contractStatus = readiness?.contract_status || (data?.canonical_contract ? "ready" : "unavailable");
  return <div className="repo-two-columns"><section className="repo-panel"><header><h2>Canonical contract</h2><Status value={contractStatus} /></header>{data?.canonical_contract ? <><dl className="repo-bindings"><div><dt>Contract version</dt><dd>{data.canonical_contract.api_version}</dd></div><div><dt>Source commit</dt><dd className="repo-mono">{data.canonical_contract.source_commit}</dd></div><div><dt>Content hash</dt><dd className="repo-mono">{data.canonical_contract.content_hash}</dd></div></dl><pre className="repo-code">{JSON.stringify(data.canonical_contract.contract, null, 2)}</pre></> : <Empty title="Canonical contract unavailable" message="Complete onboarding and validate the merged .pharness/repository.yaml at the exact revision." />}</section><section className="repo-panel"><header><h2>Coding readiness</h2><Status value={assessmentRunning ? "waiting" : readiness?.coding_status || "unavailable"} /></header>{assessmentRunning ? <p className="repo-muted">Preparation {preparation.id} is {preparation.status} for this exact Repository revision.</p> : null}{(data?.readiness_stale_reasons || []).map((reason:string) => <p className="repo-warning" key={reason}><WarningCircle size={17} />{reason.replaceAll("_"," ")}</p>)}<dl className="repo-bindings"><div><dt>Environment profile</dt><dd>{readiness?.environment_profile_id || data?.canonical_contract?.contract?.environment_profile || "Unavailable"}</dd></div><div><dt>Runner digest</dt><dd className="repo-mono">{readiness?.runner_image_digest || "Unavailable"}</dd></div><div><dt>Lock hash</dt><dd className="repo-mono">{readiness?.dependency_lock_hash || data?.canonical_contract?.contract?.dependency_lock?.sha256 || "Unavailable"}</dd></div></dl>{readiness?.blockers ? <pre className="repo-code">{JSON.stringify(readiness.blockers,null,2)}</pre> : null}{readiness?.coding_status === "ready" ? <LinkButton className="is-primary" to={`products/${data.product_bindings?.[0]?.product?.id}/work-items/new`}>Create WorkItem</LinkButton> : data?.canonical_contract ? <button className="repo-primary" type="button" disabled={preparing || assessmentRunning} onClick={runReadiness}>{preparing || assessmentRunning ? "Assessing readiness…" : "Run coding readiness"}</button> : <button type="button" disabled title={`Complete onboarding for Repository ${repositoryId}`}>Create WorkItem · complete onboarding</button>}{error ? <div className="repo-error" role="alert">{error}</div> : null}</section></div>;
}

function WorkItems({ items }: { items:any[] }) { return <section className="repo-panel"><header><h2>WorkItems</h2><span className="repo-count">{items.length}</span></header><div className="repo-list">{items.map(item => <button type="button" className="repo-list-row" key={item.id} onClick={() => navigate(`work-items/${item.id}/overview`)}><div><strong>{item.title}</strong><span>{item.current_stage} · {item.status_reason || item.intent}</span></div><Status value={item.status} /></button>)}{!items.length ? <p className="repo-muted">No WorkItems in this lifecycle partition.</p> : null}</div></section>; }

const onboardingSteps = ["Registration", "Discovery", "Proposal", "Review", "Patch", "Source PR", "Merge", "Contract validation", "Readiness"];

export function OnboardingScreen({ onboardingId, operatorName }: { onboardingId:string; operatorName?:string }) {
  const resource = useResource<any>(`/api/repository-onboardings/${encodeURIComponent(onboardingId)}/flow`, { pollMs:10_000 });
  const [selectedAction, setSelectedAction] = useState<ServerAction | null>(null);
  const flow = resource.data;
  const onboarding = flow?.onboarding;
  const patch = useResource<any>(onboarding?.patch_artifact_id ? `/api/artifacts/${encodeURIComponent(onboarding.patch_artifact_id)}` : null);
  const action = onboarding?.actions?.[0];
  const activeStep = useMemo(() => {
    const status = onboarding?.status || "registered";
    if (status.includes("discover")) return 1;
    if (status.includes("proposal")) return status === "proposal_ready" ? 3 : 2;
    if (status.includes("patch") || status === "delivery_ready") return 4;
    if (status.includes("waiting") || status.includes("delivery")) return 5;
    if (status === "merge_observed") return 6;
    if (status.includes("validation") || status === "contract_ready") return 7;
    if (status === "ready") return 8;
    return 0;
  }, [onboarding?.status]);
  return <ResourceState status={resource.status} error={resource.error}>
    <SectionHeader eyebrow="Repository onboarding" title={onboarding?.repository_id || onboardingId} summary={`Pinned revision ${onboarding?.registered_commit || "unavailable"}`} action={action ? <button className="repo-primary" type="button" disabled={action.status !== "available"} onClick={() => setSelectedAction(action)}>{action.id.replaceAll("_"," ")}</button> : undefined} />
    <ol className="repo-stepper">{onboardingSteps.map((step,index) => <li className={index < activeStep ? "is-complete" : index === activeStep ? "is-current" : ""} key={step}><span>{index < activeStep ? <CheckCircle size={16} weight="fill" /> : index + 1}</span><small>{step}</small></li>)}</ol>
    <div className="repo-two-columns repo-onboarding-grid"><section className="repo-panel"><header><h2>Deterministic discovery</h2><Status value={flow?.discovery?.status || "waiting"} /></header>{flow?.discovery?.inventory_json ? <pre className="repo-code">{JSON.stringify(flow.discovery.inventory_json,null,2)}</pre> : <p className="repo-muted">Discovery evidence has not been sealed.</p>}</section><section className="repo-panel"><header><h2>Agent proposal</h2><Status value={flow?.proposal?.status || "waiting"} /></header>{flow?.proposal?.proposal ? <><Proposal proposal={flow.proposal.proposal} /><OnboardingProposalEditor proposal={flow.proposal.proposal} onboarding={onboarding} operatorName={operatorName} onSaved={resource.refresh} /></> : <p className="repo-muted">No proposal revision is available.</p>}</section><section className="repo-panel repo-span-2"><header><h2>Exact onboarding diff</h2><Status value={patch.data ? "available" : onboarding?.patch_execution_id ? "waiting" : "inapplicable"} /></header>{patch.data?.content_text ? <pre className="repo-code repo-diff">{patch.data.content_text}</pre> : <p className="repo-muted">The reviewed contract diff appears here after bounded source materialization.</p>}</section><section className="repo-panel"><header><h2>Source delivery</h2><Status value={flow?.source_delivery_intent?.status || "inapplicable"} /></header>{flow?.source_delivery_intent ? <pre className="repo-code">{JSON.stringify({ pull_request:flow.source_delivery_intent.pull_request, provider_checks:flow.source_delivery_intent.provider_checks, merge_provenance:flow.source_delivery_intent.merge_provenance },null,2)}</pre> : <p className="repo-muted">No external source effect has been authorized.</p>}</section><section className="repo-panel"><header><h2>Readiness forecast and result</h2><Status value={flow?.readiness?.coding_status || "unavailable"} /></header><pre className="repo-code">{JSON.stringify(flow?.readiness || flow?.proposal?.proposal?.readiness_forecast || {},null,2)}</pre></section></div>
    {selectedAction ? <ActionDialog action={selectedAction} owner={{kind:"Repository onboarding",id:onboardingId,product:onboarding?.product_id,repository:onboarding?.repository_id,revision:onboarding?.registered_commit}} endpoint={`/api/repository-onboardings/${encodeURIComponent(onboardingId)}/actions/${encodeURIComponent(selectedAction.id)}/execute`} operatorName={operatorName} onClose={() => setSelectedAction(null)} onApplied={resource.refresh} /> : null}
  </ResourceState>;
}

function Proposal({ proposal }: { proposal:any }) { return <div className="repo-proposal"><div className="repo-proposal-facts"><h3>Controller-bound facts</h3><pre>{JSON.stringify({ discovery_id:proposal.discovery_id, discovery_hash:proposal.discovery_hash },null,2)}</pre></div><div><h3>Executable contract changes</h3><pre>{JSON.stringify(proposal.candidate_contract,null,2)}</pre></div><div><h3>Product and Service mappings</h3><pre>{JSON.stringify({ service_proposals:proposal.service_proposals || [], binding_proposals:proposal.binding_proposals || [] },null,2)}</pre></div><div><h3>Model guidance and forecast</h3><pre>{JSON.stringify({ instructions:proposal.instructions, readiness_forecast:proposal.readiness_forecast },null,2)}</pre></div>{proposal.assumptions?.length ? <div className="repo-warning"><strong>Assumptions</strong>{proposal.assumptions.map((value:string) => <p key={value}>{value}</p>)}</div> : null}{proposal.blockers?.length ? <div className="repo-error"><strong>Blockers</strong>{proposal.blockers.map((value:any,index:number) => <p key={index}>{typeof value === "string" ? value : JSON.stringify(value)}</p>)}</div> : null}</div>; }

function OnboardingProposalEditor({ proposal,onboarding,operatorName,onSaved }:any) {
  const [editing,setEditing] = useState(false);
  const [actor,setActor] = useState(operatorName || "operator");
  const [reason,setReason] = useState("Review and revise onboarding proposal");
  const [instructions,setInstructions] = useState(proposal.instructions || "");
  const [contract,setContract] = useState(JSON.stringify(proposal.candidate_contract || {},null,2));
  const [mappings,setMappings] = useState(JSON.stringify({service_proposals:proposal.service_proposals || [],binding_proposals:proposal.binding_proposals || []},null,2));
  const [review,setReview] = useState(JSON.stringify({assumptions:proposal.assumptions || [],conflicts:proposal.conflicts || [],blockers:proposal.blockers || [],readiness_forecast:proposal.readiness_forecast || {}},null,2));
  const [error,setError] = useState("");
  if(onboarding?.status !== "proposal_ready") return null;
  const save = async () => {
    setError("");
    try {
      const parsedMappings = JSON.parse(mappings);
      const parsedReview = JSON.parse(review);
      await sendJson(`/api/repository-onboardings/${encodeURIComponent(onboarding.id)}/proposal`,"PUT",{proposal:{...proposal,instructions,candidate_contract:JSON.parse(contract),service_proposals:parsedMappings.service_proposals || [],binding_proposals:parsedMappings.binding_proposals || [],assumptions:parsedReview.assumptions || [],conflicts:parsedReview.conflicts || [],blockers:parsedReview.blockers || [],readiness_forecast:parsedReview.readiness_forecast || {}},actor,reason,state_hash:onboarding.state_hash});
      setEditing(false); onSaved();
    } catch(caught) {
      const value = caught as Error & {status?:number};
      if(value.status === 409) { setEditing(false); onSaved(); return; }
      setError(value.message);
    }
  };
  return <div className="repo-proposal-editor"><button type="button" onClick={() => setEditing(value => !value)}>{editing ? "Close proposal editor" : "Edit proposal revision"}</button>{editing ? <div className="repo-inline-form"><label className="repo-span-2">Repository instructions<textarea rows={6} value={instructions} onChange={event => setInstructions(event.target.value)} /></label><label>Executable contract JSON<textarea rows={12} className="repo-mono" value={contract} onChange={event => setContract(event.target.value)} /></label><label>Product and Service mappings JSON<textarea rows={12} className="repo-mono" value={mappings} onChange={event => setMappings(event.target.value)} /></label><label className="repo-span-2">Assumptions, conflicts, blockers, and forecast JSON<textarea rows={10} className="repo-mono" value={review} onChange={event => setReview(event.target.value)} /></label><label>Operator<input value={actor} onChange={event => setActor(event.target.value)} /></label><label>Reason<input value={reason} onChange={event => setReason(event.target.value)} /></label>{error ? <div className="repo-error" role="alert">{error}</div> : null}<button className="repo-primary" type="button" disabled={!actor.trim() || !reason.trim()} onClick={save}>Save new proposal revision</button></div> : null}</div>;
}

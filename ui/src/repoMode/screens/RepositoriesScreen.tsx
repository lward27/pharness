import { useMemo, useState } from "react";
import { CheckCircle, FileText, MagnifyingGlass, Plus, WarningCircle } from "@phosphor-icons/react";
import { query, sendJson } from "../api";
import { ActionDialog, Empty, LinkButton, ResourceState, SectionHeader, Status, type ServerAction } from "../components";
import { navigate } from "../routes";
import { useResource } from "../useResource";
import { EvidenceReferences, FactGrid, formatMoment, Freshness, humanize, RawRecord, RecordList } from "../presentation";

export function RepositoriesScreen({ operatorName }: { operatorName?: string }) {
  const [search, setSearch] = useState("");
  const [offset, setOffset] = useState(0);
  const limit = 25;
  const resource = useResource<any>(query("/api/repositories", { search, limit, offset }));
  const products = useResource<any>("/api/products");
  const [registering, setRegistering] = useState(false);
  return <ResourceState status={resource.status} error={resource.error}>
    <SectionHeader eyebrow="Source registry" title="Repositories" summary="Registration, contract readiness, coding readiness, and capability posture remain separate facts." action={<button className="repo-primary" type="button" onClick={() => setRegistering(value => !value)}><Plus size={17} />Register Repository</button>} />
    {registering ? <RepositoryRegistration products={products.data?.products || []} operatorName={operatorName} onCancel={() => setRegistering(false)} /> : null}
    <div className="repo-filterbar"><MagnifyingGlass size={18} /><input aria-label="Search Repositories" placeholder="Search registered repositories" value={search} onChange={event => { setSearch(event.target.value); setOffset(0); }} /></div>
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
    {resource.data?.count > limit ? <nav className="repo-pagination" aria-label="Repository pages"><button type="button" disabled={offset === 0} onClick={() => setOffset(value => Math.max(0,value-limit))}>Previous</button><span>{offset + 1}–{Math.min(offset + limit,resource.data.count)} of {resource.data.count}</span><button type="button" disabled={offset + limit >= resource.data.count} onClick={() => setOffset(value => value+limit)}>Next</button></nav> : null}
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
    {section === "overview" ? <div className="repo-two-columns"><section className="repo-panel"><h2>Immutable registration</h2><FactGrid facts={[{label:"Provider",value:repository?.provider},{label:"Default branch",value:repository?.default_branch},{label:"Registered revision",value:repository?.registered_commit,mono:true},{label:"Repository state version",value:repository?.state_version}]} /><h3>Reviewed Product bindings</h3><RecordList values={(data?.product_bindings || []).map((entry:any) => ({name:entry.product?.display_name,product_id:entry.product?.id,binding:entry.binding?.id,revision:entry.current_revision?.revision,services:entry.current_revision?.service_ids?.length || 0,status:entry.binding?.status}))} empty="This Repository is not bound to a Product." /></section><CapabilityAxes repositoryId={repositoryId} capabilities={data?.capabilities || []} trust={data?.trust_policy || {}} authorization={data?.authorization || {}} onVerified={resource.refresh} /></div> : null}
    {section === "readiness" ? <Readiness data={data} repositoryId={repositoryId} operatorName={operatorName} onRefresh={resource.refresh} /> : null}
    {section === "work-items" ? <WorkItems items={data?.current_work_items || []} /> : null}
    {section === "history" ? <WorkItems items={data?.historical_work_items || []} /> : null}
  </ResourceState>;
}

function CapabilityAxes({ repositoryId, capabilities, trust, authorization, onVerified }: any) {
  const [pending,setPending] = useState("");
  const [error,setError] = useState("");
  const verify = async (capability:string) => {
    setPending(capability); setError("");
    try {
      const repositoryQuery = capability.startsWith("environment_profile:") ? "" : `?repository_id=${encodeURIComponent(repositoryId)}`;
      await sendJson(`/api/system/capabilities/${encodeURIComponent(capability)}/preflight${repositoryQuery}`, "POST", {});
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
  const blocker = (data?.readiness_stale_reasons || [])[0] || readiness?.blockers?.[0]?.code || readiness?.blockers?.[0] || (!data?.canonical_contract ? "canonical_contract_missing" : "coding_readiness_required");
  return <div className="repo-two-columns"><section className="repo-panel"><header><h2>Canonical contract</h2><Status value={contractStatus} /></header>{data?.canonical_contract ? <ContractSummary version={data.canonical_contract} /> : <Empty title="Canonical contract unavailable" message="Complete onboarding and validate the merged .pharness/repository.yaml at the exact revision." />}</section><section className="repo-panel"><header><h2>Coding readiness</h2><Status value={assessmentRunning ? "waiting" : readiness?.coding_status || "unavailable"} /></header>{assessmentRunning ? <p className="repo-muted">Preparation {preparation.id} is {preparation.status} for this exact Repository revision.</p> : null}<Freshness status={(data?.readiness_stale_reasons || []).length ? "stale" : readiness ? "current" : "unavailable"} observedAt={readiness?.assessed_at} expiresAt={readiness?.expires_at} reasons={data?.readiness_stale_reasons || []} /><FactGrid facts={[{label:"Environment profile",value:readiness?.environment_profile_id || data?.canonical_contract?.contract?.environment_profile},{label:"Runner digest",value:readiness?.runner_image_digest,mono:true},{label:"Lock hash",value:readiness?.dependency_lock_hash || data?.canonical_contract?.contract?.dependency_lock?.sha256,mono:true}]} /><h3>Controller checks</h3><RecordList values={readiness?.checks} empty="No readiness checks have been sealed." /><RecordList values={readiness?.blockers} empty="No readiness blockers." tone="error" /><EvidenceReferences values={readiness?.evidence_refs} />{readiness?.coding_status === "ready" ? <LinkButton className="is-primary" to={`products/${data.product_bindings?.[0]?.product?.id}/work-items/new`}>Create WorkItem</LinkButton> : <div className="repo-corrective-path"><WarningCircle size={18} /><div><strong>WorkItem creation is unavailable</strong><span>{humanize(typeof blocker === "string" ? blocker : blocker?.summary)}</span></div>{data?.canonical_contract ? <button className="repo-primary" type="button" disabled={preparing || assessmentRunning} onClick={runReadiness}>{preparing || assessmentRunning ? "Assessing readiness…" : "Run exact readiness assessment"}</button> : data?.latest_onboarding ? <LinkButton to={`repository-onboardings/${data.latest_onboarding.id}`}>Complete onboarding</LinkButton> : null}</div>}{error ? <div className="repo-error" role="alert">{error}</div> : null}<RawRecord label="Raw readiness assessment" value={readiness} /></section></div>;
}

function ContractSummary({ version }: { version:any }) {
  const contract = version.contract || {};
  return <><FactGrid facts={[{label:"Schema",value:contract.api_version || version.api_version},{label:"Source commit",value:version.source_commit,mono:true},{label:"Content hash",value:version.content_hash,mono:true},{label:"Environment profile",value:contract.environment_profile},{label:"Network",value:humanize(contract.agent_network)},{label:"Package installation",value:humanize(contract.package_installation)}]} /><h3>Declared acceptance</h3><RecordList values={contract.acceptance_commands} /><h3>Writable boundary</h3><RecordList values={(contract.writable_paths || []).map((path:string) => ({path}))} /><h3>Immutable dependency</h3><RecordList values={contract.dependency_lock ? [contract.dependency_lock] : []} /><RawRecord label="Raw canonical contract" value={version} /></>;
}

function WorkItems({ items }: { items:any[] }) { return <section className="repo-panel"><header><h2>WorkItems</h2><span className="repo-count">{items.length}</span></header><div className="repo-list">{items.map(item => <button type="button" className="repo-list-row" key={item.id} onClick={() => navigate(`work-items/${item.id}/overview`)}><div><strong>{item.title}</strong><span>{item.current_stage} · {item.status_reason || item.intent}</span></div><Status value={item.status} /></button>)}{!items.length ? <p className="repo-muted">No WorkItems in this lifecycle partition.</p> : null}</div></section>; }

const onboardingSteps = ["Registration", "Discovery", "Proposal", "Review", "Patch", "Source PR", "Merge", "Contract validation", "Readiness"];

export function OnboardingScreen({ onboardingId, operatorName }: { onboardingId:string; operatorName?:string }) {
  const resource = useResource<any>(`/api/repository-onboardings/${encodeURIComponent(onboardingId)}/flow`, { pollMs:10_000 });
  const [selectedAction, setSelectedAction] = useState<ServerAction | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const flow = resource.data;
  const onboarding = flow?.onboarding;
  const repositoryResource = useResource<any>(onboarding?.repository_id ? `/api/repositories/${encodeURIComponent(onboarding.repository_id)}/overview` : null);
  const repositoryIdentity = repositoryResource.data?.repository;
  const productIdentity = repositoryResource.data?.product_bindings?.find((entry:any) => entry.product?.id === onboarding?.product_id)?.product;
  const patch = useResource<any>(onboarding?.patch_artifact_id ? `/api/artifacts/${encodeURIComponent(onboarding.patch_artifact_id)}` : null);
  const action = onboarding?.actions?.[0];
  const refreshOnboarding = action?.id === "refresh_onboarding";
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
    <SectionHeader eyebrow="Repository onboarding" title={repositoryIdentity?.external_id || onboarding?.repository_id || onboardingId} summary={`${productIdentity?.display_name ? `${productIdentity.display_name} · ` : ""}Pinned revision ${onboarding?.registered_commit || "unavailable"}`} action={action ? refreshOnboarding ? <button className="repo-primary" type="button" onClick={() => setRefreshing(value => !value)}>{refreshing ? "Close refresh" : "Start fresh onboarding"}</button> : <button className="repo-primary" type="button" disabled={action.status !== "available"} onClick={() => setSelectedAction(action)}>{action.id.replaceAll("_"," ")}</button> : undefined} />
    {action?.status === "blocked" ? <div className="repo-corrective-path" role="status"><WarningCircle size={18}/><div><strong>{action.id.replaceAll("_", " ")}</strong><span>{(action.blockers || []).map((blocker:any) => typeof blocker === "string" ? blocker : blocker.summary || blocker.code).join(" · ")}</span></div></div> : null}
    {refreshing && refreshOnboarding ? <FreshOnboardingForm onboarding={onboarding} operatorName={operatorName} onCancel={() => setRefreshing(false)} /> : null}
    <ol className="repo-stepper">{onboardingSteps.map((step,index) => <li className={index < activeStep ? "is-complete" : index === activeStep ? "is-current" : ""} key={step}><span>{index < activeStep ? <CheckCircle size={16} weight="fill" /> : index + 1}</span><small>{step}</small></li>)}</ol>
    <div className="repo-two-columns repo-onboarding-grid"><section className="repo-panel"><header><h2>Deterministic discovery</h2><Status value={flow?.discovery?.status || "waiting"} /></header>{flow?.discovery?.inventory_json ? <DiscoverySummary discovery={flow.discovery.inventory_json} /> : <p className="repo-muted">Discovery evidence has not been sealed.</p>}</section><section className="repo-panel"><header><h2>Agent proposal</h2><Status value={flow?.proposal?.status || "waiting"} /></header>{flow?.proposal?.proposal ? <><Proposal proposal={flow.proposal.proposal} /><OnboardingProposalEditor proposal={flow.proposal.proposal} onboarding={onboarding} operatorName={operatorName} onSaved={resource.refresh} /></> : <p className="repo-muted">No proposal revision is available.</p>}</section><section className="repo-panel repo-span-2"><header><h2>Exact onboarding diff</h2><Status value={patch.data ? "available" : onboarding?.patch_execution_id ? "waiting" : "inapplicable"} /></header>{patch.data?.content_text ? <details className="repo-diff-disclosure"><summary><FileText size={17} />Review the exact patch authorized for one source PR</summary><pre className="repo-code repo-diff">{patch.data.content_text}</pre></details> : <p className="repo-muted">The reviewed contract diff appears here after bounded source materialization.</p>}</section><section className="repo-panel"><header><h2>Source delivery</h2><Status value={flow?.source_delivery_intent?.status || "inapplicable"} /></header>{flow?.source_delivery_intent ? <SourceDeliverySummary intent={flow.source_delivery_intent} /> : <p className="repo-muted">No external source effect has been authorized.</p>}</section><section className="repo-panel"><header><h2>Readiness forecast and result</h2><Status value={flow?.readiness?.coding_status || "unavailable"} /></header><ReadinessSummary readiness={flow?.readiness} forecast={flow?.proposal?.proposal?.readiness_forecast} /></section></div>
    {selectedAction ? <ActionDialog action={selectedAction} owner={{kind:"Repository onboarding",id:onboardingId,product:onboarding?.product_id,repository:onboarding?.repository_id,revision:onboarding?.registered_commit}} endpoint={`/api/repository-onboardings/${encodeURIComponent(onboardingId)}/actions/${encodeURIComponent(selectedAction.id)}/execute`} operatorName={operatorName} onClose={() => setSelectedAction(null)} onApplied={resource.refresh} /> : null}
  </ResourceState>;
}

function FreshOnboardingForm({ onboarding, operatorName, onCancel }: { onboarding:any; operatorName?:string; onCancel:()=>void }) {
  const [sourceCommit,setSourceCommit] = useState("");
  const [actor,setActor] = useState(operatorName || "operator");
  const [reason,setReason] = useState("Refresh onboarding at reviewed prerequisite merge");
  const [pending,setPending] = useState(false);
  const [error,setError] = useState("");
  const validCommit = /^[0-9a-f]{40}$/.test(sourceCommit);
  const submit = async (event:React.FormEvent) => {
    event.preventDefault();
    setPending(true); setError("");
    try {
      const fresh = await sendJson(`/api/repositories/${encodeURIComponent(onboarding.repository_id)}/onboardings`, "POST", {product_id:onboarding.product_id,source_commit:sourceCommit,actor,reason});
      navigate(`repository-onboardings/${fresh.id}`);
    } catch(caught) { setError(caught instanceof Error ? caught.message : String(caught)); }
    finally { setPending(false); }
  };
  return <form className="repo-inline-form" onSubmit={submit}>
    <label>New full commit SHA<input className="repo-mono" value={sourceCommit} onChange={event => setSourceCommit(event.target.value.trim().toLowerCase())} placeholder="40-character prerequisite merge SHA" /></label>
    <label>Operator<input value={actor} onChange={event => setActor(event.target.value)} /></label>
    <label>Reason<input value={reason} onChange={event => setReason(event.target.value)} /></label>
    {error ? <div className="repo-error" role="alert">{error}</div> : null}
    <button className="repo-primary" type="submit" disabled={pending || !validCommit || !actor.trim() || !reason.trim()}>{pending ? "Creating…" : "Create fresh onboarding"}</button>
    <button type="button" onClick={onCancel}>Cancel</button>
  </form>;
}

function Proposal({ proposal }: { proposal:any }) { return <div className="repo-proposal"><div className="repo-proposal-facts"><h3>Controller-bound facts</h3><FactGrid facts={[{label:"Discovery record",value:proposal.discovery_id,mono:true},{label:"Discovery hash",value:proposal.discovery_hash,mono:true}]} /></div><div><h3>Executable contract changes</h3><ContractSummary version={{contract:proposal.candidate_contract,content_hash:"Proposal · not active until merged"}} /></div><div><h3>Product topology suggestions</h3><p className="repo-muted">These suggestions never change Services or Repository bindings when the executable contract is approved. Review them separately in the owning Product topology editor.</p><RecordList values={[...(proposal.service_proposals || []),...(proposal.binding_proposals || [])]} empty="No Service or binding changes proposed." /></div><div><h3>Model guidance and forecast</h3><p className="repo-guidance">{proposal.instructions || "No bounded instructions proposed."}</p><RecordList values={Object.entries(proposal.readiness_forecast || {}).map(([key,value]) => ({key,value}))} empty="No readiness forecast claims." /></div><section className="repo-claim-zone"><h3>Agent assumptions</h3><RecordList values={proposal.assumptions} empty="No assumptions reported." tone="warning" /><h3>Conflicts and unsupported claims</h3><RecordList values={proposal.conflicts} empty="No conflicts reported." tone="warning" /></section><RecordList values={proposal.blockers} empty="No controller blockers." tone="error" /><RawRecord label="Raw proposal revision" value={proposal} /></div>; }

function DiscoverySummary({ discovery }: { discovery:any }) {
  const inspected = (discovery.files || []).filter((file:any) => file.inspected).length;
  return <><FactGrid facts={[{label:"Resolved revision",value:discovery.repository?.resolved_commit,mono:true},{label:"Inventory",value:`${discovery.files?.length || 0} entries · ${inspected} inspected`},{label:"Inspected text",value:`${Math.round((discovery.inspected_text_bytes || 0) / 1024)} KiB`},{label:"Contract",value:humanize(discovery.contract?.status)},{label:"Languages",value:Object.entries(discovery.language_indicators || {}).map(([key,value]) => `${key} (${value})`).join(" · ") || "None detected"},{label:"Roots",value:(discovery.root_candidates || []).join(" · ") || "None"}]} /><h3>Dependency and lock candidates</h3><RecordList values={discovery.dependency_candidates} /><h3>Acceptance command candidates</h3><RecordList values={discovery.command_candidates} /><h3>Conflicts and blockers</h3><RecordList values={[...(discovery.conflicts || []),...(discovery.blockers || [])]} empty="Discovery reported no conflicts or blockers." tone="warning" /><RawRecord label={`Raw discovery inventory · ${discovery.files?.length || 0} entries`} value={discovery} /></>;
}

function SourceDeliverySummary({ intent }: { intent:any }) {
  const pullRequestUrl = intent.pull_request?.url || intent.pull_request?.html_url;
  return <><FactGrid facts={[{label:"Intent",value:intent.id,mono:true},{label:"Approved head",value:intent.pull_request?.head_sha || intent.base_commit,mono:true},{label:"Pull request",value:pullRequestUrl ? <a href={pullRequestUrl} target="_blank" rel="noreferrer">PR #{intent.pull_request?.number || "open"}</a> : "Awaiting source writer"},{label:"Required checks",value:<Status value={intent.provider_checks?.status || "unavailable"} />},{label:"Merge result",value:<Status value={intent.merge_provenance?.status || (intent.status === "merged" ? "succeeded" : "waiting")} />},{label:"Merge commit",value:intent.merge_provenance?.merge_commit_sha || "Manual merge pending",mono:true}]} /><Freshness status={intent.provider_checks?.status ? "current" : "unavailable"} expiresAt={intent.provider_checks?.expires_at} /><h3>Provider observations</h3><EvidenceReferences values={[intent.provider_checks?.observation_id ? {kind:"provider_check_set_observation",id:intent.provider_checks.observation_id} : null,intent.merge_provenance?.pre_merge_observation_id ? {kind:"provider_check_set_observation",id:intent.merge_provenance.pre_merge_observation_id} : null,intent.merge_provenance?.merge_observation_id ? {kind:"provider_check_set_observation",id:intent.merge_provenance.merge_observation_id} : null].filter(Boolean)} /><RawRecord label="Raw source-delivery intent" value={intent} /></>;
}

function ReadinessSummary({ readiness, forecast }: { readiness:any; forecast:any }) {
  const value = readiness || forecast || {};
  return <><FactGrid facts={[{label:"Contract",value:<Status value={value.contract_status || "forecast"} />},{label:"Coding",value:<Status value={value.coding_status || "forecast"} />},{label:"Assessed",value:formatMoment(value.assessed_at)},{label:"Profile",value:value.environment_profile_id || "Unavailable"}]} /><RecordList values={value.checks} empty="No sealed readiness checks." /><RecordList values={value.warnings} empty="No warnings." tone="warning" /><RecordList values={value.blockers} empty="No blockers." tone="error" /><EvidenceReferences values={value.evidence_refs} /><RawRecord label="Raw readiness record" value={value} /></>;
}

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
      if(value.status === 409) { setError(value.message); onSaved(); return; }
      setError(value.message);
    }
  };
  return <div className="repo-proposal-editor"><button type="button" onClick={() => setEditing(value => !value)}>{editing ? "Close proposal editor" : "Edit proposal revision"}</button>{editing ? <div className="repo-inline-form"><label className="repo-span-2">Repository instructions<textarea rows={6} value={instructions} onChange={event => setInstructions(event.target.value)} /></label><label>Executable contract JSON<textarea rows={12} className="repo-mono" value={contract} onChange={event => setContract(event.target.value)} /></label><label>Product and Service mappings JSON<textarea rows={12} className="repo-mono" value={mappings} onChange={event => setMappings(event.target.value)} /></label><label className="repo-span-2">Assumptions, conflicts, blockers, and forecast JSON<textarea rows={10} className="repo-mono" value={review} onChange={event => setReview(event.target.value)} /></label><label>Operator<input value={actor} onChange={event => setActor(event.target.value)} /></label><label>Reason<input value={reason} onChange={event => setReason(event.target.value)} /></label>{error ? <div className="repo-error" role="alert">{error}</div> : null}<button className="repo-primary" type="button" disabled={!actor.trim() || !reason.trim()} onClick={save}>Save new proposal revision</button></div> : null}</div>;
}

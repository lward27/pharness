import { useMemo, useState } from "react";
import { ArrowLeft, CheckCircle, Clock, GitPullRequest, MagnifyingGlass, Robot, WarningCircle } from "@phosphor-icons/react";
import { RunDetailView } from "../../views/RunDetailView";
import { WorkItemDetailView } from "../../views/WorkItemDetailView";
import { getJson, query, sendJson } from "../api";
import { ActionDialog, Empty, LinkButton, Metric, OutcomeDetails, ResourceState, SectionHeader, Status, type ServerAction } from "../components";
import { navigate } from "../routes";
import { useResource } from "../useResource";

const stages = ["discover", "plan", "implement", "test", "verify", "source_delivery"];

export function WorkItemsScreen() {
  const [lifecycle, setLifecycle] = useState<"current" | "history">("current");
  const [search, setSearch] = useState("");
  const resource = useResource<any>(query("/api/work-items", { mode:"repo", lifecycle, search, include:"operator_state", limit:100 }));
  const legacy = useResource<any>(query("/api/work-items", { mode:"legacy", lifecycle, search, include:"operator_state", limit:100 }));
  const overview = useResource<any>("/api/organization/overview");
  const groups = useMemo(() => {
    const values = new Map<string,{productId:string;productName:string;stage:string;items:any[]}>();
    for(const item of resource.data?.work_items || []) {
      const stage = resource.data?.operator_state?.[item.id]?.current_lifecycle_stage || (item.status === "completed" ? "source_delivery" : "discover");
      const productId = item.product_id || "unassigned";
      const productName = overview.data?.product_summaries?.find((entry:any) => entry.id === productId)?.display_name || (item.product_id ? item.product_id : "Unassigned");
      const key = `${productId}:${stage}`;
      const group = values.get(key) || {productId,productName,stage,items:[]};
      group.items.push(item); values.set(key,group);
    }
    return Array.from(values.values());
  },[resource.data,overview.data]);
  return <ResourceState status={resource.status} error={resource.error}>
    <SectionHeader eyebrow="Intent ledger" title="WorkItems" summary="Current state is primary. Closed, terminal, cancelled, and superseded work remains under History." />
    <div className="repo-list-controls"><div className="repo-segmented" role="group" aria-label="Lifecycle partition"><button type="button" className={lifecycle === "current" ? "is-active" : ""} onClick={() => setLifecycle("current")}>Current</button><button type="button" className={lifecycle === "history" ? "is-active" : ""} onClick={() => setLifecycle("history")}>History</button></div><label className="repo-filterbar"><MagnifyingGlass size={18} /><input aria-label="Search WorkItems" value={search} onChange={event => setSearch(event.target.value)} placeholder="Search intent, title, or ID" /></label></div>
    <div className="repo-workitem-groups">
      {groups.map(group => <section className="repo-workitem-group" key={`${group.productId}:${group.stage}`}><header><div><span className="repo-eyebrow">{group.productName}</span><h2>{group.stage.replaceAll("_"," ")}</h2></div><span className="repo-count">{group.items.length}</span></header>{group.items.map((item:any) => <article className="repo-workitem-row" key={item.id}>
        <div className="repo-workitem-main"><span className="repo-eyebrow">{item.product_id || "Unassigned"}</span><h2>{item.title}</h2><p>{item.intent}</p><div className="repo-tags"><span>{item.repository_id}</span><span className="repo-mono">{item.source_commit?.slice(0,12)}</span></div></div>
        <Lifecycle stage={resource.data?.operator_state?.[item.id]?.current_lifecycle_stage || (item.status === "completed" ? "source_delivery" : "discover")} status={item.status} />
        <div className="repo-workitem-boundary"><span>Current boundary</span><strong>{resource.data?.operator_state?.[item.id]?.current_boundary || item.status_reason || item.status}</strong><Status value={item.status} /></div>
        <LinkButton to={`work-items/${item.id}/overview`}>Open WorkItem</LinkButton>
      </article>)}</section>)}
      {!resource.data?.work_items?.length ? <Empty title={`No ${lifecycle} Repo Mode WorkItems`} message={lifecycle === "current" ? "Create work from a coding-ready Repository inside its Product." : "Closed and terminal work will remain here as durable history."} /> : null}
    </div>
    {legacy.data?.count ? <section className="repo-panel repo-legacy-workitems"><header><div><span className="repo-eyebrow">Unassigned legacy</span><h2>Full-SDLC WorkItems</h2></div><span className="repo-count">{legacy.data.count}</span></header><p className="repo-muted">These records retain the existing delivery workspace and production controls. They are never attributed to a Product.</p><div className="repo-list">{legacy.data.work_items.map((item:any) => <button className="repo-list-row" type="button" key={item.id} onClick={() => navigate(`work-items/${item.id}/overview`)}><div><strong>{item.title}</strong><span>{legacy.data?.operator_state?.[item.id]?.current_boundary || item.status_reason || item.intent}</span></div><Status value={item.status} /></button>)}</div></section> : null}
  </ResourceState>;
}

function Lifecycle({ stage, status }: { stage:string; status:string }) {
  const index = Math.max(0, stages.indexOf(stage));
  return <ol className="repo-lifecycle" aria-label={`Lifecycle: ${stage}`}>
    {stages.map((value,position) => <li className={status === "completed" || position < index ? "is-complete" : position === index ? "is-current" : ""} key={value}><span>{position < index || status === "completed" ? <CheckCircle size={13} weight="fill" /> : position + 1}</span><small>{value.replace("_"," ")}</small></li>)}
  </ol>;
}

const detailSections = ["overview", "current-stage", "stage-outcomes", "delivery", "evidence", "history"];

export function WorkItemScreen({ workItemId, section, operatorName }: { workItemId:string; section:string; operatorName?:string }) {
  const resource = useResource<any>(`/api/work-items/${encodeURIComponent(workItemId)}/flow`, { pollMs:10_000 });
  const evidence = useResource<any>(section === "evidence" ? `/api/work-items/${encodeURIComponent(workItemId)}/evidence` : null);
  const [selectedAction, setSelectedAction] = useState<ServerAction | null>(null);
  const flow = resource.data;
  const item = flow?.work_item;
  const repo = flow?.repo_mode;
  if (item && item.mode !== "repo") {
    return <WorkItemDetailView workItemId={workItemId} autoRefresh operatorName={operatorName} onBack={() => navigate("work-items")} refreshDashboard={resource.refresh} />;
  }
  const executions = repo?.stage_executions || [];
  const outcomes = repo?.effective_stage_outcomes || [];
  const currentExecution = executions.find((entry:any) => entry.id === item?.current_stage_execution_id) || executions.at(-1);
  const currentRunId = currentExecution?.run_id || item?.current_run_id;
  const action = flow?.action_rail?.find((entry:any) => entry.status === "ready") || flow?.action_rail?.[0];
  const currentStage = currentExecution?.stage_key || (item?.status === "completed" ? "source_delivery" : "discover");
  return <ResourceState status={resource.status} error={resource.error}>
    <header className="repo-workitem-header"><button type="button" onClick={() => navigate("work-items")}><ArrowLeft size={18} />WorkItems</button><div><span className="repo-eyebrow">{item?.product_id} · {item?.repository_id}</span><h1>{item?.title || workItemId}</h1><p>{item?.intent}</p></div><div className="repo-workitem-status"><Status value={item?.status} /><span>{currentStage.replace("_"," ")}</span></div>{action ? <button className="repo-primary" type="button" disabled={action.status !== "ready"} title={action.blockers?.map((value:any) => typeof value === "string" ? value : value.summary || value.code).join("; ")} onClick={() => setSelectedAction(action)}>{action.id.replaceAll("_"," ")}</button> : null}</header>
    <Lifecycle stage={currentStage} status={item?.status} />
    <nav className="repo-tabs" aria-label="WorkItem sections">{detailSections.map(value => <button type="button" className={section === value ? "is-active" : ""} key={value} onClick={() => navigate(`work-items/${workItemId}/${value}`)}>{value.replaceAll("-"," ")}</button>)}</nav>
    {section === "overview" ? <WorkItemOverview item={item} flow={flow} action={action} currentStage={currentStage} /> : null}
    {section === "current-stage" ? currentRunId ? <div className="repo-embedded-run"><RunDetailView runId={currentRunId} embedded onOpenQueue={() => navigate("agents")} operatorName={operatorName} refreshDashboard={resource.refresh} /></div> : <Empty title="No active AgentRun" message="This WorkItem is currently at a controller or human boundary. Stage evidence remains available in Stage Outcomes." /> : null}
    {section === "stage-outcomes" ? <StageOutcomes outcomes={outcomes} executions={executions} /> : null}
    {section === "delivery" ? <RepoDelivery item={item} intent={repo?.source_delivery_intent} outcomes={outcomes} /> : null}
    {section === "evidence" ? <Evidence resource={evidence} /> : null}
    {section === "history" ? <History flow={flow} /> : null}
    {selectedAction ? <ActionDialog action={selectedAction} owner={{kind:"WorkItem",id:workItemId,product:item?.product_id,repository:item?.repository_id,revision:item?.source_commit}} endpoint={`/api/work-items/${encodeURIComponent(workItemId)}/actions/${encodeURIComponent(selectedAction.id)}/execute`} operatorName={operatorName} onClose={() => setSelectedAction(null)} onApplied={resource.refresh} /> : null}
  </ResourceState>;
}

function WorkItemOverview({ item, flow, action, currentStage }: any) {
  const contract = item?.repository_contract || {};
  return <div className="repo-overview-grid"><section className="repo-panel repo-span-2"><header><div><span className="repo-eyebrow">Intent boundary</span><h2>What PHarness is doing</h2></div><Status value={item?.status} /></header><p className="repo-intent">{item?.intent}</p><dl className="repo-bindings"><div><dt>Product</dt><dd>{item?.product_id}</dd></div><div><dt>Repository</dt><dd>{item?.repository_id}</dd></div><div><dt>Immutable revision</dt><dd className="repo-mono">{item?.source_commit}</dd></div><div><dt>Current stage</dt><dd>{currentStage.replace("_"," ")}</dd></div><div><dt>Stop or wait reason</dt><dd>{item?.status_reason || "No active blocker"}</dd></div><div><dt>Recommended next action</dt><dd>{action?.external_effect_summary || "Wait for durable state to change"}</dd></div></dl></section><section className="repo-panel"><h2>Acceptance boundary</h2>{(item?.acceptance_command_names || []).map((name:string,index:number) => <div className="repo-acceptance" key={name}><CheckCircle size={18} /><div><strong>{name}</strong><span className="repo-mono">{item.acceptance_criteria?.[index]}</span></div></div>)}</section><section className="repo-panel"><h2>Execution envelope</h2><div className="repo-metrics repo-compact"><Metric label="Turns" value={`${item?.run_budget?.initial_turns || 0}/${item?.run_budget?.hard_turns || 0}`} /><Metric label="Tokens" value={`${item?.run_budget?.initial_tokens || 0}/${item?.run_budget?.hard_tokens || 0}`} /><Metric label="Active time" value={`${item?.run_budget?.active_execution_seconds || 0}s`} /><Metric label="Attempts" value={`${item?.attempt_count || 0}/${item?.max_attempts || 0}`} /></div><p>{contract.environment_profile || contract.environment_profile_id || item?.environment_profile_id}</p></section><section className="repo-panel repo-span-2"><h2>One contextual action</h2>{action ? <div className="repo-next-action"><Robot size={22} /><div><strong>{action.id.replaceAll("_"," ")}</strong><p>{action.external_effect_summary}</p>{action.blockers?.map((blocker:any) => <small key={blocker.code}>{blocker.summary}</small>)}</div><Status value={action.status} /></div> : <p className="repo-muted">No action is currently eligible. The WorkItem is waiting or closed.</p>}{flow?.repo_mode?.safe_advance?.eligible ? <p className="repo-safe"><CheckCircle size={17} />Safe internal advance is server-authorized: {flow.repo_mode.safe_advance.summary}</p> : null}</section></div>;
}

function StageOutcomes({ outcomes, executions }: any) {
  const byStage = new Map(outcomes.map((outcome:any) => [outcome.stage_key,outcome]));
  return <div className="repo-stage-grid">{stages.map(stage => { const outcome:any = byStage.get(stage); const stageExecutions = executions.filter((execution:any) => execution.stage_key === stage); return <details className="repo-outcome-card" key={stage} open={Boolean(outcome) && ["failed","blocked"].includes(outcome.status)}><summary><div><span className="repo-eyebrow">{stageExecutions.length} execution{stageExecutions.length === 1 ? "" : "s"}</span><h2>{stage.replace("_"," ")}</h2><p>{outcome?.outcome?.conclusion || outcome?.outcome?.stop_reason || "No sealed outcome"}</p></div><Status value={outcome?.status || "waiting"} /></summary>{outcome ? <OutcomeDetails outcome={outcome} /> : <p className="repo-muted">This stage has not produced an effective outcome.</p>}</details>; })}</div>;
}

function RepoDelivery({ item, intent, outcomes }: any) {
  const deliveryOutcome = outcomes.find((outcome:any) => outcome.stage_key === "source_delivery");
  const releaseOutcome = outcomes.find((outcome:any) => outcome.stage_key === "release");
  const observeOutcome = outcomes.find((outcome:any) => outcome.stage_key === "observe");
  return <div className="repo-two-columns"><section className="repo-panel repo-span-2"><header><div><span className="repo-eyebrow">Repo Mode delivery</span><h2>Source pull request and manual merge</h2></div><Status value={deliveryOutcome?.status || intent?.status || "waiting"} /></header>{intent ? <><dl className="repo-bindings"><div><dt>Intent</dt><dd className="repo-mono">{intent.id}</dd></div><div><dt>Approved head</dt><dd className="repo-mono">{intent.pull_request?.head_sha || intent.base_commit}</dd></div><div><dt>Pull request</dt><dd>{intent.pull_request?.html_url || intent.pull_request?.url || "Awaiting writer"}</dd></div><div><dt>Required checks</dt><dd>{intent.provider_checks?.status || "Not observed"}</dd></div><div><dt>Observation freshness</dt><dd>{intent.provider_checks?.expires_at || "Unavailable"}</dd></div><div><dt>Merge provenance</dt><dd className="repo-mono">{intent.merge_provenance?.merge_commit_sha || "Manual merge pending"}</dd></div></dl><pre className="repo-code">{JSON.stringify({ required_checks:intent.provider_checks?.required_checks, merge_provenance:intent.merge_provenance },null,2)}</pre></> : <Empty title="Source delivery not authorized" message="The exact ChangeSet must pass verification and human review before PHarness can create one source PR." />}</section><Inapplicable title="Release" outcome={releaseOutcome} fallback="Repo Mode V1 closes after PHarness observes the exact source merge; it does not manufacture a Release." /><Inapplicable title="Observe" outcome={observeOutcome} fallback="Runtime observation is outside Repo Mode V1 and is controller-recorded as inapplicable." />{item?.status === "completed" && deliveryOutcome?.status === "succeeded" ? <div className="repo-completion repo-span-2"><CheckCircle size={24} weight="fill" /><div><strong>Source Delivery succeeded</strong><p>{item.closure_reason || deliveryOutcome.outcome?.stop_reason}</p></div></div> : null}</div>;
}

function Inapplicable({ title, outcome, fallback }: any) { return <section className="repo-panel repo-inapplicable"><header><h2>{title}</h2><Status value={outcome?.status || "inapplicable"} /></header><p>{outcome?.outcome?.stop_reason || outcome?.outcome?.conclusion || fallback}</p></section>; }

function Evidence({ resource }: any) { return <ResourceState status={resource.status} error={resource.error}><div className="repo-stage-grid">{(resource.data?.evidence_validations || []).map((validation:any) => <article className="repo-panel" key={validation.id}><header><div><span className="repo-eyebrow">{validation.validator_key}</span><h2>{validation.id}</h2></div><Status value={validation.status} /></header><dl className="repo-bindings"><div><dt>Stage execution</dt><dd className="repo-mono">{validation.stage_execution_id || "WorkItem scoped"}</dd></div><div><dt>Content hash</dt><dd className="repo-mono">{validation.content_hash}</dd></div><div><dt>Validated</dt><dd>{validation.validated_at}</dd></div></dl><h3>Verified facts</h3><pre className="repo-code">{JSON.stringify(validation.facts,null,2)}</pre><h3>Evidence references</h3><pre className="repo-code">{JSON.stringify(validation.evidence_refs,null,2)}</pre>{validation.contradictions?.length ? <div className="repo-warning"><WarningCircle size={17} />{JSON.stringify(validation.contradictions)}</div> : null}</article>)}{!resource.data?.evidence_validations?.length ? <Empty title="No evidence validations" message="Typed immutable validation records appear here as the stage chain progresses." /> : null}</div></ResourceState>; }

function History({ flow }: any) { return <div className="repo-two-columns"><section className="repo-panel"><header><h2>StageExecution history</h2><span className="repo-count">{flow?.repo_mode?.stage_executions?.length || 0}</span></header><div className="repo-list">{(flow?.repo_mode?.stage_executions || []).map((execution:any) => <div className="repo-list-row" key={execution.id}><Clock size={18} /><div><strong>{execution.stage_key} · attempt {execution.sequence}</strong><span className="repo-mono">{execution.id}</span></div><Status value={execution.status} /></div>)}</div></section><section className="repo-panel"><header><h2>Annotations and decisions</h2><span className="repo-count">{flow?.repo_mode?.operator_annotations?.length || 0}</span></header><pre className="repo-code">{JSON.stringify({ annotations:flow?.repo_mode?.operator_annotations || [], decisions:flow?.repo_mode?.operator_annotation_decisions || [] },null,2)}</pre></section><section className="repo-panel repo-span-2"><h2>Immutable closure</h2><dl className="repo-bindings"><div><dt>Closed at</dt><dd>{flow?.work_item?.closed_at || "Open"}</dd></div><div><dt>Closure reason</dt><dd>{flow?.work_item?.closure_reason || "Not closed"}</dd></div><div><dt>Product model snapshot</dt><dd className="repo-mono">{flow?.repo_mode?.product_model_snapshot?.id}</dd></div><div><dt>Repository contract</dt><dd className="repo-mono">{flow?.repo_mode?.repository_contract_version_id}</dd></div></dl></section></div>; }

const defaultBudget = { initial_turns:48, hard_turns:100, initial_tokens:400000, hard_tokens:1000000, active_execution_seconds:3600, recoverable_tool_errors:4, identical_failures:2, verification_reserve_turns:8 };

export function NewWorkItemScreen({ productId, operatorName }: { productId:string; operatorName?:string }) {
  const product = useResource<any>(`/api/products/${encodeURIComponent(productId)}/overview`);
  const profiles = useResource<any>("/api/agent-profiles");
  const [step,setStep] = useState(1);
  const [form,setForm] = useState<any>({ repository_id:"", source_commit:"", title:"", intent:"", acceptance_command_names:[], context_repositories:[], builder_budget:defaultBudget, max_attempts:2, actor:operatorName || "operator", reason:"Create bounded Repo Mode WorkItem" });
  const [repository,setRepository] = useState<any>(null);
  const [preflight,setPreflight] = useState<any>(null);
  const [error,setError] = useState("");
  const selectRepository = async (id:string) => {
    setForm((value:any) => ({...value,repository_id:id,source_commit:"",acceptance_command_names:[],context_repositories:[]})); setPreflight(null); setRepository(null); setError("");
    if(!id) return;
    try {
      const data = await getJson(`/api/repositories/${encodeURIComponent(id)}/overview`);
      setRepository(data); setForm((value:any) => ({...value,source_commit:data.repository.registered_commit}));
    } catch(caught) { setError(caught instanceof Error ? caught.message : String(caught)); }
  };
  const commands = repository?.canonical_contract?.contract?.acceptance_commands || [];
  const runPreflight = async () => {
    setError("");
    try { setPreflight(await sendJson(`/api/products/${encodeURIComponent(productId)}/work-items/preflight`,"POST",pickRequest(form))); setStep(4); } catch(caught) { setError(caught instanceof Error ? caught.message : String(caught)); }
  };
  const create = async () => { setError(""); try { const result = await sendJson(`/api/products/${encodeURIComponent(productId)}/work-items`,"POST",{...pickRequest(form),preflight_hash:preflight.preflight_hash,actor:form.actor,reason:form.reason}); navigate(`work-items/${result.work_item?.id || result.id}/overview`); } catch(caught) { setError(caught instanceof Error ? caught.message : String(caught)); setPreflight(null); setStep(3); } };
  return <ResourceState status={product.status} error={product.error}><SectionHeader eyebrow="New WorkItem" title={product.data?.product?.display_name || productId} summary="Create one bounded source-only intent against an immutable Repository revision." /><ol className="repo-stepper repo-four">{["Repository","Intent","Agent envelope","Preflight"].map((label,index) => <li className={index+1 < step ? "is-complete" : index+1 === step ? "is-current" : ""} key={label}><span>{index+1 < step ? <CheckCircle size={16} weight="fill" /> : index+1}</span><small>{label}</small></li>)}</ol><section className="repo-wizard repo-panel">
    {step === 1 ? <><h2>Repository and immutable source</h2><label>Mutable Repository<select value={form.repository_id} onChange={event => selectRepository(event.target.value)}><option value="">Select a coding-ready Repository</option>{(product.data?.repositories || []).map((repo:any) => <option value={repo.id} key={repo.id}>{repo.external_id}</option>)}</select></label>{repository ? <><dl className="repo-bindings"><div><dt>Contract readiness</dt><dd><Status value={repository.readiness?.contract_status} /></dd></div><div><dt>Coding readiness</dt><dd><Status value={repository.readiness?.coding_status} /></dd></div><div><dt>Source SHA</dt><dd className="repo-mono">{form.source_commit}</dd></div><div><dt>Service scope</dt><dd>Inherited from the reviewed Product binding</dd></div></dl><fieldset><legend>Optional read-only context Repositories · maximum 4</legend>{(product.data?.repositories || []).filter((entry:any) => entry.id !== form.repository_id).map((entry:any) => { const selected = form.context_repositories.some((value:any) => value.repository_id === entry.id); return <label className="repo-checkbox" key={entry.id}><input type="checkbox" checked={selected} disabled={!selected && form.context_repositories.length >= 4} onChange={event => setForm((value:any) => ({...value,context_repositories:event.target.checked ? [...value.context_repositories,{repository_id:entry.id,source_commit:entry.registered_commit}] : value.context_repositories.filter((context:any) => context.repository_id !== entry.id)}))} /><span><strong>{entry.external_id}</strong><small className="repo-mono">{entry.registered_commit}</small></span></label>})}{(product.data?.repositories || []).length <= 1 ? <p className="repo-muted">No other Product-bound Repository is available as context.</p> : null}</fieldset></> : null}</> : null}
    {step === 2 ? <><h2>Intent and acceptance boundary</h2><label>Title<input value={form.title} onChange={event => setForm((value:any) => ({...value,title:event.target.value}))} /></label><label>Bounded intent<textarea rows={6} value={form.intent} onChange={event => setForm((value:any) => ({...value,intent:event.target.value}))} /></label><fieldset><legend>Declared acceptance commands</legend>{commands.map((command:any) => <label className="repo-checkbox" key={command.name}><input type="checkbox" checked={form.acceptance_command_names.includes(command.name)} onChange={event => setForm((value:any) => ({...value,acceptance_command_names:event.target.checked ? [...value.acceptance_command_names,command.name] : value.acceptance_command_names.filter((name:string) => name !== command.name)}))} /><span><strong>{command.name}</strong><small className="repo-mono">{command.command}</small></span></label>)}</fieldset></> : null}
    {step === 3 ? <><h2>Agent profile and execution envelope</h2><div className="repo-profile-readonly"><Robot size={20} /><div><strong>repo-builder</strong><span>Compiled profile · selected by controller · {profiles.data?.agent_profiles?.find((profile:any) => profile.id === "repo-builder")?.profile_hash || "loading"}</span></div></div><div className="repo-form-grid">{Object.entries(form.builder_budget).map(([key,value]) => <label key={key}>{key.replaceAll("_"," ")}<input type="number" value={String(value)} onChange={event => setForm((current:any) => ({...current,builder_budget:{...current.builder_budget,[key]:Number(event.target.value)}}))} /></label>)}<label>Attempts<input type="number" min="1" max="3" value={form.max_attempts} onChange={event => setForm((value:any) => ({...value,max_attempts:Number(event.target.value)}))} /></label></div><p className="repo-muted">Provider transport retries: 3 · server-owned and read-only.</p></> : null}
    {step === 4 ? <><h2>Read-only preflight and final summary</h2><div className="repo-preflight"><dl className="repo-bindings"><div><dt>Repository</dt><dd>{preflight?.source_repo}</dd></div><div><dt>Source SHA</dt><dd className="repo-mono">{preflight?.source_commit}</dd></div><div><dt>Profile</dt><dd>{preflight?.environment_profile_id}</dd></div><div><dt>Product snapshot</dt><dd className="repo-mono">{preflight?.product_model_snapshot_id}</dd></div></dl>{preflight?.predicted_mutations?.map((value:string) => <p key={value}><CheckCircle size={17} />{value.replaceAll("_"," ")}</p>)}{preflight?.warnings?.map((value:any,index:number) => <p className="repo-warning" key={index}>{value.summary || JSON.stringify(value)}</p>)}{preflight?.blockers?.map((value:any,index:number) => <p className="repo-error" key={index}>{value.summary || JSON.stringify(value)}</p>)}</div><div className="repo-form-grid"><label>Operator<input value={form.actor} onChange={event => setForm((value:any) => ({...value,actor:event.target.value}))} /></label><label>Reason<input value={form.reason} onChange={event => setForm((value:any) => ({...value,reason:event.target.value}))} /></label></div></> : null}
    {error ? <div className="repo-error" role="alert">{error}</div> : null}<footer><button type="button" disabled={step === 1} onClick={() => {setStep(value => Math.max(1,value-1));setPreflight(null);}}>Back</button>{step < 3 ? <button className="repo-primary" type="button" disabled={step === 1 ? !form.repository_id || repository?.readiness?.coding_status !== "ready" : !form.title.trim() || !form.intent.trim() || !form.acceptance_command_names.length} onClick={() => setStep(value => value+1)}>Continue</button> : step === 3 ? <button className="repo-primary" type="button" onClick={runPreflight}>Run preflight</button> : <button className="repo-primary" type="button" disabled={Boolean(preflight?.blockers?.length) || !form.actor.trim() || !form.reason.trim()} onClick={create}>Confirm and create WorkItem</button>}</footer>
  </section></ResourceState>;
}

function pickRequest(form:any) { return { title:form.title, intent:form.intent, repository_id:form.repository_id, source_commit:form.source_commit, acceptance_command_names:form.acceptance_command_names, context_repositories:form.context_repositories, builder_budget:form.builder_budget, max_attempts:form.max_attempts }; }

import { useEffect, useRef, useState } from "react";
import { Brain, ClockCounterClockwise, PlugsConnected, Pulse, Robot, ShieldCheck } from "@phosphor-icons/react";
import { RunDetailView } from "../../views/RunDetailView";
import { getJson, query, sendJson } from "../api";
import { Empty, ResourceState, SectionHeader, Status } from "../components";
import { navigate } from "../routes";
import { useResource } from "../useResource";
import { DataLifecycleScreen } from "./DataLifecycleScreen";

export function AgentsScreen() {
  const [lifecycle, setLifecycle] = useState<"current" | "history">("current");
  const [offset, setOffset] = useState(0);
  const limit = 50;
  const runs = useResource<any>(query("/api/runs", { lifecycle, limit, offset }));
  const profiles = useResource<any>("/api/agent-profiles");
  const selectLifecycle = (value: "current" | "history") => { setLifecycle(value); setOffset(0); };
  return <ResourceState status={runs.status} error={runs.error}>
    <SectionHeader eyebrow="Execution" title="Agents" summary="Active AgentRuns are separate from immutable Run History. Compiled profiles are read-only." />
    <div className="repo-list-controls"><div className="repo-segmented"><button type="button" className={lifecycle === "current" ? "is-active" : ""} onClick={() => selectLifecycle("current")}>Active Runs</button><button type="button" className={lifecycle === "history" ? "is-active" : ""} onClick={() => selectLifecycle("history")}>Run History</button></div></div>
    <div className="repo-two-columns"><section className="repo-panel repo-span-2"><header><h2>{lifecycle === "current" ? "Active AgentRuns" : "Run History"}</h2><span className="repo-count">{runs.data?.count || 0}</span></header>
      <div className="repo-table" role="region" aria-label={`${lifecycle === "current" ? "Active" : "Historical"} AgentRuns table`} tabIndex={0}>
        <div className="repo-table-head"><span>Agent</span><span>Product / WorkItem</span><span>StageExecution</span><span>Status</span><span>Budget</span></div>
        {(runs.data?.runs || []).map((run:any) => <button type="button" className="repo-table-row" key={run.id} onClick={() => navigate(`agents/runs/${run.id}`)}><span><strong>{run.ownership?.agent_profile_id || "Legacy run"}</strong><small className="repo-mono">{run.id}</small></span><span><strong>{run.ownership?.product_id || "Unassigned"}</strong><small>{run.ownership?.work_item_id || run.task}</small></span><span className="repo-mono">{run.ownership?.stage_execution_id || "Not stage-owned"}</span><span><Status value={run.status} /><small>{run.retention_state === "compacted" ? "Raw detail intentionally expired" : "Raw detail retained"}</small></span><span>{run.budget_consumption?.turns_used || 0}/{run.budget_consumption?.allowed_turns || run.max_turns} turns</span></button>)}
      </div>
      {!runs.data?.runs?.length ? <Empty title={lifecycle === "current" ? "No agents running" : "No Run history"} message="AgentRuns appear here with their Product, WorkItem, stage, profile, budget, approvals, and evidence ownership." /> : null}
      {runs.data?.count > limit ? <nav className="repo-pagination" aria-label="AgentRun pages"><button type="button" disabled={offset === 0} onClick={() => setOffset(value => Math.max(0,value-limit))}>Previous</button><span>{offset+1}–{Math.min(offset+limit,runs.data.count)} of {runs.data.count}</span><button type="button" disabled={offset+limit >= runs.data.count} onClick={() => setOffset(value => value+limit)}>Next</button></nav> : null}
    </section><section className="repo-panel repo-span-2"><header><h2>Compiled AgentProfiles</h2><Status value="read only" /></header><div className="repo-card-grid repo-three">{(profiles.data?.agent_profiles || []).map((profile:any) => <article className="repo-profile" key={profile.id}><Robot size={22} /><div><strong>{profile.id}</strong><span>{profile.version}</span></div><p>{profile.allowed_tools?.join(" · ") || profile.tool_allowlist?.join(" · ")}</p><small className="repo-mono">{profile.profile_hash}</small></article>)}</div></section></div>
  </ResourceState>;
}

export function AgentRunScreen({ runId, operatorName }: { runId:string; operatorName?:string }) { return <div className="repo-run-screen"><button type="button" className="repo-back" onClick={() => navigate("agents")}>← Agents</button><RunDetailView runId={runId} onOpenQueue={() => navigate("agents")} operatorName={operatorName} /></div>; }

export function ReleasesScreen() {
  const resource = useResource<any>(query("/api/releases",{limit:100}));
  return <ResourceState status={resource.status} error={resource.error}><SectionHeader eyebrow="Connected delivery" title="Releases" summary="Only genuine Release resources appear here. Repo Mode source merges never manufacture runtime health." /><div className="repo-card-grid repo-three">{(resource.data?.releases || []).map((release:any) => <article className="repo-resource-card" key={release.id}><header><Pulse size={22} /><Status value={release.status} /></header><h2>{release.title || release.id}</h2><p>{release.summary}</p><dl><div><dt>Revision</dt><dd className="repo-mono">{release.source_revision || release.commit_sha || release.id}</dd></div><div><dt>Environment</dt><dd>{release.environment || release.target_environment || "Unspecified"}</dd></div><div><dt>Image digest</dt><dd className="repo-mono">{release.image_digest || "Unavailable"}</dd></div></dl></article>)}</div>{!resource.data?.releases?.length ? <Empty title="No connected Releases" message="Release data appears only when a delivery mode creates a real Release resource." /> : null}</ResourceState>;
}

const insightEndpoints:Record<string,string> = { audit:"/api/audit-events?limit=100", observations:"/api/observations?limit=100", incidents:"/api/incidents?limit=100", remediation:"/api/remediation-plans?limit=100" };
export function InsightsScreen({ section }: { section:string }) {
  const endpoint = insightEndpoints[section] || insightEndpoints.audit;
  const resource = useResource<any>(endpoint);
  const entries = resource.data?.audit_events || resource.data?.observations || resource.data?.incidents || resource.data?.remediation_plans || [];
  return <ResourceState status={resource.status} error={resource.error}><SectionHeader eyebrow="History and evidence" title="Insights" summary="Audit, observations, incidents, and remediation remain contextual historical evidence—not primary workflow navigation." /><nav className="repo-tabs">{Object.keys(insightEndpoints).map(value => <button type="button" className={section === value ? "is-active" : ""} key={value} onClick={() => navigate(`insights/${value}`)}>{value}</button>)}</nav><section className="repo-panel"><header><h2>{section}</h2><span className="repo-count">{entries.length}</span></header><div className="repo-list">{entries.map((entry:any) => <article className="repo-list-row" key={entry.id}><ClockCounterClockwise size={18} /><div><strong>{entry.title || entry.kind || entry.action || entry.id}</strong><span>{entry.summary || entry.message || entry.reason || entry.created_at}</span></div><Status value={entry.status || entry.result || "recorded"} /></article>)}{!entries.length ? <p className="repo-muted">No durable {section} records.</p> : null}</div></section></ResourceState>;
}

export function SettingsScreen({ section,operatorName }: { section:string;operatorName:string }) {
  const readiness = useResource<any>("/api/system/readiness",{pollMs:30_000});
  const config = useResource<any>("/api/config/effective");
  const profiles = useResource<any>("/api/environment-profiles");
  return <ResourceState status={readiness.status} error={readiness.error}>
    <SectionHeader eyebrow="Platform" title="Settings" summary="Revision alignment, immutable digests, capabilities, allowlists, configuration readiness, and data lifecycle. Secret names and values are never exposed." />
    <nav className="repo-tabs"><button type="button" className={section === "platform" ? "is-active" : ""} onClick={() => navigate("settings/platform")}>Platform</button><button type="button" className={section === "profiles" ? "is-active" : ""} onClick={() => navigate("settings/profiles")}>Environment profiles</button><button type="button" className={section === "inference" ? "is-active" : ""} onClick={() => navigate("settings/inference")}>Model inference</button><button type="button" className={section === "capabilities" ? "is-active" : ""} onClick={() => navigate("settings/capabilities")}>Capabilities</button><button type="button" className={section === "data-lifecycle" ? "is-active" : ""} onClick={() => navigate("settings/data-lifecycle")}>Data lifecycle</button></nav>
    {section === "platform" ? <div className="repo-two-columns"><section className="repo-panel"><header><h2>Release alignment</h2><Status value={readiness.data?.platform_versions_match ? "aligned" : "mismatch"} /></header><dl className="repo-bindings"><div><dt>API revision</dt><dd className="repo-mono">{readiness.data?.api_revision}</dd></div><div><dt>UI revision</dt><dd className="repo-mono">{readiness.data?.ui_revision}</dd></div><div><dt>Runtime image</dt><dd className="repo-mono">{readiness.data?.runtime_image_digest}</dd></div><div><dt>UI image</dt><dd className="repo-mono">{readiness.data?.ui_image_digest}</dd></div><div><dt>Database generation</dt><dd className="repo-mono">{readiness.data?.database_generation?.id || "Unavailable"}</dd></div><div><dt>Operational mode</dt><dd><Status value={readiness.data?.operational_mode || "unavailable"}/></dd></div></dl></section><section className="repo-panel"><header><h2>Feature cutover</h2><ShieldCheck size={21} /></header><dl className="repo-bindings"><div><dt>Repo Mode controller</dt><dd><Status value={config.data?.features?.repo_mode_v1?.enabled ? "enabled" : "disabled"} /></dd></div><div><dt>Repo Mode UI</dt><dd><Status value={config.data?.features?.repo_mode_v1?.ui_enabled ? "enabled" : "disabled"} /></dd></div><div><dt>Legacy WorkItem creation</dt><dd><Status value={readiness.data?.legacy_work_item_creation_enabled ? "enabled" : "disabled"}/></dd></div><div><dt>Workspace allowlist</dt><dd>{config.data?.workspace?.allowed_repo_count || 0} Repositories</dd></div></dl></section></div> : null}
    {section === "profiles" ? <div className="repo-card-grid repo-three">{(profiles.data?.profiles || []).map((profile:any) => <article className="repo-resource-card" key={profile.id}><header><Robot size={21} /><Status value={profile.status} /></header><h2>{profile.id}</h2><p>{profile.platform} · {profile.runtime_kind || "runtime unavailable"} · {profile.preparation_strategy}</p><dl><div><dt>Image</dt><dd className="repo-mono">{profile.image}</dd></div><div><dt>Revision</dt><dd className="repo-mono">{profile.revision}</dd></div><div><dt>Accepted lock</dt><dd>{profile.accepted_dependency_lock_kinds?.join(" · ") || "Unavailable"}</dd></div><div><dt>Lifecycle scripts</dt><dd>{profile.lifecycle_scripts || "Unavailable"}</dd></div><div><dt>Executables</dt><dd>{profile.required_executables?.join(" · ")}</dd></div></dl></article>)}</div> : null}
    {section === "inference" ? <InferenceSettings operatorName={operatorName} readiness={readiness.data?.inference} /> : null}
    {section === "capabilities" ? <section className="repo-panel"><div className="repo-list">{(readiness.data?.capabilities || []).map((capability:any) => <div className="repo-list-row" key={capability.capability}><div><strong>{capability.capability}</strong><span>{capability.summary}</span><small>{capability.verified_at ? `Verified ${capability.verified_at}` : "No fresh isolated verification"}</small></div><Status value={capability.status} /></div>)}</div><h3>Repository allowlists</h3><pre className="repo-code">{JSON.stringify(readiness.data?.repository_allowlists || {},null,2)}</pre></section> : null}
    {section === "data-lifecycle" ? <DataLifecycleScreen operatorName={operatorName}/> : null}
  </ResourceState>;
}

function InferenceSettings({ operatorName, readiness }: { operatorName:string; readiness:any }) {
  const targets = useResource<any>("/api/inference-targets", { pollMs:30_000 });
  const policies = useResource<any>("/api/inference-policies", { pollMs:30_000 });
  const [actor,setActor] = useState(operatorName || "operator");
  const [reason,setReason] = useState("Verify inference target protocol and isolated connectivity");
  const [pending,setPending] = useState("");
  const [error,setError] = useState("");
  const [qualificationPolicy,setQualificationPolicy] = useState<any>(null);
  const verify = async (target:any) => {
    setPending(`${target.target_id}@${target.revision}`); setError("");
    try {
      await sendJson(`/api/inference-targets/${encodeURIComponent(target.target_id)}/revisions/${encodeURIComponent(target.revision)}/preflight`,"POST",{actor:actor.trim(),reason:reason.trim(),config_hash:targets.data?.registry_hash});
      await targets.refresh();
    } catch(caught) { setError(caught instanceof Error ? caught.message : String(caught)); }
    finally { setPending(""); }
  };
  const gateway = targets.data?.gateway || readiness || {};
  return <div className="repo-two-columns">
    <section className="repo-panel repo-span-2"><header><div><span className="repo-eyebrow">Credential boundary</span><h2>Model Gateway</h2></div><Status value={gateway.status || "unavailable"} /></header><dl className="repo-bindings"><div><dt>Routing mode</dt><dd>{targets.data?.gateway_enabled ? "Gateway for new bound Runs" : "Direct Fireworks rollback path"}</dd></div><div><dt>Registry alignment</dt><dd><Status value={gateway.registry_aligned ? "aligned" : targets.data?.gateway_enabled ? "mismatch" : "disabled"} /></dd></div><div><dt>API registry hash</dt><dd className="repo-mono">{gateway.api_registry_hash || targets.data?.registry_hash || "Unavailable"}</dd></div><div><dt>Gateway registry hash</dt><dd className="repo-mono">{gateway.gateway_registry_hash || "Unavailable"}</dd></div><div><dt>Direct rollback path</dt><dd><Status value={gateway.direct_fireworks_enabled ? "available" : "disabled"} /></dd></div><div><dt>Credential posture</dt><dd>Upstream credentials are mounted only in the gateway.</dd></div></dl>{gateway.blocker ? <div className="repo-warning" role="status">{gateway.blocker}</div> : null}</section>
    <section className="repo-panel repo-span-2"><header><div><span className="repo-eyebrow">Protocol compatibility</span><h2>Inference targets</h2></div><PlugsConnected size={21} /></header><div className="repo-inference-actions"><label>Operator<input value={actor} onChange={event => setActor(event.target.value)} /></label><label>Verification reason<input value={reason} onChange={event => setReason(event.target.value)} /></label></div>{error ? <div className="repo-error" role="alert">{error}</div> : null}<ResourceState status={targets.status} error={targets.error}><div className="repo-card-grid repo-three">{(targets.data?.targets || []).map((target:any) => { const verificationStatus = inferenceVerificationStatus(target); return <article className="repo-resource-card" key={`${target.target_id}@${target.revision}`}><header><Brain size={21} /><Status value={target.selectable ? verificationStatus : "unavailable"} /></header><h2>{target.display_name}</h2><p>{target.backend_kind} · {target.upstream_model}</p><dl><div><dt>Revision</dt><dd>{target.revision}</dd></div><div><dt>Stages</dt><dd>{target.allowed_stages?.join(" · ")}</dd></div><div><dt>Transport</dt><dd>{target.transport?.scheme?.toUpperCase()} · {target.transport?.private_network ? "private network" : "exact-host proxy"}</dd></div><div><dt>Authentication</dt><dd>{target.authentication_configured ? "Gateway binding configured" : "Explicit no-auth target"}</dd></div><div><dt>Limits</dt><dd>{target.context_limit_tokens?.toLocaleString()} context · {target.output_limit_tokens?.toLocaleString()} output</dd></div><div><dt>Config hash</dt><dd className="repo-mono">{target.config_hash}</dd></div></dl>{target.latest_verification?.sanitized_failure ? <div className="repo-error">{target.latest_verification.sanitized_failure}</div> : null}{!target.selectable ? <div className="repo-warning">Disabled in GitOps. Verification does not make this target selectable.</div> : null}<button className="repo-primary" type="button" disabled={Boolean(pending) || !actor.trim() || !reason.trim() || !targets.data?.gateway_enabled} onClick={() => verify(target)}>{pending === `${target.target_id}@${target.revision}` ? "Verifying…" : "Verify target"}</button></article>; })}</div></ResourceState></section>
    <section className="repo-panel repo-span-2"><header><div><span className="repo-eyebrow">Qualified stage behavior</span><h2>Stage inference policies</h2></div><Status value="GitOps managed" /></header><ResourceState status={policies.status} error={policies.error}><div className="repo-card-grid repo-three">{(policies.data?.policies || []).map((policy:any) => { const activeEvaluation = ["queued","running"].includes(policy.latest_evaluation?.status); return <article className="repo-resource-card" key={`${policy.policy_id}@${policy.revision}`}><header><Brain size={21} /><Status value={policy.qualified ? policy.is_default ? "default" : "qualified" : activeEvaluation ? policy.latest_evaluation.status : "blocked"} /></header><h2>{policy.display_name}</h2><p>{policy.eligible_stages?.join(" · ")} · {policy.target?.target_id}</p><dl><div><dt>Reasoning</dt><dd>{policy.reasoning?.effort || "provider default"} · {policy.reasoning?.context_mode || "provider default"}</dd></div><div><dt>Temperature</dt><dd>{policy.temperature ?? "omitted"}</dd></div><div><dt>Output cap</dt><dd>{policy.maximum_output_tokens?.toLocaleString()}</dd></div><div><dt>Profiles</dt><dd>{policy.eligible_profiles?.join(" · ")}</dd></div><div><dt>Qualification</dt><dd>{policy.latest_qualification ? `${policy.latest_qualification.verdict} · ${policy.latest_qualification.suite_id}` : policy.qualification_status?.replaceAll("_"," ")}</dd></div>{policy.latest_evaluation ? <div><dt>Latest evaluation</dt><dd>{policy.latest_evaluation.status} · <span className="repo-mono">{policy.latest_evaluation.id}</span></dd></div> : null}<div><dt>Policy hash</dt><dd className="repo-mono">{policy.policy_hash}</dd></div></dl>{!policy.qualified ? <><div className="repo-warning">Not selectable until its exact stage qualification suite passes. Promotion to the default remains a separate GitOps change.</div><button type="button" disabled={!policy.qualification_contract} onClick={() => setQualificationPolicy(policy)}>{activeEvaluation ? "View qualification" : "Run qualification"}</button></> : null}</article>; })}</div></ResourceState></section>
    {qualificationPolicy ? <QualificationDialog policy={qualificationPolicy} registryHash={policies.data?.registry_hash} operatorName={actor} onClose={() => setQualificationPolicy(null)} onRecorded={async () => { setQualificationPolicy(null); await policies.refresh(); }} /> : null}
  </div>;
}

function QualificationDialog({policy,registryHash,operatorName,onClose,onRecorded}:any) {
  const [actor,setActor] = useState(operatorName || "operator");
  const [reason,setReason] = useState(`Run controlled qualification for ${policy.display_name}`);
  const [pending,setPending] = useState(false);
  const [error,setError] = useState("");
  const [evaluation,setEvaluation] = useState<any>(["queued","running"].includes(policy.latest_evaluation?.status) ? policy.latest_evaluation : null);
  const onRecordedRef = useRef(onRecorded);
  useEffect(() => { onRecordedRef.current = onRecorded; },[onRecorded]);
  useEffect(() => {
    if(!evaluation?.id || !["queued","running"].includes(evaluation.status)) return;
    let active = true;
    const timer = window.setInterval(async () => {
      try {
        const latest = await getJson(`/api/inference-evaluations/${encodeURIComponent(evaluation.id)}`);
        if(!active) return;
        setEvaluation(latest);
        if(latest.status === "completed") { window.clearInterval(timer); await onRecordedRef.current(); }
        if(latest.status === "failed") window.clearInterval(timer);
      } catch(caught) { if(active) setError(caught instanceof Error ? caught.message : String(caught)); }
    },3000);
    return () => { active = false; window.clearInterval(timer); };
  },[evaluation?.id,evaluation?.status]);
  const record = async (event:React.FormEvent) => {
    event.preventDefault(); setPending(true); setError("");
    try {
      const started = await sendJson(`/api/inference-policies/${encodeURIComponent(policy.policy_id)}/revisions/${encodeURIComponent(policy.revision)}/qualifications`,"POST",{actor:actor.trim(),reason:reason.trim(),config_hash:registryHash,attempts:2});
      setEvaluation(started);
    } catch(caught) { setError(caught instanceof Error ? caught.message : String(caught)); }
    finally { setPending(false); }
  };
  return <div className="repo-dialog-backdrop" role="presentation"><form className="repo-dialog" role="dialog" aria-modal="true" aria-labelledby="qualification-title" onSubmit={record}><header><div><span className="repo-eyebrow">Controlled gateway evaluation</span><h2 id="qualification-title">{policy.display_name}</h2></div><Status value={evaluation?.status || "review required"} /></header><p>PHarness will dispatch the exact two-attempt suite in an isolated Job. The evaluator receives only its worker identity; upstream model credentials remain in the gateway.</p><dl className="repo-bindings"><div><dt>Suite</dt><dd>{policy.qualification_contract?.suite_id}</dd></div><div><dt>AgentProfile</dt><dd>{policy.qualification_contract?.agent_profile_id}</dd></div><div><dt>Policy</dt><dd className="repo-mono">{policy.policy_hash}</dd></div>{evaluation?.id ? <div><dt>Evaluation</dt><dd className="repo-mono">{evaluation.id}</dd></div> : null}{evaluation?.job_name ? <div><dt>Job</dt><dd className="repo-mono">{evaluation.job_name}</dd></div> : null}</dl>{!evaluation ? <><label>Operator<input value={actor} onChange={event => setActor(event.target.value)} /></label><label>Reason<input value={reason} onChange={event => setReason(event.target.value)} /></label></> : null}{evaluation?.failure ? <div className="repo-error" role="alert">{evaluation.failure}</div> : null}{error ? <div className="repo-error" role="alert">{error}</div> : null}<footer>{!evaluation ? <button className="repo-primary" type="submit" disabled={pending || !actor.trim() || !reason.trim()}>{pending ? "Dispatching…" : "Run two-attempt qualification"}</button> : <span className="repo-muted" role="status">{evaluation.status === "running" ? "Evaluation is running through the model gateway." : `Evaluation ${evaluation.status}.`}</span>}<button type="button" onClick={onClose}>Close</button></footer></form></div>;
}

function inferenceVerificationStatus(target:any) {
  const value = target.latest_verification;
  if(!value) return "configured_unverified";
  if(Number(value.expires_at || 0) <= Math.floor(Date.now()/1000)) return "stale";
  return value.status === "passed" ? "available" : "failed";
}

export function CompatibilityScreen({ root, id, nestedId }: { root:string; id?:string; nestedId?:string }) {
  const [status,setStatus] = useState("Resolving durable ownership…");
  useEffect(() => {
    let active = true;
    const resolve = async () => {
      try {
        let resource:any = null;
        if(root === "flow" && id === "work_item" && nestedId) { navigate(`work-items/${nestedId}/history`); return; }
        if(root === "flow" && nestedId) resource = await getJson(`/api/${id === "change_set" ? "change-sets" : id === "work_plan" ? "work-plans" : `${id}s`}/${encodeURIComponent(nestedId)}`);
        else if(root === "workplans" && id) resource = await getJson(`/api/work-plans/${encodeURIComponent(id)}`);
        else if(root === "approvals" && id) resource = await getJson(`/api/approvals/${encodeURIComponent(id)}`);
        else if(root === "gates" && id) resource = await getJson(`/api/approval-gates/${encodeURIComponent(id)}`);
        let workItemId = findString(resource,"work_item_id");
        const workPlanId = findString(resource,"work_plan_id");
        if(!workItemId && workPlanId) workItemId = findString(await getJson(`/api/work-plans/${encodeURIComponent(workPlanId)}`),"work_item_id");
        const runId = findString(resource,"run_id");
        if(!workItemId && runId) workItemId = findString(await getJson(`/api/runs/${encodeURIComponent(runId)}`),"work_item_id");
        if(active && workItemId) navigate(`work-items/${workItemId}/history`);
        else if(active) setStatus("No owning WorkItem is recorded for this historical resource.");
      } catch { if(active) setStatus("Ownership lookup is unavailable. The historical identifier remains preserved below."); }
    };
    resolve(); return () => { active = false; };
  },[root,id,nestedId]);
  return <section className="repo-panel"><SectionHeader eyebrow="Contextual legacy record" title="Compatibility workspace" summary="This historical deep link is preserved without adding a visible Legacy navigation group." /><p>Resource kind: <strong>{root}</strong>{id ? <> · <span className="repo-mono">{id}</span></> : null}{nestedId ? <> · <span className="repo-mono">{nestedId}</span></> : null}</p><div className="repo-state" role="status">{status}</div><Empty title="Open the owning resource" message="Owned records redirect to Product or WorkItem History. Orphaned legacy records remain readable here while the cutover flag is reversible." /></section>;
}

function findString(value:any,key:string):string | undefined {
  if(!value || typeof value !== "object") return undefined;
  if(typeof value[key] === "string") return value[key];
  if(value.ownership && typeof value.ownership[key] === "string") return value.ownership[key];
  for(const nested of Object.values(value)) { const found = findString(nested,key); if(found) return found; }
  return undefined;
}

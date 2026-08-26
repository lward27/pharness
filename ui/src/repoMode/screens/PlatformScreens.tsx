import { useEffect, useState } from "react";
import { ClockCounterClockwise, Pulse, Robot, ShieldCheck } from "@phosphor-icons/react";
import { RunDetailView } from "../../views/RunDetailView";
import { getJson, query } from "../api";
import { Empty, ResourceState, SectionHeader, Status } from "../components";
import { navigate } from "../routes";
import { useResource } from "../useResource";

export function AgentsScreen() {
  const [lifecycle,setLifecycle] = useState<"current"|"history">("current");
  const runs = useResource<any>(query("/api/runs",{lifecycle,limit:100}));
  const profiles = useResource<any>("/api/agent-profiles");
  return <ResourceState status={runs.status} error={runs.error}><SectionHeader eyebrow="Execution" title="Agents" summary="Active AgentRuns are separate from immutable Run History. Compiled profiles are read-only." /><div className="repo-list-controls"><div className="repo-segmented"><button type="button" className={lifecycle === "current" ? "is-active" : ""} onClick={() => setLifecycle("current")}>Active Runs</button><button type="button" className={lifecycle === "history" ? "is-active" : ""} onClick={() => setLifecycle("history")}>Run History</button></div></div><div className="repo-two-columns"><section className="repo-panel repo-span-2"><header><h2>{lifecycle === "current" ? "Active AgentRuns" : "Run History"}</h2><span className="repo-count">{runs.data?.count || 0}</span></header><div className="repo-table" role="region" aria-label={`${lifecycle === "current" ? "Active" : "Historical"} AgentRuns table`} tabIndex={0}><div className="repo-table-head"><span>Agent</span><span>Product / WorkItem</span><span>StageExecution</span><span>Status</span><span>Budget</span></div>{(runs.data?.runs || []).map((run:any) => <button type="button" className="repo-table-row" key={run.id} onClick={() => navigate(`agents/runs/${run.id}`)}><span><strong>{run.ownership?.agent_profile_id || "Legacy run"}</strong><small className="repo-mono">{run.id}</small></span><span><strong>{run.ownership?.product_id || "Unassigned"}</strong><small>{run.ownership?.work_item_id || run.task}</small></span><span className="repo-mono">{run.ownership?.stage_execution_id || "Not stage-owned"}</span><span><Status value={run.status} /></span><span>{run.budget_consumption?.turns_used || 0}/{run.budget_consumption?.allowed_turns || run.max_turns} turns</span></button>)}</div>{!runs.data?.runs?.length ? <Empty title={lifecycle === "current" ? "No agents running" : "No Run history"} message="AgentRuns appear here with their Product, WorkItem, stage, profile, budget, approvals, and evidence ownership." /> : null}</section><section className="repo-panel repo-span-2"><header><h2>Compiled AgentProfiles</h2><Status value="read only" /></header><div className="repo-card-grid repo-three">{(profiles.data?.agent_profiles || []).map((profile:any) => <article className="repo-profile" key={profile.id}><Robot size={22} /><div><strong>{profile.id}</strong><span>{profile.version}</span></div><p>{profile.allowed_tools?.join(" · ") || profile.tool_allowlist?.join(" · ")}</p><small className="repo-mono">{profile.profile_hash}</small></article>)}</div></section></div></ResourceState>;
}

export function AgentRunScreen({ runId, operatorName }: { runId:string; operatorName?:string }) { return <div className="repo-run-screen"><button type="button" className="repo-back" onClick={() => navigate("agents")}>← Agents</button><RunDetailView runId={runId} onOpenQueue={() => navigate("agents")} operatorName={operatorName} /></div>; }

export function ReleasesScreen() {
  const resource = useResource<any>(query("/api/releases",{limit:100}));
  return <ResourceState status={resource.status} error={resource.error}><SectionHeader eyebrow="Connected delivery" title="Releases" summary="Only genuine Release resources appear here. Repo Mode source merges never manufacture runtime health." /><div className="repo-card-grid repo-three">{(resource.data?.releases || []).map((release:any) => <article className="repo-resource-card" key={release.id}><header><Pulse size={22} /><Status value={release.status} /></header><h2>{release.title || release.id}</h2><p>{release.summary}</p><dl><div><dt>Revision</dt><dd className="repo-mono">{release.source_revision || release.id}</dd></div><div><dt>Environment</dt><dd>{release.environment || "Unspecified"}</dd></div></dl></article>)}</div>{!resource.data?.releases?.length ? <Empty title="No connected Releases" message="Release data appears only when a delivery mode creates a real Release resource." /> : null}</ResourceState>;
}

const insightEndpoints:Record<string,string> = { audit:"/api/audit-events?limit=100", observations:"/api/observations?limit=100", incidents:"/api/incidents?limit=100", remediation:"/api/remediation-plans?limit=100" };
export function InsightsScreen({ section }: { section:string }) {
  const endpoint = insightEndpoints[section] || insightEndpoints.audit;
  const resource = useResource<any>(endpoint);
  const entries = resource.data?.audit_events || resource.data?.observations || resource.data?.incidents || resource.data?.remediation_plans || [];
  return <ResourceState status={resource.status} error={resource.error}><SectionHeader eyebrow="History and evidence" title="Insights" summary="Audit, observations, incidents, and remediation remain contextual historical evidence—not primary workflow navigation." /><nav className="repo-tabs">{Object.keys(insightEndpoints).map(value => <button type="button" className={section === value ? "is-active" : ""} key={value} onClick={() => navigate(`insights/${value}`)}>{value}</button>)}</nav><section className="repo-panel"><header><h2>{section}</h2><span className="repo-count">{entries.length}</span></header><div className="repo-list">{entries.map((entry:any) => <article className="repo-list-row" key={entry.id}><ClockCounterClockwise size={18} /><div><strong>{entry.title || entry.kind || entry.action || entry.id}</strong><span>{entry.summary || entry.message || entry.reason || entry.created_at}</span></div><Status value={entry.status || entry.result || "recorded"} /></article>)}{!entries.length ? <p className="repo-muted">No durable {section} records.</p> : null}</div></section></ResourceState>;
}

export function SettingsScreen({ section }: { section:string }) {
  const readiness = useResource<any>("/api/system/readiness",{pollMs:30_000});
  const config = useResource<any>("/api/config/effective");
  const profiles = useResource<any>("/api/environment-profiles");
  return <ResourceState status={readiness.status} error={readiness.error}><SectionHeader eyebrow="Platform" title="Settings" summary="Revision alignment, immutable digests, capabilities, allowlists, and configuration readiness. Secret names and values are never exposed." /><nav className="repo-tabs"><button type="button" className={section === "platform" ? "is-active" : ""} onClick={() => navigate("settings/platform")}>Platform</button><button type="button" className={section === "profiles" ? "is-active" : ""} onClick={() => navigate("settings/profiles")}>Environment profiles</button><button type="button" className={section === "capabilities" ? "is-active" : ""} onClick={() => navigate("settings/capabilities")}>Capabilities</button></nav>{section === "platform" ? <div className="repo-two-columns"><section className="repo-panel"><header><h2>Release alignment</h2><Status value={readiness.data?.platform_versions_match ? "aligned" : "mismatch"} /></header><dl className="repo-bindings"><div><dt>API revision</dt><dd className="repo-mono">{readiness.data?.api_revision}</dd></div><div><dt>UI revision</dt><dd className="repo-mono">{readiness.data?.ui_revision}</dd></div><div><dt>Runtime image</dt><dd className="repo-mono">{readiness.data?.runtime_image_digest}</dd></div><div><dt>UI image</dt><dd className="repo-mono">{readiness.data?.ui_image_digest}</dd></div></dl></section><section className="repo-panel"><header><h2>Feature cutover</h2><ShieldCheck size={21} /></header><dl className="repo-bindings"><div><dt>Repo Mode controller</dt><dd><Status value={config.data?.features?.repo_mode_v1?.enabled ? "enabled" : "disabled"} /></dd></div><div><dt>Repo Mode UI</dt><dd><Status value={config.data?.features?.repo_mode_v1?.ui_enabled ? "enabled" : "disabled"} /></dd></div><div><dt>Workspace allowlist</dt><dd>{config.data?.workspace?.allowed_repo_count || 0} Repositories</dd></div></dl></section></div> : null}{section === "profiles" ? <div className="repo-card-grid repo-three">{(profiles.data?.profiles || []).map((profile:any) => <article className="repo-resource-card" key={profile.id}><header><Robot size={21} /><Status value={profile.status} /></header><h2>{profile.id}</h2><p>{profile.platform} · {profile.preparation_strategy}</p><dl><div><dt>Image</dt><dd className="repo-mono">{profile.image}</dd></div><div><dt>Revision</dt><dd className="repo-mono">{profile.revision}</dd></div><div><dt>Executables</dt><dd>{profile.required_executables?.join(" · ")}</dd></div></dl></article>)}</div> : null}{section === "capabilities" ? <section className="repo-panel"><div className="repo-list">{(readiness.data?.capabilities || []).map((capability:any) => <div className="repo-list-row" key={capability.capability}><div><strong>{capability.capability}</strong><span>{capability.summary}</span><small>{capability.verified_at ? `Verified ${capability.verified_at}` : "No fresh isolated verification"}</small></div><Status value={capability.status} /></div>)}</div><h3>Repository allowlists</h3><pre className="repo-code">{JSON.stringify(readiness.data?.repository_allowlists || {},null,2)}</pre></section> : null}</ResourceState>;
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

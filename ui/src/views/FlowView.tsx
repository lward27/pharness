import { useEffect, useState } from "react";
import { Copy, X } from "@phosphor-icons/react";
import { DeliveryChain } from "../components/DeliveryChain";
import { EmptyState } from "../components/Operational";
import { compactId, statusText, timestampTitle } from "../lib/formatters";

type FlowViewProps = {
  dashboard: any;
  evidenceRows: any[];
  events: any[];
  navigate: (view: string, param?: any) => void;
};

function FlowRootPicker({ dashboard, navigate }: { dashboard: any; navigate: FlowViewProps["navigate"] }) {
  const changeSets = dashboard.data?.changeSets ?? [];
  const workPlans = dashboard.data?.workPlans ?? [];
  const current = dashboard.data?.flowRoot;
  const value = current ? `${current.kind}:${current.id}` : "";
  const known = (current?.kind === "change_set" && changeSets.some((item: any) => item.id === current.id)) || (current?.kind === "work_plan" && workPlans.some((item: any) => item.id === current.id));
  if (!changeSets.length && !workPlans.length) return null;
  return <label className="root-picker"><span>Root</span><select value={value} onChange={(event) => { const [kind, ...idParts] = event.target.value.split(":"); navigate("Flow", { kind, id: idParts.join(":") }); }}>
    {!known && current ? <option value={value}>{`${statusText(current.kind)} · ${compactId(current.id)}`}</option> : null}
    {changeSets.map((item: any) => <option key={item.id} value={`change_set:${item.id}`}>{`ChangeSet · ${compactId(item.id)} · ${statusText(item.status)}`}</option>)}
    {workPlans.map((item: any) => <option key={item.id} value={`work_plan:${item.id}`}>{`WorkPlan · ${compactId(item.id)} · ${statusText(item.status)}`}</option>)}
  </select></label>;
}

function EvidenceTable({ rows }: { rows: any[] }) {
  return <section className="evidence"><div className="table-heading"><div><h2>Evidence & Signals</h2><p>Typed reads that support the selected SDLC state.</p></div></div><div className="evidence-table"><div className="evidence-head"><span>Source</span><span>Status</span><span>Resource / Target</span><span>Finding</span><span>Last Event</span><span>Artifact</span></div>
    {rows.map((row) => { const Icon = row.icon; return <div className="evidence-row" key={row.source}><span className="source"><Icon size={23} /> {row.source}</span><span><i className={`dot ${row.tone}`} /> {row.status}</span><span>{row.resource}<strong>{row.target}</strong></span><span>{row.finding}</span><span>{row.lastEvent}</span><span className="link-text">{row.link}</span></div>; })}
    {!rows.length ? <div className="table-empty">No evidence rows are available for this flow.</div> : null}
  </div></section>;
}

function navTargetForResource(resourceKind: string, resourceId: string) {
  const targets: Record<string, [string, any]> = {
    run: ["Run Detail", String(resourceId)], approval: ["Approvals", resourceId], approval_gate: ["Approval Gates", resourceId], remediation_plan: ["Remediation Plans", resourceId], incident: ["Incidents", resourceId], observation: ["Observations", resourceId], work_plan: ["Flow", { kind: "work_plan", id: resourceId }], change_set: ["Flow", { kind: "change_set", id: resourceId }],
  };
  return targets[resourceKind] ?? null;
}

function EventTimeline({ events, navigate }: { events: any[]; navigate: FlowViewProps["navigate"] }) {
  return <section className="timeline-wrap"><div className="timeline-title"><h2>Control-Plane Timeline</h2></div><div className="timeline">
    {events.length ? events.map((event, index) => { const target = navTargetForResource(event.resourceKind, event.resourceId); const body = <><span className="event-time" title={timestampTitle(event.createdAt)}>{event.time}</span><strong>{event.kind}</strong><p>{event.detail}</p></>; return target ? <button className={`event-card event-${event.tone}`} key={`${event.kind}-${event.time}-${index}`} type="button" title={`${event.kind}: ${event.detail}`} onClick={() => navigate(target[0], target[1])}>{body}</button> : <div className={`event-card event-${event.tone}`} key={`${event.kind}-${event.time}-${index}`} title={`${event.kind}: ${event.detail}`}>{body}</div>; }) : <div className="timeline-empty">No audit events are attached to this flow yet.</div>}
  </div></section>;
}

function FlowResourceInspector({ resource, onClose }: { resource: any; onClose: () => void }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try { await navigator.clipboard.writeText(resource.id); setCopied(true); } catch { setCopied(false); }
  };
  return <aside className="durable-artifact-panel flow-resource-inspector" aria-label="Flow resource detail"><div><span className="eyebrow">Durable flow resource</span><h2>{resource.label}</h2></div><button className="icon-button" type="button" aria-label="Close flow resource detail" onClick={onClose}><X size={16} /></button><dl><div><dt>Identifier</dt><dd title={resource.id}>{resource.id}</dd></div><div><dt>Type</dt><dd>{resource.kind}</dd></div><div><dt>Summary</dt><dd>{resource.summary ?? "No additional summary was recorded."}</dd></div></dl><button type="button" onClick={copy}><Copy size={15} /> {copied ? "Copied" : "Copy identifier"}</button></aside>;
}

export function FlowView({ dashboard, evidenceRows, events, navigate }: FlowViewProps) {
  const flow = dashboard.data?.flow;
  const [selectedResource, setSelectedResource] = useState<any>(null);
  const [detailsOpen, setDetailsOpen] = useState(() => window.localStorage.getItem("pharness.flowResourceInspectorOpen") !== "false");
  useEffect(() => { window.localStorage.setItem("pharness.flowResourceInspectorOpen", String(detailsOpen)); }, [detailsOpen]);
  const title = flow ? `${statusText(flow.resource_kind)} Flow` : "SDLC Flow";
  const summary = flow ? `${flow.readiness?.summary ?? "Readiness unavailable"} for ${flow.resource_kind}/${flow.resource_id}.` : dashboard.status === "error" ? "API unavailable. Live SDLC state cannot be loaded." : "No SDLC flow records found yet.";
  const inspectResource = (resource: any) => { setSelectedResource(resource); setDetailsOpen(true); };
  return <><div className="section-heading"><div><h1>{title}</h1><p>{summary}</p></div><div className="legend"><FlowRootPicker dashboard={dashboard} navigate={navigate} /><span><i className="dot healthy" /> Healthy</span><span><i className="dot pending" /> Pending</span><span><i className="dot risk" /> Risk</span><span><i className="dot blocked" /> Blocked</span><span><i className="dot running" /> Running</span></div></div>
    {dashboard.error ? <div className="api-banner">API connection failed: {dashboard.error}</div> : null}
    {flow ? <><DeliveryChain segments={flow.delivery_segments ?? []} onOpenResource={inspectResource} />{selectedResource && detailsOpen ? <FlowResourceInspector resource={selectedResource} onClose={() => setDetailsOpen(false)} /> : null}<EvidenceTable rows={evidenceRows} /><EventTimeline events={events} navigate={navigate} /></> : <EmptyState title="No live SDLC flow" body="The UI did not find a WorkPlan or ChangeSet flow through the API. Run the e2e smoke or create SDLC resources, then refresh." />}
  </>;
}

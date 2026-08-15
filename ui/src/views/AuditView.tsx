import { useEffect, useState } from "react";
import { ArrowsClockwise, MagnifyingGlass } from "@phosphor-icons/react";
import { EmptyState } from "../components/Operational";
import { compactId, formatTimestamp, timestampTitle } from "../lib/formatters";
import { loadAuditEvents } from "../api/dashboard";

type AuditViewProps = {
  dashboard: any;
  openRun: (runId: string) => void;
  selectedSearch: string | null;
  scope: any;
  navigate: (view: string, param?: any) => void;
};

function navTargetForResource(resourceKind: string, resourceId: string) {
  const targets: Record<string, [string, any]> = {
    run: ["Run Detail", String(resourceId)],
    approval: ["Approvals", resourceId],
    approval_gate: ["Approval Gates", resourceId],
    remediation_plan: ["Remediation Plans", resourceId],
    incident: ["Incidents", resourceId],
    observation: ["Observations", resourceId],
    work_plan: ["Flow", { kind: "work_plan", id: resourceId }],
    change_set: ["Flow", { kind: "change_set", id: resourceId }],
  };
  return targets[resourceKind] ?? null;
}

function eventTone(kind: string) {
  if (kind.includes("denied") || kind.includes("failed") || kind.includes("rejected")) return "risk";
  if (kind.includes("approval") || kind.includes("gate")) return "policy";
  if (kind.includes("tool") || kind.includes("run")) return "tool";
  return "audit";
}

function eventPayloadSummary(payload: Record<string, any> | null | undefined) {
  if (!payload || typeof payload !== "object") return "No payload";
  if (typeof payload.summary === "string") return payload.summary;
  if (typeof payload.reason === "string") return payload.reason;
  if (typeof payload.error === "string") return payload.error;
  const keys = Object.keys(payload);
  return keys.length ? keys.slice(0, 3).join(", ") : "Empty payload";
}

function AuditFilterSelect({ label, value, options, onChange }: { label: string; value: string; options: string[]; onChange: (value: string) => void }) {
  return <label><span>{label}</span><select value={value} onChange={(event) => onChange(event.target.value)}><option value="">All</option>{options.map((option) => <option key={option} value={option}>{option}</option>)}</select></label>;
}

export function AuditView({ dashboard, openRun, selectedSearch, scope, navigate }: AuditViewProps) {
  const emptyFilters = { search: selectedSearch ?? "", kind: "", actor: "", origin: "", resourceKind: "", resourceId: "", runId: "" };
  const [draftFilters, setDraftFilters] = useState(emptyFilters);
  const [filters, setFilters] = useState(emptyFilters);
  const [state, setState] = useState({ status: "loading", events: [] as any[], error: null as string | null });
  const [reloadToken, setReloadToken] = useState(0);

  useEffect(() => {
    const search = selectedSearch ?? "";
    setDraftFilters((current) => ({ ...current, search }));
    setFilters((current) => ({ ...current, search }));
  }, [selectedSearch]);

  useEffect(() => {
    let active = true;
    setState((current) => ({ ...current, status: current.events.length ? "refreshing" : "loading", error: null }));
    loadAuditEvents(filters, scope).then((events) => {
      if (active) setState({ status: "ready", events, error: null });
    }).catch((error) => {
      if (active) setState((current) => ({ ...current, status: "error", error: error instanceof Error ? error.message : String(error) }));
    });
    return () => { active = false; };
  }, [filters, scope, reloadToken]);

  const events = state.events;
  const latest = events[0];
  const resourceKinds = new Set(events.map((event) => event.resource_kind).filter(Boolean));
  const kindOptions = [...new Set((dashboard.data?.auditEvents ?? []).map((event: any) => event.kind).filter(Boolean))].sort() as string[];
  const actorOptions = [...new Set((dashboard.data?.auditEvents ?? []).map((event: any) => event.actor).filter(Boolean))].sort() as string[];
  const resourceKindOptions = [...new Set((dashboard.data?.auditEvents ?? []).map((event: any) => event.resource_kind).filter(Boolean))].sort() as string[];
  const originOptions = [...new Set([...(dashboard.data?.scopeOptions?.origins ?? []), ...events.map((event) => event.origin).filter(Boolean)])].sort() as string[];
  const metrics = [
    ["Events", String(events.length), "latest page"],
    ["Kinds", String(resourceKinds.size), "resource classes"],
    ["Run-linked", String(events.filter((event) => event.run_id).length), "execution context"],
    ["Latest", latest ? formatTimestamp(latest.created_at) : "none", "audit time"],
  ];

  return <section className="audit-view">
    <div className="section-heading"><div><h1>Audit</h1><p>Durable control-plane events from policy, approvals, grants, evidence, and SDLC state changes.</p></div><button className="primary-action" type="button" onClick={() => setReloadToken((value) => value + 1)} disabled={state.status === "refreshing"}><ArrowsClockwise size={17} /> {state.status === "refreshing" ? "Refreshing" : "Refresh"}</button></div>
    <div className="summary-grid">{metrics.map(([label, value, note]) => <div className="metric" key={label}><span>{label}</span><strong>{value}</strong><small>{note}</small></div>)}</div>
    <form className="audit-filters" onSubmit={(event) => { event.preventDefault(); setFilters({ ...draftFilters }); }}>
      <label className="audit-search-field"><span>Search</span><div><MagnifyingGlass size={16} /><input value={draftFilters.search} onChange={(event) => setDraftFilters((current) => ({ ...current, search: event.target.value }))} placeholder="Event, actor, resource, payload..." /></div></label>
      <AuditFilterSelect label="Kind" value={draftFilters.kind} options={kindOptions} onChange={(kind) => setDraftFilters((current) => ({ ...current, kind }))} />
      <AuditFilterSelect label="Resource" value={draftFilters.resourceKind} options={resourceKindOptions} onChange={(resourceKind) => setDraftFilters((current) => ({ ...current, resourceKind }))} />
      <AuditFilterSelect label="Actor" value={draftFilters.actor} options={actorOptions} onChange={(actor) => setDraftFilters((current) => ({ ...current, actor }))} />
      <AuditFilterSelect label="Origin" value={draftFilters.origin} options={originOptions} onChange={(origin) => setDraftFilters((current) => ({ ...current, origin }))} />
      <label><span>Run ID</span><input value={draftFilters.runId} onChange={(event) => setDraftFilters((current) => ({ ...current, runId: event.target.value }))} placeholder="run_..." /></label>
      <div className="audit-filter-actions"><button className="primary-action" type="submit"><MagnifyingGlass size={16} /> Apply</button><button type="button" onClick={() => { const cleared = { search: "", kind: "", actor: "", origin: "", resourceKind: "", resourceId: "", runId: "" }; setDraftFilters(cleared); setFilters(cleared); if (selectedSearch) navigate("Audit"); }}>Clear</button></div>
    </form>
    {state.error ? <div className="api-banner">Audit query failed: {state.error}</div> : null}
    {events.length ? <div className="audit-list">
      <div className="audit-head"><span>Event</span><span>Resource</span><span>Actor</span><span>Run</span><span>Payload</span><span>Time</span></div>
      {events.map((event) => {
        const target = navTargetForResource(event.resource_kind, event.resource_id);
        return <div className="audit-row" key={event.id}>
          <span><i className={`dot ${eventTone(event.kind)}`} /><strong title={event.kind}>{event.kind}</strong></span>
          <span title={`${event.resource_kind}/${event.resource_id}`}>{target ? <button className="link-text" type="button" onClick={() => navigate(target[0], target[1])}>{event.resource_kind}/{compactId(event.resource_id)}</button> : <>{event.resource_kind}/{compactId(event.resource_id)}</>}</span>
          <span>{event.actor ?? "system"}</span>
          <span>{event.run_id ? <button className="link-text" type="button" onClick={() => openRun(event.run_id)}>{compactId(String(event.run_id))}</button> : "none"}</span>
          <details className="audit-payload"><summary title={JSON.stringify(event.payload ?? {})}>{eventPayloadSummary(event.payload)}</summary><pre>{JSON.stringify(event.payload ?? {}, null, 2)}</pre></details>
          <time title={timestampTitle(event.created_at)}>{formatTimestamp(event.created_at)}</time>
        </div>;
      })}
    </div> : <EmptyState title="No matching audit events" body="Clear filters or generate control-plane activity, then run the query again." />}
  </section>;
}

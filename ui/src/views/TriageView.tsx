import { useState } from "react";
import { ArrowRight, ArrowsClockwise, WarningCircle } from "@phosphor-icons/react";
import { EmptyState, StatusPill } from "../components/Operational";
import { formatTimestamp, statusText, timestampTitle } from "../lib/formatters";
import { buildTriageThreads } from "../lib/triagePresentation";

type TriageViewProps = {
  dashboard: any;
  openResource: (view: string, id: string) => void;
};

export function TriageView({ dashboard, openResource }: TriageViewProps) {
  const data = dashboard.data;
  const [origin, setOrigin] = useState("");
  const threads = buildTriageThreads(data);
  const visibleItems = threads.filter((item) => !origin || item.origins.includes(origin));
  const summary = data?.triageSummary ?? data?.triage?.summary ?? {};

  return (
    <section className="triage-view">
      <div className="section-heading">
        <div><span className="eyebrow">Operate · exception inbox</span><h1>Triage</h1><p>One thread per WorkItem. Review its current boundary in the cockpit; standalone tool requests remain separate.</p></div>
        <button className="primary-action" type="button" onClick={dashboard.refresh} disabled={dashboard.status === "refreshing"}><ArrowsClockwise size={17} /> Refresh</button>
      </div>
      <section className="triage-summary-strip" aria-label="Attention summary">
        <div><span>Exception threads</span><strong>{threads.length}</strong><small>one operator path each</small></div>
        <div><span>Actionable signals</span><strong>{summary.total ?? threads.reduce((count, item) => count + item.signalCount, 0)}</strong><small>preserved, not hidden</small></div>
        <div><span>Blocked WorkItems</span><strong>{summary.blocked_work_items ?? 0}</strong><small>replan or corrective review</small></div>
        <div><span>Lifecycle gates</span><strong>{summary.pending_approval_gates ?? 0}</strong><small>review at owning boundary</small></div>
        <div><span>Tool requests</span><strong>{summary.pending_tool_approvals ?? 0}</strong><small>separate execution trust</small></div>
      </section>
      <div className="triage-toolbar"><div className="triage-filter-copy"><WarningCircle size={18} /><span><strong>Oldest exceptions first</strong><small>Absolute timestamps remain available on hover.</small></span></div><div className="triage-filters"><label>Origin<select value={origin} onChange={(event) => setOrigin(event.target.value)}><option value="">All origins</option>{(data?.scopeOptions?.origins ?? []).map((value: string) => <option key={value} value={value}>{value}</option>)}</select></label></div></div>
      {visibleItems.length ? <div className="triage-list">
        {visibleItems.map((item) => <button className="triage-row" type="button" key={`${item.kind}-${item.id}`} onClick={() => openResource(item.route[0], item.route[1])}>
          <span className={`dot ${item.tone === "high" ? "blocked" : "pending"}`} />
          <span><small>{item.kind} · {item.signalCount} signal{item.signalCount === 1 ? "" : "s"}</small><strong>{item.title}</strong><em>{item.detail}</em><span className="triage-signal-list">{item.signals.map((signal) => <small key={signal}>{signal}</small>)}</span><small>origin: {item.origin}</small></span>
          <StatusPill tone={item.tone === "high" ? "blocked" : "pending"}>{statusText(item.status)}</StatusPill>
          <span className="triage-row-tail"><time title={timestampTitle(item.createdAt)}>{formatTimestamp(item.createdAt)}</time><strong>{item.actionLabel} <ArrowRight size={13} /></strong></span>
        </button>)}
      </div> : <EmptyState title="Nothing needs attention" body="Pending approvals, governance gates, and blocked WorkItems will appear here." />}
    </section>
  );
}

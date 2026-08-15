import { useState } from "react";
import { ArrowsClockwise } from "@phosphor-icons/react";
import { EmptyState, StatusPill } from "../components/Operational";
import { formatTimestamp, riskTone, statusText, timestampTitle } from "../lib/formatters";

type TriageViewProps = {
  dashboard: any;
  openResource: (view: string, id: string) => void;
};

function resourceLabel(resource: any) {
  return [resource?.resource_kind, resource?.resource_name].filter(Boolean).join("/") || resource?.resource_namespace || "not scoped";
}

function approvalActionName(approval: any) {
  return approval?.action?.action ?? approval?.kind ?? "tool approval";
}

function approvalPreviewPath(approval: any) {
  return approval?.preview?.path ?? approval?.action?.path ?? "no preview path";
}

export function TriageView({ dashboard, openResource }: TriageViewProps) {
  const data = dashboard.data;
  const [origin, setOrigin] = useState("");
  const fallbackItems = [
    ...(data?.approvalGates ?? []).filter((gate: any) => gate.status === "pending").map((gate: any) => ({
      kind: "Approval gate", id: gate.id, title: gate.title ?? gate.gate_kind ?? "Approval required", detail: gate.summary ?? resourceLabel(gate), status: gate.status, tone: riskTone(gate.risk_level), origin: gate.origin ?? "legacy", createdAt: gate.created_at, route: ["Approval Gates", gate.id],
    })),
    ...(data?.approvals ?? []).filter((approval: any) => approval.status === "pending").map((approval: any) => ({
      kind: "Tool approval", id: approval.id, title: approvalActionName(approval), detail: approval.summary ?? approvalPreviewPath(approval), status: approval.status, tone: "medium", origin: approval.origin ?? "legacy", createdAt: approval.created_at, route: ["Approvals", approval.id],
    })),
    ...(data?.workItems ?? []).filter((item: any) => item.status === "blocked").map((item: any) => ({
      kind: "Blocked WorkItem", id: item.id, title: item.title, detail: item.status_reason ?? item.intent, status: item.status, tone: "high", origin: item.origin ?? "legacy", createdAt: item.status_changed_at ?? item.updated_at, route: ["WorkItems", item.id],
    })),
  ].sort((left, right) => Number(left.createdAt ?? 0) - Number(right.createdAt ?? 0));
  const apiItems = (data?.triage?.items ?? []).map((item: any) => ({
    kind: statusText(item.kind), id: item.id, title: item.title, detail: item.summary, status: item.status, tone: riskTone(item.risk_level), origin: item.origin ?? "legacy", createdAt: item.created_at,
    route: item.resource_kind === "work_item" || item.work_item_id ? ["WorkItems", item.work_item_id ?? item.resource_id]
      : item.resource_kind === "approval" ? ["Approvals", item.resource_id]
        : item.resource_kind === "approval_gate" ? ["Approval Gates", item.resource_id]
          : item.resource_kind === "remediation_plan" ? ["Remediation Plans", item.resource_id]
            : ["Audit", item.resource_id],
  }));
  const visibleItems = (apiItems.length ? apiItems : fallbackItems).filter((item) => !origin || item.origin === origin);

  return (
    <section className="triage-view">
      <div className="section-heading">
        <div><h1>Triage</h1><p>Items that need an operator decision or a controller response.</p></div>
        <button className="primary-action" type="button" onClick={dashboard.refresh} disabled={dashboard.status === "refreshing"}><ArrowsClockwise size={17} /> Refresh</button>
      </div>
      <div className="triage-filters"><label>Origin<select value={origin} onChange={(event) => setOrigin(event.target.value)}><option value="">All origins</option>{(data?.scopeOptions?.origins ?? []).map((value: string) => <option key={value} value={value}>{value}</option>)}</select></label></div>
      {visibleItems.length ? <div className="triage-list">
        {visibleItems.map((item) => <button className="triage-row" type="button" key={`${item.kind}-${item.id}`} onClick={() => openResource(item.route[0], item.route[1])}>
          <span className={`dot ${item.tone === "high" ? "blocked" : "pending"}`} />
          <span><small>{item.kind}</small><strong>{item.title}</strong><em>{item.detail}</em><small>origin: {item.origin}</small></span>
          <StatusPill tone={item.tone === "high" ? "blocked" : "pending"}>{statusText(item.status)}</StatusPill>
          <time title={timestampTitle(item.createdAt)}>{formatTimestamp(item.createdAt)}</time>
        </button>)}
      </div> : <EmptyState title="Nothing needs attention" body="Pending approvals, governance gates, and blocked WorkItems will appear here." />}
    </section>
  );
}

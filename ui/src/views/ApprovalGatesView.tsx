import { useEffect, useMemo, useState } from "react";
import { ArrowsClockwise, CheckCircle, FileText, ShieldWarning, X } from "@phosphor-icons/react";
import { CopyIdentifier, EmptyState, OperatorListFilters, ReviewItem, StatusPill } from "../components/Operational";
import { ServerGroups } from "../components/ServerGroups";
import { compactId, formatTimestamp, riskTone, statusText, timestampTitle } from "../lib/formatters";
import { operationalFilterOptions } from "../lib/operational";
import { resourceLabel } from "../lib/resourcePresentation";
import { batchDecideApprovalGates, decideApprovalGate, loadApprovalGates } from "../api/governance";
import { getOperatorName } from "../api/operator";
import { PaginationControls } from "../components/PaginationControls";

const PAGE_SIZE = 25;

type ApprovalGatesViewProps = {
  dashboard: any;
  selectedId: string | null;
  actionNotice: string | null;
  setActionNotice: (value: string) => void;
  navigate: (view: string, param?: string) => void;
  operatorName?: string;
};

export function ApprovalGatesView({ dashboard, selectedId, actionNotice, setActionNotice, navigate, operatorName }: ApprovalGatesViewProps) {
  const [listFilters, setListFilters] = useState({ search: "", status: "", actor: "", origin: "" });
  const [offset, setOffset] = useState(0);
  const [reload, setReload] = useState(0);
  const [listState, setListState] = useState<any>({ status: "loading", data: null, error: null });
  const dashboardGates = dashboard.data?.approvalGates ?? [];
  const routeSelected = dashboardGates.find((gate: any) => gate.id === selectedId) ?? null;
  const [gateFilter, setGateFilter] = useState(routeSelected && routeSelected.status !== "pending" ? "all" : "pending");
  const [selectedGateIds, setSelectedGateIds] = useState<Set<string>>(new Set());
  const [batchActor, setBatchActor] = useState(getOperatorName());
  const [batchReason, setBatchReason] = useState("");
  const [batchDecision, setBatchDecision] = useState("satisfied");
  const [decisionActor, setDecisionActor] = useState(getOperatorName());
  const [decisionReason, setDecisionReason] = useState("");
  const filterKey = JSON.stringify({ gateFilter, ...listFilters });
  const query = useMemo(() => ({
    search: listFilters.search,
    status: listFilters.status || (gateFilter === "pending" ? "pending" : undefined),
    actor: listFilters.actor,
    origin: listFilters.origin,
    limit: PAGE_SIZE,
    offset,
  }), [gateFilter, listFilters, offset]);
  useEffect(() => { setOffset(0); }, [filterKey]);
  useEffect(() => {
    let current = true;
    setListState((state: any) => ({ ...state, status: "loading", error: null }));
    loadApprovalGates(query).then((data) => {
      if (current) setListState({ status: "ready", data, error: null });
    }).catch((error) => {
      if (current) setListState({ status: "error", data: null, error: error instanceof Error ? error.message : String(error) });
    });
    return () => { current = false; };
  }, [query, reload]);
  const response = listState.data ?? { approval_gates: [], groups: [], count: 0, limit: PAGE_SIZE, offset };
  const gates = Array.isArray(response.approval_gates) ? response.approval_gates : [];
  const approvalGateGroups = Array.isArray(response.groups) ? response.groups : [];
  const filterOptions = operationalFilterOptions(gates, dashboard.data?.scopeOptions ?? {});
  const filtersActive = Object.values(listFilters).some(Boolean);
  const gateGroups = gates.reduce((groups: Record<string, any[]>, gate: any) => {
    const key = gate.remediation_plan_id ?? "ungrouped";
    (groups[key] = groups[key] ?? []).push(gate);
    return groups;
  }, {});
  const selectedGate = gates.find((gate: any) => gate.id === selectedId) ?? routeSelected ?? gates[0];
  const matchingCount = Number(response.count ?? 0);
  const gateIsActionable = (gate: any) => gate?.status === "pending" && gate?.actionable !== false;

  useEffect(() => {
    if (operatorName?.trim()) {
      setBatchActor((current) => current === "console-operator" ? operatorName.trim() : current);
      setDecisionActor((current) => current === "console-operator" ? operatorName.trim() : current);
    }
  }, [operatorName]);

  const decideGate = async (decision: string) => {
    if (!selectedGate || !decisionActor.trim() || !decisionReason.trim()) return;
    setActionNotice(`Deciding approval gate ${compactId(selectedGate.id)}...`);
    try {
      await decideApprovalGate(selectedGate.id, decision, decisionActor.trim(), decisionReason.trim());
      setDecisionReason("");
      setActionNotice(`Approval gate ${decision}: ${selectedGate.title}`);
      await dashboard.refresh();
      setReload((value) => value + 1);
    } catch (error) {
      setActionNotice(`Approval gate decision failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  };

  const toggleBatchGate = (gateId: string) => setSelectedGateIds((current) => {
    const next = new Set(current);
    if (next.has(gateId)) next.delete(gateId);
    else next.add(gateId);
    return next;
  });

  const toggleGateGroup = (gatesInGroup: any[]) => setSelectedGateIds((current) => {
    const pendingIds = gatesInGroup.filter(gateIsActionable).map((gate) => gate.id);
    const shouldSelect = pendingIds.some((gateId) => !current.has(gateId));
    const next = new Set(current);
    for (const gateId of pendingIds) shouldSelect ? next.add(gateId) : next.delete(gateId);
    return next;
  });

  const decideBatch = async () => {
    const gateIds = [...selectedGateIds];
    if (!gateIds.length || !batchActor.trim() || !batchReason.trim()) return;
    setActionNotice(`Deciding ${gateIds.length} approval gate${gateIds.length === 1 ? "" : "s"}...`);
    try {
      await batchDecideApprovalGates(gateIds, batchDecision, batchActor.trim(), batchReason.trim());
      setSelectedGateIds(new Set());
      setBatchReason("");
      setActionNotice(`${gateIds.length} approval gates ${batchDecision} and individually audited.`);
      await dashboard.refresh();
      setReload((value) => value + 1);
    } catch (error) {
      setActionNotice(`Batch decision failed without changes: ${error instanceof Error ? error.message : String(error)}`);
    }
  };

  return (
    <section className="gate-view">
      <div className="section-heading">
        <div><h1>Approval Gates</h1><p>Governance and release-state review. Gates do not authorize tool execution by themselves.</p></div>
        <div className="detail-actions">
          <StatusPill tone={matchingCount ? "pending" : "healthy"}>{matchingCount} matching</StatusPill>
          <div className="filter-row" role="tablist" aria-label="Gate status filter">{["pending", "all"].map((option) => <button key={option} type="button" className={gateFilter === option ? "selected" : ""} onClick={() => setGateFilter(option)}>{option}</button>)}</div>
          <button className="primary-action" type="button" onClick={() => setReload((value) => value + 1)} disabled={listState.status === "loading"}><ArrowsClockwise size={17} /> {listState.status === "loading" ? "Refreshing" : "Refresh"}</button>
        </div>
      </div>
      {actionNotice ? <span className="action-notice">{actionNotice}</span> : null}
      <OperatorListFilters value={listFilters} statuses={filterOptions.statuses} actors={filterOptions.actors} origins={filterOptions.origins} onChange={setListFilters} />
      <PaginationControls label="Approval gates" count={matchingCount} limit={response.limit ?? PAGE_SIZE} offset={response.offset ?? offset} loading={listState.status === "loading"} onOffsetChange={setOffset} />
      {listState.error ? <div className="api-banner">Unable to load approval gates: {listState.error}</div> : null}
      {!filtersActive ? <ServerGroups label="approval gates" groups={approvalGateGroups} onOpen={(id) => navigate("Approval Gates", id)} /> : null}
      {selectedGateIds.size ? <section className="batch-gate-panel" aria-label="Batch gate decision">
        <div><span className="eyebrow">Batch decision</span><strong>{selectedGateIds.size} pending gate{selectedGateIds.size === 1 ? "" : "s"} selected</strong></div>
        <label>Decision<select value={batchDecision} onChange={(event) => setBatchDecision(event.target.value)}><option value="satisfied">Satisfy</option><option value="waived">Waive</option><option value="rejected">Reject</option></select></label>
        <label>Operator<input value={batchActor} onChange={(event) => setBatchActor(event.target.value)} /></label>
        <label>Reason<input value={batchReason} onChange={(event) => setBatchReason(event.target.value)} placeholder="Required for every selected gate" /></label>
        <button className="primary-action" type="button" disabled={!batchActor.trim() || !batchReason.trim()} onClick={decideBatch}>Apply to selected</button>
      </section> : null}
      {gates.length ? <div className="gate-layout">
        <div className="approval-stack">
          {Object.entries(gateGroups).map(([planId, planGates]) => <div className="gate-group" key={planId}>
            <div className="gate-group-heading"><button className="gate-group-title" type="button" title={planId} onClick={() => planId !== "ungrouped" && navigate("Remediation Plans", planId)}>plan {compactId(planId)} · {planGates.length} gate{planGates.length === 1 ? "" : "s"}</button>{planGates.some(gateIsActionable) ? <button type="button" onClick={() => toggleGateGroup(planGates)}>Select actionable group</button> : null}</div>
            {planGates.map((gate: any) => <div className="gate-selectable" key={gate.id}>
              {gate.status === "pending" ? <label title={gateIsActionable(gate) ? `Select ${gate.title} for a batch decision` : (gate.lifecycle_blocker ?? "This future gate is not at its lifecycle boundary.")}><input type="checkbox" checked={selectedGateIds.has(gate.id)} disabled={!gateIsActionable(gate)} onChange={() => toggleBatchGate(gate.id)} /><span className="sr-only">Select {gate.title}</span></label> : <span className="gate-select-spacer" />}
              <button className={`approval-card ${gate.id === selectedGate?.id ? "is-active" : ""}`} type="button" onClick={() => navigate("Approval Gates", gate.id)}><span>{gate.gate_kind} · {statusText(gate.status)}</span><strong>{gate.title}</strong><small>{resourceLabel(gate)} · {compactId(gate.id)}</small><b>{gate.risk_level}</b></button>
            </div>)}
          </div>)}
        </div>
        <div className="review-surface">
          <h2>{selectedGate?.title ?? "Approval gate"}</h2><p>{selectedGate?.summary ?? "Governance state for the selected SDLC resource. Satisfy, waive, or reject this gate after evidence review."}</p>
          <div className="review-grid">
            <ReviewItem label="Gate" value={<CopyIdentifier value={selectedGate?.id} label={selectedGate?.title} />} />
            <ReviewItem label="Status" value={statusText(selectedGate?.status)} tone={selectedGate?.status === "pending" ? "pending" : undefined} />
            <ReviewItem label="Risk" value={statusText(selectedGate?.risk_level, "Unknown")} tone={riskTone(selectedGate?.risk_level) === "high" ? "risk" : "pending"} />
            <ReviewItem label="Gate kind" value={selectedGate?.gate_kind ?? "unknown"} /><ReviewItem label="Gate order" value={selectedGate?.gate_order ?? "unknown"} />
            <ReviewItem label="Resource" value={resourceLabel(selectedGate)} /><ReviewItem label="Requested" value={<time title={timestampTitle(selectedGate?.created_at)}>{formatTimestamp(selectedGate?.created_at)}</time>} />
            <ReviewItem label="Remediation plan" value={<button className="link-text" type="button" onClick={() => navigate("Remediation Plans", selectedGate?.remediation_plan_id)}>{compactId(selectedGate?.remediation_plan_id)}</button>} />
            <ReviewItem label="Incident" value={<button className="link-text" type="button" onClick={() => navigate("Incidents", selectedGate?.incident_id)}>{compactId(selectedGate?.incident_id)}</button>} />
            {selectedGate?.decided_at ? <ReviewItem label="Decided" value={<>{selectedGate?.decided_by ?? "unknown"} · <time title={timestampTitle(selectedGate?.decided_at)}>{formatTimestamp(selectedGate?.decided_at)}</time></>} /> : null}
            {selectedGate?.decision_reason ? <ReviewItem label="Reason" value={selectedGate.decision_reason} /> : null}
            {selectedGate?.stale_at ? <ReviewItem label="Stale" value={<>{selectedGate?.stale_reason ?? "superseded"} · <time title={timestampTitle(selectedGate?.stale_at)}>{formatTimestamp(selectedGate?.stale_at)}</time></>} tone="pending" /> : null}
          </div>
          {selectedGate?.status === "pending" && selectedGate?.actionable === false ? <div className="api-banner" role="status"><strong>Future lifecycle gate</strong><br />{selectedGate.lifecycle_blocker ?? "This gate becomes actionable only when its declared lifecycle boundary is reached."}</div> : null}
          <div className="diff-box"><div><FileText size={18} /> gate payload · plan {compactId(selectedGate?.remediation_plan_id)}</div><pre>{JSON.stringify(selectedGate?.gate_json ?? {}, null, 2)}</pre></div>
          <div className="gate-decision-context"><label>Operator<input value={decisionActor} onChange={(event) => setDecisionActor(event.target.value)} /></label><label>Rationale<textarea rows={2} value={decisionReason} onChange={(event) => setDecisionReason(event.target.value)} placeholder="Required and written to the audit record" /></label></div>
          <div className="decision-row">
            <button className="approve" type="button" disabled={!gateIsActionable(selectedGate) || !decisionActor.trim() || !decisionReason.trim()} onClick={() => decideGate("satisfied")}><CheckCircle size={18} /> Satisfy</button>
            <button className="waive" type="button" disabled={!gateIsActionable(selectedGate) || !decisionActor.trim() || !decisionReason.trim()} onClick={() => decideGate("waived")}><ShieldWarning size={18} /> Waive</button>
            <button className="deny" type="button" disabled={!gateIsActionable(selectedGate) || !decisionActor.trim() || !decisionReason.trim()} onClick={() => decideGate("rejected")}><X size={18} /> Reject</button>
          </div>
        </div>
      </div> : listState.status === "loading" ? <EmptyState title="Loading approval gates" body="Reading durable governance records from the control plane." /> : (listFilters.search || listFilters.status || listFilters.actor || listFilters.origin) ? <EmptyState title="No approval gates match these filters" body="Clear or adjust filters to review other durable governance decisions." /> : gateFilter === "pending" ? <EmptyState title="No pending approval gates" body="Decided and stale gates are available under the all filter." /> : <EmptyState title="No approval gates" body="Release, deployment, and remediation gates will appear here when governance state exists." />}
    </section>
  );
}

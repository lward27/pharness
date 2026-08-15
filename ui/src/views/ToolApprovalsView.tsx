import { useEffect, useMemo, useState } from "react";
import { ArrowsClockwise, CheckCircle, FileText, X } from "@phosphor-icons/react";
import { CopyIdentifier, EmptyState, OperatorListFilters, ReviewItem, StatusPill } from "../components/Operational";
import { compactId, formatTimestamp, riskTone, statusText, timestampTitle } from "../lib/formatters";
import { operationalFilterOptions } from "../lib/operational";
import { approvalActionName, approvalPreviewDiff, approvalPreviewPath } from "../lib/resourcePresentation";
import { decideApproval, loadApprovals } from "../api/governance";
import { PaginationControls } from "../components/PaginationControls";

const PAGE_SIZE = 25;

type ToolApprovalsViewProps = {
  dashboard: any;
  selectedId: string | null;
  actionNotice: string | null;
  setActionNotice: (value: string) => void;
  openRun: (runId: string) => void;
  navigate: (view: string, param?: string) => void;
};

export function ToolApprovalsView({
  dashboard,
  selectedId,
  actionNotice,
  setActionNotice,
  openRun,
  navigate,
}: ToolApprovalsViewProps) {
  const [listFilters, setListFilters] = useState({ search: "", status: "", actor: "", origin: "" });
  const [offset, setOffset] = useState(0);
  const [reload, setReload] = useState(0);
  const [listState, setListState] = useState<any>({ status: "loading", data: null, error: null });
  const dashboardApprovals = dashboard.data?.approvals ?? [];
  const routeSelected = dashboardApprovals.find((approval: any) => approval.id === selectedId) ?? null;
  const [approvalFilter, setApprovalFilter] = useState(routeSelected && routeSelected.status !== "pending" ? "all" : "pending");
  const filterKey = JSON.stringify({ approvalFilter, ...listFilters });
  const query = useMemo(() => ({
    search: listFilters.search,
    status: listFilters.status || (approvalFilter === "pending" ? "pending" : undefined),
    actor: listFilters.actor,
    origin: listFilters.origin,
    limit: PAGE_SIZE,
    offset,
  }), [approvalFilter, listFilters, offset]);

  useEffect(() => { setOffset(0); }, [filterKey]);
  useEffect(() => {
    let current = true;
    setListState((state: any) => ({ ...state, status: "loading", error: null }));
    loadApprovals(query).then((data) => {
      if (current) setListState({ status: "ready", data, error: null });
    }).catch((error) => {
      if (current) setListState({ status: "error", data: null, error: error instanceof Error ? error.message : String(error) });
    });
    return () => { current = false; };
  }, [query, reload]);

  const response = listState.data ?? { approvals: [], count: 0, limit: PAGE_SIZE, offset, groups: [] };
  const approvals = Array.isArray(response.approvals) ? response.approvals : [];
  const selectedApproval = approvals.find((approval: any) => approval.id === selectedId) ?? routeSelected ?? approvals[0];
  const filterOptions = operationalFilterOptions(approvals, dashboard.data?.scopeOptions ?? {});
  const matchingCount = Number(response.count ?? 0);

  const decideToolApproval = async (decision: string) => {
    if (!selectedApproval) return;
    setActionNotice(`Deciding tool approval ${compactId(selectedApproval.id)}...`);
    try {
      await decideApproval(selectedApproval.id, decision);
      setActionNotice(`Tool approval ${decision}: ${approvalActionName(selectedApproval)}`);
      await dashboard.refresh();
      setReload((value) => value + 1);
    } catch (error) {
      setActionNotice(`Tool approval failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  };

  return (
    <section className="gate-view">
      <div className="section-heading">
        <div><h1>Tool Approvals</h1><p>Execution decisions for proposed tool actions. These authorize or deny a paused run action.</p></div>
        <div className="detail-actions">
          <StatusPill tone={matchingCount ? "pending" : "healthy"}>{matchingCount} matching</StatusPill>
          <div className="filter-row" role="tablist" aria-label="Approval status filter">
            {["pending", "all"].map((option) => <button key={option} type="button" className={approvalFilter === option ? "selected" : ""} onClick={() => setApprovalFilter(option)}>{option}</button>)}
          </div>
          <button className="primary-action" type="button" onClick={() => setReload((value) => value + 1)} disabled={listState.status === "loading"}><ArrowsClockwise size={17} /> {listState.status === "loading" ? "Refreshing" : "Refresh"}</button>
        </div>
      </div>
      {actionNotice ? <span className="action-notice">{actionNotice}</span> : null}
      <OperatorListFilters value={listFilters} statuses={filterOptions.statuses} actors={filterOptions.actors} origins={filterOptions.origins} onChange={setListFilters} />
      <PaginationControls label="Tool approvals" count={matchingCount} limit={response.limit ?? PAGE_SIZE} offset={response.offset ?? offset} loading={listState.status === "loading"} onOffsetChange={setOffset} />
      {listState.error ? <div className="api-banner">Unable to load tool approvals: {listState.error}</div> : null}
      {approvals.length ? <div className="gate-layout">
        <div className="approval-stack">
          {approvals.map((approval: any) => <button className={`approval-card ${approval.id === selectedApproval?.id ? "is-active" : ""}`} type="button" key={approval.id} onClick={() => navigate("Approvals", approval.id)}>
            <span>{approval.kind} · {statusText(approval.status)}</span><strong>{approval.summary}</strong><small>{compactId(approval.id)} · {compactId(String(approval.run_id))}</small><b>{approval.risk_level}</b>
          </button>)}
        </div>
        <div className="review-surface">
          <h2>Approve {approvalActionName(selectedApproval)}</h2>
          <p>This decision resumes or blocks the current run action. It is not a release gate and does not satisfy governance state.</p>
          <div className="review-grid">
            <ReviewItem label="Approval" value={<CopyIdentifier value={selectedApproval?.id} label={approvalActionName(selectedApproval)} />} />
            <ReviewItem label="Tool action" value={approvalActionName(selectedApproval)} />
            <ReviewItem label="Run" value={compactId(String(selectedApproval?.run_id))} />
            <ReviewItem label="Risk" value={statusText(selectedApproval?.risk_level, "Unknown")} tone={riskTone(selectedApproval?.risk_level) === "high" ? "risk" : "pending"} />
            <ReviewItem label="Status" value={statusText(selectedApproval?.status)} tone={selectedApproval?.status === "pending" ? "pending" : undefined} />
            <ReviewItem label="Requested" value={<time title={timestampTitle(selectedApproval?.requested_at)}>{formatTimestamp(selectedApproval?.requested_at)}</time>} />
            {selectedApproval?.decided_at ? <ReviewItem label="Decided" value={<>{selectedApproval?.decided_by ?? "unknown"} · <time title={timestampTitle(selectedApproval?.decided_at)}>{formatTimestamp(selectedApproval?.decided_at)}</time></>} /> : null}
            {selectedApproval?.decision_reason ? <ReviewItem label="Reason" value={selectedApproval.decision_reason} /> : null}
          </div>
          <div className="diff-box"><div><FileText size={18} /> {approvalPreviewPath(selectedApproval)}</div><pre>{approvalPreviewDiff(selectedApproval)}</pre></div>
          <div className="decision-row">
            <button className="approve" type="button" disabled={selectedApproval?.status !== "pending"} onClick={() => decideToolApproval("approved")}><CheckCircle size={18} /> Approve</button>
            <button className="deny" type="button" disabled={selectedApproval?.status !== "pending"} onClick={() => decideToolApproval("denied")}><X size={18} /> Deny</button>
            <button type="button" disabled={!selectedApproval?.run_id} onClick={() => selectedApproval?.run_id && openRun(selectedApproval.run_id)}><FileText size={18} /> Open run</button>
          </div>
        </div>
      </div> : listState.status === "loading" ? <EmptyState title="Loading tool approvals" body="Reading durable approval records from the control plane." /> : (listFilters.search || listFilters.status || listFilters.actor || listFilters.origin) ? <EmptyState title="No tool approvals match these filters" body="Clear or adjust filters to review other durable execution decisions." /> : approvalFilter === "pending" ? <EmptyState title="No pending tool approvals" body="Decided approvals are available under the all filter." /> : <EmptyState title="No tool approvals" body="Paused write, shell, and network actions will appear here when a run requests human review." />}
    </section>
  );
}

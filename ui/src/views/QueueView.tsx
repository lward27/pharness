import { useEffect, useMemo, useState, type FormEvent } from "react";
import { ArrowsClockwise, RocketLaunch } from "@phosphor-icons/react";
import { EmptyState, OperatorListFilters, StatusPill } from "../components/Operational";
import { PaginationControls } from "../components/PaginationControls";
import { ServerGroups } from "../components/ServerGroups";
import { compactId, formatTimestamp, lifecycleTone, statusText, timestampTitle } from "../lib/formatters";
import { operationalFilterOptions } from "../lib/operational";
import { cancelRun, loadRuns, submitRun } from "../api/runs";

const PAGE_SIZE = 25;

type QueueViewProps = {
  dashboard: any;
  scope: any;
  autoRefresh: boolean;
  openRun: (runId: string) => void;
};

function statusCount(summary: any, status: string) {
  return summary?.by_status?.find((item: any) => item.value === status)?.count ?? 0;
}

function runScopeLabel(scope: any) {
  if (!scope) return "unscoped";
  return [scope.namespace, scope.repo, scope.branch].filter(Boolean).map(compactId).join(" / ") || "unscoped";
}

function canCancelRun(run: any) {
  return Boolean(run?.status) && !["completed", "failed", "cancelled"].includes(run.status);
}

export function QueueView({ dashboard, scope, autoRefresh, openRun }: QueueViewProps) {
  const scopeOptions = dashboard.data?.scopeOptions ?? {};
  const [listFilters, setListFilters] = useState({ search: "", status: "", actor: "", origin: "" });
  const [offset, setOffset] = useState(0);
  const [reload, setReload] = useState(0);
  const [listState, setListState] = useState<any>({ status: "loading", data: null, error: null });
  const query = useMemo(() => ({ ...listFilters, limit: PAGE_SIZE, offset }), [listFilters, offset]);
  const scopeKey = JSON.stringify(scope);
  const filterKey = JSON.stringify(listFilters);

  useEffect(() => { setOffset(0); }, [filterKey, scopeKey]);
  useEffect(() => {
    let current = true;
    const refresh = () => {
      setListState((value: any) => ({ ...value, status: value.data ? "refreshing" : "loading", error: null }));
      loadRuns(query, scope).then((data) => {
        if (current) setListState({ status: "ready", data, error: null });
      }).catch((error) => {
        if (current) setListState((value: any) => ({ ...value, status: "error", error: error instanceof Error ? error.message : String(error) }));
      });
    };
    refresh();
    if (!autoRefresh) return () => { current = false; };
    const timer = window.setInterval(() => {
      if (document.visibilityState === "visible") refresh();
    }, 15_000);
    return () => { current = false; window.clearInterval(timer); };
  }, [query, scopeKey, reload, autoRefresh]);

  const response = listState.data ?? { runs: [], groups: [], count: 0, limit: PAGE_SIZE, offset };
  const runs = Array.isArray(response.runs) ? response.runs : [];
  const runGroups = Array.isArray(response.groups) ? response.groups : [];
  const filterOptions = operationalFilterOptions(runs, scopeOptions);
  const filtersActive = Object.values(listFilters).some(Boolean);
  const summary = dashboard.data?.runsSummary?.summary;
  const triage = dashboard.data?.triageSummary ?? {};
  const workerEnabled = Boolean(dashboard.data?.config?.worker?.enabled);
  const [task, setTask] = useState("List the top-level files, then finish with one sentence.");
  const [cwd, setCwd] = useState(".");
  const [maxTurns, setMaxTurns] = useState(20);
  const [queueNotice, setQueueNotice] = useState("");
  const metrics = [
    ["Running", String(statusCount(summary, "running")), "execution state"],
    ["Approval required", String(statusCount(summary, "approval_required")), "execution state"],
    ["Tool approvals", String(triage.pending_tool_approvals ?? 0), "governance state"],
    ["Approval gates", String(triage.pending_approval_gates ?? 0), "governance state"],
  ];

  const handleSubmitRun = async (event: FormEvent) => {
    event.preventDefault();
    const trimmedTask = task.trim();
    if (!trimmedTask) { setQueueNotice("Task is required."); return; }
    if (!workerEnabled) { setQueueNotice("Worker is disabled. Start the API with a configured model provider before submitting runs."); return; }
    setQueueNotice("Submitting run...");
    try {
      const run = await submitRun({ task: trimmedTask, cwd: cwd.trim() || ".", maxTurns });
      setQueueNotice(`Run submitted: ${compactId(String(run.id))}`);
      openRun(run.id);
      await dashboard.refresh();
    } catch (error) {
      setQueueNotice(`Run submit failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  };

  const handleCancelRun = async (runId: string) => {
    setQueueNotice(`Cancelling ${compactId(runId)}...`);
    try {
      await cancelRun(runId);
      setQueueNotice(`Cancel requested: ${compactId(runId)}`);
      setReload((value) => value + 1);
      await dashboard.refresh();
    } catch (error) {
      setQueueNotice(`Cancel failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  };

  return (
    <section className="queue-view">
      <div className="section-heading">
        <div><h1>Run Queue</h1><p>Execution state and governance state are reported separately.</p></div>
        <button className="primary-action" type="button" onClick={() => setReload((value) => value + 1)} disabled={listState.status === "refreshing"}><ArrowsClockwise size={17} /> {listState.status === "refreshing" ? "Refreshing" : "Refresh"}</button>
      </div>
      <div className="summary-grid">{metrics.map(([label, value, note]) => <div className="metric" key={label}><span>{label}</span><strong>{value}</strong><small>{note}</small></div>)}</div>
      <form className="run-submit" onSubmit={handleSubmitRun}>
        {!workerEnabled ? <div className="inline-warning">Worker disabled. The API can queue runs, but it will not execute them until a model-backed worker is enabled.</div> : null}
        <label><span>Task</span><textarea value={task} onChange={(event) => setTask(event.target.value)} rows={3} /></label>
        <div className="run-submit-grid"><label><span>CWD</span><input value={cwd} onChange={(event) => setCwd(event.target.value)} /></label><label><span>Max turns</span><input min="1" max="80" type="number" value={maxTurns} onChange={(event) => setMaxTurns(Number(event.target.value) || 1)} /></label><button className="primary-action" type="submit" disabled={!workerEnabled}><RocketLaunch size={17} /> Submit run</button></div>
        {queueNotice ? <span className="action-notice">{queueNotice}</span> : null}
      </form>
      <OperatorListFilters value={listFilters} statuses={filterOptions.statuses} actors={filterOptions.actors} origins={filterOptions.origins} onChange={setListFilters} />
      <PaginationControls label="Runs" count={Number(response.count ?? 0)} limit={response.limit ?? PAGE_SIZE} offset={response.offset ?? offset} loading={listState.status === "loading" || listState.status === "refreshing"} onOffsetChange={setOffset} />
      {listState.error ? <div className="api-banner">Unable to load runs: {listState.error}</div> : null}
      {!filtersActive ? <ServerGroups label="runs" groups={runGroups} onOpen={openRun} /> : null}
      {runs.length ? <div className="run-list">{runs.map((run: any) => <div className="run-row" key={run.id}>
        <span><strong>{run.task}</strong><small title={run.id}>{compactId(run.id)}</small></span><span>{runScopeLabel(run.scope)}</span><StatusPill tone={run.status === "approval_required" ? "pending" : lifecycleTone(run.status)}>{statusText(run.status)}</StatusPill><span>{run.result?.turns != null ? `${run.result.turns} turns` : "-"}</span><span><time title={timestampTitle(run.started_at)}>{formatTimestamp(run.started_at)}</time>{run.finished_at ? <small title={timestampTitle(run.finished_at)}>finished {formatTimestamp(run.finished_at)}</small> : null}</span><span className="row-actions"><button type="button" onClick={() => openRun(run.id)}>Open</button><button className="deny" type="button" disabled={!canCancelRun(run)} onClick={() => handleCancelRun(run.id)}>Cancel</button></span>{!workerEnabled && run.status === "queued" ? <small className="run-warning">worker disabled</small> : null}
      </div>)}</div> : listState.status === "loading" ? <EmptyState title="Loading runs" body="Reading durable execution records." /> : <EmptyState title="No runs match these filters" body="Clear or adjust filters to review other durable execution records." />}
    </section>
  );
}

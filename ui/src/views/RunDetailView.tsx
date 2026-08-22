import { useEffect, useRef, useState } from "react";
import { ArrowsClockwise, CheckCircle, FileText, Rows, Warning, X } from "@phosphor-icons/react";
import { CopyIdentifier, EmptyState, ReviewItem, StatusPill } from "../components/Operational";
import { compactId, formatTimestamp, lifecycleTone, statusText, timestampTitle } from "../lib/formatters";
import { acceptanceRows, budgetMetric, changedPaths, formatRunDuration, isActiveRun, workspaceEvents } from "../lib/runWorkspace";
import { cancelRun, decideRunApproval, loadRunDetail, subscribeRunEvents } from "../api/runs";

type RunDetailViewProps = {
  runId?: string | null;
  refreshDashboard?: () => Promise<unknown> | void;
  onOpenQueue: () => void;
  operatorName?: string;
  embedded?: boolean;
};

function canCancelRun(run: any) {
  return Boolean(run?.status) && !["completed", "failed", "cancelled"].includes(run.status);
}

function runScopeLabel(scope: any) {
  if (!scope) return "unscoped";
  return [scope.namespace, scope.repo, scope.branch].filter(Boolean).map(compactId).join(" / ") || "unscoped";
}

function eventTone(kind?: string) {
  if (kind?.includes("approval") || kind?.includes("gate") || kind?.includes("stale")) return "policy";
  if (kind?.includes("run") || kind?.includes("tool")) return "tool";
  return "audit";
}

function eventPayloadSummary(payload: any) {
  if (!payload || typeof payload !== "object") return "no payload";
  if (typeof payload.summary === "string") return payload.summary;
  if (typeof payload.error === "string") return payload.error;
  if (typeof payload.action === "string") return payload.reason ? `${payload.action}: ${payload.reason}` : payload.action;
  if (typeof payload.raw_provider_id === "string") return compactId(payload.raw_provider_id);
  const keys = Object.keys(payload);
  return keys.length ? keys.slice(0, 4).join(", ") : "empty payload";
}

function artifactSummary(artifact: any) {
  if (typeof artifact.content_text === "string" && artifact.content_text.trim()) return artifact.content_text.trim().slice(0, 180);
  if (artifact.content_json && typeof artifact.content_json === "object") {
    const keys = Object.keys(artifact.content_json);
    return keys.length ? keys.slice(0, 4).join(", ") : "structured artifact";
  }
  return artifact.path ?? "metadata only";
}

function mergeRunEvent(detail: any, runId: string, event: any) {
  const base = detail ?? { run: null, events: [], diff: { run_id: runId, changes: [], diff: "" }, artifacts: [] };
  const eventKey = event.event_id ?? `${event.seq}-${event.type}`;
  const existing = new Set(base.events.map((item: any) => item.event_id ?? `${item.seq}-${item.type}`));
  if (existing.has(eventKey)) return base;
  return { ...base, events: [...base.events, event].sort((left: any, right: any) => Number(left.seq ?? 0) - Number(right.seq ?? 0)) };
}

function latestEventSeq(events: any[]) {
  return (events ?? []).reduce((latest, event) => {
    const seq = Number(event.seq ?? 0);
    return Number.isFinite(seq) && seq > latest ? seq : latest;
  }, 0);
}

function isTerminalEvent(event: any) { return ["run.finished", "run.failed", "run.cancelled"].includes(event?.type); }
function isTerminalStatus(status?: string) { return ["completed", "failed", "cancelled", "approval_required", "budget_extension_required"].includes(status ?? ""); }
function eventShouldRefreshRunDetail(event: any) { return ["run.finished", "run.failed", "run.cancelled", "approval.required", "approval.decided", "tool.finished"].includes(event?.type); }

function streamLabel(streamState: any) {
  if (streamState.status === "connecting") return "Connecting";
  if (streamState.status === "live") return "Live events";
  if (streamState.status === "closed") return "Stream closed";
  if (streamState.status === "error") return "Stream disconnected";
  return "Stream idle";
}

function streamDescription(streamState: any) {
  if (streamState.status === "connecting") return "Opening the API-backed event stream from the latest durable event cursor.";
  if (streamState.status === "live") return "Receiving new durable events from the API stream.";
  if (streamState.status === "closed") return "Run is terminal or paused at an approval boundary; the event log is a durable snapshot.";
  if (streamState.status === "error") return streamState.error ?? "The event stream disconnected.";
  return "Waiting for a selected run and its durable event cursor.";
}

function StreamStatusPanel({ streamState, eventCount, cursor, run }: any) {
  return <div className={`attempt-stream-status stream-panel-${streamState.status}`}>
    <span className={`stream-chip stream-${streamState.status}`}><i className={`dot ${streamState.status === "error" ? "blocked" : streamState.status === "live" ? "running" : "future"}`} />{streamLabel(streamState)}</span>
    <p>{streamDescription(streamState)}</p>
    <small>{eventCount} durable events · {cursor === null ? "terminal snapshot" : `replay after ${cursor}`} · {statusText(run?.status, "loading")}</small>
  </div>;
}

function BudgetPanel({ run, operatorSummary }: { run: any; operatorSummary: any }) {
  const budget = run?.run_budget ?? {};
  const consumed = run?.budget_consumption ?? {};
  const turns = budgetMetric(consumed.turns_used ?? operatorSummary?.turns, consumed.allowed_turns ?? budget.initial_turns);
  const tokens = budgetMetric(consumed.tokens_used ?? operatorSummary?.actual_total_tokens, consumed.allowed_tokens ?? budget.initial_tokens);
  const activeTime = budgetMetric(consumed.active_execution_seconds_used, budget.active_execution_seconds);
  const metrics = [
    { label: "Turns", metric: turns, used: String(turns.used), remaining: String(turns.remaining), limit: `${turns.limit} allowed` },
    { label: "Tokens", metric: tokens, used: turns.limit ? tokens.used.toLocaleString() : String(tokens.used), remaining: tokens.remaining.toLocaleString(), limit: `${tokens.limit.toLocaleString()} allowed` },
    { label: "Active time", metric: activeTime, used: formatRunDuration(activeTime.used), remaining: formatRunDuration(activeTime.remaining), limit: `${formatRunDuration(activeTime.limit)} allowed` },
  ];
  return <section className="attempt-budget-panel" aria-label="Run budget">
    <div className="attempt-panel-heading"><div><span className="eyebrow">Execution budget</span><h3>Capacity for this workspace</h3></div><small>{operatorSummary?.budget_extensions ?? consumed.extensions ?? 0} extensions</small></div>
    <div className="attempt-budget-grid">{metrics.map(({ label, metric, used, remaining, limit }) => <article className={`budget-meter tone-${metric.tone}`} key={label}>
      <div><strong>{label}</strong><small>{limit}</small></div>
      <progress max={100} value={metric.percent} aria-label={`${label} budget used`} />
      <p><b>{used} used · {remaining} remaining</b></p>
    </article>)}</div>
  </section>;
}

function EnvironmentPanel({ preparation, active }: { preparation: any; active: boolean }) {
  if (!preparation?.status) return <section className="attempt-environment-panel is-pending"><div className="attempt-panel-heading"><div><span className="eyebrow">Environment readiness</span><h3>Preparation evidence pending</h3></div><StatusPill tone="pending">Pending</StatusPill></div><p>The model will not start until the isolated runner, immutable source, executables, and dependency preparation have been recorded.</p></section>;
  const snapshot = preparation.environment_snapshot ?? {};
  const contract = preparation.project_contract ?? {};
  const logs = Array.isArray(preparation.logs) ? preparation.logs : [];
  return <details className="attempt-environment-panel" open={active}>
    <summary><div><span className="eyebrow">Environment readiness</span><h3>{statusText(preparation.status, "Not prepared")}</h3><p>{preparation.environment_profile_id ?? "Runner unavailable"} · {snapshot.python_version ?? "Python pending"}</p></div><StatusPill tone={preparation.status === "succeeded" ? "healthy" : preparation.status === "failed" ? "risk" : "pending"}>{statusText(preparation.status)}</StatusPill></summary>
    <div className="environment-fact-grid">
      <ReviewItem label="Pinned source" value={preparation.source_commit ?? "Unavailable"} />
      <ReviewItem label="Runner digest" value={snapshot.runner_image_digest ?? "Pending"} />
      <ReviewItem label="Python" value={snapshot.python_path ? `${snapshot.python_version} · ${snapshot.python_path}` : "Pending"} />
      <ReviewItem label="Writable paths" value={snapshot.writable_paths?.join(" · ") ?? contract.writable_paths?.join(" · ") ?? "Pending"} />
      <ReviewItem label="Network policy" value={snapshot.agent_network_policy ?? contract.agent_network ?? "Undeclared"} />
      <ReviewItem label="Unavailable tools" value={snapshot.unavailable_tools?.join(" · ") || "None declared"} />
    </div>
    {preparation.error ? <p className="api-banner">{preparation.error}</p> : null}
    {logs.length ? <details className="preparation-log"><summary>Preparation log · {logs.length} steps</summary><ol>{logs.map((entry: any, index: number) => <li key={`${entry?.step ?? "step"}-${index}`}><strong>{statusText(entry?.step, `Step ${index + 1}`)}</strong><StatusPill tone={entry?.status === "succeeded" ? "healthy" : entry?.status === "failed" ? "risk" : undefined}>{statusText(entry?.status, "Recorded")}</StatusPill></li>)}</ol></details> : null}
  </details>;
}

function ToolStream({ events }: { events: any[] }) {
  const executionEvents = workspaceEvents(events);
  return <section className="attempt-tool-stream" aria-label="Live tool and model stream">
    <div className="attempt-panel-heading"><div><span className="eyebrow">Live execution</span><h3>Tool and model stream</h3></div><strong className="counter-label">{executionEvents.length}</strong></div>
    {executionEvents.length ? <div className="attempt-event-list">{executionEvents.map((event) => <article key={event.event_id ?? `${event.seq}-${event.type}`}>
      <span>{event.seq}</span><i className={`dot ${eventTone(event.type)}`} /><div><strong>{statusText(event.type)}</strong><p>{eventPayloadSummary(event.payload)}</p></div>
    </article>)}</div> : <EmptyState title="Waiting for execution events" body="Preparation and model/tool actions will stream here from the durable event cursor." />}
  </section>;
}

function AcceptancePanel({ operatorSummary }: { operatorSummary: any }) {
  const rows = acceptanceRows(operatorSummary);
  return <section className="attempt-acceptance-panel" aria-label="Acceptance evidence">
    <div className="attempt-panel-heading"><div><span className="eyebrow">Verification reserve</span><h3>Declared acceptance</h3></div><strong className="counter-label">{rows.filter((row) => row.passed).length}/{rows.length}</strong></div>
    {rows.length ? <div className="acceptance-result-list">{rows.map((row) => <article key={row.id}>{row.passed ? <CheckCircle size={18} weight="fill" /> : <Warning size={18} />}<div><strong>{row.command}</strong><p>{row.passed ? "Passed with durable tool evidence." : "Failed or did not complete successfully."}</p></div><StatusPill tone={row.passed ? "healthy" : "risk"}>{row.passed ? "Passed" : "Failed"}</StatusPill></article>)}</div> : <EmptyState title="Acceptance not executed yet" body="Only exact commands declared by the WorkItem are counted as acceptance evidence." />}
  </section>;
}

function WorkspaceChanges({ diff, changes, operatorSummary }: { diff: any; changes: any[]; operatorSummary: any }) {
  const paths = changedPaths(changes, operatorSummary);
  return <section className="attempt-changes-panel" aria-label="Workspace changes">
    <div className="attempt-panel-heading"><div><span className="eyebrow">Persisted workspace</span><h3>Changed paths and diff</h3></div><strong className="counter-label">{paths.length}</strong></div>
    {paths.length ? <ul className="changed-path-list">{paths.map((path) => <li key={path}><FileText size={16} /><strong>{path}</strong></li>)}</ul> : <EmptyState title="No file changes" body="The run has not persisted a changed path or diff yet." />}
    {(changes.length || diff?.diff) ? <details className="persisted-diff"><summary>Review full persisted diff</summary><div className="change-list">{changes.length ? changes.map((change) => <article className="change-card" key={change.id}><div><strong>{change.path}</strong><small title={timestampTitle(change.created_at)}>{formatTimestamp(change.created_at)}</small></div><pre>{change.diff}</pre></article>) : <pre>{diff.diff}</pre>}</div></details> : null}
  </section>;
}

function ReliabilityPanel({ operatorSummary, run }: { operatorSummary: any; run: any }) {
  if (!operatorSummary) return null;
  return <section className="attempt-reliability-panel"><div className="attempt-panel-heading"><div><span className="eyebrow">Context and recovery</span><h3>Harness telemetry</h3></div></div><div className="reliability-facts">
    <ReviewItem label="Context / actual" value={`${operatorSummary.estimated_context_tokens ?? 0} estimated · ${(operatorSummary.actual_total_tokens ?? 0).toLocaleString()} actual`} />
    <ReviewItem label="Tools" value={`${operatorSummary.tools_completed ?? 0} completed · ${operatorSummary.tools_failed ?? 0} failed`} />
    <ReviewItem label="Recoveries / retries" value={`${operatorSummary.recoverable_failures ?? 0} / ${operatorSummary.retries ?? 0}`} />
    <ReviewItem label="Environment probes" value={operatorSummary.environment_discovery_turns ?? 0} tone={operatorSummary.environment_discovery_turns ? "risk" : "healthy"} />
    <ReviewItem label="Approvals / wait" value={`${operatorSummary.approval_count ?? 0} · ${formatRunDuration((operatorSummary.approval_wait_ms ?? 0) / 1000)}`} />
    <ReviewItem label="Stop reason" value={operatorSummary.stop_reason ?? run?.stop_reason ?? (isActiveRun(run) ? "Running" : "Not recorded")} />
  </div></section>;
}

function RunArtifacts({ artifacts }: { artifacts: any[] }) {
  if (!artifacts.length) return null;
  return <details className="attempt-artifacts" open><summary>Additional durable artifacts <strong>{artifacts.length}</strong></summary><div className="artifact-grid">{artifacts.map((artifact) => <div className="artifact-card" key={artifact.id}><span>{artifact.kind}</span><strong>{artifact.label}</strong><small>{artifact.mime_type ?? artifact.path ?? compactId(artifact.id)}</small><p>{artifactSummary(artifact)}</p></div>)}</div></details>;
}

export function RunDetailView({ runId, refreshDashboard, onOpenQueue, operatorName, embedded = false }: RunDetailViewProps) {
  const [state, setState] = useState<any>({ status: runId ? "loading" : "empty", detail: null, error: null });
  const [reloadToken, setReloadToken] = useState(0);
  const [streamState, setStreamState] = useState<any>({ status: "idle", error: null });
  const [runNotice, setRunNotice] = useState<string | null>(null);
  const [approvalReason, setApprovalReason] = useState("");
  const streamRunIdRef = useRef<string | null>(null);
  const [streamCursor, setStreamCursor] = useState<number | null>(null);

  useEffect(() => { streamRunIdRef.current = null; setStreamCursor(null); setStreamState({ status: "idle", error: null }); setRunNotice(null); }, [runId]);

  useEffect(() => {
    let active = true;
    async function load() {
      if (!runId) { setState({ status: "empty", detail: null, error: null }); return; }
      setState((current: any) => ({ ...current, status: current.detail ? "refreshing" : "loading" }));
      try {
        const detail = await loadRunDetail(runId);
        if (!active) return;
        setState({ status: "ready", detail, error: null });
        if (streamRunIdRef.current !== runId) {
          streamRunIdRef.current = runId;
          if (isTerminalStatus(detail.run?.status)) { setStreamState({ status: "closed", error: null }); setStreamCursor(null); }
          else setStreamCursor(latestEventSeq(detail.events));
        }
      } catch (error) {
        if (active) setState((current: any) => ({ status: "error", detail: current.detail, error: error instanceof Error ? error.message : String(error) }));
      }
    }
    load();
    return () => { active = false; };
  }, [runId, reloadToken]);

  useEffect(() => {
    if (!runId) { setStreamState({ status: "idle", error: null }); return undefined; }
    if (streamCursor === null || streamRunIdRef.current !== runId) return undefined;
    setStreamState({ status: "connecting", error: null });
    let closeStream = () => {};
    closeStream = subscribeRunEvents(runId, {
      afterSeq: streamCursor,
      onEvent: (event) => {
        setStreamState({ status: isTerminalEvent(event) ? "closed" : "live", error: null });
        setState((current: any) => ({ ...current, detail: mergeRunEvent(current.detail, runId, event) }));
        if (eventShouldRefreshRunDetail(event)) setReloadToken((value) => value + 1);
        if (isTerminalEvent(event)) closeStream();
      },
      onError: (error) => setStreamState({ status: "error", error: error.message }),
    });
    return closeStream;
  }, [runId, streamCursor]);

  const detail = state.detail;
  const run = detail?.run;
  const result = run?.result ?? {};
  const events = detail?.events ?? [];
  const changes = detail?.diff?.changes ?? [];
  const artifacts = detail?.artifacts ?? [];
  const operatorSummary = detail?.operatorSummary;
  const preparation = detail?.environmentPreparation;
  const cancelAllowed = canCancelRun(run);
  const cancel = async () => {
    if (!runId || !cancelAllowed) return;
    setRunNotice(`Cancelling ${compactId(runId)}...`);
    try { await cancelRun(runId); setRunNotice(`Cancel requested: ${compactId(runId)}`); setReloadToken((value) => value + 1); await refreshDashboard?.(); }
    catch (error) { setRunNotice(`Cancel failed: ${error instanceof Error ? error.message : String(error)}`); }
  };
  const decideApproval = async (decision: "approve" | "deny") => {
    if (!runId || !approvalReason.trim()) return;
    try {
      await decideRunApproval(runId, { decision, decidedBy: operatorName ?? "console-operator", reason: approvalReason.trim() });
      setApprovalReason("");
      setReloadToken((value) => value + 1);
      await refreshDashboard?.();
    } catch (error) { setRunNotice(`Approval failed: ${error instanceof Error ? error.message : String(error)}`); }
  };

  if (!runId) return <EmptyState title="No run selected" body="Open a run from the Queue view to inspect events, diffs, artifacts, and final result JSON." />;
  const pendingApprovals = operatorSummary?.pending_approvals ?? [];
  const active = isActiveRun(run);
  return <section className={`run-detail-view attempt-workspace ${embedded ? "is-embedded" : "is-standalone"}`}>
    <header className="attempt-workspace-header">
      <div><span className="eyebrow">{embedded ? "Current isolated run" : "Run Detail"}</span>{embedded ? <h3>{run?.task ?? `Loading ${compactId(runId)}...`}</h3> : <h1>Run Detail</h1>}<p>{embedded ? <CopyIdentifier value={runId} label={run?.task ?? "Run"} /> : (run?.task ?? `Loading ${compactId(runId)}...`)}</p></div>
      <div className="attempt-header-state"><StatusPill tone={lifecycleTone(run?.status)}>{statusText(run?.status, state.status)}</StatusPill><StreamStatusPanel streamState={streamState} eventCount={events.length} cursor={streamCursor} run={run} /></div>
      <div className="detail-actions">{!embedded ? <button className="primary-action" type="button" onClick={onOpenQueue}><Rows size={17} /> Queue</button> : null}<button type="button" onClick={() => setReloadToken((value) => value + 1)}><ArrowsClockwise size={17} /> Reload evidence</button><button className="deny" type="button" disabled={!cancelAllowed} onClick={cancel}><X size={17} /> Cancel run</button></div>
    </header>

    <div className="attempt-scope-strip"><ReviewItem label="Run" value={<CopyIdentifier value={runId} label={compactId(runId)} />} /><ReviewItem label="Pinned scope" value={runScopeLabel(run?.scope ?? result.run_scope)} /><ReviewItem label="Started" value={run?.started_at ? <time title={timestampTitle(run.started_at)}>{formatTimestamp(run.started_at)}</time> : "Unknown"} /><ReviewItem label="Finished" value={run?.finished_at ? <time title={timestampTitle(run.finished_at)}>{formatTimestamp(run.finished_at)}</time> : "Active"} /></div>

    {state.error ? <div className="api-banner">Run detail failed: {state.error}</div> : null}{streamState.status === "error" ? <div className="api-banner">Event stream: {streamState.error}</div> : null}{runNotice ? <span className="action-notice">{runNotice}</span> : null}

    {run?.status === "budget_extension_required" ? <section className="attempt-boundary-banner is-budget"><Warning size={22} /><div><span className="eyebrow">Budget boundary</span><h3>Workspace retained for in-place extension</h3><p>The run transcript and workspace remain intact. Review the current WorkItem action to authorize only the server-derived extension.</p></div></section> : null}
    {pendingApprovals.length ? <section className="attempt-boundary-banner attempt-inline-approval"><Warning size={22} /><div><span className="eyebrow">Inline tool approval</span><h3>Model action is paused for review</h3><p>Pending approval {pendingApprovals.join(", ")}. The exact action remains durable and execution resumes only after this decision.</p><label>Decision reason<textarea rows={2} value={approvalReason} onChange={(event) => setApprovalReason(event.target.value)} /></label><div className="inline-approval-actions"><button className="primary-action" type="button" disabled={!approvalReason.trim()} onClick={() => decideApproval("approve")}>Approve and resume</button><button className="deny" type="button" disabled={!approvalReason.trim()} onClick={() => decideApproval("deny")}>Deny</button></div></div></section> : null}

    <BudgetPanel run={run} operatorSummary={operatorSummary} />
    <EnvironmentPanel preparation={preparation} active={run?.status === "preparing" || preparation?.status === "failed"} />

    <div className="attempt-workspace-grid">
      <div className="attempt-workspace-primary"><ToolStream events={events} /><AcceptancePanel operatorSummary={operatorSummary} /></div>
      <aside className="attempt-workspace-evidence"><WorkspaceChanges diff={detail?.diff} changes={changes} operatorSummary={operatorSummary} /><ReliabilityPanel operatorSummary={operatorSummary} run={run} /></aside>
    </div>

    {(result.summary || result.error || !active) ? <section className="attempt-result-panel"><div className="attempt-panel-heading"><div><span className="eyebrow">Run outcome</span><h3>{result.summary ?? result.error ?? "No final result has been recorded."}</h3></div><StatusPill tone={lifecycleTone(result.status ?? run?.status)}>{statusText(result.status ?? run?.status, state.status)}</StatusPill></div></section> : null}
    <RunArtifacts artifacts={artifacts} />
  </section>;
}

import { useEffect, useRef, useState } from "react";
import { ArrowsClockwise, FileText, Rows, X } from "@phosphor-icons/react";
import { CopyIdentifier, EmptyState, ReviewItem, StatusPill } from "../components/Operational";
import { compactId, formatTimestamp, lifecycleTone, statusText, timestampTitle } from "../lib/formatters";
import { cancelRun, decideRunApproval, loadRunDetail, subscribeRunEvents } from "../api/runs";

type RunDetailViewProps = {
  runId?: string | null;
  refreshDashboard?: () => Promise<unknown> | void;
  onOpenQueue: () => void;
  operatorName?: string;
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
function isTerminalStatus(status?: string) { return ["completed", "failed", "cancelled"].includes(status ?? ""); }
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
  const rows = [["Source", "API events/stream"], ["Replay cursor", cursor === null ? "terminal snapshot" : `after seq ${cursor}`], ["Durable events", String(eventCount)], ["Run state", statusText(run?.status, "loading")]];
  return <section className={`stream-status-panel stream-panel-${streamState.status}`}><div><strong>{streamLabel(streamState)}</strong><span>{streamDescription(streamState)}</span></div><div className="stream-facts">{rows.map(([label, value]) => <span key={label}><small>{label}</small><b>{value}</b></span>)}</div></section>;
}

function RunEvents({ events }: { events: any[] }) {
  return <section className="review-surface"><div className="table-heading"><div><h2>Events</h2><p>Durable event log for replaying the run.</p></div><strong className="counter-label">{events.length}</strong></div>{events.length ? <div className="event-list">{events.map((event) => <div className="event-list-row" key={event.event_id ?? `${event.seq}-${event.type}`}><span>{event.seq}</span><i className={`dot ${eventTone(event.type)}`} /><strong>{event.type}</strong><p>{eventPayloadSummary(event.payload)}</p></div>)}</div> : <EmptyState title="No events" body="No durable events have been recorded for this run yet." />}</section>;
}

function RunDiff({ diff, changes }: { diff: any; changes: any[] }) {
  return <section className="review-surface"><div className="table-heading"><div><h2>Diff</h2><p>File changes persisted for this run.</p></div><strong className="counter-label">{changes.length}</strong></div>{changes.length ? <div className="change-list">{changes.map((change) => <div className="change-card" key={change.id}><strong>{change.path}</strong><small title={timestampTitle(change.created_at)}>{formatTimestamp(change.created_at)}</small><pre>{change.diff}</pre></div>)}</div> : <div className="diff-box"><div><FileText size={18} /> No file changes</div><pre>{diff?.diff || "This run did not persist a file diff."}</pre></div>}</section>;
}

function RunArtifacts({ artifacts }: { artifacts: any[] }) {
  return <section className="review-surface"><div className="table-heading"><div><h2>Artifacts</h2><p>Observation and tool artifacts recorded by the runtime.</p></div><strong className="counter-label">{artifacts.length}</strong></div>{artifacts.length ? <div className="artifact-grid">{artifacts.map((artifact) => <div className="artifact-card" key={artifact.id}><span>{artifact.kind}</span><strong>{artifact.label}</strong><small>{artifact.mime_type ?? artifact.path ?? compactId(artifact.id)}</small><p>{artifactSummary(artifact)}</p></div>)}</div> : <EmptyState title="No artifacts" body="Read-only file-listing runs often have no artifacts. Cluster, Tekton, Argo, Prometheus, and Loki reads should appear here." />}</section>;
}

export function RunDetailView({ runId, refreshDashboard, onOpenQueue, operatorName }: RunDetailViewProps) {
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
  return <section className="run-detail-view">
    <div className="section-heading"><div><h1>Run Detail</h1><p>{run?.task ?? `Loading ${compactId(runId)}...`}</p></div><div className="detail-actions"><span className={`stream-chip stream-${streamState.status}`}><i className={`dot ${streamState.status === "error" ? "blocked" : streamState.status === "live" ? "running" : "future"}`} />{streamLabel(streamState)}</span><button className="primary-action" type="button" onClick={onOpenQueue}><Rows size={17} /> Queue</button><button className="primary-action" type="button" onClick={() => setReloadToken((value) => value + 1)}><ArrowsClockwise size={17} /> Reload</button><button className="primary-action deny" type="button" disabled={!cancelAllowed} onClick={cancel}><X size={17} /> Cancel</button></div></div>
    {state.error ? <div className="api-banner">Run detail failed: {state.error}</div> : null}{streamState.status === "error" ? <div className="api-banner">Event stream: {streamState.error}</div> : null}{runNotice ? <span className="action-notice">{runNotice}</span> : null}
    <div className="run-detail-grid"><ReviewItem label="Run" value={<CopyIdentifier value={runId} label={run?.task ?? "Run"} />} /><ReviewItem label="Status" value={statusText(run?.status, state.status)} tone={run?.status === "failed" ? "risk" : run?.status === "approval_required" ? "pending" : undefined} /><ReviewItem label="Submitted" value={run?.started_at ? <time title={timestampTitle(run.started_at)}>{formatTimestamp(run.started_at)}</time> : "unknown"} /><ReviewItem label="Finished" value={run?.finished_at ? <time title={timestampTitle(run.finished_at)}>{formatTimestamp(run.finished_at)}</time> : "not finished"} /><ReviewItem label="Turns" value={result.turns ?? "unknown"} /><ReviewItem label="Scope" value={runScopeLabel(run?.scope ?? result.run_scope)} /></div>
    {operatorSummary ? <section className="operator-summary"><ReviewItem label="Context estimate" value={`${operatorSummary.estimated_context_tokens ?? 0} tokens`} /><ReviewItem label="Actual usage" value={`${operatorSummary.actual_total_tokens ?? 0} total`} /><ReviewItem label="Recoveries / retries" value={`${operatorSummary.recoverable_failures ?? 0} / ${operatorSummary.retries ?? 0}`} /><ReviewItem label="Tools" value={`${operatorSummary.tools_completed ?? 0} completed · ${operatorSummary.tools_failed ?? 0} failed`} /><ReviewItem label="Compactions" value={operatorSummary.compactions ?? 0} /><ReviewItem label="Truncated results" value={operatorSummary.truncated_tool_results ?? 0} /></section> : null}
    {operatorSummary?.pending_approvals?.length ? <section className="inline-approval"><span className="eyebrow">Inline tool approval</span><h2>Model action is paused for review</h2><p>Pending approval {operatorSummary.pending_approvals.join(", ")}. The exact action remains durable and execution resumes only after this decision.</p><label>Decision reason<textarea rows={2} value={approvalReason} onChange={(event) => setApprovalReason(event.target.value)} /></label><div><button className="primary-action" type="button" disabled={!approvalReason.trim()} onClick={() => decideApproval("approve")}>Approve and resume</button><button className="primary-action deny" type="button" disabled={!approvalReason.trim()} onClick={() => decideApproval("deny")}>Deny</button></div></section> : null}
    <StreamStatusPanel streamState={streamState} eventCount={events.length} cursor={streamCursor} run={run} />
    <section className="review-surface"><div className="table-heading"><div><h2>Result</h2><p>Structured final result returned by the run.</p></div><StatusPill tone={lifecycleTone(result.status ?? run?.status)}>{statusText(result.status ?? run?.status, state.status)}</StatusPill></div><p>{result.summary ?? result.error ?? "No result summary has been recorded yet."}</p></section>
    <section className="run-detail-layout"><RunEvents events={events} /><RunDiff diff={detail?.diff} changes={changes} /></section><RunArtifacts artifacts={artifacts} />
  </section>;
}

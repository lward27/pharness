import { useMemo, useRef, useState, type CSSProperties } from "react";
import { Clock, Rows, X } from "@phosphor-icons/react";
import { LinkButton, OutcomeDetails, Status } from "./components";
import { FactGrid, formatMoment, humanize } from "./presentation";
import { useDialog } from "./useDialog";
import { useResource } from "./useResource";

export const lifecycleStages = ["discover", "plan", "implement", "test", "verify", "source_delivery"] as const;
export type TimelineInterval = {
  id: string; stage_key: string; sequence: number; resource_kind: string; resource_id: string;
  stage_execution_id?: string | null; run_id?: string | null; outcome_id?: string | null;
  kind: "execution" | "marker" | "unavailable" | "delivery_wait"; timing_basis: string;
  started_at?: string | null; finished_at?: string | null; queued_at?: string;
  is_current: boolean; is_effective: boolean; is_ongoing: boolean;
  status: string; origin: string; correction_of?: unknown; diagnosis_of?: unknown; stop_reason?: string;
};
export type TimelineProjection = { as_of: string; elapsed_includes_waits: boolean; intervals: TimelineInterval[] };

export function timestamp(value?: string | null): number | null {
  if (!value) return null;
  const n = /^\d{13}$/.test(value) ? Number(value) : Date.parse(value);
  return Number.isFinite(n) ? n : null;
}
export function intervalTimes(interval: TimelineInterval, asOf: string) {
  const start = timestamp(interval.started_at);
  const end = timestamp(interval.finished_at || (interval.is_ongoing ? asOf : null));
  return start !== null && end !== null && end >= start ? { start, end } : null;
}
function duration(ms: number) {
  if (ms < 1000) return "<1s";
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`;
  return `${Math.floor(s / 3600)}h ${Math.floor(s % 3600 / 60)}m`;
}
const intervalLabel = (entry: TimelineInterval) => entry.kind === "delivery_wait" ? "PR delivery · includes waits" : `${entry.correction_of ? "Repair" : entry.diagnosis_of ? "Diagnosis" : humanize(entry.stage_key)} ${entry.sequence}${entry.kind === "marker" ? " · sealed" : ""}${["failed","cancelled","blocked"].includes(entry.status) ? ` · ${entry.status}` : ""}`;

export function LifecycleTimeline({ projection, outcomes = [], workItemId }: { projection?: TimelineProjection; outcomes?: any[]; workItemId: string }) {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const entries = projection?.intervals || [];
  const selected = entries.find(entry => entry.id === selectedId);
  const range = useMemo(() => {
    const times = entries.flatMap(entry => { const t = intervalTimes(entry, projection?.as_of || ""); return t ? [t.start, t.end] : []; });
    return times.length ? { start: Math.min(...times), end: Math.max(...times) } : null;
  }, [entries, projection?.as_of]);
  const extent = range ? Math.max(1, range.end - range.start) : 1;
  return <section className="repo-panel lamina-timeline" aria-labelledby="lamina-timeline-title">
    <header><div><span className="repo-eyebrow">Recorded lifecycle</span><h2 id="lamina-timeline-title"><Rows size={20} />WorkItem activity</h2><p>Elapsed time includes pauses and external waits. Not active model time.</p></div><span className="lamina-fit"><Clock size={14} />Fit recorded history</span></header>
    {!projection ? <p className="repo-warning">Timeline projection unavailable. Stage Outcomes and History retain the recorded evidence.</p> : null}
    <div className="lamina-timeline-scroll" role="region" aria-label="Lifecycle time lanes; scroll horizontally for the full axis" tabIndex={0}>
      <div className="lamina-time-axis"><span>Stage / outcome</span><div>{[0, .25, .5, .75, 1].map(position => <time key={position} title={range ? formatMoment(String(range.start + extent * position)) : undefined}>{range ? new Date(range.start + extent * position).toLocaleTimeString([], {hour:"2-digit",minute:"2-digit"}) : "—"}</time>)}</div></div>
      {lifecycleStages.map(stage => {
        const lane = entries.filter(entry => entry.stage_key === stage);
        const outcome = outcomes.find(entry => entry.stage_key === stage);
        const current = lane.find(entry => entry.is_current);
        const origins = [...new Set(lane.map(entry => entry.origin || "unavailable"))];
        return <div className={`lamina-lane ${current ? "is-current" : ""}`} key={stage}>
          <div className="lamina-lane-label"><strong>{humanize(stage)}</strong><Status value={outcome?.status || current?.status || "not recorded"} />{origins.length ? <small>{origins.map(humanize).join(" / ")}{stage === "test" && origins.length === 1 && origins[0] === "controller" ? " · deterministic" : ""}</small> : null}</div>
          <div className="lamina-lane-tracks">{lane.length ? lane.map(entry => {
            const time = intervalTimes(entry, projection?.as_of || "");
            const start = time && range ? (time.start - range.start) / extent * 100 : 0;
            const width = time ? (time.end - time.start) / extent * 100 : 0;
            const point = entry.kind === "marker" || (time && width === 0);
            return <div className="lamina-track" key={entry.id}><button type="button" className={`lamina-interval is-${entry.kind} ${entry.is_current ? "is-current" : ""} ${!entry.is_effective && !entry.is_current ? "is-history" : ""} ${point ? "is-point" : time && width < 8 ? "is-short" : ""} ${start > 80 ? "is-edge" : ""}`} style={time ? {"--interval-start":`${start}%`, "--interval-width":`${width}%`} as CSSProperties : undefined} onClick={() => setSelectedId(entry.id)} aria-label={`${intervalLabel(entry)} · ${entry.status} · ${point ? "Instantaneous marker" : time ? duration(time.end - time.start) + " elapsed" : "Timing not recorded"}`} title={`${intervalLabel(entry)} · ${entry.status}${entry.is_effective ? " · effective" : ""}`}><span>{intervalLabel(entry)}</span>{!point ? <small>{time ? duration(time.end - time.start) : "Timing not recorded"}</small> : null}</button></div>;
          }) : <p className="lamina-no-interval">No execution interval recorded</p>}</div>
        </div>;
      })}
    </div>
    <footer className="lamina-timeline-legend"><span><i />Current</span><span><i />Effective outcome</span><span><i />Historical / superseded</span><small title={formatMoment(projection?.as_of)}>As of {formatMoment(projection?.as_of)}</small></footer>
    <details className="repo-raw-record"><summary>Text timeline · all recorded intervals</summary><ol className="lamina-text-timeline">{entries.map(entry => <li key={entry.id}><button type="button" onClick={() => setSelectedId(entry.id)}>{intervalLabel(entry)} · {entry.status}</button><span>{formatMoment(entry.started_at)} → {entry.finished_at ? formatMoment(entry.finished_at) : entry.is_ongoing ? "Ongoing at observation time" : "End unavailable"}</span></li>)}</ol></details>
    {selected ? <IntervalInspector entry={selected} outcome={outcomes.find(o => o.id === selected.outcome_id)} asOf={projection?.as_of || ""} workItemId={workItemId} onClose={() => setSelectedId(null)} /> : null}
  </section>;
}

function IntervalInspector({ entry, outcome, asOf, workItemId, onClose }: { entry: TimelineInterval; outcome?: any; asOf: string; workItemId: string; onClose: () => void }) {
  const ref = useRef<HTMLElement>(null);
  useDialog(ref, onClose);
  const times = intervalTimes(entry, asOf);
  const historical = useResource<any>(!outcome && entry.stage_execution_id ? `/api/stage-executions/${encodeURIComponent(entry.stage_execution_id)}/outcome` : null);
  const sealed = outcome || historical.data?.outcome;
  return <div className="lamina-inspector-backdrop" onMouseDown={event => { if (event.currentTarget === event.target) onClose(); }}><section className="lamina-inspector" role="dialog" aria-modal="true" aria-label="Recorded interval" ref={ref}><header><div><span className="repo-eyebrow">Read-only inspection</span><h2>{intervalLabel(entry)}</h2></div><button type="button" aria-label="Close interval inspector" onClick={onClose}><X size={20} /></button></header><Status value={entry.status} /><FactGrid facts={[{label:"Record",value:entry.resource_id,mono:true},{label:"Origin",value:entry.origin || "Unavailable"},{label:"Timing basis",value:humanize(entry.timing_basis)},{label:"Started",value:formatMoment(entry.started_at)},{label:"Ended",value:entry.finished_at ? formatMoment(entry.finished_at) : entry.is_ongoing ? `Ongoing as of ${formatMoment(asOf)}` : "Unavailable"},{label:"Elapsed including waits",value:entry.kind === "marker" ? "Instantaneous marker; no duration" : times ? duration(times.end-times.start) : "Timing not recorded"},{label:"Active model time",value:"Not supplied by this projection; inspect Run budgets"},{label:"Standing",value:entry.is_current ? "Current execution" : entry.is_effective ? "Effective" : "Historical / superseded"},{label:"Stop reason",value:entry.stop_reason || "Not recorded"}]} />{entry.correction_of ? <div className="repo-warning"><strong>Correction lineage</strong><pre>{JSON.stringify(entry.correction_of,null,2)}</pre></div> : null}{sealed ? <OutcomeDetails outcome={sealed} /> : <p className="repo-muted">{historical.error ? "Recorded outcome unavailable; retry from History." : historical.status === "loading" ? "Loading the recorded outcome…" : "No sealed outcome recorded for this interval."}</p>}<div className="lamina-inspector-links" onClick={onClose}>{entry.run_id ? <LinkButton to={`agents/runs/${entry.run_id}`}>Open Run</LinkButton> : null}<LinkButton to={`work-items/${workItemId}/${entry.resource_kind === "source_delivery_intent" ? "delivery" : "history"}`}>Inspect owning evidence</LinkButton></div></section></div>;
}

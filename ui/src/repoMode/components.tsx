import { useEffect, useRef, useState, type ReactNode } from "react";
import { ArrowRight, CheckCircle, Clock, LockKey, WarningCircle, X } from "@phosphor-icons/react";
import { sendJson } from "./api";
import { navigate } from "./routes";

export function Status({ value = "unavailable" }: { value?: string | null }) {
  const normalized = String(value || "unavailable").toLowerCase();
  const tone = ["ready", "available", "healthy", "completed", "succeeded", "passing", "active"].includes(normalized)
    ? "good"
    : ["failed", "blocked", "unavailable", "error", "rejected"].includes(normalized)
      ? "bad"
      : ["waiting", "waiting_external", "pending", "stale", "configured_unverified", "paused"].includes(normalized)
        ? "wait"
        : "neutral";
  return <span className={`repo-status is-${tone}`}><span aria-hidden="true" />{normalized.replaceAll("_", " ")}</span>;
}

export function ResourceState({ status, error, empty, children }: { status: string; error?: string | null; empty?: boolean; children: ReactNode }) {
  if (status === "loading") return <div className="repo-state" role="status"><Clock size={22} />Loading current state…</div>;
  if (status === "error") return <div className="repo-state is-error" role="alert"><WarningCircle size={22} />{error || "This resource is unavailable."}</div>;
  if (empty) return <div className="repo-state"><span>No durable records yet.</span></div>;
  return <>{error ? <div className="repo-stale" role="status">Showing retained data. Refresh failed: {error}</div> : null}{children}</>;
}

export function SectionHeader({ eyebrow, title, summary, action }: { eyebrow?: string; title: string; summary?: string; action?: ReactNode }) {
  return <header className="repo-section-header"><div>{eyebrow ? <span className="repo-eyebrow">{eyebrow}</span> : null}<h1>{title}</h1>{summary ? <p>{summary}</p> : null}</div>{action}</header>;
}

export function Metric({ label, value, detail }: { label: string; value: ReactNode; detail?: ReactNode }) {
  return <div className="repo-metric"><span>{label}</span><strong>{value}</strong>{detail ? <small>{detail}</small> : null}</div>;
}

export function Empty({ title, message, action }: { title: string; message: string; action?: ReactNode }) {
  return <div className="repo-empty"><div><h2>{title}</h2><p>{message}</p></div>{action}</div>;
}

export function LinkButton({ to, children, className = "" }: { to: string; children: ReactNode; className?: string }) {
  return <button className={`repo-link-button ${className}`} type="button" onClick={() => navigate(to)}>{children}<ArrowRight size={16} /></button>;
}

export type ServerAction = {
  id: string;
  lifecycle_stage?: string;
  resource?: string | Record<string, unknown>;
  status?: string;
  effect_class?: string;
  blockers?: Array<{ code?: string; summary?: string } | string>;
  approval_required?: boolean;
  approval_requirements?: string[];
  external_effect_summary?: string;
  expected_result?: string;
  state_hash: string;
};

export function ActionDialog({ action, owner, endpoint, operatorName, onClose, onApplied }: { action: ServerAction; owner: { kind: string; id: string; product?: string; repository?: string; revision?: string }; endpoint: string; operatorName?: string; onClose: () => void; onApplied: () => void }) {
  const [actor, setActor] = useState(operatorName || "operator");
  const [reason, setReason] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState("");
  const dialogRef = useRef<HTMLDivElement>(null);
  const external = action.effect_class?.includes("external");
  const model = action.effect_class?.includes("model");
  const effectTone = external ? "is-external" : model ? "is-model" : "is-internal";
  const resourceTarget = typeof action.resource === "string"
    ? action.resource
    : action.resource
      ? Object.entries(action.resource).map(([key, value]) => `${key.replaceAll("_", " ")}: ${String(value)}`).join(" · ")
      : `${owner.kind} ${owner.id}`;

  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    dialogRef.current?.querySelector<HTMLElement>("input")?.focus();
    const handleKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
      if (event.key === "Tab") {
        const focusable = Array.from(dialogRef.current?.querySelectorAll<HTMLElement>("button,input,textarea") || []).filter(element => !element.hasAttribute("disabled"));
        if (!focusable.length) return;
        const first = focusable[0];
        const last = focusable[focusable.length - 1];
        if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
        else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
      }
    };
    window.addEventListener("keydown", handleKey);
    return () => { window.removeEventListener("keydown", handleKey); previous?.focus(); };
  }, [onClose]);

  const submit = async () => {
    setSubmitting(true); setError("");
    try {
      await sendJson(endpoint, "POST", { actor: actor.trim(), reason: reason.trim(), state_hash: action.state_hash });
      onApplied(); onClose();
    } catch (caught) {
      const value = caught as Error & { status?: number };
      if (value.status === 409) {
        onClose(); onApplied();
        return;
      }
      setError(value.message);
    } finally { setSubmitting(false); }
  };

  return <div className="repo-dialog-backdrop" onMouseDown={event => { if (event.currentTarget === event.target) onClose(); }}>
    <div className="repo-dialog" role="dialog" aria-modal="true" aria-labelledby="action-title" ref={dialogRef}>
      <header><div><span className="repo-eyebrow">{action.lifecycle_stage || "Lifecycle"} boundary</span><h2 id="action-title">{action.id.replaceAll("_", " ")}</h2></div><button type="button" aria-label="Close action review" onClick={onClose}><X size={18} /></button></header>
      <div className={`repo-effect ${effectTone}`}><LockKey size={20} /><div><strong>{external ? "External effect" : model ? "Model execution" : action.effect_class?.replaceAll("_", " ") || "Controller action"}</strong><p>{action.external_effect_summary || "Advance the owning resource at this exact state."}</p></div></div>
      <dl className="repo-bindings">
        <div><dt>Owner</dt><dd>{owner.kind} · {owner.id}</dd></div>
        <div><dt>Exact action target</dt><dd>{resourceTarget}</dd></div>
        {owner.product ? <div><dt>Product</dt><dd>{owner.product}</dd></div> : null}
        {owner.repository ? <div><dt>Repository</dt><dd>{owner.repository}</dd></div> : null}
        {owner.revision ? <div><dt>Revision</dt><dd className="repo-mono">{owner.revision}</dd></div> : null}
        <div><dt>State hash</dt><dd className="repo-mono">{action.state_hash}</dd></div>
        <div><dt>Expected result</dt><dd>{action.expected_result || "A new durable controller state is recorded."}</dd></div>
      </dl>
      {action.blockers?.length ? <div className="repo-blockers"><strong>Blocked</strong>{action.blockers.map((blocker, index) => <p key={index}>{typeof blocker === "string" ? blocker : blocker.summary || blocker.code}</p>)}</div> : null}
      <label>Operator<input value={actor} onChange={event => setActor(event.target.value)} /></label>
      <label>Reason<textarea rows={3} value={reason} onChange={event => setReason(event.target.value)} placeholder="Why this action is appropriate now" /></label>
      {error ? <div className="repo-error" role="alert">{error}</div> : null}
      <footer><button className="repo-primary" type="button" disabled={submitting || action.status === "blocked" || !actor.trim() || !reason.trim()} onClick={submit}>{submitting ? "Applying…" : "Confirm and apply"}</button><button type="button" onClick={onClose}>Cancel</button></footer>
    </div>
  </div>;
}

export function OutcomeDetails({ outcome }: { outcome: any }) {
  const payload = outcome?.outcome || {};
  const groups = [
    ["Verified facts", payload.verified_facts || payload.facts],
    ["Outputs", payload.outputs],
    ["Acceptance evidence", payload.acceptance],
    ["Agent claims", payload.agent_claims || payload.claims],
    ["Risks and contradictions", [...(payload.risks || []), ...(payload.contradictions || [])]],
    ["Decisions and authorizations", [...(payload.decisions || []), ...(payload.authorizations || [])]],
  ].filter(([, value]) => Array.isArray(value) ? value.length : value && Object.keys(value).length);
  return <div className="repo-outcome-details">
    {groups.map(([label, value]) => <section key={label as string}><h4>{label as string}</h4><StructuredValues value={value} /></section>)}
    {payload.unavailable_capabilities?.length ? <section><h4>Unavailable capabilities</h4><StructuredValues value={payload.unavailable_capabilities} /></section> : null}
    {payload.recommendations?.length ? <section><h4>Recommendations</h4><StructuredValues value={payload.recommendations} /></section> : null}
    <section className="repo-provenance"><h4>Freshness and provenance</h4><dl><div><dt>Outcome</dt><dd className="repo-mono">{outcome.id}</dd></div><div><dt>Content hash</dt><dd className="repo-mono">{outcome.content_hash}</dd></div><div><dt>Sealed</dt><dd>{outcome.sealed_at || "Unavailable"}</dd></div><div><dt>State version</dt><dd>{outcome.sealed_state_version ?? "Unavailable"}</dd></div>{outcome.supersedes_outcome_id ? <div><dt>Supersedes</dt><dd className="repo-mono">{outcome.supersedes_outcome_id}</dd></div> : null}</dl></section>
    <details className="repo-raw-record"><summary>Raw sealed outcome</summary><pre>{JSON.stringify(outcome, null, 2)}</pre></details>
  </div>;
}

function StructuredValues({ value }: { value: any }) {
  const entries = Array.isArray(value) ? value : value && typeof value === "object" ? Object.entries(value).map(([key, detail]) => ({ key, detail })) : [value];
  return <div className="repo-structured-list">{entries.map((entry, index) => {
    if (entry && typeof entry === "object") {
      const record = entry as Record<string, any>;
      const title = record.statement || record.summary || record.name || record.key || record.kind || record.command || `Record ${index + 1}`;
      const details = Object.entries(record).filter(([key]) => !["statement", "summary", "name", "key", "kind", "command"].includes(key));
      return <article className="repo-structured-record" key={`${title}-${index}`}><strong>{String(title)}</strong>{details.length ? <dl>{details.map(([key, detail]) => <div key={key}><dt>{key.replaceAll("_", " ")}</dt><dd>{typeof detail === "object" ? JSON.stringify(detail) : String(detail)}</dd></div>)}</dl> : null}</article>;
    }
    return <article className="repo-structured-record" key={`${String(entry)}-${index}`}>{String(entry)}</article>;
  })}</div>;
}

export function SuccessMark() { return <CheckCircle size={18} weight="fill" aria-hidden="true" />; }

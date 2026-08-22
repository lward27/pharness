import { useEffect } from "react";
import { CheckCircle, LockKey, ShieldWarning, X } from "@phosphor-icons/react";
import { ReviewItem, StatusPill } from "../components/Operational";
import { statusText } from "../lib/formatters";
import { buildLifecycleReview, reviewAlternatives, type LifecycleAction } from "../lib/lifecycleReview";

type LifecycleReviewDrawerProps = {
  action: LifecycleAction;
  actions: LifecycleAction[];
  item: any;
  flow: any;
  preview?: any;
  rollbackIntent?: any;
  actor: string;
  reason: string;
  applying: boolean;
  error?: string | null;
  onActorChange: (value: string) => void;
  onReasonChange: (value: string) => void;
  onActionChange: (action: LifecycleAction) => void;
  onConfirm: () => void;
  onClose: () => void;
};

export function LifecycleReviewDrawer({ action, actions, item, flow, preview, rollbackIntent, actor, reason, applying, error, onActorChange, onReasonChange, onActionChange, onConfirm, onClose }: LifecycleReviewDrawerProps) {
  const model = buildLifecycleReview(action, { item, flow, preview, rollbackIntent });
  const alternatives = reviewAlternatives(action, actions);
  const rejected = action.id.startsWith("reject_");
  const recovery = model.kind === "rollback_intent";
  const externalEffect = action.effect_class === "external_effect" || action.effect_class === "external";
  const modelExecution = action.effect_class === "model_execution";

  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [onClose]);

  return <div className="lifecycle-review-overlay" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
    <aside className={`lifecycle-review-drawer ${recovery ? "is-recovery" : ""} ${rejected ? "is-reject" : ""}`} role="dialog" aria-modal="true" aria-label="Lifecycle review">
      <header>
        <div><span className="eyebrow">{statusText(action.lifecycle_stage)} lifecycle boundary</span><h2>{model.heading}</h2><p>{model.resourceId}</p></div>
        <button className="icon-button" type="button" aria-label="Close lifecycle review" onClick={onClose}><X size={18} /></button>
      </header>

      {alternatives.length > 1 ? <nav className="review-decision-switch" aria-label="Resource decision">
        {alternatives.map((candidate) => <button className={candidate.id === action.id ? "is-active" : ""} type="button" key={candidate.id} onClick={() => onActionChange(candidate)}>{statusText(candidate.id)}</button>)}
      </nav> : null}

      <div className="lifecycle-review-body">
        <section className="lifecycle-review-evidence" aria-label="Decision evidence">
          <div className="review-section-heading"><span><CheckCircle size={18} /> Decision evidence</span><StatusPill tone={["proposed", "ready", "approved"].includes(model.resourceStatus) ? "healthy" : undefined}>{statusText(model.resourceStatus)}</StatusPill></div>
          {model.groups.map((group) => <article className="review-evidence-group" key={group.title}>
            <div><h3>{group.title}</h3>{group.summary ? <p>{group.summary}</p> : null}</div>
            <div className="review-fact-grid">{group.facts.map((entry) => <ReviewItem key={`${group.title}-${entry.label}`} label={entry.label} value={entry.value} tone={entry.tone} />)}</div>
            {group.evidence?.length ? <ul>{group.evidence.map((entry) => <li key={entry}>{entry}</li>)}</ul> : null}
          </article>)}
          {model.warnings.map((warning) => <div className="review-warning" role="status" key={warning}><ShieldWarning size={18} /><span>{warning}</span></div>)}
        </section>

        <section className="lifecycle-review-authorization" aria-label="Effect and authorization">
          <div className="review-section-heading"><span><LockKey size={18} /> Effect and authorization</span></div>
          <div className={`exact-effect ${externalEffect ? "is-external" : ""} ${modelExecution ? "is-model" : ""}`}><span className="eyebrow">{externalEffect ? "External effect" : modelExecution ? "Model execution boundary" : "Controller effect"}</span><p>{model.effectSummary}</p></div>
          {action.blockers?.length ? <div className="drawer-blockers"><strong>Blocked by</strong>{action.blockers.map((blocker) => <p key={`${blocker.code}-${blocker.summary}`}><b>{statusText(blocker.code)}</b>{blocker.summary}</p>)}</div> : null}
          <div className="authorization-binding">
            <ReviewItem label="Required approvals" value={model.approvalRequirements.length ? model.approvalRequirements.map(statusText).join(" · ") : "No additional approval gate"} />
            <ReviewItem label="Resource binding" value={model.resourceId} />
            <ReviewItem label="State hash" value={model.stateHash ?? "Legacy reconcile preview"} tone={model.stateHash ? "healthy" : "risk"} />
          </div>
          {preview?.authorization_checks?.length ? <div className="drawer-authorization-checks"><strong>Boundary checks</strong>{preview.authorization_checks.map((check: any) => <ReviewItem key={`${check.kind}-${check.resource_id ?? "none"}`} label={statusText(check.kind)} value={`${statusText(check.status)}${check.summary ? ` · ${check.summary}` : ""}`} tone={["missing", "blocked", "unavailable"].includes(check.status) ? "risk" : check.status === "ready" ? "healthy" : undefined} />)}</div> : null}
          <label>Operator<input value={actor} onChange={(event) => onActorChange(event.target.value)} /></label>
          <label>Decision reason<textarea value={reason} onChange={(event) => onReasonChange(event.target.value)} rows={3} placeholder={`Why ${statusText(action.id).toLowerCase()} is appropriate at this boundary`} /></label>
          {error ? <div className="api-banner">{error}</div> : null}
        </section>
      </div>

      <footer>
        <button className={`primary-action ${rejected ? "is-danger" : ""}`} type="button" disabled={!actor.trim() || !reason.trim() || applying || action.status !== "ready"} onClick={onConfirm}>{applying ? "Applying…" : `Confirm ${statusText(action.id)}`}</button>
        <button type="button" onClick={onClose}>Cancel</button>
      </footer>
    </aside>
  </div>;
}

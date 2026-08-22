import { useEffect, useRef, useState } from "react";
import { ArrowLeft, CheckCircle, Clock, Lifebuoy, RocketLaunch, Warning } from "@phosphor-icons/react";
import { DeliveryWorkspace } from "../components/DeliveryWorkspace";
import { CopyIdentifier, EmptyState, ReviewItem, StatusPill } from "../components/Operational";
import { compactId, formatTimestamp, lifecycleTone, statusText, timestampTitle } from "../lib/formatters";
import { findCorrectiveAction, type LifecycleAction } from "../lib/lifecycleReview";
import { defaultWorkItemSection, type WorkItemSection } from "../lib/runWorkspace";
import { selectPrimaryWorkItemAction, selectRecoveryActions } from "../lib/workItemActions";
import { advanceWorkItem, applyWorkItemReconcile, executeWorkItemAction, getOperatorName, loadRollbackIntent, loadWorkItem, loadWorkItemFlow, previewWorkItemReconcile } from "../pharnessApi";
import { LifecycleReviewDrawer } from "./LifecycleReviewDrawer";
import { RunDetailView } from "./RunDetailView";

type WorkItemDetailViewProps = {
  workItemId: string;
  refreshDashboard: () => Promise<unknown> | void;
  autoRefresh: boolean;
  operatorName?: string;
  onBack: () => void;
};

function repositoryLabel(value?: string) {
  if (!value) return "Repository unavailable";
  const normalized = value.replace(/\.git$/, "");
  const parts = normalized.split("/").filter(Boolean);
  return parts.slice(-2).join("/") || value;
}

function DurableArtifactPanel({ artifact, onClose }: { artifact: any; onClose: () => void }) {
  const [copied, setCopied] = useState(false);
  const copyId = async () => {
    try {
      await navigator.clipboard.writeText(artifact.id);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  };
  return <section className="durable-artifact-panel">
    <div><span className="eyebrow">Durable evidence</span><h2>{artifact.label ?? "Resource"}</h2></div>
    <button className="icon-button" type="button" aria-label="Close durable evidence" onClick={onClose}>×</button>
    <dl>
      <div><dt>Identifier</dt><dd title={artifact.id}>{artifact.id}</dd></div>
      <div><dt>Type</dt><dd>{artifact.kind ?? "resource"}</dd></div>
      <div><dt>Summary</dt><dd>{artifact.summary ?? artifact.title ?? "No additional summary was recorded."}</dd></div>
    </dl>
    <button type="button" onClick={copyId}>{copied ? "Copied" : "Copy identifier"}</button>
  </section>;
}

function WaitSummary({ wait }: { wait: any }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, []);
  const deadline = Number(wait.deadline_at);
  const remaining = Number.isFinite(deadline) ? Math.max(0, deadline - now) : null;
  const seconds = remaining == null ? null : Math.floor(remaining / 1000);
  const countdown = seconds == null ? "deadline unavailable" : seconds === 0 ? "deadline reached" : seconds < 60 ? `${seconds}s remaining` : `${Math.ceil(seconds / 60)}m remaining`;
  return <div className="wait-summary"><strong>Active wait</strong><span>{statusText(wait.wait_kind)} · {wait.check_count}/{wait.max_checks} checks</span><span>Next observation <time title={timestampTitle(wait.next_check_at)}>{formatTimestamp(wait.next_check_at)}</time></span><span>Deadline <time title={timestampTitle(wait.deadline_at)}>{formatTimestamp(wait.deadline_at)}</time> · {countdown}</span></div>;
}

function AttemptHistory({ events }: { events: any[] }) {
  const attempts = events.filter((event) => event.kind === "work_item.attempt_finished");
  if (!attempts.length) return null;
  return <section className="attempt-history" aria-label="Coding attempt history">
    <div><span className="eyebrow">Attempt history</span><h2>Terminal coding attempts</h2><p>Classifications and next actions are advisory evidence; they do not dispatch work.</p></div>
    <div className="attempt-history-list">{attempts.map((event) => {
      const classification = event.payload?.classification ?? event.payload_json?.classification ?? {};
      const outcome = event.payload?.outcome ?? event.payload_json?.outcome ?? {};
      return <article key={event.id}>
        <div><strong>{statusText(classification.code ?? outcome.status, "Unclassified")}</strong><StatusPill tone={lifecycleTone(outcome.status)}>{statusText(outcome.status, "Unknown")}</StatusPill></div>
        <p>Recommended action: <strong>{statusText(classification.recommended_action, "Inspect and replan")}</strong></p>
        <small>{event.run_id ? `run ${compactId(String(event.run_id))} · ` : ""}{outcome.turns != null ? `${outcome.turns} turns · ` : ""}<time title={timestampTitle(event.created_at)}>{formatTimestamp(event.created_at)}</time></small>
      </article>;
    })}</div>
  </section>;
}

function ActionInventory({ actions, completed, primaryAction, onReview }: { actions: any[]; completed: boolean; primaryAction: any; onReview: (action: any) => void }) {
  if (!actions.length) return null;
  return <details className="action-inventory">
    <summary><span><strong>All lifecycle actions</strong><small>Advanced controller detail</small></span><b>{actions.length}</b></summary>
    <div className="action-inventory-list">{actions.map((entry) => {
      const terminalComplete = completed && entry.id === "terminal";
      const status = terminalComplete ? "complete" : entry.status;
      const isPrimary = primaryAction?.id === entry.id;
      return <article key={`${entry.lifecycle_stage}-${entry.id}`}>
        <span>{statusText(entry.lifecycle_stage)}</span>
        <strong>{statusText(entry.id)}</strong>
        <StatusPill tone={status === "ready" || status === "completed" || status === "complete" ? "healthy" : status === "blocked" ? "blocked" : undefined}>{statusText(status)}</StatusPill>
        <p>{terminalComplete ? "No forward controller action remains for this completed WorkItem." : entry.external_effect_summary}</p>
        {!terminalComplete && entry.blockers?.map((blocker: any) => <small className="action-blocker" key={blocker.code}>{blocker.summary}</small>)}
        {entry.approval_requirements?.length ? <small>Approvals: {entry.approval_requirements.map(statusText).join(" · ")}</small> : null}
        <small>{statusText(entry.effect_class)} · state {compactId(entry.state_hash)}</small>
        {entry.status === "ready" && !isPrimary && entry.lifecycle_stage !== "rollback" ? <button type="button" onClick={() => onReview(entry)}>Review exact action</button> : null}
      </article>;
    })}</div>
  </details>;
}

function SectionNavigation({ active, attemptActive, onSelect }: { active: WorkItemSection; attemptActive: boolean; onSelect: (section: WorkItemSection) => void }) {
  const sections: { id: WorkItemSection; label: string }[] = [
    { id: "overview", label: "Overview" },
    { id: "attempt", label: "Attempt" },
    { id: "delivery", label: "Delivery" },
    { id: "evidence", label: "Evidence" },
  ];
  return <nav className="cockpit-section-nav" aria-label="WorkItem sections">
    {sections.map((section) => <button key={section.id} type="button" aria-current={active === section.id ? "page" : undefined} onClick={() => onSelect(section.id)}>{section.label}{section.id === "attempt" && attemptActive ? <span>Live</span> : null}</button>)}
  </nav>;
}

export function WorkItemDetailView({ workItemId, refreshDashboard, autoRefresh, operatorName, onBack }: WorkItemDetailViewProps) {
  const [state, setState] = useState<any>({ status: "loading", item: null, flow: null, preview: null, error: null });
  const [actor, setActor] = useState(getOperatorName());
  const [reason, setReason] = useState("");
  const [reviewMode, setReviewMode] = useState<false | "action" | "advance">(false);
  const [selectedRailAction, setSelectedRailAction] = useState<LifecycleAction | null>(null);
  const [selectedArtifact, setSelectedArtifact] = useState<any>(null);
  const [activeSection, setActiveSection] = useState<WorkItemSection>("overview");
  const defaultedWorkItem = useRef<string | null>(null);

  useEffect(() => {
    if (operatorName) setActor((current) => current === "console-operator" ? operatorName : current);
  }, [operatorName]);

  const refresh = async () => {
    setState((current: any) => ({ ...current, status: current.item ? "refreshing" : "loading", error: null }));
    try {
      const [item, preview, flowResult, rollbackIntent] = await Promise.all([
        loadWorkItem(workItemId),
        previewWorkItemReconcile(workItemId),
        loadWorkItemFlow(workItemId).catch(() => null),
        loadRollbackIntent(workItemId).catch(() => null),
      ]);
      setState({ status: "ready", item, preview, flow: flowResult, rollbackIntent, error: null });
    } catch (error) {
      setState((current: any) => ({ ...current, status: "error", error: error instanceof Error ? error.message : String(error) }));
    }
  };

  useEffect(() => {
    refresh();
    if (!autoRefresh) return undefined;
    const activeWait = state.flow?.controller_waits?.some((entry: any) => entry.status === "active");
    const activeWork = activeWait || ["executing", "verifying", "planning"].includes(state.item?.status);
    const timer = window.setInterval(() => {
      if (document.visibilityState === "visible") refresh();
    }, activeWork ? 5_000 : 30_000);
    return () => window.clearInterval(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workItemId, autoRefresh, state.item?.status, state.flow?.controller_waits?.length]);

  useEffect(() => {
    if (!state.item?.id || defaultedWorkItem.current === state.item.id) return;
    defaultedWorkItem.current = state.item.id;
    setActiveSection(defaultWorkItemSection(state.item));
  }, [state.item]);

  const apply = async () => {
    if (!reason.trim() || !actor.trim()) return;
    setState((current: any) => ({ ...current, status: "applying", error: null }));
    try {
      const selectedAction = selectedRailAction ?? selectPrimaryWorkItemAction(state.flow?.action_rail, state.preview, state.item?.status);
      if (selectedAction && !selectedAction.legacy_reconcile) await executeWorkItemAction(workItemId, selectedAction.id, { actor: actor.trim(), reason: reason.trim(), stateHash: selectedAction.state_hash });
      else await applyWorkItemReconcile(workItemId, { actor: actor.trim(), reason: reason.trim() });
      setReviewMode(false);
      setSelectedRailAction(null);
      setReason("");
      await refresh();
      await refreshDashboard();
    } catch (error) {
      setState((current: any) => ({ ...current, status: "ready", error: error instanceof Error ? error.message : String(error) }));
    }
  };

  const advanceSafe = async () => {
    if (!reason.trim() || !actor.trim()) return;
    setState((current: any) => ({ ...current, status: "applying", error: null }));
    try {
      await advanceWorkItem(workItemId, { actor: actor.trim(), reason: reason.trim() });
      setReviewMode(false);
      setSelectedRailAction(null);
      setReason("");
      await refresh();
      await refreshDashboard();
    } catch (error) {
      setState((current: any) => ({ ...current, status: "ready", error: error instanceof Error ? error.message : String(error) }));
    }
  };

  const item = state.item;
  const preview = state.preview;
  if (!item && state.status !== "error") return <EmptyState title="Loading WorkItem" body="Fetching the durable controller state and read-only reconcile preview." />;
  if (!item) return <EmptyState title="WorkItem unavailable" body={state.error ?? "The requested WorkItem could not be loaded."} />;

  const workspace = state.flow?.workspaces?.slice(-1)[0] ?? preview?.workspace;
  const wait = state.flow?.controller_waits?.find((entry: any) => entry.status === "active") ?? preview?.controller_wait;
  const completed = item.status === "completed";
  const actions = state.flow?.action_rail ?? [];
  const railAction = selectPrimaryWorkItemAction(actions, preview, item.status);
  const recoveryActions = selectRecoveryActions(actions);
  const canApply = railAction ? railAction.status === "ready" : (!completed && (preview?.can_apply ?? false));
  const action = railAction?.id ?? preview?.action ?? "reconcile";
  const primaryReviewAction: LifecycleAction | undefined = railAction ?? (!completed ? {
    id: preview?.action ?? "reconcile",
    lifecycle_stage: preview?.boundary ?? "source",
    resource: item.id,
    status: preview?.can_apply ? "ready" : "blocked",
    effect_class: "internal",
    blockers: preview?.blockers ?? [],
    approval_requirements: [],
    external_effect_summary: preview?.effect_summary ?? preview?.message,
    legacy_reconcile: true,
  } : undefined);
  const blockers = completed ? [] : (preview?.blockers ?? []);
  const safeAdvance = !completed && preview?.can_apply && ["declare_work_plan", "capture_change_set", "prepare_git_delivery", "complete_work_item"].includes(preview?.action);
  const incompleteDeliveryEvidence = completed && (state.flow?.delivery_segments ?? []).some((segment: any) => segment.status !== "complete");
  const attemptActive = defaultWorkItemSection(item) === "attempt";

  const reviewAction = (entry: LifecycleAction, mode: "action" | "advance" = "action") => {
    setSelectedRailAction(entry);
    setReviewMode(mode);
  };

  const reviewSafeAdvance = () => reviewAction({
    id: "advance_safe_steps",
    lifecycle_stage: preview?.boundary ?? "source",
    resource: item.id,
    status: "ready",
    effect_class: "internal",
    blockers: [],
    approval_requirements: [],
    external_effect_summary: "The server may execute at most ten idempotent internal steps and will stop before every model, approval, Git, Tekton, Argo, wait, or error boundary.",
  }, "advance");

  return <section className="work-item-detail">
    <header className="work-item-cockpit-header" id="work-item-overview">
      <button className="back-link" type="button" onClick={onBack}><ArrowLeft size={14} /> WorkItems</button>
      <div className="work-item-title-row">
        <div><span className="eyebrow">Supervised WorkItem · {compactId(item.id)}</span><h1>{item.title}</h1><p>{item.intent}</p></div>
        <StatusPill tone={lifecycleTone(item.status)}>{statusText(item.status)}</StatusPill>
      </div>
      <div className="cockpit-trust-grid">
        <ReviewItem label="Target" value={`${item.target_environment}${item.target_namespace ? ` · ${item.target_namespace}` : ""}`} />
        <ReviewItem label="Immutable source" value={<span title={`${item.source_repo} @ ${item.source_commit ?? item.source_ref}`}>{repositoryLabel(item.source_repo)} · {compactId(item.source_commit ?? item.source_ref)}</span>} tone={item.source_commit ? "healthy" : "risk"} />
        <ReviewItem label="Runner" value={item.environment_profile_id ?? "Legacy generic runner"} />
        <ReviewItem label="Last controller change" value={<time title={timestampTitle(item.updated_at)}>{formatTimestamp(item.updated_at)}</time>} />
      </div>
    </header>

    <SectionNavigation active={activeSection} attemptActive={attemptActive} onSelect={setActiveSection} />

    {completed ? <section className="action-center is-complete" aria-label="WorkItem outcome">
      <CheckCircle size={28} weight="fill" />
      <div><span className="eyebrow">Outcome</span><h2>WorkItem complete</h2><p>{item.status_reason ?? "The controller recorded successful completion."}</p></div>
      <button type="button" onClick={refresh}>Refresh evidence</button>
    </section> : <section className={`action-center ${canApply ? "is-ready" : "is-blocked"}`} aria-label="Current lifecycle boundary">
      <div className="action-center-icon">{wait ? <Clock size={24} /> : canApply ? <RocketLaunch size={24} /> : <Warning size={24} />}</div>
      <div className="action-center-copy"><span className="eyebrow">Next required step · {statusText(railAction?.lifecycle_stage ?? preview?.boundary, "Controller boundary")}</span><h2>{statusText(action)}</h2><p>{railAction?.external_effect_summary ?? preview?.effect_summary ?? preview?.message ?? "The controller has not supplied a preview yet."}</p></div>
      {wait ? <WaitSummary wait={wait} /> : null}
      <div className="reconcile-actions"><button className="primary-action" type="button" disabled={!canApply || state.status === "applying" || !primaryReviewAction} title={!canApply ? (railAction?.blockers?.[0]?.summary ?? preview?.blockers?.[0]?.summary ?? preview?.message ?? "The controller cannot apply this action yet.") : undefined} onClick={() => primaryReviewAction && reviewAction(primaryReviewAction)}><RocketLaunch size={17} /> {statusText(action)}</button>{safeAdvance ? <button type="button" onClick={reviewSafeAdvance} title="Execute only idempotent internal controller transitions and stop before model execution or any external effect.">Advance internal steps</button> : null}<button type="button" onClick={refresh}>Refresh preview</button></div>
    </section>}

    {blockers.length ? <section className="reconcile-blockers" aria-label="Controller blockers"><span className="eyebrow">Why this cannot advance</span>{blockers.map((blocker: any) => {
      const correctiveAction = findCorrectiveAction(blocker, actions);
      return <p key={`${blocker.code}-${blocker.summary}`}><strong>{statusText(blocker.code)}</strong><span>{blocker.summary}</span>{correctiveAction ? <button type="button" onClick={() => reviewAction(correctiveAction)}>Review {statusText(correctiveAction.id)}</button> : null}</p>;
    })}</section> : null}

    {recoveryActions.length ? <section className="recovery-options" aria-label="Recovery options">
      <Lifebuoy size={25} />
      <div><span className="eyebrow">Recovery options</span><h2>{completed ? "Rollback is prepared, not recommended" : "Rollback readiness"}</h2><p>{completed ? "The release completed successfully. These actions are retained as an emergency contingency and are not part of forward progress." : "Rollback never runs automatically. Review the captured baseline and exact target before continuing."}</p></div>
      <div>{recoveryActions.map((entry: any) => <article key={entry.id}><span><strong>{statusText(entry.id)}</strong><StatusPill tone={entry.status === "ready" ? "healthy" : "blocked"}>{statusText(entry.status)}</StatusPill></span><p>{entry.external_effect_summary}</p>{entry.approval_requirements?.length ? <small>Approvals: {entry.approval_requirements.map(statusText).join(" · ")}</small> : null}{entry.status === "ready" ? <button type="button" onClick={() => reviewAction(entry)}>Review exact action</button> : null}</article>)}</div>
    </section> : null}

    {incompleteDeliveryEvidence ? <section className="evidence-consistency-warning" role="status"><Warning size={20} /><div><strong>Delivery evidence needs reconciliation</strong><p>The controller reports this WorkItem as completed, while the server-backed delivery stage model still contains incomplete stages. No missing stage state has been inferred; inspect the durable artifacts before reusing this record as release proof.</p></div></section> : null}

    {activeSection === "overview" ? <section className="work-item-run-envelope" id="work-item-overview-panel">
      <div><span className="eyebrow">Execution envelope</span><h2>Attempt and trust boundaries</h2></div>
      <div className="work-item-facts"><ReviewItem label="Attempts" value={`${item.attempt_count} used / ${item.max_attempts} total`} /><ReviewItem label="Turns" value={`${item.run_budget?.initial_turns ?? 48} initial / ${item.run_budget?.hard_turns ?? 100} hard`} /><ReviewItem label="Tokens" value={`${item.run_budget?.initial_tokens ?? 400000} initial / ${item.run_budget?.hard_tokens ?? 1000000} hard`} /><ReviewItem label="Workspace" value={workspace ? `${repositoryLabel(workspace.source_repo)} · ${compactId(workspace.resolved_commit ?? workspace.source_ref)}` : "Not provisioned"} /><ReviewItem label="Attempt branch" value={workspace?.branch ?? "Not created"} /><ReviewItem label="Retention" value={workspace?.retention_status ?? "Not created"} /><ReviewItem label="Acceptance commands" value={item.acceptance_criteria?.join(" · ") || "No explicit criteria"} /><ReviewItem label="WorkItem" value={<CopyIdentifier value={item.id} label={item.title} />} /></div>
    </section> : null}

    {activeSection === "attempt" ? <section className="cockpit-section attempt-cockpit-section" id="work-item-attempt"><div className="cockpit-section-heading"><span className="eyebrow">Attempt</span><h2>Agent workspace</h2><p>Environment, execution budget, live tool activity, changes, and acceptance evidence for the current isolated run.</p></div>{item.current_run_id ? <RunDetailView runId={String(item.current_run_id)} refreshDashboard={refreshDashboard} onOpenQueue={() => {}} operatorName={actor} embedded /> : <EmptyState title="No active attempt" body="The live model and tool console appears here while an approved coding attempt is active." />}<AttemptHistory events={state.flow?.audit_events ?? []} /></section> : null}

    {activeSection === "delivery" ? <section className="cockpit-section" id="work-item-delivery">
      <div className="cockpit-section-heading"><span className="eyebrow">Delivery</span><h2>External delivery and release evidence</h2><p>Manual merges, Tekton output, GitOps provenance, Argo state, and verification stay in the stage that owns them.</p></div>
      <DeliveryWorkspace flow={state.flow} item={item} onOpenResource={setSelectedArtifact} />
    </section> : null}

    {activeSection === "evidence" ? <section className="cockpit-section" id="work-item-evidence"><div className="cockpit-section-heading"><span className="eyebrow">Evidence</span><h2>Durable controller record</h2></div><section className="evidence-summary"><ReviewItem label="Immutable source" value={item.source_commit ?? "Legacy mutable source"} tone={item.source_commit ? "healthy" : "risk"} /><ReviewItem label="Audit events" value={state.flow?.audit_events?.length ?? 0} /><ReviewItem label="Persisted workspaces" value={state.flow?.workspaces?.length ?? 0} /><ReviewItem label="Controller waits" value={state.flow?.controller_waits?.length ?? 0} /></section></section> : null}

    <ActionInventory actions={actions} completed={completed} primaryAction={railAction} onReview={reviewAction} />

    {reviewMode && selectedRailAction ? <LifecycleReviewDrawer action={selectedRailAction} actions={actions} item={item} flow={state.flow} preview={preview} rollbackIntent={state.rollbackIntent} actor={actor} reason={reason} applying={state.status === "applying"} error={state.error} onActorChange={setActor} onReasonChange={setReason} onActionChange={setSelectedRailAction} onConfirm={reviewMode === "advance" ? advanceSafe : apply} onClose={() => { setReviewMode(false); setSelectedRailAction(null); setReason(""); }} /> : null}
    {selectedArtifact ? <DurableArtifactPanel artifact={selectedArtifact} onClose={() => setSelectedArtifact(null)} /> : null}
    {state.error ? <div className="api-banner">{state.error}</div> : null}
  </section>;
}

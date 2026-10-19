//! Production promotion after verified staging: one read-only preview, one
//! human decision bound to the exact diff, one admitted GitOps write, a ten
//! minute production window sealing `release`, and a second window sealing
//! `observe` before the WorkItem completes. Every step is a recorded operation
//! with its own clocks; nothing here repeats source, build or staging work.
use super::staging::{self, evidence, required, runtime};
use super::state::{condition, now, Condition, Snapshot};
use crate::app::hashing::canonical_material_hash as hash;
use crate::app::{ApiError, AppState, CONTROLLER_WAIT_INTERVAL_MS, CONTROLLER_WAIT_MAX_CHECKS};
use pharness_core::hosted_sdlc::{HostedAutomaticAction, ProductionApprovalBoundary};
use pharness_store::{
    BeginWorkflowOperation, StoredWorkflowOperation, StoredWorkflowReconciliation,
};
use serde_json::json;

mod callbacks;
mod delivery;
mod gate;
mod observe;
mod proposal;
mod seal;
pub(in crate::app) use callbacks::{
    internal_production_attempt, internal_production_context, internal_production_outcome,
    internal_production_preview, internal_production_preview_context,
};
pub(in crate::app) use gate::{decide as decide_gate, GATE_KIND};

pub(super) const PROPOSE_ACTION: &str = "propose_production_release";
pub(super) const PROMOTE_ACTION: &str = "promote_approved_release";
pub(super) const OBSERVE_ACTION: &str = "observe_production_release";
pub(super) const ACTIONS: [&str; 3] = [PROPOSE_ACTION, PROMOTE_ACTION, OBSERVE_ACTION];
pub(super) const GITOPS_LOCK: &str = "gitops:lucas_engineering:main";

fn deadline(operation: &StoredWorkflowOperation) -> i64 {
    operation.created_at.saturating_add(
        (CONTROLLER_WAIT_INTERVAL_MS as i64).saturating_mul(i64::from(CONTROLLER_WAIT_MAX_CHECKS)),
    )
}
fn artifact_id(kind: &str, execution: &str) -> String {
    format!("production_{kind}_{execution}")
}
fn suffix(operation: &StoredWorkflowOperation) -> &str {
    operation.id.trim_start_matches("workflowop_")
}
fn operation_id(work_item_id: &str, action: &str, input_hash: &str) -> Result<String, ApiError> {
    let identity = hash(&json!([work_item_id, action, input_hash]))?;
    Ok(format!(
        "workflowop_{}",
        identity.trim_start_matches("sha256:")
    ))
}
pub(super) fn production_lock(repository_id: &str) -> String {
    format!("delivery:{repository_id}:production")
}

pub(super) struct Start {
    pub action: &'static str,
    pub input_hash: String,
    pub resource: String,
    pub effect: &'static str,
    pub keys: Vec<String>,
}

pub(super) enum Step {
    Start(Start),
    Wait(Condition),
}

/// The next production step for a WorkItem whose staging window is verified.
/// `None` means staging has not completed; the caller keeps its own waits.
pub(super) async fn next(state: &AppState, snapshot: &Snapshot) -> Result<Option<Step>, ApiError> {
    let Some(verified) = staging::verified(state, snapshot).await? else {
        return Ok(None);
    };
    let (policy, _) = super::state::workflow_policy(&snapshot.metadata)?;
    if !policy
        .automatic_actions
        .contains(&HostedAutomaticAction::Observe)
        || policy.production_approval != ProductionApprovalBoundary::BeforeGitopsMerge
    {
        return Err(ApiError::conflict(
            "the saved workflow does not authorize production observation after a human decision",
        ));
    }
    let item = &snapshot.metadata.work_item_id;
    let repository = &snapshot.metadata.repository_id;
    let production = production_lock(repository);
    let repo = format!("repository:{repository}");

    // An operation that exists but has not succeeded is either in flight (the
    // caller recovers it with this same Start) or terminally blocked.
    let propose_input = proposal::input_hash(&verified, &snapshot.metadata)?;
    let propose_id = operation_id(item, PROPOSE_ACTION, &propose_input)?;
    let propose = state.store.get_workflow_operation(&propose_id).await?;
    let propose = match propose.as_ref().map(|o| o.status.as_str()) {
        Some("succeeded") => propose.unwrap(),
        Some("blocked") => {
            return Ok(Some(Step::Wait(condition("blocked", "The production proposal could not be completed. Its preview and baseline evidence are retained; no production change was made."))));
        }
        _ => {
            return Ok(Some(Step::Start(Start {
                action: PROPOSE_ACTION,
                input_hash: propose_input,
                resource: verified.saved.deployment.id.clone(),
                effect: "observation",
                keys: vec![repo, production],
            })));
        }
    };
    let recorded = proposal::recorded(state, &propose).await?;
    let promote_input = delivery::input_hash(&recorded)?;
    let promote_id = operation_id(item, PROMOTE_ACTION, &promote_input)?;
    let promote = state.store.get_workflow_operation(&promote_id).await?;
    let decision = gate::state(state, &recorded).await?;
    let approval = match decision {
        gate::Decision::Pending => {
            return Ok(Some(Step::Wait(condition("awaiting_human", "The staged candidate is verified. Its exact production diff awaits one operator decision; nothing is written to production until then."))));
        }
        gate::Decision::Rejected => {
            return Ok(Some(Step::Wait(condition("blocked", "The production release was rejected by an operator. Staging evidence is retained and production is unchanged."))));
        }
        gate::Decision::Approved(approval) => approval,
    };
    let promote = match promote.as_ref().map(|o| o.status.as_str()) {
        Some("succeeded") => promote.unwrap(),
        Some("blocked") => {
            return Ok(Some(Step::Wait(condition("blocked", "Production promotion did not verify. Its commit, deployment and runtime evidence are retained; no further production change is started."))));
        }
        None if now() >= approval.approval.expires_at_ms => {
            return Ok(Some(Step::Wait(condition("blocked", "The production approval expired before promotion started. A new proposal and decision are required; production is unchanged."))));
        }
        _ => {
            return Ok(Some(Step::Start(Start {
                action: PROMOTE_ACTION,
                input_hash: promote_input,
                resource: recorded.deployment.id.clone(),
                effect: "development",
                keys: vec![repo, production, GITOPS_LOCK.into()],
            })));
        }
    };
    let release = seal::release_sealed(state, snapshot, &promote).await?;
    let observe_input = observe::input_hash(&recorded, &release)?;
    let observe_id = operation_id(item, OBSERVE_ACTION, &observe_input)?;
    let observed = state.store.get_workflow_operation(&observe_id).await?;
    let observed = match observed.as_ref().map(|o| o.status.as_str()) {
        Some("succeeded") => observed.unwrap(),
        Some("blocked") => {
            return Ok(Some(Step::Wait(condition("blocked", "Post-release production observation did not verify. Its evidence is retained; the WorkItem stays open for operator review."))));
        }
        _ => {
            return Ok(Some(Step::Start(Start {
                action: OBSERVE_ACTION,
                input_hash: observe_input,
                resource: recorded.deployment.id.clone(),
                effect: "observation",
                keys: vec![production],
            })));
        }
    };
    observe::finish(state, snapshot, &observed).await?;
    Ok(Some(Step::Wait(condition(
        "closed",
        "Production release and observation are sealed; the WorkItem is complete.",
    ))))
}

pub(super) async fn reconcile(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    snapshot: &Snapshot,
    operation: &StoredWorkflowOperation,
    expired: bool,
) -> Result<Condition, ApiError> {
    if operation.status == "blocked" {
        return Ok(condition(
            "blocked",
            operation.resource_refs["blocked"]["reason"]
                .as_str()
                .unwrap_or("The production operation is blocked; its evidence is retained."),
        ));
    }
    let timed_out = now() >= deadline(operation);
    let expired = expired || timed_out;
    let dispatch_key = match operation.action.as_str() {
        PROPOSE_ACTION => "production_preview_dispatch",
        PROMOTE_ACTION => "production_dispatch",
        _ => "observe_dispatch",
    };
    let mut operation = operation.clone();
    if operation.resource_refs.get(dispatch_key).is_none() {
        if claim.control != "active" || expired {
            if operation.status == "pending" {
                state
                    .store
                    .release_pending_workflow_locks(claim, &operation.id, now())
                    .await?;
            }
            return Ok(condition(
                if expired {
                    "wait_expired"
                } else {
                    &claim.control
                },
                "New production work is stopped; the original operation remains recorded.",
            ));
        }
        let start = match next(state, snapshot).await? {
            Some(Step::Start(start)) if start.action == operation.action => start,
            _ => {
                return Err(ApiError::conflict(
                    "the recorded production operation is no longer the next eligible step",
                ))
            }
        };
        if start.input_hash != operation.input_hash {
            return Err(ApiError::conflict(
                "production evidence or decision changed before preparation",
            ));
        }
        if operation.status == "pending" {
            let keys = operation
                .resource_keys
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            state
                .store
                .begin_workflow_operation(
                    claim,
                    BeginWorkflowOperation {
                        id: &operation.id,
                        action: &operation.action,
                        input_hash: &operation.input_hash,
                        effect: &operation.effect,
                        resource_keys: &keys,
                    },
                    now(),
                )
                .await?;
        }
        let mut refs = operation.resource_refs.clone();
        refs["action_resource"] = json!(start.resource);
        operation = state
            .store
            .record_workflow_operation(
                claim,
                &operation.id,
                "running",
                &refs,
                "Recover the original production step without changing its authority",
                now(),
            )
            .await?;
        operation = match operation.action.as_str() {
            PROPOSE_ACTION => proposal::prepare(state, claim, snapshot, &operation).await?,
            PROMOTE_ACTION => delivery::prepare(state, claim, snapshot, &operation).await?,
            _ => observe::prepare(state, claim, snapshot, &operation).await?,
        };
    }
    match operation.action.as_str() {
        PROPOSE_ACTION => {
            proposal::reconcile(state, claim, snapshot, &operation, expired, timed_out).await
        }
        PROMOTE_ACTION => {
            delivery::reconcile(state, claim, snapshot, &operation, expired, timed_out).await
        }
        _ => observe::reconcile(state, claim, snapshot, &operation, timed_out).await,
    }
}

/// Mark an operation blocked once its native evidence proves it cannot finish.
async fn block(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    operation: &StoredWorkflowOperation,
    key: &str,
    reason: &str,
) -> Result<Condition, ApiError> {
    let mut refs = operation.resource_refs.clone();
    refs[key] = json!({"status":"blocked","reason":reason,"blocked_at_ms":now()});
    refs["blocked"] = json!({"key":key,"reason":reason});
    state
        .store
        .record_workflow_operation(claim, &operation.id, "blocked", &refs, reason, now())
        .await?;
    Ok(condition("blocked", reason))
}

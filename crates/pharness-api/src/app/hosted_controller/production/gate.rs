//! The human production decision. The gate carries the complete reviewed file
//! and its proposed replacement; the decision persists a HumanProductionApproval
//! bound to the proposal and plan hashes, written only by this authenticated
//! operator path and never by a worker callback or automatic approval.
use super::super::state::Snapshot;
use super::proposal::{Dispatched, Recorded};
use super::{artifact_id, evidence, now, staging, ApiError, AppState};
use crate::app::auth::DecisionActor;
use pharness_core::hosted_sdlc::production::{
    HumanProductionApproval, ProductionGitOpsPlan, ProductionProposal, APPROVAL_SCHEMA,
    MAX_APPROVAL_MS,
};
use pharness_store::{CreateApprovalGate, StoredApprovalGate, StoredArtifact};
use serde_json::{json, Value};

pub(in crate::app) const GATE_KIND: &str = "hosted_production_deployment";
const GATE_SCHEMA: &str = "pharness.dev/hosted-production-gate/v1alpha1";

pub(super) struct Approved {
    pub approval: HumanProductionApproval,
    pub artifact: StoredArtifact,
    pub approval_hash: String,
}

pub(super) enum Decision {
    Pending,
    Rejected,
    Approved(Box<Approved>),
}

fn gate_json(
    dispatched: &Dispatched,
    proposal: &ProductionProposal,
    plan: &ProductionGitOpsPlan,
    proposal_artifact_id: &str,
) -> Result<Value, ApiError> {
    let (path, image) = proposal.coordinates().map_err(ApiError::conflict)?;
    Ok(json!({
        "schema_version":GATE_SCHEMA,
        "work_item_id":proposal.work_item_id,
        "operation_id":proposal.operation_id,
        "deployment_intent_id":proposal.deployment_intent_id,
        "execution_id":dispatched.authority.execution_id,
        "proposal_artifact_id":proposal_artifact_id,
        "proposal_hash":proposal.material_hash().map_err(ApiError::conflict)?,
        "plan_hash":plan.material_hash().map_err(ApiError::conflict)?,
        "proposal":proposal,
        "plan":plan,
        "review":{
            "gitops_repository":"lward27/lucas_engineering","target_ref":"main","path":path,
            "expected_head_oid":plan.base_commit_sha,"image":image,
            "previous_image_digest":proposal.previous_image_digest,"candidate_image_digest":proposal.image_digest,
            "source_commit_sha":proposal.source_commit_sha,"application_repository":proposal.application_repository,
            "staging_evidence_artifact_id":proposal.staging_evidence_artifact_id,
            "baseline_evidence_artifact_id":proposal.baseline_evidence_artifact_id,
        },
        "approval_window_ms":MAX_APPROVAL_MS,
        "decision_endpoint":"single_gate_only",
    }))
}

pub(super) async fn ensure(
    state: &AppState,
    dispatched: &Dispatched,
    proposal: &ProductionProposal,
    plan: &ProductionGitOpsPlan,
    proposal_artifact_id: &str,
    gate_id: &str,
) -> Result<StoredApprovalGate, ApiError> {
    let document = gate_json(dispatched, proposal, plan, proposal_artifact_id)?;
    if let Some(existing) = state.store.get_approval_gate(gate_id).await? {
        if existing.gate_kind != GATE_KIND
            || existing.work_item_id.as_deref() != Some(&proposal.work_item_id)
            || existing.gate_json != document
        {
            return Err(ApiError::conflict(
                "an existing production gate cannot be rebound to a different proposal",
            ));
        }
        return Ok(existing);
    }
    let workload = dispatched
        .deployment
        .resource_name
        .clone()
        .unwrap_or_default();
    state
        .store
        .create_approval_gate(CreateApprovalGate {
            id: gate_id.into(),
            work_item_id: Some(proposal.work_item_id.clone()),
            remediation_plan_id: None,
            incident_id: None,
            session_id: dispatched.pipeline.session_id.clone(),
            run_id: dispatched.pipeline.run_id.clone(),
            status: "pending".into(),
            gate_kind: GATE_KIND.into(),
            gate_order: 1,
            title: format!("Approve production release of {workload}"),
            summary: format!(
                "Replace {} with {} in {} at GitOps {}; staging verified, rollback {:?}",
                proposal.previous_image_digest,
                proposal.image_digest,
                proposal.coordinates().map_err(ApiError::conflict)?.0,
                &plan.base_commit_sha[..12],
                proposal.rollback
            ),
            risk_level: "high".into(),
            resource_namespace: Some("apps-prod".into()),
            resource_kind: Some("Deployment".into()),
            resource_name: Some(workload),
            gate_json: document,
        })
        .await
        .map_err(Into::into)
}

async fn load_gate(state: &AppState, recorded: &Recorded) -> Result<StoredApprovalGate, ApiError> {
    let gate = state
        .store
        .get_approval_gate(&recorded.gate_id)
        .await?
        .ok_or_else(|| ApiError::conflict("production approval gate is unavailable"))?;
    if gate.gate_kind != GATE_KIND
        || gate.work_item_id.as_deref() != Some(&recorded.proposal.work_item_id)
        || gate.gate_json["proposal_hash"] != recorded.proposal_hash
        || gate.gate_json["plan_hash"] != recorded.plan_hash
        || gate.gate_json["proposal_artifact_id"] != recorded.proposal_artifact_id
        || gate.gate_json["deployment_intent_id"] != recorded.deployment.id
    {
        return Err(ApiError::conflict(
            "production approval gate differs from the recorded proposal",
        ));
    }
    Ok(gate)
}

pub(super) async fn state(state_: &AppState, recorded: &Recorded) -> Result<Decision, ApiError> {
    let gate = load_gate(state_, recorded).await?;
    match gate.status.as_str() {
        "pending" => Ok(Decision::Pending),
        "satisfied" => approved(state_, recorded, &gate)
            .await
            .map(|approved| Decision::Approved(Box::new(approved))),
        _ => Ok(Decision::Rejected),
    }
}

pub(super) async fn approved(
    state: &AppState,
    recorded: &Recorded,
    gate: &StoredApprovalGate,
) -> Result<Approved, ApiError> {
    let artifact = state
        .store
        .get_artifact(&artifact_id("approval", &recorded.authority.execution_id))
        .await?
        .ok_or_else(|| ApiError::conflict("satisfied production gate has no approval record"))?;
    let body = artifact
        .content_json
        .as_ref()
        .ok_or_else(|| ApiError::conflict("production approval has no content"))?;
    let approval: HumanProductionApproval = serde_json::from_value(body["approval"].clone())
        .map_err(|_| ApiError::conflict("production approval is invalid"))?;
    // Structural validity at approval time; the delivery window is checked by
    // whoever consumes this decision, so an expired grant still loads here.
    approval
        .validate_for_delivery(&recorded.proposal, &recorded.plan, approval.approved_at_ms)
        .map_err(ApiError::conflict)?;
    if artifact.kind != "hosted_production_approval"
        || artifact.session_id != recorded.pipeline.session_id
        || artifact.run_id != recorded.pipeline.run_id
        || body["gate_id"] != gate.id
        || gate.decided_by.as_deref() != Some(approval.operator_identity.as_str())
        || gate.decision_reason.as_deref() != Some(approval.reason.as_str())
        || gate.status != "satisfied"
    {
        return Err(ApiError::conflict(
            "production approval record differs from the decided gate",
        ));
    }
    Ok(Approved {
        approval_hash: approval.material_hash().map_err(ApiError::conflict)?,
        approval,
        artifact,
    })
}

/// Called from the operator gate endpoints. Decides the gate and persists the
/// bound approval together under the controller's write boundary.
pub(in crate::app) async fn decide(
    state: &AppState,
    actor: DecisionActor,
    gate: StoredApprovalGate,
    status: &str,
    reason: Option<String>,
) -> Result<StoredApprovalGate, ApiError> {
    let _boundary = super::super::DISPATCH_BOUNDARY.lock().await;
    let (name, claimed) = actor.require("production decisions require an identified operator")?;
    let reason = reason
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty())
        .ok_or_else(|| ApiError::bad_request("production decisions require a reason"))?;
    if !matches!(status, "satisfied" | "rejected") {
        return Err(ApiError::conflict(
            "production release cannot be waived; approve or reject the exact diff",
        ));
    }
    let gate = state
        .store
        .get_approval_gate(&gate.id)
        .await?
        .ok_or_else(|| ApiError::not_found("approval_gate", &gate.id))?;
    if gate.status != "pending" {
        return Err(ApiError::conflict("approval gate is not pending"));
    }
    let work_item_id = gate
        .work_item_id
        .clone()
        .ok_or_else(|| ApiError::conflict("production gate has no WorkItem"))?;
    let operation_id = gate.gate_json["operation_id"]
        .as_str()
        .ok_or_else(|| ApiError::conflict("production gate has no proposal operation"))?;
    let operation = state
        .store
        .get_workflow_operation(operation_id)
        .await?
        .filter(|o| o.work_item_id == work_item_id && o.status == "succeeded")
        .ok_or_else(|| ApiError::conflict("production proposal operation is unavailable"))?;
    let recorded = super::proposal::recorded(state, &operation).await?;
    if recorded.gate_id != gate.id {
        return Err(ApiError::conflict(
            "production gate belongs to another proposal",
        ));
    }
    load_gate(state, &recorded).await?;
    // The decision is only meaningful over current evidence: staging must still
    // verify, the saved workflow must be unchanged and work must not be stopped.
    let snapshot = Snapshot::load(state, &work_item_id).await?;
    let still_verified = staging::verified(state, &snapshot)
        .await?
        .is_some_and(|v| v.result.id == recorded.proposal.staging_evidence_artifact_id);
    if status == "satisfied"
        && (!still_verified
            || snapshot.metadata.workflow_policy_hash.as_deref()
                != Some(recorded.proposal.workflow_policy_hash.as_str())
            || !evidence::may_advance(state, &work_item_id).await?)
    {
        return Err(ApiError::conflict(
            "production cannot be approved: staging evidence, workflow or control changed",
        ));
    }
    let execution = &recorded.authority.execution_id;
    if status == "satisfied" {
        let approved_at = now();
        let approval = HumanProductionApproval {
            schema_version: APPROVAL_SCHEMA.into(),
            work_item_id: work_item_id.clone(),
            operation_id: operation.id.clone(),
            proposal_hash: recorded.proposal_hash.clone(),
            gitops_plan_hash: recorded.plan_hash.clone(),
            operator_identity: name.clone(),
            reason: reason.clone(),
            approved_at_ms: approved_at,
            expires_at_ms: approved_at.saturating_add(MAX_APPROVAL_MS),
        };
        approval
            .validate_for_delivery(&recorded.proposal, &recorded.plan, approved_at)
            .map_err(ApiError::conflict)?;
        let id = artifact_id("approval", execution);
        if state.store.get_artifact(&id).await?.is_some() {
            return Err(ApiError::conflict(
                "a production approval is already recorded for this proposal",
            ));
        }
        evidence::artifact(state, &recorded.pipeline, &id, "hosted_production_approval", json!({"approval":approval,"approval_hash":approval.material_hash().map_err(ApiError::conflict)?,"gate_id":gate.id,"claimed_by":claimed,"proposal_hash":recorded.proposal_hash,"plan_hash":recorded.plan_hash})).await?;
    } else {
        evidence::artifact(state, &recorded.pipeline, &artifact_id("rejection", execution), "hosted_production_rejection", json!({"gate_id":gate.id,"operator_identity":name,"reason":reason,"claimed_by":claimed,"proposal_hash":recorded.proposal_hash,"decided_at_ms":now()})).await?;
    }
    let decided = state
        .store
        .decide_approval_gate(&gate.id, status, Some(name.clone()), Some(reason.clone()))
        .await?;
    state
        .store
        .update_deployment_intent_status(
            &recorded.deployment.id,
            if status == "satisfied" {
                "approved"
            } else {
                "rejected"
            },
            Some(name),
            Some(reason),
        )
        .await?;
    state.store.wake_workflow(&work_item_id, now()).await?;
    Ok(decided)
}

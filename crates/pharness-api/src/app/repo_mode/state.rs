use crate::app::hashing::canonical_material_hash;
use crate::app::identifiers::new_prefixed_id;
use crate::app::{ApiError, AppState};
use pharness_store::{
    CreateAuditEvent, CreateStageExecution, SealStageOutcome, StoredRepoWorkItemMetadata,
};
use serde_json::{json, Value};

pub(in crate::app) async fn is_repo_work_item(
    state: &AppState,
    work_item_id: &str,
) -> Result<bool, ApiError> {
    Ok(state
        .store
        .get_repo_work_item_metadata(work_item_id)
        .await?
        .is_some())
}

pub(super) async fn append_repo_audit(
    state: &AppState,
    work_item_id: &str,
    kind: &str,
    actor: &str,
    reason: &str,
    payload: Value,
) -> Result<(), ApiError> {
    state
        .store
        .create_audit_event(CreateAuditEvent {
            id: new_prefixed_id("audit"),
            kind: kind.into(),
            actor: Some(actor.into()),
            resource_kind: "work_item".into(),
            resource_id: work_item_id.into(),
            run_id: None,
            payload_json: json!({"reason":reason,"details":payload}),
        })
        .await?;
    seal_repo_inapplicable_tail(&state.store, work_item_id).await?;
    Ok(())
}

pub(in crate::app) async fn seal_repo_inapplicable_tail(
    store: &pharness_store::SqliteStore,
    work_item_id: &str,
) -> Result<(), ApiError> {
    let metadata = store
        .get_repo_work_item_metadata(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repo_work_item", work_item_id))?;
    if metadata.workflow_policy.is_some() {
        return Ok(());
    }
    let existing = store.list_effective_stage_outcomes(work_item_id).await?;
    for stage in [
        pharness_core::RepoStageKey::Release,
        pharness_core::RepoStageKey::Observe,
    ] {
        if existing
            .iter()
            .any(|outcome| outcome.stage_key == stage.as_str())
        {
            continue;
        }
        let input = json!({
            "mode":"repo",
            "source_only":true,
            "upstream_stage":"source_delivery",
        });
        let execution = store
            .create_stage_execution(CreateStageExecution {
                id: new_prefixed_id("stageexec"),
                work_item_id: work_item_id.into(),
                stage_key: stage.as_str().into(),
                sequence: 1,
                status: "inapplicable".into(),
                agent_profile_id: None,
                agent_profile_version: None,
                agent_profile_hash: None,
                context_pack_id: None,
                run_id: None,
                workspace_id: None,
                input_hash: canonical_material_hash(&input)?,
                input_snapshot: input.clone(),
            })
            .await?;
        let metadata = store
            .get_repo_work_item_metadata(work_item_id)
            .await?
            .ok_or_else(|| ApiError::not_found("repo_work_item", work_item_id))?;
        let document = pharness_core::StageOutcomeDocument {
            schema_version: pharness_core::STAGE_OUTCOME_SCHEMA.into(),
            work_item_id: work_item_id.into(),
            stage_execution_id: execution.id.clone(),
            stage,
            origin: "controller".into(),
            status: pharness_core::StageTerminalStatus::Inapplicable,
            objective: json!({"kind":"repo_mode_source_only_boundary"}),
            pinned_inputs: input,
            verified_facts: vec![json!({
                "kind":"mode_contract",
                "mode":"repo",
                "source_delivery_only":true,
            })],
            agent_claims: Vec::new(),
            outputs: Vec::new(),
            acceptance: Vec::new(),
            decisions: vec![json!({
                "kind":"controller_applicability",
                "status":"inapplicable",
            })],
            authorizations: Vec::new(),
            contradictions: Vec::new(),
            risks: Vec::new(),
            unavailable_capabilities: Vec::new(),
            recommendations: Vec::new(),
            stop_reason: "Repo Mode V1 closes after observed source merge; deployment and post-deploy observation are out of scope".into(),
            sealed_state_version: metadata.state_version,
        };
        let value = serde_json::to_value(document)
            .map_err(|error| ApiError::internal(error.to_string()))?;
        store
            .seal_stage_outcome(SealStageOutcome {
                id: new_prefixed_id("stageout"),
                stage_execution_id: execution.id,
                work_item_id: work_item_id.into(),
                stage_key: stage.as_str().into(),
                status: "inapplicable".into(),
                content_hash: canonical_material_hash(&value)?,
                outcome: value,
                state_version: metadata.state_version,
                supersedes_outcome_id: None,
                effective: true,
                actor: "controller:repo-mode".into(),
                reason: "Repo Mode V1 source-only lifecycle boundary".into(),
            })
            .await?;
    }
    Ok(())
}

pub(super) async fn repo_metadata(
    state: &AppState,
    work_item_id: &str,
) -> Result<StoredRepoWorkItemMetadata, ApiError> {
    state
        .store
        .get_repo_work_item_metadata(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("repo_work_item", work_item_id))
}

pub(in crate::app) fn repo_work_item_state_hash(
    metadata: &StoredRepoWorkItemMetadata,
) -> Result<String, ApiError> {
    let mut material = json!({
        "work_item_id": metadata.work_item_id,
        "state_version": metadata.state_version,
        "product_model_snapshot_id": metadata.product_model_snapshot_id,
        "product_model_snapshot_hash": metadata.product_model_snapshot_hash,
        "repository_contract_version_id": metadata.repository_contract_version_id,
        "current_stage_execution_id": metadata.current_stage_execution_id,
        "closed_at": metadata.closed_at,
    });
    if let Some(hash) = &metadata.workflow_policy_hash {
        material["workflow_policy_hash"] = json!(hash);
    }
    canonical_material_hash(&material)
}

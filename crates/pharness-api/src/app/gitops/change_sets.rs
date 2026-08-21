use super::super::audit::append_gitops_change_set_audit_event;
use super::super::clock::unique_suffix;
use super::super::json_values::string_at;
use super::super::pipeline::intents::pipeline_intent_is_deployment_eligible;
use super::super::validation::{clean_optional_text, required_text};
use super::super::work_items::preflight::work_item_target_supported;
use super::super::work_items::reconcile::gitops_observation_closed_unmerged;
use super::super::{ApiError, AppState};
use super::delivery::{
    gitops_delivery_artifact_matches_plan, gitops_delivery_plan_matches_change_set,
};
use crate::dto::{
    CreateGitOpsChangeSetRequest, CreateGitOpsChangeSetResponse, GitOpsChangeSetResponse,
    GitOpsChangeSetsResponse, TransitionGitOpsChangeSetRequest, TransitionGitOpsChangeSetResponse,
};
use axum::extract::{Path, Query, State};
use axum::Json;
use pharness_store::{CreateGitOpsChangeSet, GitOpsChangeSetListFilter};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub(in crate::app) fn safe_relative_gitops_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        && path.len() <= 512
}

#[derive(Debug, Default, serde::Deserialize)]
pub(in crate::app) struct ListGitOpsChangeSetsQuery {
    work_item_id: Option<String>,
    pipeline_intent_id: Option<String>,
    deployment_intent_id: Option<String>,
    status: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
}

pub(in crate::app) async fn list_gitops_change_sets(
    State(state): State<AppState>,
    Query(query): Query<ListGitOpsChangeSetsQuery>,
) -> Result<Json<GitOpsChangeSetsResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let gitops_change_sets = state
        .store
        .list_gitops_change_sets(GitOpsChangeSetListFilter {
            work_item_id: clean_optional_text(query.work_item_id),
            pipeline_intent_id: clean_optional_text(query.pipeline_intent_id),
            deployment_intent_id: clean_optional_text(query.deployment_intent_id),
            status: clean_optional_text(query.status),
            limit,
            offset,
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    let count = gitops_change_sets.len();
    Ok(Json(GitOpsChangeSetsResponse {
        gitops_change_sets,
        count,
        limit,
        offset,
    }))
}

pub(in crate::app) async fn get_gitops_change_set(
    State(state): State<AppState>,
    Path(gitops_change_set_id): Path<String>,
) -> Result<Json<GitOpsChangeSetResponse>, ApiError> {
    let change_set = state
        .store
        .get_gitops_change_set(&gitops_change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("gitops_change_set", &gitops_change_set_id))?;
    Ok(Json(change_set.into()))
}

/// Materialize a GitOps ChangeSet from the immutable, digest-pinned planning
/// artifact. This writes Pharness state only; no Git repository is contacted.
pub(in crate::app) async fn create_gitops_change_set(
    State(state): State<AppState>,
    Json(request): Json<CreateGitOpsChangeSetRequest>,
) -> Result<Json<CreateGitOpsChangeSetResponse>, ApiError> {
    let pipeline_intent_id = required_text(request.pipeline_intent_id, "pipeline_intent_id")?;
    let plan_artifact_id = required_text(
        request.gitops_update_plan_artifact_id,
        "gitops_update_plan_artifact_id",
    )?;
    let intent = state
        .store
        .get_pipeline_intent(&pipeline_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_intent", &pipeline_intent_id))?;
    if !pipeline_intent_is_deployment_eligible(&intent.status)
        || intent
            .intent_json
            .pointer("/evidence/status")
            .and_then(Value::as_str)
            != Some("satisfied")
    {
        return Err(ApiError::conflict(
            "GitOps ChangeSet requires a completed PipelineIntent with satisfied evidence",
        ));
    }
    let existing = state
        .store
        .get_gitops_change_set_by_pipeline_intent(&intent.id)
        .await?;
    if let Some(existing) = existing {
        if existing.gitops_update_plan_artifact_id != plan_artifact_id {
            return Err(ApiError::conflict(
                "PipelineIntent already has a GitOps ChangeSet from a different update plan artifact",
            ));
        }
        return Ok(Json(CreateGitOpsChangeSetResponse {
            gitops_change_set: existing.into(),
            created: false,
        }));
    }
    let run_id = intent.run_id.clone().ok_or_else(|| {
        ApiError::conflict("GitOps ChangeSet requires PipelineIntent run provenance")
    })?;
    let artifact = state
        .store
        .get_artifact(&plan_artifact_id)
        .await?
        .ok_or_else(|| ApiError::not_found("artifact", &plan_artifact_id))?;
    if artifact.kind != "gitops_update_plan" || artifact.run_id.as_ref() != Some(&run_id) {
        return Err(ApiError::conflict(
            "GitOps ChangeSet must use a gitops_update_plan artifact from the PipelineIntent run",
        ));
    }
    let plan = artifact.content_json.as_ref().ok_or_else(|| {
        ApiError::conflict("GitOps update plan artifact is missing structured content")
    })?;
    if plan.get("kind").and_then(Value::as_str) != Some("gitops_update_plan")
        || plan.get("version").and_then(Value::as_i64) != Some(1)
        || plan.get("pipeline_intent_id").and_then(Value::as_str) != Some(intent.id.as_str())
        || plan.get("work_plan_id").and_then(Value::as_str) != Some(intent.work_plan_id.as_str())
        || plan.get("change_set_id").and_then(Value::as_str) != Some(intent.change_set_id.as_str())
    {
        return Err(ApiError::conflict(
            "GitOps update plan artifact does not match PipelineIntent lineage",
        ));
    }
    let work_plan = state
        .store
        .get_work_plan(&intent.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &intent.work_plan_id))?;
    let work_item_id = work_plan.work_item_id.as_deref().ok_or_else(|| {
        ApiError::conflict("GitOps ChangeSet requires a WorkItem-backed PipelineIntent")
    })?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    if !work_item_target_supported(&work_item) {
        return Err(ApiError::conflict(
            "GitOps ChangeSet creation is limited to dev or the exact protected production target",
        ));
    }
    if plan.get("work_item_id").and_then(Value::as_str) != Some(work_item.id.as_str()) {
        return Err(ApiError::conflict(
            "GitOps update plan artifact does not match WorkItem lineage",
        ));
    }
    let deployment_intent_id = plan
        .get("deployment_intent_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("GitOps update plan missing deployment_intent_id"))?
        .to_string();
    let deployment_intent = state
        .store
        .get_deployment_intent(&deployment_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("deployment_intent", &deployment_intent_id))?;
    if deployment_intent.pipeline_intent_id != intent.id
        || deployment_intent.change_set_id != intent.change_set_id
    {
        return Err(ApiError::conflict(
            "GitOps update plan DeploymentIntent does not match PipelineIntent lineage",
        ));
    }
    let gitops_repo = string_at(plan, "/gitops/repository")
        .ok_or_else(|| ApiError::conflict("GitOps update plan missing repository"))?;
    let gitops_ref = string_at(plan, "/gitops/base_ref")
        .ok_or_else(|| ApiError::conflict("GitOps update plan missing base_ref"))?;
    let head_branch = string_at(plan, "/gitops/head_branch")
        .ok_or_else(|| ApiError::conflict("GitOps update plan missing head_branch"))?;
    let kustomization_path = string_at(plan, "/update/kustomization_path")
        .ok_or_else(|| ApiError::conflict("GitOps update plan missing kustomization_path"))?;
    let image_name = string_at(plan, "/update/image_name")
        .ok_or_else(|| ApiError::conflict("GitOps update plan missing image_name"))?;
    let image_ref = string_at(plan, "/update/new_image")
        .ok_or_else(|| ApiError::conflict("GitOps update plan missing new_image"))?;
    let material_hash = string_at(plan, "/material_hash")
        .ok_or_else(|| ApiError::conflict("GitOps update plan missing material_hash"))?;
    if work_item.gitops_repo.as_deref() != Some(gitops_repo.as_str())
        || work_item.gitops_ref.as_deref() != Some(gitops_ref.as_str())
        || !safe_relative_gitops_path(&kustomization_path)
        || !image_ref.contains("@sha256:")
    {
        return Err(ApiError::conflict(
            "GitOps update plan no longer matches its declared WorkItem target or safety constraints",
        ));
    }
    let expected_material_hash = format!(
        "{:x}",
        Sha256::digest(
            format!(
                "{}\n{}\n{}\n{}\n{}",
                gitops_repo, gitops_ref, kustomization_path, image_name, image_ref
            )
            .as_bytes()
        )
    );
    if material_hash != expected_material_hash {
        return Err(ApiError::conflict(
            "GitOps update plan material hash does not match its immutable target and image update",
        ));
    }
    let change_set = state
        .store
        .create_gitops_change_set(CreateGitOpsChangeSet {
            id: format!("gcset_{}", unique_suffix()),
            work_item_id: work_item.id.clone(),
            work_plan_id: work_plan.id.clone(),
            source_change_set_id: intent.change_set_id.clone(),
            pipeline_intent_id: intent.id.clone(),
            deployment_intent_id: deployment_intent.id,
            gitops_update_plan_artifact_id: artifact.id,
            session_id: intent.session_id.clone(),
            run_id,
            status: "proposed".to_string(),
            title: format!("GitOps ChangeSet: {}", work_item.title),
            summary: format!(
                "Update {} at {} to digest-pinned image {}.",
                gitops_repo, kustomization_path, image_ref
            ),
            risk_level: intent.risk_level.clone(),
            material_hash,
            gitops_repo,
            gitops_ref,
            head_branch,
            kustomization_path,
            image_name,
            image_ref,
            gitops_change_set_json: json!({
                "kind": "gitops_change_set",
                "version": 1,
                "source_plan": plan,
                "execution": {
                    "enabled": false,
                    "reason": "requires approved GitOps ChangeSet, satisfied gitops_mutation gate, and dedicated GitOps writer"
                }
            }),
        })
        .await?;
    append_gitops_change_set_audit_event(
        &state.store,
        &change_set,
        "gitops_change_set.proposed",
        clean_optional_text(request.actor),
        clean_optional_text(request.reason),
        json!({ "source": "gitops_update_plan" }),
    )
    .await?;
    Ok(Json(CreateGitOpsChangeSetResponse {
        gitops_change_set: change_set.into(),
        created: true,
    }))
}

pub(in crate::app) async fn transition_gitops_change_set(
    State(state): State<AppState>,
    Path(gitops_change_set_id): Path<String>,
    Json(request): Json<TransitionGitOpsChangeSetRequest>,
) -> Result<Json<TransitionGitOpsChangeSetResponse>, ApiError> {
    let current = state
        .store
        .get_gitops_change_set(&gitops_change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("gitops_change_set", &gitops_change_set_id))?;
    let target = GitOpsChangeSetStatus::parse(&request.target_status)?;
    GitOpsChangeSetStatus::parse(&current.status)?.ensure_can_transition_to(target)?;
    let change_set = state
        .store
        .update_gitops_change_set_status(
            &gitops_change_set_id,
            target.as_str(),
            clean_optional_text(request.actor.clone()),
            clean_optional_text(request.reason.clone()),
        )
        .await?;
    append_gitops_change_set_audit_event(
        &state.store,
        &change_set,
        &format!("gitops_change_set.{}", target.as_str()),
        clean_optional_text(request.actor),
        clean_optional_text(request.reason),
        json!({ "previous_status": current.status, "target_status": target.as_str() }),
    )
    .await?;
    Ok(Json(TransitionGitOpsChangeSetResponse {
        gitops_change_set: change_set.into(),
    }))
}

pub(in crate::app) async fn repropose_failed_gitops_change_set(
    state: &AppState,
    gitops_change_set_id: &str,
    actor: String,
    reason: String,
) -> Result<TransitionGitOpsChangeSetResponse, ApiError> {
    let current = state
        .store
        .get_gitops_change_set(gitops_change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("gitops_change_set", gitops_change_set_id))?;
    if current.status != "approved" {
        return Err(ApiError::conflict(
            "GitOps delivery retry review requires an approved GitOps ChangeSet",
        ));
    }
    let artifacts = state.store.list_artifacts(&current.run_id).await?;
    let plan = artifacts
        .iter()
        .filter(|artifact| gitops_delivery_plan_matches_change_set(artifact, &current))
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .ok_or_else(|| ApiError::conflict("failed GitOps delivery has no current plan"))?;
    let result = artifacts
        .iter()
        .filter(|artifact| {
            gitops_delivery_artifact_matches_plan(artifact, "gitops_delivery_result", &plan.id)
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .ok_or_else(|| ApiError::conflict("GitOps delivery has no terminal result"))?;
    let observation = artifacts
        .iter()
        .filter(|artifact| {
            gitops_delivery_artifact_matches_plan(
                artifact,
                "gitops_delivery_pr_observation",
                &plan.id,
            )
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id));
    let failed = result
        .content_json
        .as_ref()
        .and_then(|content| content.get("status"))
        .and_then(Value::as_str)
        .is_some_and(|status| matches!(status, "failed" | "dispatch_failed"));
    let closed_unmerged = observation.is_some_and(|observation| {
        gitops_observation_closed_unmerged(observation.content_json.as_ref())
    });
    if !failed && !closed_unmerged {
        return Err(ApiError::conflict(
            "GitOps ChangeSet can be re-proposed only after a failed bounded delivery or a closed, unmerged pull request",
        ));
    }
    if artifacts.iter().any(|artifact| {
        gitops_delivery_artifact_matches_plan(artifact, "gitops_delivery_merge", &plan.id)
    }) {
        return Err(ApiError::conflict(
            "GitOps ChangeSet cannot be re-proposed after an observed merge",
        ));
    }
    let grant_id = artifacts
        .iter()
        .filter(|artifact| {
            gitops_delivery_artifact_matches_plan(artifact, "gitops_delivery_execution", &plan.id)
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .and_then(|artifact| artifact.content_json.as_ref())
        .and_then(|content| content.get("permission_grant_id"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    if let Some(grant_id) = grant_id.as_deref() {
        if state
            .store
            .get_permission_grant(grant_id)
            .await?
            .is_some_and(|grant| grant.status == "active")
        {
            state
                .store
                .revoke_permission_grant(
                    grant_id,
                    Some(actor.clone()),
                    Some(format!(
                        "GitOps ChangeSet {gitops_change_set_id} delivery reached a terminal state without merge; fresh review and authorization are required"
                    )),
                )
                .await?;
        }
    }
    let previous_revision = current.revision;
    let next_revision = previous_revision + 1;
    let base_branch = current
        .gitops_change_set_json
        .pointer("/source_plan/gitops/head_branch")
        .and_then(Value::as_str)
        .unwrap_or(current.head_branch.as_str());
    // A retry must be a sibling of the failed branch. Git cannot store both
    // `refs/heads/<branch>` and `refs/heads/<branch>/revision-N`, so nesting a
    // retry below a branch that was already pushed creates a ref-lock conflict.
    let next_head_branch = format!("{base_branch}-revision-{next_revision}");
    if next_head_branch.len() > 240
        || next_head_branch.starts_with('-')
        || next_head_branch.contains([' ', '~', '^', ':', '?', '*', '[', '\\', '\n'])
        || next_head_branch.contains("..")
    {
        return Err(ApiError::conflict(
            "GitOps ChangeSet cannot derive a safe revision-scoped retry branch",
        ));
    }
    let change_set = state
        .store
        .repropose_gitops_change_set(
            gitops_change_set_id,
            &next_head_branch,
            Some(actor.clone()),
            Some(reason.clone()),
        )
        .await?;
    append_gitops_change_set_audit_event(
        &state.store,
        &change_set,
        "gitops_change_set.reproposed_after_delivery_failure",
        Some(actor),
        Some(reason),
        json!({
            "previous_revision": previous_revision,
            "revision": change_set.revision,
            "failed_result_artifact_id": result.id,
            "failed_plan_artifact_id": plan.id,
            "closed_pull_request_observation_artifact_id": observation.map(|artifact| &artifact.id),
            "revoked_permission_grant_id": grant_id,
            "external_mutation_observed": closed_unmerged,
            "next_head_branch": change_set.head_branch,
        }),
    )
    .await?;
    Ok(TransitionGitOpsChangeSetResponse {
        gitops_change_set: change_set.into(),
    })
}

/// Resolve the declared GitOps base ref through the read-only observer
/// identity. The result is durable evidence for a later, separately guarded
/// writer; this route itself cannot mutate a repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GitOpsChangeSetStatus {
    Proposed,
    Approved,
    Rejected,
    Applied,
    Stale,
}

impl GitOpsChangeSetStatus {
    fn parse(value: &str) -> Result<Self, ApiError> {
        match value {
            "proposed" => Ok(Self::Proposed),
            "approved" => Ok(Self::Approved),
            "rejected" => Ok(Self::Rejected),
            "applied" => Ok(Self::Applied),
            "stale" => Ok(Self::Stale),
            other => Err(ApiError::bad_request(format!(
                "unsupported GitOps change set status: {other}"
            ))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Applied => "applied",
            Self::Stale => "stale",
        }
    }

    fn ensure_can_transition_to(self, target: Self) -> Result<(), ApiError> {
        let allowed = match self {
            Self::Proposed => matches!(target, Self::Approved | Self::Rejected | Self::Stale),
            Self::Approved => matches!(target, Self::Applied | Self::Rejected | Self::Stale),
            Self::Rejected | Self::Applied | Self::Stale => false,
        };
        if allowed {
            Ok(())
        } else {
            Err(ApiError::conflict(format!(
                "cannot transition GitOps change set from {} to {}",
                self.as_str(),
                target.as_str()
            )))
        }
    }
}

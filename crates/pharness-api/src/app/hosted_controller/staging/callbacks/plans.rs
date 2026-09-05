use super::{artifact_id, evidence, hash, preparation, ApiError, AppState};
use axum::extract::{Path, State};
use axum::Json;
use pharness_core::hosted_sdlc::staging::{StagingGitOpsPlan, GITOPS_REPOSITORY};
use pharness_store::CreateGitOpsChangeSet;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct StagingPlanRequest {
    pub execution_id: String,
    pub authority_hash: String,
    pub plan: StagingGitOpsPlan,
}

pub(in crate::app) async fn internal_staging_plan(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<StagingPlanRequest>,
) -> Result<Json<Value>, ApiError> {
    let _boundary = super::super::super::DISPATCH_BOUNDARY.lock().await;
    let saved = preparation::saved(&state, &id, &request.execution_id).await?;
    preparation::validate_current(&state, &saved, true).await?;
    if request.authority_hash
        != saved
            .authority
            .material_hash()
            .map_err(ApiError::conflict)?
    {
        return Err(ApiError::conflict("staging plan has different authority"));
    }
    request
        .plan
        .validate(&saved.authority)
        .map_err(ApiError::conflict)?;
    if saved.plan.as_ref().is_some_and(|p| p != &request.plan) {
        return Err(ApiError::conflict(
            "the original staging plan cannot be replaced after observation",
        ));
    }
    if state
        .store
        .get_artifact(&artifact_id("attempt", &request.execution_id))
        .await?
        .is_some()
    {
        return Err(ApiError::conflict(
            "staging write was admitted; its plan cannot change",
        ));
    }
    ensure_record(&state, &saved, &request.plan).await?;
    Ok(Json(
        json!({"recorded":true,"plan_hash":request.plan.material_hash().map_err(ApiError::conflict)?}),
    ))
}

pub(super) async fn ensure_record(
    state: &AppState,
    saved: &preparation::Saved,
    plan: &StagingGitOpsPlan,
) -> Result<(), ApiError> {
    plan.validate(&saved.authority)
        .map_err(ApiError::conflict)?;
    let artifact = evidence::artifact(
        state,
        &saved.pipeline,
        &artifact_id("plan", &saved.authority.execution_id),
        "hosted_staging_plan",
        json!(plan),
    )
    .await?;
    let (path, image) = saved.authority.coordinates().map_err(ApiError::conflict)?;
    let document = json!({"delivery_mode":"atomic_commit_expected_head","authority":saved.authority,"plan_artifact_id":artifact.id,"plan_hash":plan.material_hash().map_err(ApiError::conflict)?,"expected_head_oid":plan.base_commit_sha,"target_ref":"main","scope":"staging","production_authorized":false});
    if let Some(existing) = state
        .store
        .get_gitops_change_set_by_deployment_intent(&saved.deployment.id)
        .await?
    {
        if existing.id != saved.authority.gitops_change_set_id
            || existing.work_item_id != saved.authority.work_item_id
            || existing.pipeline_intent_id != saved.pipeline.id
            || existing.source_change_set_id != saved.pipeline.change_set_id
            || existing.gitops_update_plan_artifact_id != artifact.id
            || existing.gitops_change_set_json != document
            || existing.material_hash != hash(&document)?
            || existing.gitops_repo != GITOPS_REPOSITORY
            || existing.gitops_ref != "main"
            || existing.head_branch != "main"
            || existing.kustomization_path != path
            || existing.image_name != image
            || existing.image_ref != saved.authority.image_ref().map_err(ApiError::conflict)?
        {
            return Err(ApiError::conflict(
                "staging GitOps ChangeSet differs from the original one-file plan",
            ));
        }
    } else {
        state
            .store
            .create_gitops_change_set(CreateGitOpsChangeSet {
                id: saved.authority.gitops_change_set_id.clone(),
                work_item_id: saved.authority.work_item_id.clone(),
                work_plan_id: saved.pipeline.work_plan_id.clone(),
                source_change_set_id: saved.pipeline.change_set_id.clone(),
                pipeline_intent_id: saved.pipeline.id.clone(),
                deployment_intent_id: saved.deployment.id.clone(),
                gitops_update_plan_artifact_id: artifact.id,
                session_id: saved.pipeline.session_id.clone(),
                run_id: saved.pipeline.run_id.clone(),
                status: "approved".into(),
                title: "Update the Finance staging image digest".into(),
                summary:
                    "One immutable image pin under the saved workflow; production remains gated"
                        .into(),
                risk_level: "medium".into(),
                material_hash: hash(&document)?,
                gitops_repo: GITOPS_REPOSITORY.into(),
                gitops_ref: "main".into(),
                head_branch: "main".into(),
                kustomization_path: path.into(),
                image_name: image.into(),
                image_ref: saved.authority.image_ref().map_err(ApiError::conflict)?,
                gitops_change_set_json: document,
            })
            .await?;
    }
    Ok(())
}

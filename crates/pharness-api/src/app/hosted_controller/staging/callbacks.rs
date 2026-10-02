use super::{artifact_id, baseline, evidence, hash, now, preparation, ApiError, AppState};
use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

mod outcomes;
mod plans;
pub(in crate::app) use outcomes::internal_staging_outcome;
pub(super) use outcomes::settle;
pub(in crate::app) use plans::internal_staging_plan;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct StagingQuery {
    pub execution_id: String,
    #[serde(default)]
    pub observe_only: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct StagingAttempt {
    pub execution_id: String,
    pub authority_hash: String,
    pub plan_hash: String,
}

pub(in crate::app) async fn internal_staging_context(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<StagingQuery>,
) -> Result<Json<Value>, ApiError> {
    let saved = preparation::saved(&state, &id, &query.execution_id).await?;
    let admitted = state
        .store
        .get_artifact(&artifact_id("attempt", &query.execution_id))
        .await?
        .is_some();
    if query.observe_only
        && (!admitted
            || saved
                .operation
                .resource_refs
                .get("staging_observer_dispatch")
                .is_none())
    {
        return Err(ApiError::conflict(
            "the bounded staging reader has not been dispatched",
        ));
    }
    let may_advance = !query.observe_only
        && !admitted
        && preparation::validate_current(&state, &saved, true)
            .await
            .is_ok()
        && baseline::fresh(&state, &saved).await?;
    // GET is strictly observational, including a partially saved plan. Admission
    // repairs its linked record under the same serialized write boundary.
    Ok(Json(
        json!({"authority":saved.authority,"authority_hash":saved.authority.material_hash().map_err(ApiError::conflict)?,"plan":saved.plan,"plan_hash":saved.plan.as_ref().map(|p| p.material_hash()).transpose().map_err(ApiError::conflict)?,"may_advance":may_advance,"admission_recorded":admitted}),
    ))
}

pub(in crate::app) async fn internal_staging_attempt(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<StagingAttempt>,
) -> Result<Json<Value>, ApiError> {
    let _boundary = super::super::DISPATCH_BOUNDARY.lock().await;
    let saved = preparation::saved(&state, &id, &request.execution_id).await?;
    preparation::validate_current(&state, &saved, true).await?;
    let plan = saved.plan.as_ref().ok_or_else(|| {
        ApiError::conflict("staging admission requires its persisted digest diff")
    })?;
    if request.authority_hash
        != saved
            .authority
            .material_hash()
            .map_err(ApiError::conflict)?
        || request.plan_hash != plan.material_hash().map_err(ApiError::conflict)?
    {
        return Err(ApiError::conflict(
            "staging admission differs from the saved authority or plan",
        ));
    }
    if state
        .store
        .get_artifact(&artifact_id("attempt", &request.execution_id))
        .await?
        .is_some()
        || state
            .store
            .get_artifact(&artifact_id("commit", &request.execution_id))
            .await?
            .is_some()
    {
        return Err(ApiError::conflict("the original staging write was already admitted; observe GitHub without another mutation"));
    }
    plans::ensure_record(&state, &saved, plan).await?;
    let baseline = baseline::revalidate(&state, &saved).await?;
    let body = json!({"execution_id":request.execution_id,"operation_id":saved.operation.id,"authority_hash":request.authority_hash,"plan_hash":request.plan_hash,"admitted_at_ms":now(),"baseline":baseline,"meaning":"one expected-head GitOps commit admitted before the unchanged baseline expiry; no deployment or runtime success claimed"});
    let record = evidence::artifact(
        &state,
        &saved.pipeline,
        &artifact_id("attempt", &request.execution_id),
        "hosted_staging_admission",
        body,
    )
    .await?;
    Ok(Json(
        json!({"admitted":true,"admission_id":record.id,"authority_hash":request.authority_hash,"plan_hash":request.plan_hash,"baseline":baseline}),
    ))
}

use super::{artifact_id, evidence, now, preparation, ApiError, AppState};
use pharness_core::tools::{
    FinanceApplication, FinanceDeploymentExpectation, FinanceEnvironment, FinanceRuntimeWindow,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WindowRecord {
    pub schema_version: String,
    pub authority_hash: String,
    pub plan_hash: String,
    pub created_at_ms: i64,
    pub expected: FinanceDeploymentExpectation,
    pub window: FinanceRuntimeWindow,
}

impl WindowRecord {
    pub fn new(saved: &preparation::Saved, created_at_ms: i64) -> Result<Self, ApiError> {
        let plan = saved
            .plan
            .as_ref()
            .ok_or_else(|| ApiError::conflict("baseline requires its saved staging plan"))?;
        let application = match saved.authority.application_repository.as_str() {
            "https://github.com/lward27/yfinance_wrapper.git" => FinanceApplication::Yfinance,
            "https://github.com/lward27/finance-frontend.git" => FinanceApplication::Frontend,
            _ => {
                return Err(ApiError::conflict(
                    "baseline requires a registered Finance application",
                ))
            }
        };
        let window =
            FinanceRuntimeWindow::starting_after(created_at_ms.saturating_add(30_000) as u64, 300)
                .map_err(|_| ApiError::conflict("baseline window cannot fit its original clock"))?;
        Ok(Self {
            schema_version: "pharness.dev/hosted-staging-baseline/v1alpha1".into(),
            authority_hash: saved
                .authority
                .material_hash()
                .map_err(ApiError::conflict)?,
            plan_hash: plan.material_hash().map_err(ApiError::conflict)?,
            created_at_ms,
            expected: FinanceDeploymentExpectation {
                application,
                environment: FinanceEnvironment::Staging,
                gitops_commit_sha: plan.base_commit_sha.clone(),
                image_digest: plan
                    .previous_image_digest(&saved.authority)
                    .map_err(ApiError::conflict)?,
            },
            window,
        })
    }

    pub fn validate(&self, saved: &preparation::Saved) -> Result<(), ApiError> {
        if self.created_at_ms < saved.operation.created_at
            || self.created_at_ms > now()
            || json!(self) != json!(Self::new(saved, self.created_at_ms)?)
            || self.expires_at() >= saved.operation.created_at.saturating_add(30 * 60 * 1000)
            || self.expires_at() >= saved.authority.expires_at_ms
        {
            return Err(ApiError::conflict(
                "baseline window differs from the original authority, plan or execution allowance",
            ));
        }
        Ok(())
    }

    pub fn expires_at(&self) -> i64 {
        (self.window.end_unix_seconds as i64)
            .saturating_mul(1000)
            .saturating_add(60_000)
    }
}

pub(super) async fn window(
    state: &AppState,
    saved: &preparation::Saved,
) -> Result<Option<WindowRecord>, ApiError> {
    read(state, saved, "window")
        .await?
        .map(|value| {
            let record: WindowRecord = serde_json::from_value(value)
                .map_err(|_| ApiError::conflict("baseline window record is invalid"))?;
            record.validate(saved)?;
            Ok(record)
        })
        .transpose()
}

pub(super) async fn read(
    state: &AppState,
    saved: &preparation::Saved,
    step: &str,
) -> Result<Option<Value>, ApiError> {
    let Some(record) = state.store.get_artifact(&id(saved, step)).await? else {
        return Ok(None);
    };
    if record.kind != format!("hosted_staging_baseline_{step}")
        || record.session_id != saved.pipeline.session_id
        || record.run_id != saved.pipeline.run_id
    {
        return Err(ApiError::conflict(
            "baseline evidence belongs to a different native execution",
        ));
    }
    let body = record
        .content_json
        .ok_or_else(|| ApiError::conflict("baseline evidence has no content"))?;
    if body["authority_hash"]
        != saved
            .authority
            .material_hash()
            .map_err(ApiError::conflict)?
        || body["plan_hash"]
            != saved
                .plan
                .as_ref()
                .ok_or_else(|| ApiError::conflict("baseline plan is missing"))?
                .material_hash()
                .map_err(ApiError::conflict)?
    {
        return Err(ApiError::conflict(
            "baseline evidence differs from the original authority or plan",
        ));
    }
    Ok(Some(body))
}

pub(super) async fn write(
    state: &AppState,
    saved: &preparation::Saved,
    step: &str,
    mut body: Value,
) -> Result<(), ApiError> {
    body["authority_hash"] = json!(saved
        .authority
        .material_hash()
        .map_err(ApiError::conflict)?);
    body["plan_hash"] = json!(saved
        .plan
        .as_ref()
        .ok_or_else(|| ApiError::conflict("baseline plan is missing"))?
        .material_hash()
        .map_err(ApiError::conflict)?);
    evidence::artifact(
        state,
        &saved.pipeline,
        &id(saved, step),
        &format!("hosted_staging_baseline_{step}"),
        body,
    )
    .await?;
    Ok(())
}

pub(super) fn id(saved: &preparation::Saved, step: &str) -> String {
    artifact_id(&format!("baseline_{step}"), &saved.authority.execution_id)
}

/// Persist an attempt before the read. A crash cannot grant another collection
/// allowance or replace a partially observed window with an unrecorded retry.
pub(super) async fn begin(
    state: &AppState,
    saved: &preparation::Saved,
    step: &str,
) -> Result<bool, ApiError> {
    let key = format!("{step}_started");
    if read(state, saved, &key).await?.is_some() {
        return Ok(false);
    }
    write(state, saved, &key, json!({"started_at_ms":now()})).await?;
    Ok(true)
}

pub(super) async fn fail(
    state: &AppState,
    saved: &preparation::Saved,
    reason: &str,
) -> Result<(), ApiError> {
    write(state, saved, "result", json!({"completed_at_ms":now(),"runtime_evidence":null,"assessment":{"runtime_verification":"inconclusive","reasons":[reason],"work_item_completion":"not_evaluated"}})).await
}

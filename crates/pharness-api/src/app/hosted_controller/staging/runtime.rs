//! Candidate observation starts only after the admitted GitOps change is observed.
use super::super::state::{condition, Condition};
use super::{artifact_id, evidence, now, preparation, ApiError, AppState};
use pharness_store::{StoredArtifact, StoredWorkflowReconciliation};
use serde_json::{json, Value};

mod collection;
mod records;
mod result;
#[cfg(test)]
pub(super) mod test_support;

pub(super) async fn verified(
    state: &AppState,
    saved: &preparation::Saved,
    receipt: &StoredArtifact,
) -> Result<bool, ApiError> {
    let context = records::Context::new(state, saved, receipt).await?;
    let Some(record) = context.read(state, "result").await? else {
        return Ok(false);
    };
    result::verified(state, &context, &record).await
}

pub(super) async fn reconcile(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    saved: &preparation::Saved,
    receipt: &StoredArtifact,
    expired: bool,
) -> Result<Condition, ApiError> {
    let context = records::Context::new(state, saved, receipt).await?;
    if let Some(record) = context.read(state, "result").await? {
        return result::record(state, claim, &context, &record).await;
    }
    if expired || now() >= saved.authority.expires_at_ms {
        context
            .fail(state, "staging_original_operation_expired")
            .await?;
    } else if let Some(record) = context.read(state, "window").await? {
        let window: records::Window = serde_json::from_value(record["window_record"].clone())
            .map_err(|_| ApiError::conflict("staging runtime window record is invalid"))?;
        window.validate(&context)?;
        link_window(state, claim, &context, &saved.operation, &window).await?;
        collection::collect(state, &context, &window).await?;
    } else {
        wait_for_identity(state, claim, &context).await?;
    }
    if let Some(record) = context.read(state, "result").await? {
        return result::record(state, claim, &context, &record).await;
    }
    Ok(condition("waiting","The staging GitOps commit is recorded. Waiting for the exact deployment and collecting its original five-minute runtime window; production remains unauthorized."))
}

async fn wait_for_identity(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    context: &records::Context<'_>,
) -> Result<(), ApiError> {
    // Accepted callbacks allow a bounded worker clock skew. Wait for that
    // recorded time rather than creating an observation that predates it.
    if now() < context.commit_checked_at_ms {
        return Ok(());
    }
    let mut operation = context.saved.operation.clone();
    for index in 1..=crate::app::CONTROLLER_WAIT_MAX_CHECKS {
        let step = format!("identity_{index}");
        let key = format!("staging_runtime_{step}");
        if let Some(marker) = operation.resource_refs.get(&key) {
            let created = marker["started_at_ms"].as_i64().ok_or_else(|| {
                ApiError::conflict("staging identity observation has no original clock")
            })?;
            if marker["artifact_id"] != context.id(&step)
                || created < operation.created_at
                || created > now()
            {
                return Err(ApiError::conflict(
                    "staging identity observation marker has different authority",
                ));
            }
            if let Some(observation) = context.read(state, &step).await? {
                if save_window(
                    state,
                    claim,
                    context,
                    &operation,
                    &step,
                    created,
                    &observation,
                )
                .await?
                {
                    return Ok(());
                }
            } else {
                // A replacement claim cannot repeat a possibly consumed read.
                context.write(state,&step,json!({"observation":{"identity_state":"inconclusive","reasons":["staging_identity_collection_interrupted"]}})).await?;
            }
            continue;
        }
        let created = now();
        let mut refs = operation.resource_refs.clone();
        refs[&key] = json!({"started_at_ms":created,"artifact_id":context.id(&step)});
        operation = state
            .store
            .record_workflow_operation(
                claim,
                &operation.id,
                "running",
                &refs,
                "One bounded native staging identity observation recorded before reading",
                now(),
            )
            .await?;
        let observation = collection::content(
            state
                .cluster_tools
                .observe_finance_deployment(&context.expected)
                .await,
        );
        context
            .write(state, &step, json!({"observation":observation}))
            .await?;
        let record = context.read(state, &step).await?.unwrap();
        save_window(state, claim, context, &operation, &step, created, &record).await?;
        return Ok(());
    }
    context
        .fail(state, "staging_identity_observation_allowance_exhausted")
        .await
}

async fn save_window(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    context: &records::Context<'_>,
    operation: &pharness_store::StoredWorkflowOperation,
    step: &str,
    created: i64,
    record: &Value,
) -> Result<bool, ApiError> {
    let observation = &record["observation"];
    if observation["identity_state"] != "verified" || created < context.commit_checked_at_ms {
        return Ok(false);
    }
    let window = records::Window::new(context, created, step.into())?;
    let start = window.window.start_unix_seconds as i64 * 1000;
    if now() >= start
        || observation["completed_at_unix_ms"]
            .as_i64()
            .unwrap_or(i64::MAX)
            > start
        || observation["started_at_unix_ms"]
            .as_i64()
            .unwrap_or_default()
            < created
    {
        return Ok(false);
    }
    context
        .write(state, "window", json!({"window_record":window}))
        .await?;
    link_window(state, claim, context, operation, &window).await?;
    Ok(true)
}

async fn link_window(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    context: &records::Context<'_>,
    operation: &pharness_store::StoredWorkflowOperation,
    window: &records::Window,
) -> Result<(), ApiError> {
    let mut refs = operation.resource_refs.clone();
    refs["staging_runtime_window"] = json!({"artifact_id":context.id("window"),"window":window.window,"expected":context.expected});
    if refs != operation.resource_refs {
        state
            .store
            .record_workflow_operation(
                claim,
                &operation.id,
                "running",
                &refs,
                "Exact staging identity observed; its single five-minute runtime window is fixed",
                now(),
            )
            .await?;
    }
    Ok(())
}

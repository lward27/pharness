use super::state::{condition, Condition};
use crate::app::hosted_workflow::stages as hosted;
use crate::app::{ApiError, AppState};
use pharness_store::{StoredRun, StoredWorkflowReconciliation, UpdateEnvironmentPreparation};

pub(super) async fn reconcile(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    run: &StoredRun,
    expired: bool,
) -> Result<Condition, ApiError> {
    let preparation = state.store.get_environment_preparation_by_run(&run.id).await?
        .ok_or_else(|| ApiError::conflict("Builder startup has no durable preparation record; no new preparation is dispatched"))?;
    let work_item = state
        .store
        .get_work_item(&claim.work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("preparation WorkItem is unavailable"))?;
    let profile: pharness_core::EnvironmentProfile =
        serde_json::from_value(run.execution_target_json["runner_profile"].clone())
            .map_err(|_| ApiError::conflict("preparation has no immutable runner profile"))?;
    profile
        .validate()
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    if preparation.work_item_id != claim.work_item_id
        || preparation.run_id.as_ref() != Some(&run.id)
        || Some(preparation.source_commit.as_str()) != work_item.source_commit.as_deref()
        || preparation.environment_profile_id != profile.id
    {
        return Err(ApiError::conflict(
            "preparation identity no longer matches its authorized Run",
        ));
    }
    if preparation.status == "succeeded" {
        return Err(ApiError::conflict("Preparation and Run state are inconsistent. A signature cannot be reconstructed from an incomplete historical callback."));
    }
    if preparation.status == "failed" {
        crate::worker::fail_run_from_dispatch(
            &state.store,
            &run.id,
            preparation
                .error
                .unwrap_or_else(|| "recorded environment preparation failed".into()),
        )
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
        return Ok(condition(
            "blocked",
            "The existing environment preparation failed; no additional attempt was created.",
        ));
    }
    let observed = state
        .worker
        .observe_hosted_preparation(run, &profile)
        .await
        .map_err(|e| ApiError::unavailable(e.to_string()))?;
    if matches!(
        observed.as_ref().map(|o| o.status),
        Some("failed" | "completed")
    ) {
        // A callback can finish while the Job observation is in flight. Re-read
        // before deciding that the terminal Job lacks a durable signed result.
        let current = state
            .store
            .get_environment_preparation_by_run(&run.id)
            .await?
            .ok_or_else(|| ApiError::conflict("preparation disappeared during observation"))?;
        let current_run =
            state.store.get_run(&run.id).await?.ok_or_else(|| {
                ApiError::conflict("Run disappeared during preparation observation")
            })?;
        if current.status != preparation.status || current_run.status != "preparing" {
            return Ok(condition(
                "progressing",
                "A preparation callback supplied new durable evidence during observation.",
            ));
        }
        let reason = "The exact preparation Job terminated without a validated durable result; Job completion cannot authorize coding.";
        let failed = state
            .store
            .fail_hosted_preparation(
                &run.id,
                UpdateEnvironmentPreparation {
                    id: preparation.id,
                    status: "failed".into(),
                    project_contract_json: preparation.project_contract_json,
                    project_contract_hash: preparation.project_contract_hash,
                    environment_snapshot_json: None,
                    logs_json: preparation.logs_json,
                    error: Some(reason.into()),
                },
            )
            .await?;
        if failed.is_none() {
            return Ok(condition(
                "progressing",
                "A newer preparation result won the update; terminal evidence was preserved.",
            ));
        }
        crate::worker::fail_run_from_dispatch(&state.store, &run.id, reason.into())
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
        return Ok(condition("blocked", reason));
    }
    if observed.is_none()
        && (preparation.status != "queued" || claim.control != "active" || expired)
    {
        return Ok(condition("blocked", "The recorded preparation Job is absent. An acknowledged, paused, or expired operation is not replayed."));
    }
    if preparation.status == "queued"
        && (observed.is_some() || (claim.control == "active" && !expired))
    {
        if observed.is_none() {
            hosted::validate_run(state, run).await?;
        }
        // Existing Jobs are reconciled before this adapter considers creation.
        // The same Run, preparation, workspace and bounded grant are retained.
        let job_name = if let Some(observed) = observed {
            observed.job_name
        } else {
            state
                .worker
                .dispatch_environment_preparation(run, &profile)
                .await
                .map_err(|e| ApiError::unavailable(e.to_string()))?
                .job_name
        };
        state
            .store
            .mark_environment_preparation_dispatched(&preparation.id, &job_name)
            .await?;
    }
    Ok(condition(
        if expired { "wait_expired" } else { "waiting" },
        "Observing the existing isolated preparation; coding requires its validated snapshot.",
    ))
}

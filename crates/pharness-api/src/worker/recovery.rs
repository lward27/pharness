use super::*;
use pharness_store::{CreateEvidenceValidation, StoredStageExecution, StoredStageOutcome};

/// Resume local normalization only. No model, worker, budget or source action is
/// created, and missing historical evidence cannot be reconstructed as success.
pub(crate) async fn reconcile_terminal_hosted_run(
    store: &SqliteStore,
    run: &StoredRun,
) -> anyhow::Result<()> {
    let _finalization = RUN_FINALIZATION.lock().await;
    let run = store
        .get_run(&run.id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("terminal Run is unavailable"))?;
    if run
        .execution_target_json
        .get("hosted_workflow_policy_hash")
        .is_none()
        || !matches!(run.status.as_str(), "completed" | "failed" | "cancelled")
    {
        anyhow::bail!("local finalization requires a terminal hosted Run");
    }
    let execution_id = run
        .execution_target_json
        .pointer("/repo_mode/stage_execution_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("terminal Run has no stage binding"))?;
    let execution = store
        .get_stage_execution(execution_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("terminal stage is unavailable"))?;
    if execution.run_id.as_ref() != Some(&run.id)
        || run.execution_target_json["repo_mode"]["stage"] != execution.stage_key
        || run.execution_target_json["run_scope"]["work_item_id"] != execution.work_item_id
    {
        anyhow::bail!("terminal Run and stage identities disagree");
    }
    if let Some(sealed) = store.get_stage_outcome_for_execution(execution_id).await? {
        return finish_sealed_stage(store, &run, &execution, &sealed).await;
    }
    let result = run
        .result_json
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("terminal Run has no saved result"))?;
    let outcome: AttemptOutcome = serde_json::from_value(result["terminal_attempt"].clone())
        .map_err(|_| {
            anyhow::anyhow!(
                "terminal Run lacks its original normalization evidence; no result is inferred"
            )
        })?;
    if outcome.status != run.status
        || outcome.consumption != run.budget_consumption
        || result["status"] != outcome.status
        || result["turns"] != outcome.turns
    {
        anyhow::bail!("saved terminal evidence disagrees with the durable Run");
    }
    sync_repo_stage_run_inner(store, &run, &outcome).await
}

pub(super) async fn finish_sealed_stage(
    store: &SqliteStore,
    run: &StoredRun,
    execution: &StoredStageExecution,
    sealed: &StoredStageOutcome,
) -> anyhow::Result<()> {
    if run
        .execution_target_json
        .get("hosted_workflow_policy_hash")
        .is_none()
    {
        return Ok(());
    }
    if sealed.stage_execution_id != execution.id
        || sealed.work_item_id != execution.work_item_id
        || sealed.stage_key != execution.stage_key
        || pharness_core::canonical_json_sha256(&sealed.outcome)? != sealed.content_hash
        || sealed.outcome["status"] != sealed.status
    {
        anyhow::bail!("sealed stage identity or content hash is inconsistent");
    }
    if execution.stage_key == "implement" && sealed.status != "succeeded" {
        revoke_repo_chain_for_run(store, run, "Builder reached a terminal failure").await?;
    }
    if !matches!(execution.stage_key.as_str(), "verify" | "test") {
        return Ok(());
    }
    let repair = sealed.outcome["recommendations"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("sealed stage has no recorded continuation decision"))?
        .iter()
        .any(|r| r["next"] == "dispatch_single_bounded_repair");
    if execution.stage_key == "verify" {
        let effective = store
            .list_effective_stage_outcomes(&execution.work_item_id)
            .await?;
        if !effective.iter().any(|o| o.id == sealed.id) {
            anyhow::bail!("verification evidence was superseded; finalization cannot use it");
        }
        let passed = sealed.status == "succeeded";
        if passed
            && !sealed.outcome["contradictions"]
                .as_array()
                .is_some_and(Vec::is_empty)
        {
            anyhow::bail!(
                "successful verification contains unresolved or missing contradiction evidence"
            );
        }
        if passed {
            let change = create_repo_change_set(store, run, execution).await?;
            // A decision already made on this same evidence must not be reset.
            if change.status == "proposed" {
                set_status_if_needed(
                    store,
                    &execution.work_item_id,
                    "awaiting_approval",
                    &sealed.outcome,
                )
                .await?;
            }
        } else {
            set_status_if_needed(
                store,
                &execution.work_item_id,
                if repair { "executing" } else { "blocked" },
                &sealed.outcome,
            )
            .await?;
        }
        if passed || !repair {
            revoke_repo_chain_for_run(store, run, "Verifier reached a terminal outcome").await?;
        }
    } else {
        if run
            .execution_target_json
            .pointer("/repo_mode/test_diagnosis")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            set_status_if_needed(
                store,
                &execution.work_item_id,
                if repair { "executing" } else { "blocked" },
                &sealed.outcome,
            )
            .await?;
        }
        if sealed.status != "succeeded" && !repair {
            revoke_repo_chain_for_run(
                store,
                run,
                "Test reached a terminal outcome without a correction",
            )
            .await?;
        }
    }
    Ok(())
}

async fn set_status_if_needed(
    store: &SqliteStore,
    id: &str,
    status: &str,
    outcome: &serde_json::Value,
) -> anyhow::Result<()> {
    let current = store
        .get_work_item(id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("stage WorkItem is unavailable"))?;
    if current.status != status {
        let reason = outcome["stop_reason"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("sealed stage has no stop reason"))?;
        store
            .update_repo_work_item_status(id, status, "controller", reason, false)
            .await?;
    }
    Ok(())
}

/// Each exact validation has one stable identity, including its references.
pub(super) async fn persist_validation(
    store: &SqliteStore,
    mut validation: CreateEvidenceValidation,
) -> anyhow::Result<()> {
    validation.id.clear();
    let hash = pharness_core::canonical_json_sha256(&serde_json::to_value(&validation)?)?;
    validation.id = format!("evalid_{}", hash.trim_start_matches("sha256:"));
    if let Some(existing) = store.get_evidence_validation(&validation.id).await? {
        let mut expected = serde_json::to_value(validation)?;
        let mut actual = serde_json::to_value(existing)?;
        actual.as_object_mut().unwrap().remove("schema_version");
        actual.as_object_mut().unwrap().remove("validated_at");
        expected.as_object_mut().unwrap().remove("id");
        actual.as_object_mut().unwrap().remove("id");
        if actual != expected {
            anyhow::bail!("saved evidence validation changed");
        }
        return Ok(());
    }
    store.create_evidence_validation(validation).await?;
    Ok(())
}

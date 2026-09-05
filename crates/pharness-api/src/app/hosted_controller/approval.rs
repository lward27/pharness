use crate::app::hashing::canonical_material_hash;
use crate::app::repo_mode::{validate_change_set_outcome_binding, ChangeSetOutcomeBinding};
use crate::app::{ApiError, AppState};
use pharness_store::StoredStageOutcome;
use serde_json::json;

fn successful(outcome: &StoredStageOutcome) -> Result<(), ApiError> {
    if outcome.status != "succeeded"
        || outcome.outcome["status"] != "succeeded"
        || !outcome.outcome["contradictions"]
            .as_array()
            .is_some_and(Vec::is_empty)
        || canonical_material_hash(&outcome.outcome)? != outcome.content_hash
    {
        return Err(ApiError::conflict("automatic approval requires current successful evidence without unresolved contradictions"));
    }
    Ok(())
}

// Human review used to supply this boundary. Automatic approval must consume
// the exact controller-sealed agent output, not any proposed database row.
pub(super) async fn validate(
    state: &AppState,
    work_item_id: &str,
    action: &str,
    resource: &str,
) -> Result<(), ApiError> {
    validate_stored(&state.store, work_item_id, action, resource).await
}

pub(in crate::app) async fn validate_stored(
    store: &pharness_store::SqliteStore,
    work_item_id: &str,
    action: &str,
    resource: &str,
) -> Result<(), ApiError> {
    let plan = store
        .get_work_plan_by_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("automatic approval has no current WorkPlan"))?;
    let outcomes = store.list_effective_stage_outcomes(work_item_id).await?;
    if action == "approve_work_plan" {
        let outcome = outcomes
            .iter()
            .find(|o| o.stage_key == "plan")
            .ok_or_else(|| {
                ApiError::conflict("automatic plan approval requires sealed Planner evidence")
            })?;
        successful(outcome)?;
        let execution = store
            .get_stage_execution(&outcome.stage_execution_id)
            .await?
            .ok_or_else(|| ApiError::conflict("Planner execution is unavailable"))?;
        if plan.id != resource
            || plan.run_id.is_none()
            || plan.run_id != execution.run_id
            || !outcome.outcome["outputs"]
                .as_array()
                .is_some_and(|outputs| {
                    outputs.iter().any(|o| {
                        o["kind"] == "work_plan"
                            && o["id"] == plan.id
                            && o["revision"] == plan.revision
                    })
                })
            || !outcome.outcome["agent_claims"]
                .as_array()
                .is_some_and(|claims| {
                    claims.iter().any(|c| {
                        c["kind"] == "planner_submission" && c["document"] == plan.work_plan_json
                    })
                })
        {
            return Err(ApiError::conflict(
                "the proposed WorkPlan does not match the sealed Planner revision",
            ));
        }
    } else if action == "approve_change_set" {
        let change = store
            .get_change_set_by_work_plan(&plan.id)
            .await?
            .ok_or_else(|| ApiError::conflict("automatic approval has no current ChangeSet"))?;
        let material = &change.change_set_json;
        let bound = material["effective_outcomes"]
            .as_array()
            .ok_or_else(|| ApiError::conflict("ChangeSet has no bound stage outcomes"))?;
        if change.id != resource
            || canonical_material_hash(material)? != change.material_hash
            || !matches!(
                validate_change_set_outcome_binding(bound, &outcomes)?,
                ChangeSetOutcomeBinding::Current
            )
        {
            return Err(ApiError::conflict(
                "automatic approval requires the exact current verified ChangeSet",
            ));
        }
        for stage in ["discover", "plan", "implement", "test", "verify"] {
            let outcome = outcomes
                .iter()
                .find(|o| o.stage_key == stage)
                .ok_or_else(|| {
                    ApiError::conflict(format!("automatic approval is missing {stage} evidence"))
                })?;
            successful(outcome)?;
            if stage == "verify"
                && material["verification_stage_execution_id"] != json!(outcome.stage_execution_id)
            {
                return Err(ApiError::conflict("ChangeSet verifier identity changed"));
            }
        }
    }
    Ok(())
}

use crate::app::hashing::canonical_material_hash;
use crate::app::repo_mode;
use crate::app::{ApiError, AppState};
use crate::dto::WorkItemActionResponse;
use pharness_store::{RunListFilter, StoredRepoWorkItemMetadata, StoredRun, StoredStageExecution};
use serde_json::json;

pub(super) struct Snapshot {
    pub(super) metadata: StoredRepoWorkItemMetadata,
    pub(super) runs: Vec<StoredRun>,
    pub(super) stages: Vec<StoredStageExecution>,
    pub(super) actions: Vec<WorkItemActionResponse>,
    pub hash: String,
}

impl Snapshot {
    pub async fn load(state: &AppState, work_item_id: &str) -> Result<Self, ApiError> {
        let metadata = state
            .store
            .get_repo_work_item_metadata(work_item_id)
            .await?
            .ok_or_else(|| ApiError::conflict("hosted WorkItem metadata is unavailable"))?;
        metadata
            .workflow_policy
            .as_ref()
            .ok_or_else(|| {
                ApiError::conflict("source-only work cannot enter the hosted controller")
            })?
            .validate()
            .map_err(ApiError::conflict)?;
        let stages = state.store.list_stage_executions(work_item_id).await?;
        let runs = state
            .store
            .list_runs(RunListFilter {
                work_item_id: Some(work_item_id.into()),
                limit: 200,
                ..RunListFilter::default()
            })
            .await?;
        if runs.len() == 200 {
            return Err(ApiError::conflict(
                "hosted execution history exceeds its bounded reconciliation window",
            ));
        }
        let actions = repo_mode::repo_controller_actions(state, work_item_id).await?;
        let hash = canonical_material_hash(&json!({
            "metadata":metadata,
            "stages":stages.iter().map(|s| json!([s.id,s.status,s.input_hash,s.context_pack_id])).collect::<Vec<_>>(),
            "runs":runs.iter().map(|r| json!([r.id,r.status,r.budget_consumption,r.finished_at])).collect::<Vec<_>>(),
            "actions":actions.iter().map(|a| json!([a.id,a.status,a.state_hash])).collect::<Vec<_>>(),
        }))?;
        Ok(Self {
            metadata,
            runs,
            stages,
            actions,
            hash,
        })
    }
}

pub(super) type Condition = (String, String);
pub(super) fn condition(name: &str, reason: impl Into<String>) -> Condition {
    (name.into(), reason.into())
}

pub(super) fn continuation_candidate(snapshot: &Snapshot) -> Option<&StoredRun> {
    let stage = snapshot.stages.iter().rev().find(|s| {
        matches!(
            s.stage_key.as_str(),
            "plan" | "implement" | "test" | "verify"
        )
    })?;
    if !matches!(stage.stage_key.as_str(), "implement" | "test" | "verify") {
        return None;
    }
    snapshot
        .runs
        .iter()
        .find(|r| Some(&r.id) == stage.run_id.as_ref() && r.status == "completed")
}

pub(super) fn now() -> i64 {
    i64::try_from(crate::app::clock::current_millis()).unwrap_or(i64::MAX)
}

use super::{now_string, SqliteStore, StoreError};
use crate::{StoredEnvironmentPreparation, StoredRun, UpdateEnvironmentPreparation};
use pharness_core::RunId;
use serde_json::json;

impl SqliteStore {
    pub async fn fail_hosted_preparation(
        &self,
        run_id: &RunId,
        update: UpdateEnvironmentPreparation,
    ) -> Result<Option<StoredEnvironmentPreparation>, StoreError> {
        if update.status != "failed"
            || update.error.is_none()
            || update.environment_snapshot_json.is_some()
        {
            return Err(StoreError::Conflict(
                "preparation failure requires an error and no successful snapshot".into(),
            ));
        }
        let changed = sqlx::query("UPDATE environment_preparations SET status='failed',logs_json=?3,error=?4,finished_at=?5,updated_at=?5 WHERE id=?1 AND run_id=?2 AND status IN ('queued','running') AND EXISTS (SELECT 1 FROM runs r JOIN work_items w ON w.id=environment_preparations.work_item_id WHERE r.id=?2 AND r.status='preparing' AND w.workflow_policy_json IS NOT NULL)")
            .bind(&update.id).bind(run_id.as_str()).bind(update.logs_json.to_string()).bind(update.error).bind(now_string()).execute(&self.pool).await?;
        if changed.rows_affected() == 0 {
            return Ok(None);
        }
        self.get_environment_preparation(&update.id).await
    }

    /// A fast callback may finish before the create acknowledgement returns.
    /// Recording that acknowledgement must not reopen or erase its result.
    pub async fn mark_environment_preparation_dispatched(
        &self,
        id: &str,
        job_name: &str,
    ) -> Result<StoredEnvironmentPreparation, StoreError> {
        let entry = json!({"step":"dispatch","status":"succeeded","job_name":job_name});
        sqlx::query("UPDATE environment_preparations SET status='running', started_at=COALESCE(started_at,?2), updated_at=?2, logs_json=json_insert(logs_json,'$[#]',json(?3)) WHERE id=?1 AND status='queued' AND json_type(logs_json)='array'")
            .bind(id).bind(now_string()).bind(entry.to_string()).execute(&self.pool).await?;
        let current =
            self.get_environment_preparation(id)
                .await?
                .ok_or_else(|| StoreError::NotFound {
                    entity: "environment_preparation".into(),
                    id: id.into(),
                })?;
        if current.status == "queued" {
            return Err(StoreError::Conflict(
                "preparation dispatch evidence is not a valid sequence".into(),
            ));
        }
        Ok(current)
    }

    /// Call only after signature, source, contract, and runner validation. The
    /// accepted preparation and its existing Run become usable atomically.
    pub async fn complete_hosted_preparation(
        &self,
        run_id: &RunId,
        preparation: UpdateEnvironmentPreparation,
    ) -> Result<(StoredEnvironmentPreparation, StoredRun), StoreError> {
        let (Some(contract), Some(hash), Some(snapshot)) = (
            preparation.project_contract_json,
            preparation.project_contract_hash,
            preparation.environment_snapshot_json,
        ) else {
            return Err(StoreError::Conflict(
                "successful hosted preparation requires complete validated evidence".into(),
            ));
        };
        if preparation.status != "succeeded" || preparation.error.is_some() {
            return Err(StoreError::Conflict(
                "hosted preparation completion requires successful validation".into(),
            ));
        }
        let time = now_string();
        let mut tx = self.pool.begin().await?;
        let work_item: Option<String> = sqlx::query_scalar("SELECT p.work_item_id FROM environment_preparations p JOIN work_items w ON w.id=p.work_item_id WHERE p.id=? AND p.run_id=? AND p.status IN ('queued','running') AND w.workflow_policy_json IS NOT NULL AND w.repository_contract_hash=?")
            .bind(&preparation.id).bind(run_id.as_str()).bind(&hash).fetch_optional(&mut *tx).await?;
        let work_item = work_item.ok_or_else(|| {
            StoreError::Conflict("preparation changed or is not bound to hosted authority".into())
        })?;
        let changed = sqlx::query("UPDATE runs SET execution_target_json=json_set(execution_target_json,'$.environment_snapshot',json(?2)), status='queued', stop_reason=NULL WHERE id=?1 AND status='preparing' AND json_extract(execution_target_json,'$.run_scope.work_item_id')=?3")
            .bind(run_id.as_str()).bind(snapshot.to_string()).bind(&work_item).execute(&mut *tx).await?;
        if changed.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "Run changed before preparation completion".into(),
            ));
        }
        sqlx::query("UPDATE environment_preparations SET status='succeeded', project_contract_json=?2, project_contract_hash=?3, environment_snapshot_json=?4, logs_json=?5, error=NULL, started_at=COALESCE(started_at,?6), finished_at=?6, updated_at=?6 WHERE id=?1")
            .bind(&preparation.id).bind(contract.to_string()).bind(hash).bind(snapshot.to_string())
            .bind(preparation.logs_json.to_string()).bind(&time).execute(&mut *tx).await?;
        sqlx::query("UPDATE work_items SET environment_preparation_status='succeeded',current_environment_snapshot_id=?2,updated_at=?3 WHERE id=?1")
            .bind(&work_item).bind(&preparation.id).bind(&time).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok((
            self.get_environment_preparation(&preparation.id)
                .await?
                .ok_or_else(|| StoreError::NotFound {
                    entity: "environment_preparation".into(),
                    id: preparation.id,
                })?,
            self.get_run(run_id)
                .await?
                .ok_or_else(|| StoreError::NotFound {
                    entity: "run".into(),
                    id: run_id.to_string(),
                })?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CreateEnvironmentPreparation, CreateRun, CreateSession, CreateWorkspace};
    use pharness_core::SessionId;

    #[tokio::test]
    async fn hosted_preparation_completion_rolls_back_as_a_unit_and_late_ack_cannot_reopen_it() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        sqlx::query("INSERT INTO work_items(id,status,title,intent,acceptance_criteria_json,source_repo,source_ref,target_environment,production_impacting,max_attempts,max_elapsed_seconds,created_at,updated_at,status_changed_at,mode,state_version,workflow_policy_json,workflow_policy_hash,repository_contract_hash) VALUES('item','submitted','Fixture','Bounded','[]','https://github.com/example/app.git','main','repository',0,2,3600,'100','100','100','repo',1,'{\"schema_version\":\"pharness.dev/hosted-workflow/v1alpha1\"}','sha256:fixture','sha256:contract')").execute(&store.pool).await.unwrap();
        let run_id = RunId::new("run_prep");
        let session = SessionId::new("session_prep");
        store
            .create_session(CreateSession {
                id: session.clone(),
                title: "Fixture".into(),
                cwd: "/workspace".into(),
            })
            .await
            .unwrap();
        let run = store
            .create_run(CreateRun {
                id: run_id.clone(),
                session_id: session,
                user_task: "Fixture".into(),
                cwd: "/workspace".into(),
                max_turns: 10,
                initial_status: "preparing".into(),
                execution_target_json: json!({"run_scope":{"work_item_id":"item"}}),
            })
            .await
            .unwrap();
        store
            .create_workspace(CreateWorkspace {
                id: "workspace".into(),
                work_item_id: "item".into(),
                run_id: Some(run_id.clone()),
                status: "declared".into(),
                source_repo: "https://github.com/example/app.git".into(),
                source_ref: "main".into(),
                resolved_commit: Some("a".repeat(40)),
                branch: Some("pharness/item".into()),
                retention_status: "retained".into(),
                actor: Some("fixture".into()),
                reason: Some("fixture".into()),
            })
            .await
            .unwrap();
        let initial = store
            .create_environment_preparation(CreateEnvironmentPreparation {
                id: "prep".into(),
                work_item_id: "item".into(),
                workspace_id: "workspace".into(),
                run_id: Some(run_id.clone()),
                status: "queued".into(),
                environment_profile_id: "python".into(),
                source_commit: "a".repeat(40),
            })
            .await
            .unwrap();
        let update = || UpdateEnvironmentPreparation {
            id: "prep".into(),
            status: "succeeded".into(),
            project_contract_json: Some(json!({"api_version":"fixture"})),
            project_contract_hash: Some("sha256:contract".into()),
            environment_snapshot_json: Some(json!({"validated":"fixture"})),
            logs_json: json!([]),
            error: None,
        };
        sqlx::query("CREATE TRIGGER reject_fixture_preparation BEFORE UPDATE ON environment_preparations WHEN NEW.status='succeeded' BEGIN SELECT RAISE(ABORT,'injected persistence failure'); END").execute(&store.pool).await.unwrap();
        assert!(store
            .complete_hosted_preparation(&run_id, update())
            .await
            .is_err());
        assert_eq!(store.get_run(&run_id).await.unwrap().unwrap(), run);
        assert_eq!(
            store
                .get_environment_preparation("prep")
                .await
                .unwrap()
                .unwrap(),
            initial
        );
        let snapshot: Option<String> = sqlx::query_scalar(
            "SELECT current_environment_snapshot_id FROM work_items WHERE id='item'",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap();
        assert!(snapshot.is_none());
        sqlx::query("DROP TRIGGER reject_fixture_preparation")
            .execute(&store.pool)
            .await
            .unwrap();
        let (accepted, queued) = store
            .complete_hosted_preparation(&run_id, update())
            .await
            .unwrap();
        assert_eq!(accepted.status, "succeeded");
        assert_eq!(queued.status, "queued");
        assert_eq!(queued.run_budget, run.run_budget);
        assert_eq!(
            queued.execution_target_json["environment_snapshot"],
            json!({"validated":"fixture"})
        );
        assert_eq!(
            store
                .mark_environment_preparation_dispatched("prep", "job-existing")
                .await
                .unwrap(),
            accepted
        );
        assert_eq!(
            store
                .mark_environment_preparation_dispatched("prep", "job-existing")
                .await
                .unwrap(),
            accepted
        );
        let mut stale_failure = update();
        stale_failure.status = "failed".into();
        stale_failure.error = Some("late failed callback".into());
        stale_failure.environment_snapshot_json = None;
        assert!(store
            .fail_hosted_preparation(&run_id, stale_failure)
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            store
                .get_environment_preparation("prep")
                .await
                .unwrap()
                .unwrap(),
            accepted
        );
        assert!(store
            .complete_hosted_preparation(&run_id, update())
            .await
            .is_err());
    }
}

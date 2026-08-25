use super::{now_string, SqliteStore, StoreError};
use crate::{
    CreateAgentContextPack, CreateEvidenceValidation, CreateOperatorAnnotation, CreateRepoWorkItem,
    CreateStageExecution, SealStageOutcome, StoredAgentContextPack, StoredOperatorAnnotation,
    StoredRepoWorkItemMetadata, StoredStageExecution, StoredStageOutcome,
};
use pharness_core::RunId;
use sqlx::Row;

impl SqliteStore {
    pub async fn create_repo_work_item(
        &self,
        item: CreateRepoWorkItem,
    ) -> Result<crate::StoredWorkItem, StoreError> {
        let now = now_string();
        let acceptance = serde_json::to_string(&item.acceptance_commands)?;
        let names = serde_json::to_string(&item.acceptance_command_names)?;
        let context = serde_json::to_string(&item.context_repositories)?;
        let budget = serde_json::to_string(&item.run_budget)?;
        let contract = serde_json::to_string(&item.repository_contract_json)?;
        sqlx::query(
            r#"
            INSERT INTO work_items (
              id, status, title, intent, acceptance_criteria_json, source_repo, source_ref,
              source_commit, target_environment, production_impacting, max_attempts,
              max_elapsed_seconds, created_by, origin, environment_profile_id, run_budget_json,
              repository_contract_json, repository_contract_hash, environment_preparation_status,
              mode, product_id, mutable_repository_id, product_model_snapshot_id,
              product_model_snapshot_hash, repository_contract_version_id, contract_version,
              selected_acceptance_names_json, context_repositories_json, state_version,
              created_at, updated_at, status_changed_at, status_changed_by, status_reason
            ) VALUES (
              ?1, 'proposed', ?2, ?3, ?4, ?5, ?6, ?7, 'repository', 0, ?8, ?9, ?10,
              'operator', ?11, ?12, ?13, ?14, 'ready', 'repo', ?15, ?16, ?17, ?18,
              ?19, ?20, ?21, ?22, 1, ?23, ?23, ?23, ?10, 'Repo Mode WorkItem created'
            )
            "#,
        )
        .bind(&item.id)
        .bind(&item.title)
        .bind(&item.intent)
        .bind(acceptance)
        .bind(&item.source_repo)
        .bind(&item.source_ref)
        .bind(&item.source_commit)
        .bind(i64::from(item.max_attempts))
        .bind(i64::try_from(item.run_budget.active_execution_seconds).unwrap_or(i64::MAX))
        .bind(&item.actor)
        .bind(&item.environment_profile_id)
        .bind(budget)
        .bind(contract)
        .bind(&item.repository_contract_hash)
        .bind(&item.product_id)
        .bind(&item.repository_id)
        .bind(&item.product_model_snapshot_id)
        .bind(&item.product_model_snapshot_hash)
        .bind(&item.repository_contract_version_id)
        .bind(&item.contract_version)
        .bind(names)
        .bind(context)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        self.get_work_item(&item.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "work_item".into(),
                id: item.id,
            })
    }

    pub async fn get_repo_work_item_metadata(
        &self,
        work_item_id: &str,
    ) -> Result<Option<StoredRepoWorkItemMetadata>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, mode, product_id, mutable_repository_id, product_model_snapshot_id,
                   product_model_snapshot_hash, repository_contract_version_id, contract_version,
                   selected_acceptance_names_json, context_repositories_json,
                   current_stage_execution_id, state_version, closed_at, closure_reason
            FROM work_items WHERE id = ?1 AND mode = 'repo'
            "#,
        )
        .bind(work_item_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_repo_metadata).transpose()
    }

    pub async fn create_stage_execution(
        &self,
        execution: CreateStageExecution,
    ) -> Result<StoredStageExecution, StoreError> {
        let now = now_string();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO stage_executions (
              id, work_item_id, stage_key, sequence, status, agent_profile_id,
              agent_profile_version, agent_profile_hash, context_pack_id, run_id, workspace_id,
              input_snapshot_json, input_hash, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
            "#,
        )
        .bind(&execution.id)
        .bind(&execution.work_item_id)
        .bind(&execution.stage_key)
        .bind(execution.sequence as i64)
        .bind(&execution.status)
        .bind(execution.agent_profile_id)
        .bind(execution.agent_profile_version)
        .bind(execution.agent_profile_hash)
        .bind(execution.context_pack_id)
        .bind(execution.run_id.as_ref().map(RunId::as_str))
        .bind(execution.workspace_id)
        .bind(serde_json::to_string(&execution.input_snapshot)?)
        .bind(execution.input_hash)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE work_items SET current_stage_execution_id = ?2, state_version = state_version + 1, updated_at = ?3 WHERE id = ?1 AND mode = 'repo'",
        )
        .bind(&execution.work_item_id)
        .bind(&execution.id)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.get_stage_execution(&execution.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "stage_execution".into(),
                id: execution.id,
            })
    }

    pub async fn get_stage_execution(
        &self,
        id: &str,
    ) -> Result<Option<StoredStageExecution>, StoreError> {
        let row = sqlx::query(&stage_execution_select("WHERE id = ?1"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_stage_execution).transpose()
    }

    pub async fn list_stage_executions(
        &self,
        work_item_id: &str,
    ) -> Result<Vec<StoredStageExecution>, StoreError> {
        let rows = sqlx::query(&stage_execution_select(
            "WHERE work_item_id = ?1 ORDER BY created_at, id",
        ))
        .bind(work_item_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_stage_execution).collect()
    }

    pub async fn seal_stage_outcome(
        &self,
        outcome: SealStageOutcome,
    ) -> Result<StoredStageOutcome, StoreError> {
        let now = now_string();
        let mut tx = self.pool.begin().await?;
        let execution = sqlx::query(
            "SELECT work_item_id, stage_key, status FROM stage_executions WHERE id = ?1",
        )
        .bind(&outcome.stage_execution_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StoreError::NotFound {
            entity: "stage_execution".into(),
            id: outcome.stage_execution_id.clone(),
        })?;
        let work_item_id: String = execution.try_get("work_item_id")?;
        let stage_key: String = execution.try_get("stage_key")?;
        if work_item_id != outcome.work_item_id || stage_key != outcome.stage_key {
            return Err(StoreError::Conflict(
                "stage outcome scope does not match its execution".into(),
            ));
        }
        sqlx::query(
            r#"
            INSERT INTO stage_outcomes (
              id, stage_execution_id, work_item_id, stage_key, status, schema_version,
              outcome_json, content_hash, state_version, supersedes_outcome_id, sealed_by, sealed_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, 'pharness.dev/stage-outcome/v1alpha1',
                      ?6, ?7, ?8, ?9, ?10, ?11)
            "#,
        )
        .bind(&outcome.id)
        .bind(&outcome.stage_execution_id)
        .bind(&outcome.work_item_id)
        .bind(&outcome.stage_key)
        .bind(&outcome.status)
        .bind(serde_json::to_string(&outcome.outcome)?)
        .bind(&outcome.content_hash)
        .bind(outcome.state_version as i64)
        .bind(&outcome.supersedes_outcome_id)
        .bind(&outcome.actor)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO effective_stage_outcomes (
              work_item_id, stage_key, outcome_id, state_version, changed_by, change_reason, changed_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(work_item_id, stage_key) DO UPDATE SET
              outcome_id = excluded.outcome_id,
              state_version = excluded.state_version,
              changed_by = excluded.changed_by,
              change_reason = excluded.change_reason,
              changed_at = excluded.changed_at
            "#,
        )
        .bind(&outcome.work_item_id)
        .bind(&outcome.stage_key)
        .bind(&outcome.id)
        .bind(outcome.state_version as i64)
        .bind(&outcome.actor)
        .bind(&outcome.reason)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE stage_executions SET status = ?2, stop_reason = json_extract(?3, '$.stop_reason'), finished_at = ?4 WHERE id = ?1 AND finished_at IS NULL",
        )
        .bind(&outcome.stage_execution_id)
        .bind(&outcome.status)
        .bind(serde_json::to_string(&outcome.outcome)?)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.get_stage_outcome(&outcome.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "stage_outcome".into(),
                id: outcome.id,
            })
    }

    pub async fn get_stage_outcome(
        &self,
        id: &str,
    ) -> Result<Option<StoredStageOutcome>, StoreError> {
        let row = sqlx::query(&stage_outcome_select("WHERE id = ?1"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_stage_outcome).transpose()
    }

    pub async fn get_stage_outcome_for_execution(
        &self,
        execution_id: &str,
    ) -> Result<Option<StoredStageOutcome>, StoreError> {
        let row = sqlx::query(&stage_outcome_select("WHERE stage_execution_id = ?1"))
            .bind(execution_id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_stage_outcome).transpose()
    }

    pub async fn create_evidence_validation(
        &self,
        validation: CreateEvidenceValidation,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO evidence_validations (
              id, work_item_id, stage_execution_id, validator_key, schema_version, status,
              subject_json, evidence_refs_json, facts_json, contradictions_json,
              content_hash, validated_at
            ) VALUES (?1, ?2, ?3, ?4, 'pharness.dev/evidence-validation/v1alpha1',
                      ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            "#,
        )
        .bind(validation.id)
        .bind(validation.work_item_id)
        .bind(validation.stage_execution_id)
        .bind(validation.validator_key)
        .bind(validation.status)
        .bind(serde_json::to_string(&validation.subject)?)
        .bind(serde_json::to_string(&validation.evidence_refs)?)
        .bind(serde_json::to_string(&validation.facts)?)
        .bind(serde_json::to_string(&validation.contradictions)?)
        .bind(validation.content_hash)
        .bind(now_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn create_agent_context_pack(
        &self,
        pack: CreateAgentContextPack,
    ) -> Result<StoredAgentContextPack, StoreError> {
        let now = now_string();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO agent_context_packs (
              id, work_item_id, stage_execution_id, schema_version, context_json,
              estimated_tokens, content_hash, created_at
            ) VALUES (?1, ?2, ?3, 'pharness.dev/agent-context/v1alpha1', ?4, ?5, ?6, ?7)
            "#,
        )
        .bind(&pack.id)
        .bind(&pack.work_item_id)
        .bind(&pack.stage_execution_id)
        .bind(serde_json::to_string(&pack.context)?)
        .bind(pack.estimated_tokens as i64)
        .bind(&pack.content_hash)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE stage_executions SET context_pack_id = ?2 WHERE id = ?1 AND work_item_id = ?3",
        )
        .bind(&pack.stage_execution_id)
        .bind(&pack.id)
        .bind(&pack.work_item_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.get_agent_context_pack(&pack.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "agent_context_pack".into(),
                id: pack.id,
            })
    }

    pub async fn get_agent_context_pack(
        &self,
        id: &str,
    ) -> Result<Option<StoredAgentContextPack>, StoreError> {
        let row = sqlx::query(
            "SELECT id, work_item_id, stage_execution_id, schema_version, context_json, estimated_tokens, content_hash, created_at FROM agent_context_packs WHERE id = ?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_context_pack).transpose()
    }

    pub async fn create_operator_annotation(
        &self,
        annotation: CreateOperatorAnnotation,
    ) -> Result<StoredOperatorAnnotation, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            INSERT INTO operator_annotations (
              id, work_item_id, target_kind, target_id, statement, evidence_refs_json,
              requested_effect, actor, reason, state_hash, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            "#,
        )
        .bind(&annotation.id)
        .bind(&annotation.work_item_id)
        .bind(&annotation.target_kind)
        .bind(&annotation.target_id)
        .bind(&annotation.statement)
        .bind(serde_json::to_string(&annotation.evidence_refs)?)
        .bind(&annotation.requested_effect)
        .bind(&annotation.actor)
        .bind(&annotation.reason)
        .bind(&annotation.state_hash)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        self.list_operator_annotations(&annotation.work_item_id)
            .await?
            .into_iter()
            .find(|stored| stored.id == annotation.id)
            .ok_or_else(|| StoreError::NotFound {
                entity: "operator_annotation".into(),
                id: annotation.id,
            })
    }

    pub async fn list_operator_annotations(
        &self,
        work_item_id: &str,
    ) -> Result<Vec<StoredOperatorAnnotation>, StoreError> {
        let rows = sqlx::query(
            "SELECT id, work_item_id, target_kind, target_id, statement, evidence_refs_json, requested_effect, actor, reason, state_hash, created_at FROM operator_annotations WHERE work_item_id = ?1 ORDER BY created_at, id",
        )
        .bind(work_item_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_annotation).collect()
    }
}

fn stage_execution_select(where_clause: &str) -> String {
    format!(
        "SELECT id, work_item_id, stage_key, sequence, status, agent_profile_id, \
         agent_profile_version, agent_profile_hash, context_pack_id, run_id, workspace_id, \
         input_snapshot_json, input_hash, stop_reason, created_at, started_at, finished_at \
         FROM stage_executions {where_clause}"
    )
}

fn stage_outcome_select(where_clause: &str) -> String {
    format!(
        "SELECT id, stage_execution_id, work_item_id, stage_key, status, schema_version, \
         outcome_json, content_hash, state_version, supersedes_outcome_id, sealed_by, sealed_at \
         FROM stage_outcomes {where_clause}"
    )
}

fn row_to_repo_metadata(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredRepoWorkItemMetadata, StoreError> {
    Ok(StoredRepoWorkItemMetadata {
        work_item_id: row.try_get("id")?,
        mode: row.try_get("mode")?,
        product_id: row.try_get("product_id")?,
        repository_id: row.try_get("mutable_repository_id")?,
        product_model_snapshot_id: row.try_get("product_model_snapshot_id")?,
        product_model_snapshot_hash: row.try_get("product_model_snapshot_hash")?,
        repository_contract_version_id: row.try_get("repository_contract_version_id")?,
        contract_version: row.try_get("contract_version")?,
        acceptance_command_names: serde_json::from_str(
            &row.try_get::<String, _>("selected_acceptance_names_json")?,
        )?,
        context_repositories: serde_json::from_str(
            &row.try_get::<String, _>("context_repositories_json")?,
        )?,
        current_stage_execution_id: row.try_get("current_stage_execution_id")?,
        state_version: row.try_get::<i64, _>("state_version")? as u64,
        closed_at: row.try_get("closed_at")?,
        closure_reason: row.try_get("closure_reason")?,
    })
}

fn row_to_stage_execution(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredStageExecution, StoreError> {
    Ok(StoredStageExecution {
        id: row.try_get("id")?,
        work_item_id: row.try_get("work_item_id")?,
        stage_key: row.try_get("stage_key")?,
        sequence: row.try_get::<i64, _>("sequence")? as u64,
        status: row.try_get("status")?,
        agent_profile_id: row.try_get("agent_profile_id")?,
        agent_profile_version: row.try_get("agent_profile_version")?,
        agent_profile_hash: row.try_get("agent_profile_hash")?,
        context_pack_id: row.try_get("context_pack_id")?,
        run_id: row.try_get::<Option<String>, _>("run_id")?.map(RunId::new),
        workspace_id: row.try_get("workspace_id")?,
        input_snapshot: serde_json::from_str(&row.try_get::<String, _>("input_snapshot_json")?)?,
        input_hash: row.try_get("input_hash")?,
        stop_reason: row.try_get("stop_reason")?,
        created_at: row.try_get("created_at")?,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
    })
}

fn row_to_stage_outcome(row: sqlx::sqlite::SqliteRow) -> Result<StoredStageOutcome, StoreError> {
    Ok(StoredStageOutcome {
        id: row.try_get("id")?,
        stage_execution_id: row.try_get("stage_execution_id")?,
        work_item_id: row.try_get("work_item_id")?,
        stage_key: row.try_get("stage_key")?,
        status: row.try_get("status")?,
        schema_version: row.try_get("schema_version")?,
        outcome: serde_json::from_str(&row.try_get::<String, _>("outcome_json")?)?,
        content_hash: row.try_get("content_hash")?,
        state_version: row.try_get::<i64, _>("state_version")? as u64,
        supersedes_outcome_id: row.try_get("supersedes_outcome_id")?,
        sealed_by: row.try_get("sealed_by")?,
        sealed_at: row.try_get("sealed_at")?,
    })
}

fn row_to_context_pack(row: sqlx::sqlite::SqliteRow) -> Result<StoredAgentContextPack, StoreError> {
    Ok(StoredAgentContextPack {
        id: row.try_get("id")?,
        work_item_id: row.try_get("work_item_id")?,
        stage_execution_id: row.try_get("stage_execution_id")?,
        schema_version: row.try_get("schema_version")?,
        context: serde_json::from_str(&row.try_get::<String, _>("context_json")?)?,
        estimated_tokens: row.try_get::<i64, _>("estimated_tokens")? as u64,
        content_hash: row.try_get("content_hash")?,
        created_at: row.try_get("created_at")?,
    })
}

fn row_to_annotation(row: sqlx::sqlite::SqliteRow) -> Result<StoredOperatorAnnotation, StoreError> {
    Ok(StoredOperatorAnnotation {
        id: row.try_get("id")?,
        work_item_id: row.try_get("work_item_id")?,
        target_kind: row.try_get("target_kind")?,
        target_id: row.try_get("target_id")?,
        statement: row.try_get("statement")?,
        evidence_refs: serde_json::from_str(&row.try_get::<String, _>("evidence_refs_json")?)?,
        requested_effect: row.try_get("requested_effect")?,
        actor: row.try_get("actor")?,
        reason: row.try_get("reason")?,
        state_hash: row.try_get("state_hash")?,
        created_at: row.try_get("created_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn store_with_repo_work_item() -> SqliteStore {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let now = now_string();
        sqlx::query(
            r#"
            INSERT INTO work_items (
              id, status, title, intent, acceptance_criteria_json, source_repo, source_ref,
              target_environment, production_impacting, max_attempts, max_elapsed_seconds,
              created_at, updated_at, status_changed_at, mode, state_version
            ) VALUES ('witem_repo', 'proposed', 'Repo item', 'Test', '[]',
                      'https://github.com/example/repo.git', 'main', 'repository', 0, 2, 3600,
                      ?1, ?1, ?1, 'repo', 1)
            "#,
        )
        .bind(now)
        .execute(&store.pool)
        .await
        .unwrap();
        store
    }

    #[tokio::test]
    async fn stage_outcomes_are_immutable_and_effective_pointer_is_transactional() {
        let store = store_with_repo_work_item().await;
        let execution = store
            .create_stage_execution(CreateStageExecution {
                id: "stageexec_one".into(),
                work_item_id: "witem_repo".into(),
                stage_key: "discover".into(),
                sequence: 1,
                status: "running".into(),
                agent_profile_id: None,
                agent_profile_version: None,
                agent_profile_hash: None,
                context_pack_id: None,
                run_id: None,
                workspace_id: None,
                input_snapshot: json!({"source_commit": "a".repeat(40)}),
                input_hash: "sha256:input".into(),
            })
            .await
            .unwrap();
        assert_eq!(execution.sequence, 1);
        let outcome = store
            .seal_stage_outcome(SealStageOutcome {
                id: "stageout_one".into(),
                stage_execution_id: execution.id,
                work_item_id: "witem_repo".into(),
                stage_key: "discover".into(),
                status: "succeeded".into(),
                outcome: json!({"stop_reason":"controller sealed readiness"}),
                content_hash: "sha256:outcome".into(),
                state_version: 2,
                supersedes_outcome_id: None,
                actor: "controller".into(),
                reason: "validated evidence".into(),
            })
            .await
            .unwrap();
        assert_eq!(outcome.status, "succeeded");
        let effective: String = sqlx::query_scalar(
            "SELECT outcome_id FROM effective_stage_outcomes WHERE work_item_id = 'witem_repo' AND stage_key = 'discover'",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap();
        assert_eq!(effective, "stageout_one");
        let update =
            sqlx::query("UPDATE stage_outcomes SET status = 'failed' WHERE id = 'stageout_one'")
                .execute(&store.pool)
                .await;
        assert!(update.is_err());
        let delete = sqlx::query("DELETE FROM stage_outcomes WHERE id = 'stageout_one'")
            .execute(&store.pool)
            .await;
        assert!(delete.is_err());
    }
}

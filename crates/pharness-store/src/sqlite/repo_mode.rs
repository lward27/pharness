use super::{now_string, SqliteStore, StoreError};
use crate::{
    CreateAgentContextPack, CreateEvidenceRetrieval, CreateEvidenceValidation,
    CreateOperatorAnnotation, CreateProviderCheckSetObservation, CreateRepoWorkItem,
    CreateStageChainAuthorization, CreateStageExecution, SealStageOutcome, StoredAgentContextPack,
    StoredOperatorAnnotation, StoredProviderCheckSetObservation, StoredRepoWorkItemMetadata,
    StoredStageChainAuthorization, StoredStageExecution, StoredStageOutcome,
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

    pub async fn update_repo_work_item_status(
        &self,
        work_item_id: &str,
        status: &str,
        actor: &str,
        reason: &str,
        close: bool,
    ) -> Result<crate::StoredWorkItem, StoreError> {
        let now = now_string();
        let result = sqlx::query(
            r#"
            UPDATE work_items
            SET status = ?2, updated_at = ?3, status_changed_at = ?3,
                status_changed_by = ?4, status_reason = ?5,
                state_version = state_version + 1,
                closed_at = CASE WHEN ?6 = 1 THEN ?3 ELSE closed_at END,
                closure_reason = CASE WHEN ?6 = 1 THEN ?5 ELSE closure_reason END
            WHERE id = ?1 AND mode = 'repo' AND closed_at IS NULL
            "#,
        )
        .bind(work_item_id)
        .bind(status)
        .bind(&now)
        .bind(actor)
        .bind(reason)
        .bind(if close { 1 } else { 0 })
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "Repo WorkItem is closed or no longer mutable".into(),
            ));
        }
        self.get_work_item(work_item_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "work_item".into(),
                id: work_item_id.into(),
            })
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

    pub async fn list_effective_stage_outcomes(
        &self,
        work_item_id: &str,
    ) -> Result<Vec<StoredStageOutcome>, StoreError> {
        let rows = sqlx::query(&stage_outcome_select(
            "JOIN effective_stage_outcomes effective ON effective.outcome_id = stage_outcomes.id WHERE effective.work_item_id = ?1 ORDER BY stage_outcomes.sealed_at, stage_outcomes.id",
        ))
        .bind(work_item_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_stage_outcome).collect()
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

    pub async fn create_stage_chain_authorization(
        &self,
        authorization: CreateStageChainAuthorization,
    ) -> Result<StoredStageChainAuthorization, StoreError> {
        let now = now_string();
        let mut tx = self.pool.begin().await?;
        let active: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM stage_chain_authorizations WHERE work_item_id = ?1 AND status = 'active' AND revoked_at IS NULL",
        )
        .bind(&authorization.work_item_id)
        .fetch_one(&mut *tx)
        .await?;
        if active != 0 {
            return Err(StoreError::Conflict(
                "WorkItem already has an active stage-chain authorization".into(),
            ));
        }
        sqlx::query(
            r#"
            INSERT INTO stage_chain_authorizations (
              id, work_item_id, work_plan_id, work_plan_revision,
              product_model_snapshot_id, product_model_snapshot_hash, repository_id,
              source_commit, workspace_id, writable_paths_json, profile_chain_json,
              budget_chain_json, state_hash, status, created_by, creation_reason,
              created_at, expires_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                      ?13, 'active', ?14, ?15, ?16, ?17)
            "#,
        )
        .bind(&authorization.id)
        .bind(&authorization.work_item_id)
        .bind(&authorization.work_plan_id)
        .bind(authorization.work_plan_revision)
        .bind(&authorization.product_model_snapshot_id)
        .bind(&authorization.product_model_snapshot_hash)
        .bind(&authorization.repository_id)
        .bind(&authorization.source_commit)
        .bind(&authorization.workspace_id)
        .bind(serde_json::to_string(&authorization.writable_paths)?)
        .bind(serde_json::to_string(&authorization.profile_chain)?)
        .bind(serde_json::to_string(&authorization.budget_chain)?)
        .bind(&authorization.state_hash)
        .bind(&authorization.created_by)
        .bind(&authorization.creation_reason)
        .bind(&now)
        .bind(&authorization.expires_at)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE work_items SET state_version = state_version + 1, updated_at = ?2 WHERE id = ?1 AND mode = 'repo'",
        )
        .bind(&authorization.work_item_id)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.get_stage_chain_authorization(&authorization.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "stage_chain_authorization".into(),
                id: authorization.id,
            })
    }

    pub async fn get_stage_chain_authorization(
        &self,
        id: &str,
    ) -> Result<Option<StoredStageChainAuthorization>, StoreError> {
        let row = sqlx::query(&stage_chain_authorization_select("WHERE id = ?1"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_stage_chain_authorization).transpose()
    }

    pub async fn active_stage_chain_authorization(
        &self,
        work_item_id: &str,
    ) -> Result<Option<StoredStageChainAuthorization>, StoreError> {
        let row = sqlx::query(&stage_chain_authorization_select(
            "WHERE work_item_id = ?1 AND status = 'active' AND revoked_at IS NULL ORDER BY created_at DESC LIMIT 1",
        ))
        .bind(work_item_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_stage_chain_authorization).transpose()
    }

    pub async fn revoke_stage_chain_authorization(
        &self,
        id: &str,
        reason: &str,
    ) -> Result<StoredStageChainAuthorization, StoreError> {
        sqlx::query(
            "UPDATE stage_chain_authorizations SET status = 'revoked', revoked_at = ?2, revocation_reason = ?3 WHERE id = ?1 AND status = 'active'",
        )
        .bind(id)
        .bind(now_string())
        .bind(reason)
        .execute(&self.pool)
        .await?;
        self.get_stage_chain_authorization(id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "stage_chain_authorization".into(),
                id: id.into(),
            })
    }

    pub async fn create_evidence_retrieval(
        &self,
        retrieval: CreateEvidenceRetrieval,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO evidence_retrievals (
              id, work_item_id, stage_execution_id, run_id, actor, evidence_kind,
              evidence_id, evidence_version, returned_hash, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
        )
        .bind(retrieval.id)
        .bind(retrieval.work_item_id)
        .bind(retrieval.stage_execution_id)
        .bind(retrieval.run_id.as_str())
        .bind(retrieval.actor)
        .bind(retrieval.evidence_kind)
        .bind(retrieval.evidence_id)
        .bind(retrieval.evidence_version)
        .bind(retrieval.returned_hash)
        .bind(now_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn create_provider_check_set_observation(
        &self,
        observation: CreateProviderCheckSetObservation,
    ) -> Result<StoredProviderCheckSetObservation, StoreError> {
        sqlx::query(
            r#"
            INSERT INTO provider_check_set_observations (
              id, source_delivery_intent_id, phase, repository_id, pull_request_number,
              head_sha, required_set_hash, authoritative_rules_succeeded, status,
              required_checks_json, check_runs_json, commit_statuses_json, content_hash,
              observed_at, expires_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                      ?13, ?14, ?15)
            "#,
        )
        .bind(&observation.id)
        .bind(&observation.source_delivery_intent_id)
        .bind(&observation.phase)
        .bind(&observation.repository_id)
        .bind(observation.pull_request_number as i64)
        .bind(&observation.head_sha)
        .bind(&observation.required_set_hash)
        .bind(if observation.authoritative_rules_succeeded {
            1
        } else {
            0
        })
        .bind(&observation.status)
        .bind(serde_json::to_string(&observation.required_checks)?)
        .bind(serde_json::to_string(&observation.check_runs)?)
        .bind(serde_json::to_string(&observation.commit_statuses)?)
        .bind(&observation.content_hash)
        .bind(now_string())
        .bind(&observation.expires_at)
        .execute(&self.pool)
        .await?;
        self.get_provider_check_set_observation(&observation.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "provider_check_set_observation".into(),
                id: observation.id,
            })
    }

    pub async fn get_provider_check_set_observation(
        &self,
        id: &str,
    ) -> Result<Option<StoredProviderCheckSetObservation>, StoreError> {
        let row = sqlx::query(&provider_check_set_observation_select("WHERE id = ?1"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_provider_check_set_observation).transpose()
    }

    pub async fn latest_provider_check_set_observation(
        &self,
        source_delivery_intent_id: &str,
        phase: &str,
    ) -> Result<Option<StoredProviderCheckSetObservation>, StoreError> {
        let row = sqlx::query(&provider_check_set_observation_select(
            "WHERE source_delivery_intent_id = ?1 AND phase = ?2 ORDER BY observed_at DESC, id DESC LIMIT 1",
        ))
        .bind(source_delivery_intent_id)
        .bind(phase)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_provider_check_set_observation).transpose()
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
        "SELECT stage_outcomes.id, stage_outcomes.stage_execution_id, stage_outcomes.work_item_id, \
         stage_outcomes.stage_key, stage_outcomes.status, stage_outcomes.schema_version, \
         stage_outcomes.outcome_json, stage_outcomes.content_hash, stage_outcomes.state_version, \
         stage_outcomes.supersedes_outcome_id, stage_outcomes.sealed_by, stage_outcomes.sealed_at \
         FROM stage_outcomes {where_clause}"
    )
}

fn stage_chain_authorization_select(where_clause: &str) -> String {
    format!(
        "SELECT id, work_item_id, work_plan_id, work_plan_revision, product_model_snapshot_id, \
         product_model_snapshot_hash, repository_id, source_commit, workspace_id, \
         writable_paths_json, profile_chain_json, budget_chain_json, state_hash, status, \
         created_by, creation_reason, created_at, expires_at, revoked_at, revocation_reason \
         FROM stage_chain_authorizations {where_clause}"
    )
}

fn provider_check_set_observation_select(where_clause: &str) -> String {
    format!(
        "SELECT id, source_delivery_intent_id, phase, repository_id, pull_request_number, \
         head_sha, required_set_hash, authoritative_rules_succeeded, status, required_checks_json, \
         check_runs_json, commit_statuses_json, content_hash, observed_at, expires_at \
         FROM provider_check_set_observations {where_clause}"
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

fn row_to_stage_chain_authorization(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredStageChainAuthorization, StoreError> {
    Ok(StoredStageChainAuthorization {
        id: row.try_get("id")?,
        work_item_id: row.try_get("work_item_id")?,
        work_plan_id: row.try_get("work_plan_id")?,
        work_plan_revision: row.try_get("work_plan_revision")?,
        product_model_snapshot_id: row.try_get("product_model_snapshot_id")?,
        product_model_snapshot_hash: row.try_get("product_model_snapshot_hash")?,
        repository_id: row.try_get("repository_id")?,
        source_commit: row.try_get("source_commit")?,
        workspace_id: row.try_get("workspace_id")?,
        writable_paths: serde_json::from_str(&row.try_get::<String, _>("writable_paths_json")?)?,
        profile_chain: serde_json::from_str(&row.try_get::<String, _>("profile_chain_json")?)?,
        budget_chain: serde_json::from_str(&row.try_get::<String, _>("budget_chain_json")?)?,
        state_hash: row.try_get("state_hash")?,
        status: row.try_get("status")?,
        created_by: row.try_get("created_by")?,
        creation_reason: row.try_get("creation_reason")?,
        created_at: row.try_get("created_at")?,
        expires_at: row.try_get("expires_at")?,
        revoked_at: row.try_get("revoked_at")?,
        revocation_reason: row.try_get("revocation_reason")?,
    })
}

fn row_to_provider_check_set_observation(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredProviderCheckSetObservation, StoreError> {
    Ok(StoredProviderCheckSetObservation {
        id: row.try_get("id")?,
        source_delivery_intent_id: row.try_get("source_delivery_intent_id")?,
        phase: row.try_get("phase")?,
        repository_id: row.try_get("repository_id")?,
        pull_request_number: row.try_get::<i64, _>("pull_request_number")? as u64,
        head_sha: row.try_get("head_sha")?,
        required_set_hash: row.try_get("required_set_hash")?,
        authoritative_rules_succeeded: row.try_get::<i64, _>("authoritative_rules_succeeded")? != 0,
        status: row.try_get("status")?,
        required_checks: serde_json::from_str(&row.try_get::<String, _>("required_checks_json")?)?,
        check_runs: serde_json::from_str(&row.try_get::<String, _>("check_runs_json")?)?,
        commit_statuses: serde_json::from_str(&row.try_get::<String, _>("commit_statuses_json")?)?,
        content_hash: row.try_get("content_hash")?,
        observed_at: row.try_get("observed_at")?,
        expires_at: row.try_get("expires_at")?,
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

    async fn insert_stage_chain_scope(store: &SqliteStore) {
        let now = now_string();
        sqlx::query(
            "INSERT INTO organizations (id, organization_key, display_name, created_at, updated_at) VALUES ('org_test', 'test', 'Test', ?1, ?1)",
        )
        .bind(&now)
        .execute(&store.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO products (id, organization_id, product_key, display_name, description, owner_principal, created_at, updated_at) VALUES ('prod_test', 'org_test', 'test', 'Test', '', 'operator', ?1, ?1)",
        )
        .bind(&now)
        .execute(&store.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO product_model_snapshots (id, product_id, version, model_json, content_hash, created_by, creation_reason, created_at) VALUES ('pmodel_test', 'prod_test', 1, '{}', 'sha256:model', 'operator', 'test', ?1)",
        )
        .bind(&now)
        .execute(&store.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO repositories (id, provider, external_id, canonical_url, default_branch, registered_commit, created_at, updated_at) VALUES ('repo_test', 'github', 'example/repo', 'https://github.com/example/repo.git', 'main', ?1, ?2, ?2)",
        )
        .bind("a".repeat(40))
        .bind(&now)
        .execute(&store.pool)
        .await
        .unwrap();
        sqlx::query("UPDATE work_items SET product_id = 'prod_test', mutable_repository_id = 'repo_test', product_model_snapshot_id = 'pmodel_test', product_model_snapshot_hash = 'sha256:model' WHERE id = 'witem_repo'")
            .execute(&store.pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO sessions (id, title, cwd, created_at, updated_at) VALUES ('ses_plan', 'Plan', '/workspace', ?1, ?1)",
        )
        .bind(&now)
        .execute(&store.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO work_plans (id, work_item_id, session_id, status, title, summary, risk_level, requires_approval, work_plan_json, created_at, revision) VALUES ('wplan_test', 'witem_repo', 'ses_plan', 'approved', 'Plan', 'Plan', 'medium', 1, '{}', ?1, 1)",
        )
        .bind(&now)
        .execute(&store.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO workspaces (id, work_item_id, status, source_repo, source_ref, retention_status, created_at, updated_at, status_changed_at) VALUES ('ws_test', 'witem_repo', 'declared', 'https://github.com/example/repo.git', 'main', 'retained', ?1, ?1, ?1)",
        )
        .bind(&now)
        .execute(&store.pool)
        .await
        .unwrap();
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

    #[tokio::test]
    async fn stage_chain_authorization_is_unique_active_and_revocable() {
        let store = store_with_repo_work_item().await;
        insert_stage_chain_scope(&store).await;
        let create = CreateStageChainAuthorization {
            id: "chain_test".into(),
            work_item_id: "witem_repo".into(),
            work_plan_id: "wplan_test".into(),
            work_plan_revision: 1,
            product_model_snapshot_id: "pmodel_test".into(),
            product_model_snapshot_hash: "sha256:model".into(),
            repository_id: "repo_test".into(),
            source_commit: "a".repeat(40),
            workspace_id: "ws_test".into(),
            writable_paths: json!(["src/**"]),
            profile_chain: json!(["repo-builder", "repo-tester", "repo-verifier"]),
            budget_chain: json!({"repo-builder":{"initial_turns":48}}),
            state_hash: "sha256:state".into(),
            created_by: "operator".into(),
            creation_reason: "approve bounded chain".into(),
            expires_at: "9999999999999".into(),
        };
        let chain = store
            .create_stage_chain_authorization(create.clone())
            .await
            .unwrap();
        assert_eq!(chain.status, "active");
        assert!(store
            .create_stage_chain_authorization(CreateStageChainAuthorization {
                id: "chain_duplicate".into(),
                ..create
            })
            .await
            .is_err());

        let revoked = store
            .revoke_stage_chain_authorization(&chain.id, "terminal stage")
            .await
            .unwrap();
        assert_eq!(revoked.status, "revoked");
        assert!(store
            .active_stage_chain_authorization("witem_repo")
            .await
            .unwrap()
            .is_none());
    }
}

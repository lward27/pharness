use super::{now_string, SqliteStore, StoreError};
use crate::{
    CreateInferenceEvaluation, CreateInferenceEvaluationGrantIssuance,
    CreateInferencePolicyQualification, CreateInferenceTargetVerification,
    CreateModelGrantIssuance, CreateStageInferenceSelection, StoredInferenceEvaluation,
    StoredInferencePolicyQualification, StoredInferenceTargetVerification,
    StoredStageInferenceSelection,
};
use sqlx::Row;

impl SqliteStore {
    pub async fn create_inference_evaluation(
        &self,
        evaluation: CreateInferenceEvaluation,
    ) -> Result<StoredInferenceEvaluation, StoreError> {
        evaluation
            .resolved_binding
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        let now = now_string();
        let binding = &evaluation.resolved_binding;
        let result = sqlx::query(
            r#"
            INSERT INTO inference_evaluations (
              id, status, suite_id, suite_hash, attempts, agent_profile_id,
              agent_profile_hash, target_id, target_revision, target_hash, policy_id,
              policy_revision, policy_hash, resolved_binding_json, binding_hash,
              runtime_revision, actor, reason, config_hash, created_at
            )
            SELECT ?1, 'queued', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                   ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19
            WHERE NOT EXISTS (
              SELECT 1 FROM inference_evaluations
              WHERE status IN ('queued', 'running')
            )
            "#,
        )
        .bind(&evaluation.id)
        .bind(&evaluation.suite_id)
        .bind(&evaluation.suite_hash)
        .bind(i64::from(evaluation.attempts))
        .bind(&evaluation.agent_profile_id)
        .bind(&evaluation.agent_profile_hash)
        .bind(&binding.target.target_id)
        .bind(&binding.target.revision)
        .bind(&binding.target.config_hash)
        .bind(&binding.policy.policy_id)
        .bind(&binding.policy.revision)
        .bind(&binding.policy.policy_hash)
        .bind(serde_json::to_string(binding)?)
        .bind(&binding.binding_hash)
        .bind(&evaluation.runtime_revision)
        .bind(&evaluation.actor)
        .bind(&evaluation.reason)
        .bind(&evaluation.config_hash)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            let active = self
                .list_active_inference_evaluations()
                .await?
                .into_iter()
                .next()
                .map(|evaluation| format!(" ({})", evaluation.id))
                .unwrap_or_default();
            return Err(StoreError::Conflict(format!(
                "another inference qualification is already queued or running{active}"
            )));
        }
        self.get_inference_evaluation(&evaluation.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "inference_evaluation".into(),
                id: evaluation.id,
            })
    }

    pub async fn get_inference_evaluation(
        &self,
        id: &str,
    ) -> Result<Option<StoredInferenceEvaluation>, StoreError> {
        let row = sqlx::query("SELECT * FROM inference_evaluations WHERE id = ?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_evaluation).transpose()
    }

    pub async fn list_active_inference_evaluations(
        &self,
    ) -> Result<Vec<StoredInferenceEvaluation>, StoreError> {
        let rows = sqlx::query(
            "SELECT * FROM inference_evaluations WHERE status IN ('queued', 'running') ORDER BY created_at, id",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_evaluation).collect()
    }

    pub async fn list_inference_evaluations(
        &self,
        policy_id: &str,
        policy_revision: &str,
    ) -> Result<Vec<StoredInferenceEvaluation>, StoreError> {
        let rows = sqlx::query(
            "SELECT * FROM inference_evaluations WHERE policy_id = ?1 AND policy_revision = ?2 ORDER BY created_at DESC, id DESC",
        )
        .bind(policy_id)
        .bind(policy_revision)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_evaluation).collect()
    }

    pub async fn mark_inference_evaluation_running(
        &self,
        id: &str,
        job_name: &str,
    ) -> Result<StoredInferenceEvaluation, StoreError> {
        let now = now_string();
        let result = sqlx::query(
            "UPDATE inference_evaluations SET status = 'running', job_name = ?2, started_at = ?3 WHERE id = ?1 AND status = 'queued'",
        )
        .bind(id)
        .bind(job_name)
        .bind(now)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "inference evaluation is not queued".into(),
            ));
        }
        self.get_inference_evaluation(id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "inference_evaluation".into(),
                id: id.into(),
            })
    }

    pub async fn complete_inference_evaluation(
        &self,
        id: &str,
        report: &serde_json::Value,
        report_hash: &str,
        qualification: CreateInferencePolicyQualification,
    ) -> Result<StoredInferenceEvaluation, StoreError> {
        let now = now_string();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO inference_policy_qualifications (
              id, policy_id, policy_revision, policy_hash, target_id, target_revision,
              target_hash, agent_profile_id, agent_profile_hash, suite_id, suite_hash,
              runtime_revision, attempts, metrics_json, verdict, evidence_artifact_id,
              actor, reason, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
            "#,
        )
        .bind(&qualification.id)
        .bind(&qualification.policy_id)
        .bind(&qualification.policy_revision)
        .bind(&qualification.policy_hash)
        .bind(&qualification.target_id)
        .bind(&qualification.target_revision)
        .bind(&qualification.target_hash)
        .bind(&qualification.agent_profile_id)
        .bind(&qualification.agent_profile_hash)
        .bind(&qualification.suite_id)
        .bind(&qualification.suite_hash)
        .bind(&qualification.runtime_revision)
        .bind(i64::from(qualification.attempts))
        .bind(serde_json::to_string(&qualification.metrics)?)
        .bind(&qualification.verdict)
        .bind(&qualification.evidence_artifact_id)
        .bind(&qualification.actor)
        .bind(&qualification.reason)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        let result = sqlx::query(
            "UPDATE inference_evaluations SET status = 'completed', report_json = ?2, report_hash = ?3, qualification_id = ?4, finished_at = ?5 WHERE id = ?1 AND status = 'running'",
        )
        .bind(id)
        .bind(serde_json::to_string(report)?)
        .bind(report_hash)
        .bind(&qualification.id)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "inference evaluation is not running".into(),
            ));
        }
        tx.commit().await?;
        self.get_inference_evaluation(id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "inference_evaluation".into(),
                id: id.into(),
            })
    }

    pub async fn fail_inference_evaluation(
        &self,
        id: &str,
        failure: &str,
    ) -> Result<StoredInferenceEvaluation, StoreError> {
        let now = now_string();
        let result = sqlx::query(
            "UPDATE inference_evaluations SET status = 'failed', failure = ?2, finished_at = ?3 WHERE id = ?1 AND status IN ('queued', 'running')",
        )
        .bind(id)
        .bind(failure)
        .bind(now)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "inference evaluation is already terminal".into(),
            ));
        }
        self.get_inference_evaluation(id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "inference_evaluation".into(),
                id: id.into(),
            })
    }

    pub async fn create_inference_evaluation_grant_issuance(
        &self,
        issuance: CreateInferenceEvaluationGrantIssuance,
    ) -> Result<(), StoreError> {
        let result = sqlx::query(
            r#"
            INSERT INTO inference_evaluation_grant_issuances (
              evaluation_id, fixture_run_id, request_sequence, request_body_hash,
              nonce, issued_at_epoch_seconds, expires_at_epoch_seconds
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
        )
        .bind(&issuance.evaluation_id)
        .bind(&issuance.fixture_run_id)
        .bind(i64::from(issuance.request_sequence))
        .bind(&issuance.request_body_hash)
        .bind(&issuance.nonce)
        .bind(
            i64::try_from(issuance.issued_at_epoch_seconds).map_err(|_| {
                StoreError::Conflict(
                    "evaluation model-grant issued time exceeds SQLite range".into(),
                )
            })?,
        )
        .bind(
            i64::try_from(issuance.expires_at_epoch_seconds).map_err(|_| {
                StoreError::Conflict("evaluation model-grant expiry exceeds SQLite range".into())
            })?,
        )
        .execute(&self.pool)
        .await;
        match result {
            Ok(_) => Ok(()),
            Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
                Err(StoreError::Conflict(
                    "a model grant was already issued for this evaluation fixture sequence".into(),
                ))
            }
            Err(error) => Err(error.into()),
        }
    }

    pub async fn create_model_grant_issuance(
        &self,
        issuance: CreateModelGrantIssuance,
    ) -> Result<(), StoreError> {
        let result = sqlx::query(
            r#"
            INSERT INTO model_grant_issuances (
              run_id, request_sequence, selection_id, request_body_hash, nonce,
              issued_at_epoch_seconds, expires_at_epoch_seconds
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
        )
        .bind(&issuance.run_id)
        .bind(i64::from(issuance.request_sequence))
        .bind(&issuance.selection_id)
        .bind(&issuance.request_body_hash)
        .bind(&issuance.nonce)
        .bind(
            i64::try_from(issuance.issued_at_epoch_seconds).map_err(|_| {
                StoreError::Conflict("model-grant issued time exceeds SQLite range".into())
            })?,
        )
        .bind(
            i64::try_from(issuance.expires_at_epoch_seconds).map_err(|_| {
                StoreError::Conflict("model-grant expiry exceeds SQLite range".into())
            })?,
        )
        .execute(&self.pool)
        .await;
        match result {
            Ok(_) => Ok(()),
            Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
                Err(StoreError::Conflict(
                    "a model grant was already issued for this Run sequence".into(),
                ))
            }
            Err(error) => Err(error.into()),
        }
    }

    pub async fn create_stage_inference_selection(
        &self,
        selection: CreateStageInferenceSelection,
    ) -> Result<StoredStageInferenceSelection, StoreError> {
        selection
            .resolved_binding
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        let now = now_string();
        sqlx::query(
            r#"
            INSERT INTO stage_inference_selections (
              id, subject_kind, subject_id, stage_key, target_id, target_revision, target_hash,
              policy_id, policy_revision, policy_hash, effective_settings_json,
              resolved_binding_json, binding_hash, actor, reason, state_hash,
              supersedes_selection_id, stage_execution_id, run_id, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)
            "#,
        )
        .bind(&selection.id)
        .bind(&selection.subject_kind)
        .bind(&selection.subject_id)
        .bind(&selection.stage_key)
        .bind(&selection.resolved_binding.target.target_id)
        .bind(&selection.resolved_binding.target.revision)
        .bind(&selection.resolved_binding.target.config_hash)
        .bind(&selection.resolved_binding.policy.policy_id)
        .bind(&selection.resolved_binding.policy.revision)
        .bind(&selection.resolved_binding.policy.policy_hash)
        .bind(serde_json::to_string(&selection.effective_settings)?)
        .bind(serde_json::to_string(&selection.resolved_binding)?)
        .bind(&selection.resolved_binding.binding_hash)
        .bind(&selection.actor)
        .bind(&selection.reason)
        .bind(&selection.state_hash)
        .bind(&selection.supersedes_selection_id)
        .bind(&selection.stage_execution_id)
        .bind(&selection.run_id)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        self.get_stage_inference_selection(&selection.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "stage_inference_selection".into(),
                id: selection.id,
            })
    }

    pub async fn get_stage_inference_selection(
        &self,
        id: &str,
    ) -> Result<Option<StoredStageInferenceSelection>, StoreError> {
        let row = sqlx::query("SELECT * FROM stage_inference_selections WHERE id = ?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_selection).transpose()
    }

    pub async fn get_stage_inference_selection_for_run(
        &self,
        run_id: &str,
    ) -> Result<Option<StoredStageInferenceSelection>, StoreError> {
        let row = sqlx::query("SELECT * FROM stage_inference_selections WHERE run_id = ?1")
            .bind(run_id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_selection).transpose()
    }

    pub async fn list_stage_inference_selections(
        &self,
        subject_kind: &str,
        subject_id: &str,
    ) -> Result<Vec<StoredStageInferenceSelection>, StoreError> {
        let rows = sqlx::query(
            "SELECT * FROM stage_inference_selections WHERE subject_kind = ?1 AND subject_id = ?2 ORDER BY created_at, id",
        )
        .bind(subject_kind)
        .bind(subject_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_selection).collect()
    }

    pub async fn create_inference_target_verification(
        &self,
        verification: CreateInferenceTargetVerification,
    ) -> Result<StoredInferenceTargetVerification, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            INSERT INTO inference_target_verifications (
              id, target_id, target_revision, target_hash, status, reachability,
              model_visible, streaming_compatible, tool_compatible, observed_capabilities_json,
              sanitized_failure, actor, reason, config_hash, created_at, expires_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
            "#,
        )
        .bind(&verification.id)
        .bind(&verification.target_id)
        .bind(&verification.target_revision)
        .bind(&verification.target_hash)
        .bind(&verification.status)
        .bind(&verification.reachability)
        .bind(verification.model_visible)
        .bind(verification.streaming_compatible)
        .bind(verification.tool_compatible)
        .bind(serde_json::to_string(&verification.observed_capabilities)?)
        .bind(&verification.sanitized_failure)
        .bind(&verification.actor)
        .bind(&verification.reason)
        .bind(&verification.config_hash)
        .bind(&now)
        .bind(&verification.expires_at)
        .execute(&self.pool)
        .await?;
        self.get_inference_target_verification(&verification.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "inference_target_verification".into(),
                id: verification.id,
            })
    }

    pub async fn get_inference_target_verification(
        &self,
        id: &str,
    ) -> Result<Option<StoredInferenceTargetVerification>, StoreError> {
        let row = sqlx::query("SELECT * FROM inference_target_verifications WHERE id = ?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_verification).transpose()
    }

    pub async fn list_inference_target_verifications(
        &self,
        target_id: &str,
        revision: &str,
    ) -> Result<Vec<StoredInferenceTargetVerification>, StoreError> {
        let rows = sqlx::query(
            "SELECT * FROM inference_target_verifications WHERE target_id = ?1 AND target_revision = ?2 ORDER BY created_at DESC, id DESC",
        )
        .bind(target_id)
        .bind(revision)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_verification).collect()
    }

    pub async fn create_inference_policy_qualification(
        &self,
        qualification: CreateInferencePolicyQualification,
    ) -> Result<StoredInferencePolicyQualification, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            INSERT INTO inference_policy_qualifications (
              id, policy_id, policy_revision, policy_hash, target_id, target_revision,
              target_hash, agent_profile_id, agent_profile_hash, suite_id, suite_hash,
              runtime_revision, attempts, metrics_json, verdict, evidence_artifact_id,
              actor, reason, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
            "#,
        )
        .bind(&qualification.id)
        .bind(&qualification.policy_id)
        .bind(&qualification.policy_revision)
        .bind(&qualification.policy_hash)
        .bind(&qualification.target_id)
        .bind(&qualification.target_revision)
        .bind(&qualification.target_hash)
        .bind(&qualification.agent_profile_id)
        .bind(&qualification.agent_profile_hash)
        .bind(&qualification.suite_id)
        .bind(&qualification.suite_hash)
        .bind(&qualification.runtime_revision)
        .bind(i64::from(qualification.attempts))
        .bind(serde_json::to_string(&qualification.metrics)?)
        .bind(&qualification.verdict)
        .bind(&qualification.evidence_artifact_id)
        .bind(&qualification.actor)
        .bind(&qualification.reason)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        self.get_inference_policy_qualification(&qualification.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "inference_policy_qualification".into(),
                id: qualification.id,
            })
    }

    pub async fn get_inference_policy_qualification(
        &self,
        id: &str,
    ) -> Result<Option<StoredInferencePolicyQualification>, StoreError> {
        let row = sqlx::query("SELECT * FROM inference_policy_qualifications WHERE id = ?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_qualification).transpose()
    }

    pub async fn list_inference_policy_qualifications(
        &self,
        policy_id: &str,
        revision: &str,
    ) -> Result<Vec<StoredInferencePolicyQualification>, StoreError> {
        let rows = sqlx::query(
            "SELECT * FROM inference_policy_qualifications WHERE policy_id = ?1 AND policy_revision = ?2 ORDER BY created_at DESC, id DESC",
        )
        .bind(policy_id)
        .bind(revision)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_qualification).collect()
    }
}

fn row_to_selection(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredStageInferenceSelection, StoreError> {
    Ok(StoredStageInferenceSelection {
        id: row.try_get("id")?,
        subject_kind: row.try_get("subject_kind")?,
        subject_id: row.try_get("subject_id")?,
        stage_key: row.try_get("stage_key")?,
        target_id: row.try_get("target_id")?,
        target_revision: row.try_get("target_revision")?,
        target_hash: row.try_get("target_hash")?,
        policy_id: row.try_get("policy_id")?,
        policy_revision: row.try_get("policy_revision")?,
        policy_hash: row.try_get("policy_hash")?,
        effective_settings: serde_json::from_str(
            &row.try_get::<String, _>("effective_settings_json")?,
        )?,
        resolved_binding: serde_json::from_str(
            &row.try_get::<String, _>("resolved_binding_json")?,
        )?,
        binding_hash: row.try_get("binding_hash")?,
        actor: row.try_get("actor")?,
        reason: row.try_get("reason")?,
        state_hash: row.try_get("state_hash")?,
        supersedes_selection_id: row.try_get("supersedes_selection_id")?,
        stage_execution_id: row.try_get("stage_execution_id")?,
        run_id: row.try_get("run_id")?,
        created_at: row.try_get("created_at")?,
    })
}

fn row_to_verification(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredInferenceTargetVerification, StoreError> {
    Ok(StoredInferenceTargetVerification {
        id: row.try_get("id")?,
        target_id: row.try_get("target_id")?,
        target_revision: row.try_get("target_revision")?,
        target_hash: row.try_get("target_hash")?,
        status: row.try_get("status")?,
        reachability: row.try_get("reachability")?,
        model_visible: row.try_get::<i64, _>("model_visible")? != 0,
        streaming_compatible: row.try_get::<i64, _>("streaming_compatible")? != 0,
        tool_compatible: row.try_get::<i64, _>("tool_compatible")? != 0,
        observed_capabilities: serde_json::from_str(
            &row.try_get::<String, _>("observed_capabilities_json")?,
        )?,
        sanitized_failure: row.try_get("sanitized_failure")?,
        actor: row.try_get("actor")?,
        reason: row.try_get("reason")?,
        config_hash: row.try_get("config_hash")?,
        created_at: row.try_get("created_at")?,
        expires_at: row.try_get("expires_at")?,
    })
}

fn row_to_qualification(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredInferencePolicyQualification, StoreError> {
    Ok(StoredInferencePolicyQualification {
        id: row.try_get("id")?,
        policy_id: row.try_get("policy_id")?,
        policy_revision: row.try_get("policy_revision")?,
        policy_hash: row.try_get("policy_hash")?,
        target_id: row.try_get("target_id")?,
        target_revision: row.try_get("target_revision")?,
        target_hash: row.try_get("target_hash")?,
        agent_profile_id: row.try_get("agent_profile_id")?,
        agent_profile_hash: row.try_get("agent_profile_hash")?,
        suite_id: row.try_get("suite_id")?,
        suite_hash: row.try_get("suite_hash")?,
        runtime_revision: row.try_get("runtime_revision")?,
        attempts: row.try_get::<i64, _>("attempts")? as u32,
        metrics: serde_json::from_str(&row.try_get::<String, _>("metrics_json")?)?,
        verdict: row.try_get("verdict")?,
        evidence_artifact_id: row.try_get("evidence_artifact_id")?,
        actor: row.try_get("actor")?,
        reason: row.try_get("reason")?,
        created_at: row.try_get("created_at")?,
    })
}

fn row_to_evaluation(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredInferenceEvaluation, StoreError> {
    Ok(StoredInferenceEvaluation {
        id: row.try_get("id")?,
        status: row.try_get("status")?,
        suite_id: row.try_get("suite_id")?,
        suite_hash: row.try_get("suite_hash")?,
        attempts: row.try_get::<i64, _>("attempts")? as u32,
        agent_profile_id: row.try_get("agent_profile_id")?,
        agent_profile_hash: row.try_get("agent_profile_hash")?,
        target_id: row.try_get("target_id")?,
        target_revision: row.try_get("target_revision")?,
        target_hash: row.try_get("target_hash")?,
        policy_id: row.try_get("policy_id")?,
        policy_revision: row.try_get("policy_revision")?,
        policy_hash: row.try_get("policy_hash")?,
        resolved_binding: serde_json::from_str(
            &row.try_get::<String, _>("resolved_binding_json")?,
        )?,
        binding_hash: row.try_get("binding_hash")?,
        runtime_revision: row.try_get("runtime_revision")?,
        actor: row.try_get("actor")?,
        reason: row.try_get("reason")?,
        config_hash: row.try_get("config_hash")?,
        job_name: row.try_get("job_name")?,
        report: row
            .try_get::<Option<String>, _>("report_json")?
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
        report_hash: row.try_get("report_hash")?,
        failure: row.try_get("failure")?,
        qualification_id: row.try_get("qualification_id")?,
        created_at: row.try_get("created_at")?,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CreateRun, CreateSession};
    use pharness_core::{
        InferenceBackendKind, InferenceCapabilities, InferenceStage, InferenceTargetRef,
        InferenceTargetRevision, InferenceTransportPolicy, ReasoningContextMode, ReasoningEffort,
        ReasoningRequestPolicy, ResolvedInferenceBinding, RunId, SessionId,
        StageInferencePolicyRevision, ToolProtocolMode, INFERENCE_POLICY_SCHEMA,
        INFERENCE_TARGET_SCHEMA, RESOLVED_INFERENCE_BINDING_SCHEMA,
    };

    fn evaluation(id: &str) -> CreateInferenceEvaluation {
        let mut target = InferenceTargetRevision {
            schema_version: INFERENCE_TARGET_SCHEMA.into(),
            target_id: "target".into(),
            revision: "v1".into(),
            display_name: "Target".into(),
            backend_kind: InferenceBackendKind::Fireworks,
            protocol: "openai_chat_completions_v1".into(),
            upstream_base_url: "https://api.fireworks.ai/inference/v1".into(),
            upstream_model: "accounts/example/models/model".into(),
            authentication_binding: Some("credential-binding".into()),
            transport: InferenceTransportPolicy::default(),
            capabilities: InferenceCapabilities {
                native_tools: true,
                streaming: true,
                json_schema: true,
                stream_options: true,
                reasoning_efforts: vec![ReasoningEffort::Medium],
                reasoning_context_modes: vec![ReasoningContextMode::CurrentTurn],
                tool_choice_modes: vec![
                    pharness_core::ToolChoiceMode::Auto,
                    pharness_core::ToolChoiceMode::Required,
                ],
            },
            context_limit_tokens: 32_768,
            output_limit_tokens: 8_192,
            allowed_stages: vec![InferenceStage::Plan],
            selectable: true,
            openrouter: None,
            config_hash: String::new(),
        };
        target.config_hash = target.computed_hash().unwrap();
        let mut policy = StageInferencePolicyRevision {
            schema_version: INFERENCE_POLICY_SCHEMA.into(),
            policy_id: "planner-policy".into(),
            revision: "v1".into(),
            display_name: "Planner policy".into(),
            eligible_profiles: vec!["repo-planner".into()],
            eligible_stages: vec![InferenceStage::Plan],
            target: InferenceTargetRef {
                target_id: target.target_id.clone(),
                revision: target.revision.clone(),
            },
            target_hash: target.config_hash.clone(),
            reasoning: ReasoningRequestPolicy {
                effort: Some(ReasoningEffort::Medium),
                context_mode: ReasoningContextMode::CurrentTurn,
                expose_replay: true,
            },
            temperature_milli: Some(100),
            max_output_tokens: 8_192,
            max_input_tokens: 16_000,
            tool_protocol: ToolProtocolMode::NativeTools,
            tool_choice: pharness_core::ToolChoiceMode::Required,
            transport_max_attempts: 3,
            selectable: true,
            policy_hash: String::new(),
        };
        policy.policy_hash = policy.computed_hash().unwrap();
        let mut binding = ResolvedInferenceBinding {
            schema_version: RESOLVED_INFERENCE_BINDING_SCHEMA.into(),
            target,
            policy,
            prompt_version: "prompt-v1".into(),
            stage_prompt: None,
            base_agent_profile_hash: format!("sha256:{}", "a".repeat(64)),
            agent_profile_hash: String::new(),
            tool_schema_hash: "tool-schema-hash".into(),
            context_policy_hash: String::new(),
            protocol_calibration_hash: String::new(),
            profile_budget_hash: "budget-hash".into(),
            binding_hash: String::new(),
        };
        binding.agent_profile_hash = binding.computed_agent_profile_hash().unwrap();
        binding.binding_hash = binding.computed_hash().unwrap();
        CreateInferenceEvaluation {
            id: id.into(),
            suite_id: "planner-v1".into(),
            suite_hash: "suite-hash".into(),
            attempts: 2,
            agent_profile_id: "repo-planner".into(),
            agent_profile_hash: binding.agent_profile_hash.clone(),
            resolved_binding: binding,
            runtime_revision: "runtime-sha".into(),
            actor: "operator".into(),
            reason: "test qualification single flight".into(),
            config_hash: "registry-hash".into(),
        }
    }

    #[tokio::test]
    async fn inference_evaluations_are_globally_single_flight() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let first = store
            .create_inference_evaluation(evaluation("infeval_first"))
            .await
            .unwrap();
        assert_eq!(first.status, "queued");
        assert!(matches!(
            store
                .create_inference_evaluation(evaluation("infeval_duplicate"))
                .await,
            Err(StoreError::Conflict(message))
                if message.contains("already queued or running (infeval_first)")
        ));
        store
            .fail_inference_evaluation(&first.id, "test terminal state")
            .await
            .unwrap();
        let next = store
            .create_inference_evaluation(evaluation("infeval_next"))
            .await
            .unwrap();
        assert_eq!(next.status, "queued");
    }

    #[tokio::test]
    async fn inference_provenance_rows_are_append_only() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let now = "2026-08-29T12:00:00Z";
        let session_id = SessionId::new("ses_inference_immutable");
        let run_id = RunId::new("run_inference_immutable");
        store
            .create_session(CreateSession {
                id: session_id.clone(),
                title: "inference immutability".into(),
                cwd: ".".into(),
            })
            .await
            .unwrap();
        store
            .create_run(CreateRun {
                id: run_id.clone(),
                session_id,
                user_task: "test model grant durability".into(),
                cwd: ".".into(),
                max_turns: 2,
                initial_status: "queued".into(),
                execution_target_json: serde_json::json!({"kind":"local_process"}),
            })
            .await
            .unwrap();

        sqlx::query(
            r#"INSERT INTO stage_inference_selections (
              id, subject_kind, subject_id, stage_key, target_id, target_revision, target_hash,
              policy_id, policy_revision, policy_hash, effective_settings_json,
              resolved_binding_json, binding_hash, actor, reason, state_hash, run_id, created_at
            ) VALUES (
              'infsel_test', 'work_item', 'witem_test', 'plan', 'target', 'v1', 'target-hash',
              'policy', 'v1', 'policy-hash', '{}', '{}', 'binding-hash',
              'operator', 'test immutable selection', 'state-hash', ?1, ?2
            )"#,
        )
        .bind(run_id.as_str())
        .bind(now)
        .execute(&store.pool)
        .await
        .unwrap();

        let issuance = CreateModelGrantIssuance {
            run_id: run_id.as_str().into(),
            request_sequence: 1,
            selection_id: "infsel_test".into(),
            request_body_hash: "a".repeat(64),
            nonce: "nonce-one".into(),
            issued_at_epoch_seconds: 1,
            expires_at_epoch_seconds: 61,
        };
        store
            .create_model_grant_issuance(issuance.clone())
            .await
            .unwrap();
        assert!(matches!(
            store.create_model_grant_issuance(issuance).await,
            Err(StoreError::Conflict(_))
        ));
        sqlx::query(
            r#"INSERT INTO inference_target_verifications (
              id, target_id, target_revision, target_hash, status, reachability,
              model_visible, streaming_compatible, tool_compatible, observed_capabilities_json,
              actor, reason, config_hash, created_at, expires_at
            ) VALUES (
              'infverify_test', 'target', 'v1', 'target-hash', 'passed', 'reachable',
              1, 1, 1, '{}', 'operator', 'test immutable verification', 'registry-hash', ?1, ?2
            )"#,
        )
        .bind(now)
        .bind("2026-08-29T12:15:00Z")
        .execute(&store.pool)
        .await
        .unwrap();
        sqlx::query(
            r#"INSERT INTO inference_policy_qualifications (
              id, policy_id, policy_revision, policy_hash, target_id, target_revision,
              target_hash, agent_profile_id, agent_profile_hash, suite_id, suite_hash,
              runtime_revision, attempts, metrics_json, verdict, actor, reason, created_at
            ) VALUES (
              'infqual_test', 'policy', 'v1', 'policy-hash', 'target', 'v1',
              'target-hash', 'repo-planner', 'profile-hash', 'planner-v1', 'suite-hash',
              'runtime-sha', 2, '{}', 'passed', 'operator', 'test immutable qualification', ?1
            )"#,
        )
        .bind(now)
        .execute(&store.pool)
        .await
        .unwrap();
        sqlx::query(
            r#"INSERT INTO inference_evaluations (
              id, status, suite_id, suite_hash, attempts, agent_profile_id,
              agent_profile_hash, target_id, target_revision, target_hash, policy_id,
              policy_revision, policy_hash, resolved_binding_json, binding_hash,
              runtime_revision, actor, reason, config_hash, created_at
            ) VALUES (
              'infeval_test', 'running', 'planner-v1', 'suite-hash', 2,
              'repo-planner', 'profile-hash', 'target', 'v1', 'target-hash',
              'policy', 'v1', 'policy-hash', '{}', 'binding-hash', 'runtime-sha',
              'operator', 'test immutable evaluation grants', 'registry-hash', ?1
            )"#,
        )
        .bind(now)
        .execute(&store.pool)
        .await
        .unwrap();
        let evaluation_issuance = CreateInferenceEvaluationGrantIssuance {
            evaluation_id: "infeval_test".into(),
            fixture_run_id: "fixture-run-one".into(),
            request_sequence: 1,
            request_body_hash: "b".repeat(64),
            nonce: "evaluation-nonce-one".into(),
            issued_at_epoch_seconds: 1,
            expires_at_epoch_seconds: 61,
        };
        store
            .create_inference_evaluation_grant_issuance(evaluation_issuance.clone())
            .await
            .unwrap();
        assert!(matches!(
            store
                .create_inference_evaluation_grant_issuance(evaluation_issuance)
                .await,
            Err(StoreError::Conflict(_))
        ));

        for statement in [
            "UPDATE stage_inference_selections SET reason = 'changed' WHERE id = 'infsel_test'",
            "DELETE FROM stage_inference_selections WHERE id = 'infsel_test'",
            "UPDATE inference_target_verifications SET status = 'failed' WHERE id = 'infverify_test'",
            "DELETE FROM inference_target_verifications WHERE id = 'infverify_test'",
            "UPDATE inference_policy_qualifications SET verdict = 'failed' WHERE id = 'infqual_test'",
            "DELETE FROM inference_policy_qualifications WHERE id = 'infqual_test'",
            "UPDATE model_grant_issuances SET request_body_hash = 'changed' WHERE run_id = 'run_inference_immutable'",
            "DELETE FROM model_grant_issuances WHERE run_id = 'run_inference_immutable'",
            "UPDATE inference_evaluation_grant_issuances SET request_body_hash = 'changed' WHERE evaluation_id = 'infeval_test'",
            "DELETE FROM inference_evaluation_grant_issuances WHERE evaluation_id = 'infeval_test'",
        ] {
            assert!(
                sqlx::query(statement).execute(&store.pool).await.is_err(),
                "append-only trigger accepted {statement}"
            );
        }
    }
}

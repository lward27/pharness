use crate::{
    ApprovalBooleanCountBucket, ApprovalCountBucket, ApprovalGateCountBucket,
    ApprovalGateListFilter, ApprovalGateSummary, ApprovalGateSummaryFilter, ApprovalListFilter,
    ApprovalSummary, ApprovalSummaryFilter, AuditEventListFilter, BooleanCountBucket,
    ChangeSetListFilter, CountBucket, CreateApproval, CreateApprovalGate, CreateArtifact,
    CreateAuditEvent, CreateChangeSet, CreateDeploymentContract, CreateDeploymentIntent,
    CreateFileChange, CreateIncident, CreateObservation, CreatePermissionGrant,
    CreatePipelineContract, CreatePipelineIntent, CreateRegistryEvidence, CreateRelease,
    CreateRemediationPlan, CreateRun, CreateSession, CreateWorkItem, CreateWorkPlan,
    CreateWorkspace, DeploymentContractListFilter, DeploymentIntentListFilter, IncidentListFilter,
    ObservationListFilter, PipelineContractListFilter, PipelineIntentListFilter,
    RegistryEvidenceListFilter, ReleaseListFilter, RemediationPlanListFilter,
    ReplacePipelineContract, RunListFilter, RunSummary, RunSummaryFilter, StoredApproval,
    StoredApprovalGate, StoredArtifact, StoredAuditEvent, StoredChangeSet,
    StoredDeploymentContract, StoredDeploymentIntent, StoredFileChange, StoredIncident,
    StoredObservation, StoredPermissionGrant, StoredPipelineContract, StoredPipelineIntent,
    StoredRegistryEvidence, StoredRelease, StoredRemediationPlan, StoredRun, StoredWorkItem,
    StoredWorkPlan, StoredWorkspace, UpdateChangeSetRevision, UpdateDeploymentIntentDraft,
    UpdateDeploymentIntentEvidence, UpdatePipelineIntentDraft, UpdatePipelineIntentEvidence,
    UpdatePipelineIntentExecution, UpdateRegistryEvidenceDraft, UpdateReleaseDraft,
    UpdateReleaseEvidence, UpdateWorkPlanRevision, WorkItemListFilter, WorkPlanListFilter,
    WorkspaceListFilter,
};
use pharness_core::{AgentEvent, EventId, RunId, SessionId};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::path::Path;
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct SqliteStore {
    pool: SqlitePool,
}

impl SqliteStore {
    pub async fn connect(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let options = SqliteConnectOptions::new()
            .filename(path.as_ref())
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal);
        Self::connect_with_options(options).await
    }

    pub async fn connect_in_memory() -> Result<Self, StoreError> {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")?;
        Self::connect_with_options(options).await
    }

    async fn connect_with_options(options: SqliteConnectOptions) -> Result<Self, StoreError> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn create_session(&self, session: CreateSession) -> Result<(), StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            INSERT INTO sessions (id, title, cwd, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?4)
            ON CONFLICT(id) DO NOTHING
            "#,
        )
        .bind(session.id.as_str())
        .bind(session.title)
        .bind(session.cwd)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn create_run(&self, run: CreateRun) -> Result<StoredRun, StoreError> {
        let now = now_string();
        let execution_target_json = serde_json::to_string(&run.execution_target_json)?;
        sqlx::query(
            r#"
            INSERT INTO runs (
              id, session_id, status, user_task, max_turns, started_at, execution_target_json
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
        )
        .bind(run.id.as_str())
        .bind(run.session_id.as_str())
        .bind(&run.initial_status)
        .bind(&run.user_task)
        .bind(i64::from(run.max_turns))
        .bind(&now)
        .bind(execution_target_json)
        .execute(&self.pool)
        .await?;

        self.get_run(&run.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "run".to_string(),
                id: run.id.to_string(),
            })
    }

    pub async fn get_run(&self, run_id: &RunId) -> Result<Option<StoredRun>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT runs.id, runs.session_id, sessions.cwd, status, user_task, max_turns, started_at,
                   finished_at, cancel_requested_at, error, result_json, execution_target_json
            FROM runs
            JOIN sessions ON sessions.id = runs.session_id
            WHERE runs.id = ?1
            "#,
        )
        .bind(run_id.as_str())
        .fetch_optional(&self.pool)
        .await?;

        row.map(row_to_run).transpose()
    }

    pub async fn list_runs(&self, filter: RunListFilter) -> Result<Vec<StoredRun>, StoreError> {
        let limit = i64::from(filter.limit.clamp(1, 200));
        let offset = i64::from(filter.offset);
        let production_impacting =
            filter
                .production_impacting
                .map(|value| if value { 1_i64 } else { 0_i64 });
        let rows = sqlx::query(
            r#"
            SELECT runs.id, runs.session_id, sessions.cwd, status, user_task, max_turns, started_at,
                   finished_at, cancel_requested_at, error, result_json, execution_target_json
            FROM runs
            JOIN sessions ON sessions.id = runs.session_id
            WHERE (?1 IS NULL OR status = ?1)
              AND (?2 IS NULL OR json_extract(execution_target_json, '$.run_scope.namespace') = ?2)
              AND (?3 IS NULL OR json_extract(execution_target_json, '$.run_scope.repo') = ?3)
              AND (?4 IS NULL OR json_extract(execution_target_json, '$.run_scope.branch') = ?4)
              AND (?5 IS NULL OR json_extract(execution_target_json, '$.run_scope.production_impacting') = ?5)
              AND (?6 IS NULL OR CAST(started_at AS INTEGER) >= ?6)
              AND (?7 IS NULL OR CAST(started_at AS INTEGER) <= ?7)
            ORDER BY started_at DESC, runs.id DESC
            LIMIT ?8 OFFSET ?9
            "#,
        )
        .bind(filter.status)
        .bind(filter.namespace)
        .bind(filter.repo)
        .bind(filter.branch)
        .bind(production_impacting)
        .bind(filter.started_after_ms)
        .bind(filter.started_before_ms)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_run).collect()
    }

    pub async fn run_summary(&self, filter: RunSummaryFilter) -> Result<RunSummary, StoreError> {
        let total = run_summary_total(&self.pool, &filter).await?;
        let by_status = run_summary_text_buckets(&self.pool, &filter, "status").await?;
        let by_age_bucket = run_summary_age_buckets(&self.pool, &filter).await?;
        let by_namespace = run_summary_text_buckets(
            &self.pool,
            &filter,
            "json_extract(execution_target_json, '$.run_scope.namespace')",
        )
        .await?;
        let by_repo = run_summary_text_buckets(
            &self.pool,
            &filter,
            "json_extract(execution_target_json, '$.run_scope.repo')",
        )
        .await?;
        let by_branch = run_summary_text_buckets(
            &self.pool,
            &filter,
            "json_extract(execution_target_json, '$.run_scope.branch')",
        )
        .await?;
        let by_production_impacting = run_summary_bool_buckets(
            &self.pool,
            &filter,
            "json_extract(execution_target_json, '$.run_scope.production_impacting')",
        )
        .await?;

        Ok(RunSummary {
            total,
            by_status,
            by_age_bucket,
            by_namespace,
            by_repo,
            by_branch,
            by_production_impacting,
        })
    }

    pub async fn mark_run_running(&self, run_id: &RunId) -> Result<StoredRun, StoreError> {
        sqlx::query(
            r#"
            UPDATE runs
            SET status = 'running'
            WHERE id = ?1
              AND status NOT IN ('completed', 'failed', 'cancelled')
            "#,
        )
        .bind(run_id.as_str())
        .execute(&self.pool)
        .await?;

        self.get_run(run_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "run".to_string(),
                id: run_id.to_string(),
            })
    }

    pub async fn complete_run(
        &self,
        run_id: &RunId,
        status: &str,
        result_json: serde_json::Value,
        error: Option<String>,
    ) -> Result<StoredRun, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            UPDATE runs
            SET status = ?2,
                finished_at = ?3,
                result_json = ?4,
                error = ?5
            WHERE id = ?1
            "#,
        )
        .bind(run_id.as_str())
        .bind(status)
        .bind(now)
        .bind(serde_json::to_string(&result_json)?)
        .bind(error)
        .execute(&self.pool)
        .await?;

        self.get_run(run_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "run".to_string(),
                id: run_id.to_string(),
            })
    }

    pub async fn mark_run_approval_required(
        &self,
        run_id: &RunId,
        result_json: serde_json::Value,
    ) -> Result<StoredRun, StoreError> {
        sqlx::query(
            r#"
            UPDATE runs
            SET status = 'approval_required',
                result_json = ?2
            WHERE id = ?1
            "#,
        )
        .bind(run_id.as_str())
        .bind(serde_json::to_string(&result_json)?)
        .execute(&self.pool)
        .await?;

        self.get_run(run_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "run".to_string(),
                id: run_id.to_string(),
            })
    }

    pub async fn create_approval(
        &self,
        approval: CreateApproval,
    ) -> Result<StoredApproval, StoreError> {
        let now = now_string();
        let action_json = approval
            .action_json
            .map(|value| serde_json::to_string(&value))
            .transpose()?;
        let preview_json = approval
            .preview_json
            .map(|value| serde_json::to_string(&value))
            .transpose()?;
        let resume_messages_json = approval
            .resume_messages_json
            .map(|value| serde_json::to_string(&value))
            .transpose()?;
        let run_scope_json = approval
            .run_scope_json
            .map(|value| serde_json::to_string(&value))
            .transpose()?;

        sqlx::query(
            r#"
            INSERT INTO approvals (
              id, session_id, run_id, status, kind, summary, risk_level,
              requested_at, run_scope_json, action_json, preview_json,
              resume_messages_json, turns_completed
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            "#,
        )
        .bind(&approval.id)
        .bind(approval.session_id.as_str())
        .bind(approval.run_id.as_str())
        .bind(&approval.status)
        .bind(&approval.kind)
        .bind(&approval.summary)
        .bind(&approval.risk_level)
        .bind(now)
        .bind(run_scope_json)
        .bind(action_json)
        .bind(preview_json)
        .bind(resume_messages_json)
        .bind(i64::from(approval.turns_completed))
        .execute(&self.pool)
        .await?;

        self.get_approval(&approval.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "approval".to_string(),
                id: approval.id,
            })
    }

    pub async fn get_approval(
        &self,
        approval_id: &str,
    ) -> Result<Option<StoredApproval>, StoreError> {
        let sql = approval_select_sql("WHERE id = ?1");
        let row = sqlx::query(&sql)
            .bind(approval_id)
            .fetch_optional(&self.pool)
            .await?;

        row.map(row_to_approval).transpose()
    }

    pub async fn pending_approval_for_run(
        &self,
        run_id: &RunId,
    ) -> Result<Option<StoredApproval>, StoreError> {
        let sql = format!(
            "{} ORDER BY requested_at DESC LIMIT 1",
            approval_select_sql("WHERE run_id = ?1 AND status = 'pending'")
        );
        let row = sqlx::query(&sql)
            .bind(run_id.as_str())
            .fetch_optional(&self.pool)
            .await?;

        row.map(row_to_approval).transpose()
    }

    pub async fn list_approvals(
        &self,
        filter: ApprovalListFilter,
    ) -> Result<Vec<StoredApproval>, StoreError> {
        let limit = i64::from(filter.limit.clamp(1, 200));
        let offset = i64::from(filter.offset);
        let production_impacting =
            filter
                .production_impacting
                .map(|value| if value { 1_i64 } else { 0_i64 });
        let sql = format!(
            r#"
            {}
            ORDER BY requested_at DESC, id DESC
            LIMIT ?8 OFFSET ?9
            "#,
            approval_select_sql(
                r#"
                WHERE (?1 IS NULL OR status = ?1)
                  AND (?2 IS NULL OR json_extract(run_scope_json, '$.namespace') = ?2)
                  AND (?3 IS NULL OR json_extract(run_scope_json, '$.repo') = ?3)
                  AND (?4 IS NULL OR json_extract(run_scope_json, '$.branch') = ?4)
                  AND (?5 IS NULL OR json_extract(run_scope_json, '$.production_impacting') = ?5)
                  AND (?6 IS NULL OR CAST(requested_at AS INTEGER) >= ?6)
                  AND (?7 IS NULL OR CAST(requested_at AS INTEGER) <= ?7)
                "#
            )
        );
        let rows = sqlx::query(&sql)
            .bind(filter.status)
            .bind(filter.namespace)
            .bind(filter.repo)
            .bind(filter.branch)
            .bind(production_impacting)
            .bind(filter.requested_after_ms)
            .bind(filter.requested_before_ms)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;

        rows.into_iter().map(row_to_approval).collect()
    }

    pub async fn approval_summary(
        &self,
        filter: ApprovalSummaryFilter,
    ) -> Result<ApprovalSummary, StoreError> {
        let total = approval_summary_total(&self.pool, &filter).await?;
        let by_status = approval_summary_text_buckets(&self.pool, &filter, "status").await?;
        let by_kind = approval_summary_text_buckets(&self.pool, &filter, "kind").await?;
        let by_risk_level =
            approval_summary_text_buckets(&self.pool, &filter, "risk_level").await?;
        let by_age_bucket = approval_summary_age_buckets(&self.pool, &filter).await?;
        let by_namespace = approval_summary_text_buckets(
            &self.pool,
            &filter,
            "json_extract(run_scope_json, '$.namespace')",
        )
        .await?;
        let by_repo = approval_summary_text_buckets(
            &self.pool,
            &filter,
            "json_extract(run_scope_json, '$.repo')",
        )
        .await?;
        let by_branch = approval_summary_text_buckets(
            &self.pool,
            &filter,
            "json_extract(run_scope_json, '$.branch')",
        )
        .await?;
        let by_production_impacting = approval_summary_bool_buckets(
            &self.pool,
            &filter,
            "json_extract(run_scope_json, '$.production_impacting')",
        )
        .await?;

        Ok(ApprovalSummary {
            total,
            by_status,
            by_kind,
            by_risk_level,
            by_age_bucket,
            by_namespace,
            by_repo,
            by_branch,
            by_production_impacting,
        })
    }

    pub async fn decide_pending_approval(
        &self,
        run_id: &RunId,
        status: &str,
        decided_by: Option<String>,
        decision_reason: Option<String>,
    ) -> Result<StoredApproval, StoreError> {
        let now = now_string();
        let Some(pending) = self.pending_approval_for_run(run_id).await? else {
            return Err(StoreError::NotFound {
                entity: "pending approval".to_string(),
                id: run_id.to_string(),
            });
        };

        sqlx::query(
            r#"
            UPDATE approvals
            SET status = ?2,
                decided_at = ?3,
                decided_by = ?4,
                decision_reason = ?5
            WHERE id = ?1 AND status = 'pending'
            "#,
        )
        .bind(&pending.id)
        .bind(status)
        .bind(now)
        .bind(decided_by)
        .bind(decision_reason)
        .execute(&self.pool)
        .await?;

        self.get_approval(&pending.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "approval".to_string(),
                id: pending.id,
            })
    }

    pub async fn cancel_run(&self, run_id: &RunId) -> Result<StoredRun, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            UPDATE runs
            SET status = 'cancelled',
                cancel_requested_at = ?2,
                finished_at = COALESCE(finished_at, ?2)
            WHERE id = ?1
            "#,
        )
        .bind(run_id.as_str())
        .bind(now)
        .execute(&self.pool)
        .await?;

        self.get_run(run_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "run".to_string(),
                id: run_id.to_string(),
            })
    }

    pub async fn append_event(&self, event: &AgentEvent) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO events (id, session_id, run_id, seq, type, ts, payload_json)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
        )
        .bind(event.event_id.as_str())
        .bind(event.session_id.as_str())
        .bind(event.run_id.as_str())
        .bind(event.seq as i64)
        .bind(event.kind.as_str())
        .bind(now_string())
        .bind(serde_json::to_string(&event.payload)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn create_file_change(
        &self,
        change: CreateFileChange,
    ) -> Result<StoredFileChange, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            INSERT INTO file_changes (
              id, session_id, run_id, path, before_hash, after_hash, diff, created_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
        )
        .bind(&change.id)
        .bind(change.session_id.as_str())
        .bind(change.run_id.as_str())
        .bind(&change.path)
        .bind(change.before_hash)
        .bind(change.after_hash)
        .bind(change.diff)
        .bind(now)
        .execute(&self.pool)
        .await?;

        self.get_file_change(&change.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "file change".to_string(),
                id: change.id,
            })
    }

    pub async fn get_file_change(
        &self,
        change_id: &str,
    ) -> Result<Option<StoredFileChange>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, session_id, run_id, path, before_hash, after_hash, diff, created_at
            FROM file_changes
            WHERE id = ?1
            "#,
        )
        .bind(change_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(row_to_file_change).transpose()
    }

    pub async fn list_file_changes(
        &self,
        run_id: &RunId,
    ) -> Result<Vec<StoredFileChange>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, session_id, run_id, path, before_hash, after_hash, diff, created_at
            FROM file_changes
            WHERE run_id = ?1
            ORDER BY created_at ASC, id ASC
            "#,
        )
        .bind(run_id.as_str())
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_file_change).collect()
    }

    pub async fn create_artifact(
        &self,
        artifact: CreateArtifact,
    ) -> Result<StoredArtifact, StoreError> {
        let now = now_string();
        let content_json = artifact
            .content_json
            .map(|value| serde_json::to_string(&value))
            .transpose()?;
        sqlx::query(
            r#"
            INSERT INTO artifacts (
              id, session_id, run_id, kind, label, mime_type, path,
              content_text, content_json, created_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
        )
        .bind(&artifact.id)
        .bind(artifact.session_id.as_str())
        .bind(artifact.run_id.as_ref().map(RunId::as_str))
        .bind(&artifact.kind)
        .bind(&artifact.label)
        .bind(artifact.mime_type)
        .bind(artifact.path)
        .bind(artifact.content_text)
        .bind(content_json)
        .bind(now)
        .execute(&self.pool)
        .await?;

        self.get_artifact(&artifact.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "artifact".to_string(),
                id: artifact.id,
            })
    }

    pub async fn get_artifact(
        &self,
        artifact_id: &str,
    ) -> Result<Option<StoredArtifact>, StoreError> {
        let row = sqlx::query(artifact_select_sql("WHERE id = ?1"))
            .bind(artifact_id)
            .fetch_optional(&self.pool)
            .await?;

        row.map(row_to_artifact).transpose()
    }

    pub async fn list_artifacts(&self, run_id: &RunId) -> Result<Vec<StoredArtifact>, StoreError> {
        let rows = sqlx::query(artifact_select_sql(
            "WHERE run_id = ?1 ORDER BY created_at ASC, id ASC",
        ))
        .bind(run_id.as_str())
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_artifact).collect()
    }

    pub async fn create_observation(
        &self,
        observation: CreateObservation,
    ) -> Result<StoredObservation, StoreError> {
        let now = now_string();
        let resource_ref_json = observation
            .resource_ref_json
            .map(|value| serde_json::to_string(&value))
            .transpose()?;
        let data_json = serde_json::to_string(&observation.data_json)?;
        sqlx::query(
            r#"
            INSERT INTO observations (
              id, session_id, run_id, source, kind, subject, summary,
              resource_namespace, resource_kind, resource_name,
              resource_ref_json, artifact_id, data_json, observed_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
            "#,
        )
        .bind(&observation.id)
        .bind(observation.session_id.as_str())
        .bind(observation.run_id.as_ref().map(RunId::as_str))
        .bind(&observation.source)
        .bind(&observation.kind)
        .bind(&observation.subject)
        .bind(&observation.summary)
        .bind(observation.resource_namespace)
        .bind(observation.resource_kind)
        .bind(observation.resource_name)
        .bind(resource_ref_json)
        .bind(observation.artifact_id)
        .bind(data_json)
        .bind(now)
        .execute(&self.pool)
        .await?;

        self.get_observation(&observation.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "observation".to_string(),
                id: observation.id,
            })
    }

    pub async fn get_observation(
        &self,
        observation_id: &str,
    ) -> Result<Option<StoredObservation>, StoreError> {
        let row = sqlx::query(observation_select_sql("WHERE id = ?1"))
            .bind(observation_id)
            .fetch_optional(&self.pool)
            .await?;

        row.map(row_to_observation).transpose()
    }

    pub async fn list_run_observations(
        &self,
        run_id: &RunId,
    ) -> Result<Vec<StoredObservation>, StoreError> {
        let rows = sqlx::query(observation_select_sql(
            "WHERE run_id = ?1 ORDER BY observed_at ASC, id ASC",
        ))
        .bind(run_id.as_str())
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_observation).collect()
    }

    pub async fn list_observations(
        &self,
        filter: ObservationListFilter,
    ) -> Result<Vec<StoredObservation>, StoreError> {
        let limit = i64::from(filter.limit.clamp(1, 200));
        let offset = i64::from(filter.offset);
        let rows = sqlx::query(
            r#"
            SELECT id, session_id, run_id, source, kind, subject, summary,
                   resource_namespace, resource_kind, resource_name,
                   resource_ref_json, artifact_id, data_json, observed_at
            FROM observations
            WHERE (?1 IS NULL OR run_id = ?1)
              AND (?2 IS NULL OR source = ?2)
              AND (?3 IS NULL OR kind = ?3)
              AND (?4 IS NULL OR subject = ?4)
              AND (?5 IS NULL OR resource_namespace = ?5)
              AND (?6 IS NULL OR resource_kind = ?6)
              AND (?7 IS NULL OR resource_name = ?7)
              AND (?8 IS NULL OR CAST(observed_at AS INTEGER) >= ?8)
              AND (?9 IS NULL OR CAST(observed_at AS INTEGER) <= ?9)
            ORDER BY observed_at DESC, id DESC
            LIMIT ?10 OFFSET ?11
            "#,
        )
        .bind(filter.run_id.as_ref().map(RunId::as_str))
        .bind(filter.source)
        .bind(filter.kind)
        .bind(filter.subject)
        .bind(filter.resource_namespace)
        .bind(filter.resource_kind)
        .bind(filter.resource_name)
        .bind(filter.observed_after_ms)
        .bind(filter.observed_before_ms)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_observation).collect()
    }

    pub async fn create_incident(
        &self,
        incident: CreateIncident,
    ) -> Result<StoredIncident, StoreError> {
        let now = now_string();
        let data_json = serde_json::to_string(&incident.data_json)?;
        sqlx::query(
            r#"
            INSERT INTO incidents (
              id, observation_id, session_id, run_id, status, severity, title, summary,
              resource_namespace, resource_kind, resource_name, data_json, created_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            "#,
        )
        .bind(&incident.id)
        .bind(&incident.observation_id)
        .bind(incident.session_id.as_str())
        .bind(incident.run_id.as_ref().map(RunId::as_str))
        .bind(&incident.status)
        .bind(&incident.severity)
        .bind(&incident.title)
        .bind(&incident.summary)
        .bind(incident.resource_namespace)
        .bind(incident.resource_kind)
        .bind(incident.resource_name)
        .bind(data_json)
        .bind(now)
        .execute(&self.pool)
        .await?;

        self.get_incident(&incident.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "incident".to_string(),
                id: incident.id,
            })
    }

    pub async fn get_incident(
        &self,
        incident_id: &str,
    ) -> Result<Option<StoredIncident>, StoreError> {
        let row = sqlx::query(incident_select_sql("WHERE id = ?1"))
            .bind(incident_id)
            .fetch_optional(&self.pool)
            .await?;

        row.map(row_to_incident).transpose()
    }

    pub async fn list_incidents(
        &self,
        filter: IncidentListFilter,
    ) -> Result<Vec<StoredIncident>, StoreError> {
        let limit = i64::from(filter.limit.clamp(1, 200));
        let offset = i64::from(filter.offset);
        let rows = sqlx::query(
            r#"
            SELECT id, observation_id, session_id, run_id, status, severity, title, summary,
                   resource_namespace, resource_kind, resource_name, data_json, created_at
            FROM incidents
            WHERE (?1 IS NULL OR run_id = ?1)
              AND (?2 IS NULL OR status = ?2)
              AND (?3 IS NULL OR severity = ?3)
              AND (?4 IS NULL OR resource_namespace = ?4)
              AND (?5 IS NULL OR resource_kind = ?5)
              AND (?6 IS NULL OR resource_name = ?6)
              AND (?7 IS NULL OR CAST(created_at AS INTEGER) >= ?7)
              AND (?8 IS NULL OR CAST(created_at AS INTEGER) <= ?8)
            ORDER BY created_at DESC, id DESC
            LIMIT ?9 OFFSET ?10
            "#,
        )
        .bind(filter.run_id.as_ref().map(RunId::as_str))
        .bind(filter.status)
        .bind(filter.severity)
        .bind(filter.resource_namespace)
        .bind(filter.resource_kind)
        .bind(filter.resource_name)
        .bind(filter.created_after_ms)
        .bind(filter.created_before_ms)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_incident).collect()
    }

    pub async fn create_remediation_plan(
        &self,
        plan: CreateRemediationPlan,
    ) -> Result<StoredRemediationPlan, StoreError> {
        let now = now_string();
        let plan_json = serde_json::to_string(&plan.plan_json)?;
        sqlx::query(
            r#"
            INSERT INTO remediation_plans (
              id, incident_id, session_id, run_id, status, title, summary, risk_level,
              requires_approval, resource_namespace, resource_kind, resource_name,
              plan_json, created_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
            "#,
        )
        .bind(&plan.id)
        .bind(&plan.incident_id)
        .bind(plan.session_id.as_str())
        .bind(plan.run_id.as_ref().map(RunId::as_str))
        .bind(&plan.status)
        .bind(&plan.title)
        .bind(&plan.summary)
        .bind(&plan.risk_level)
        .bind(if plan.requires_approval { 1 } else { 0 })
        .bind(plan.resource_namespace)
        .bind(plan.resource_kind)
        .bind(plan.resource_name)
        .bind(plan_json)
        .bind(now)
        .execute(&self.pool)
        .await?;

        self.get_remediation_plan(&plan.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "remediation_plan".to_string(),
                id: plan.id,
            })
    }

    pub async fn get_remediation_plan(
        &self,
        plan_id: &str,
    ) -> Result<Option<StoredRemediationPlan>, StoreError> {
        let row = sqlx::query(remediation_plan_select_sql("WHERE id = ?1"))
            .bind(plan_id)
            .fetch_optional(&self.pool)
            .await?;

        row.map(row_to_remediation_plan).transpose()
    }

    pub async fn list_remediation_plans(
        &self,
        filter: RemediationPlanListFilter,
    ) -> Result<Vec<StoredRemediationPlan>, StoreError> {
        let limit = i64::from(filter.limit.clamp(1, 200));
        let offset = i64::from(filter.offset);
        let rows = sqlx::query(
            r#"
            SELECT id, incident_id, session_id, run_id, status, title, summary, risk_level,
                   requires_approval, resource_namespace, resource_kind, resource_name,
                   plan_json, created_at
            FROM remediation_plans
            WHERE (?1 IS NULL OR incident_id = ?1)
              AND (?2 IS NULL OR run_id = ?2)
              AND (?3 IS NULL OR status = ?3)
              AND (?4 IS NULL OR risk_level = ?4)
              AND (?5 IS NULL OR resource_namespace = ?5)
              AND (?6 IS NULL OR resource_kind = ?6)
              AND (?7 IS NULL OR resource_name = ?7)
              AND (?8 IS NULL OR CAST(created_at AS INTEGER) >= ?8)
              AND (?9 IS NULL OR CAST(created_at AS INTEGER) <= ?9)
            ORDER BY created_at DESC, id DESC
            LIMIT ?10 OFFSET ?11
            "#,
        )
        .bind(filter.incident_id)
        .bind(filter.run_id.as_ref().map(RunId::as_str))
        .bind(filter.status)
        .bind(filter.risk_level)
        .bind(filter.resource_namespace)
        .bind(filter.resource_kind)
        .bind(filter.resource_name)
        .bind(filter.created_after_ms)
        .bind(filter.created_before_ms)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_remediation_plan).collect()
    }

    pub async fn create_work_plan(
        &self,
        plan: CreateWorkPlan,
    ) -> Result<StoredWorkPlan, StoreError> {
        let now = now_string();
        let work_plan_json = serde_json::to_string(&plan.work_plan_json)?;
        sqlx::query(
            r#"
            INSERT INTO work_plans (
              id, work_item_id, remediation_plan_id, incident_id, session_id, run_id, status,
              title, summary, risk_level, requires_approval, resource_namespace, resource_kind,
              resource_name, work_plan_json, created_at, updated_at, status_changed_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
            "#,
        )
        .bind(&plan.id)
        .bind(&plan.work_item_id)
        .bind(&plan.remediation_plan_id)
        .bind(&plan.incident_id)
        .bind(plan.session_id.as_str())
        .bind(plan.run_id.as_ref().map(RunId::as_str))
        .bind(&plan.status)
        .bind(&plan.title)
        .bind(&plan.summary)
        .bind(&plan.risk_level)
        .bind(if plan.requires_approval { 1 } else { 0 })
        .bind(plan.resource_namespace)
        .bind(plan.resource_kind)
        .bind(plan.resource_name)
        .bind(work_plan_json)
        .bind(now.clone())
        .bind(now.clone())
        .bind(now)
        .execute(&self.pool)
        .await?;

        self.get_work_plan(&plan.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "work_plan".to_string(),
                id: plan.id,
            })
    }

    pub async fn create_work_item(
        &self,
        item: CreateWorkItem,
    ) -> Result<StoredWorkItem, StoreError> {
        let now = now_string();
        let acceptance_criteria_json = serde_json::to_string(&item.acceptance_criteria)?;
        sqlx::query(
            r#"
            INSERT INTO work_items (
              id, status, title, intent, acceptance_criteria_json, source_repo, source_ref,
              gitops_repo, gitops_ref, target_environment, target_namespace, argo_application,
              production_impacting, max_attempts, max_elapsed_seconds, created_by, created_at,
              updated_at, status_changed_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?17, ?17)
            "#,
        )
        .bind(&item.id)
        .bind(&item.status)
        .bind(&item.title)
        .bind(&item.intent)
        .bind(acceptance_criteria_json)
        .bind(&item.source_repo)
        .bind(&item.source_ref)
        .bind(item.gitops_repo)
        .bind(item.gitops_ref)
        .bind(&item.target_environment)
        .bind(item.target_namespace)
        .bind(item.argo_application)
        .bind(if item.production_impacting { 1 } else { 0 })
        .bind(i64::from(item.max_attempts))
        .bind(i64::try_from(item.max_elapsed_seconds).unwrap_or(i64::MAX))
        .bind(item.created_by)
        .bind(now)
        .execute(&self.pool)
        .await?;

        self.get_work_item(&item.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "work_item".to_string(),
                id: item.id,
            })
    }

    pub async fn get_work_item(
        &self,
        work_item_id: &str,
    ) -> Result<Option<StoredWorkItem>, StoreError> {
        let row = sqlx::query(work_item_select_sql("WHERE id = ?1"))
            .bind(work_item_id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_work_item).transpose()
    }

    pub async fn list_work_items(
        &self,
        filter: WorkItemListFilter,
    ) -> Result<Vec<StoredWorkItem>, StoreError> {
        let limit = i64::from(filter.limit.clamp(1, 200));
        let offset = i64::from(filter.offset);
        let production_impacting =
            filter
                .production_impacting
                .map(|value| if value { 1_i64 } else { 0_i64 });
        let rows = sqlx::query(
            r#"
            SELECT id, status, title, intent, acceptance_criteria_json, source_repo, source_ref,
                   gitops_repo, gitops_ref, target_environment, target_namespace,
                   argo_application, production_impacting, max_attempts, max_elapsed_seconds,
                   attempt_count, current_run_id, created_by, created_at, updated_at,
                   status_changed_at, status_changed_by, status_reason
            FROM work_items
            WHERE (?1 IS NULL OR status = ?1)
              AND (?2 IS NULL OR source_repo = ?2)
              AND (?3 IS NULL OR target_environment = ?3)
              AND (?4 IS NULL OR target_namespace = ?4)
              AND (?5 IS NULL OR production_impacting = ?5)
            ORDER BY created_at DESC, id DESC
            LIMIT ?6 OFFSET ?7
            "#,
        )
        .bind(filter.status)
        .bind(filter.source_repo)
        .bind(filter.target_environment)
        .bind(filter.target_namespace)
        .bind(production_impacting)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_work_item).collect()
    }

    pub async fn update_work_item_status(
        &self,
        work_item_id: &str,
        status: &str,
        actor: Option<String>,
        reason: Option<String>,
    ) -> Result<StoredWorkItem, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            UPDATE work_items
            SET status = ?2,
                updated_at = ?3,
                status_changed_at = ?3,
                status_changed_by = ?4,
                status_reason = ?5
            WHERE id = ?1
            "#,
        )
        .bind(work_item_id)
        .bind(status)
        .bind(now)
        .bind(actor)
        .bind(reason)
        .execute(&self.pool)
        .await?;

        self.get_work_item(work_item_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "work_item".to_string(),
                id: work_item_id.to_string(),
            })
    }

    pub async fn create_workspace(
        &self,
        workspace: CreateWorkspace,
    ) -> Result<StoredWorkspace, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            INSERT INTO workspaces (
              id, work_item_id, run_id, status, source_repo, source_ref, resolved_commit,
              branch, retention_status, created_at, updated_at, status_changed_at,
              status_changed_by, status_reason
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10, ?10, ?11, ?12)
            "#,
        )
        .bind(&workspace.id)
        .bind(&workspace.work_item_id)
        .bind(workspace.run_id.as_ref().map(RunId::as_str))
        .bind(&workspace.status)
        .bind(&workspace.source_repo)
        .bind(&workspace.source_ref)
        .bind(workspace.resolved_commit)
        .bind(workspace.branch)
        .bind(&workspace.retention_status)
        .bind(now)
        .bind(workspace.actor)
        .bind(workspace.reason)
        .execute(&self.pool)
        .await?;

        self.get_workspace(&workspace.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "workspace".to_string(),
                id: workspace.id,
            })
    }

    pub async fn get_workspace(
        &self,
        workspace_id: &str,
    ) -> Result<Option<StoredWorkspace>, StoreError> {
        let row = sqlx::query(workspace_select_sql("WHERE id = ?1"))
            .bind(workspace_id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_workspace).transpose()
    }

    pub async fn list_workspaces(
        &self,
        filter: WorkspaceListFilter,
    ) -> Result<Vec<StoredWorkspace>, StoreError> {
        let limit = i64::from(filter.limit.clamp(1, 200));
        let offset = i64::from(filter.offset);
        let rows = sqlx::query(
            r#"
            SELECT id, work_item_id, run_id, status, source_repo, source_ref, resolved_commit,
                   branch, retention_status, created_at, updated_at, status_changed_at,
                   status_changed_by, status_reason
            FROM workspaces
            WHERE (?1 IS NULL OR work_item_id = ?1)
              AND (?2 IS NULL OR run_id = ?2)
              AND (?3 IS NULL OR status = ?3)
            ORDER BY created_at DESC, id DESC
            LIMIT ?4 OFFSET ?5
            "#,
        )
        .bind(filter.work_item_id)
        .bind(filter.run_id.as_ref().map(RunId::as_str))
        .bind(filter.status)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_workspace).collect()
    }

    pub async fn get_work_plan(
        &self,
        work_plan_id: &str,
    ) -> Result<Option<StoredWorkPlan>, StoreError> {
        let row = sqlx::query(work_plan_select_sql("WHERE id = ?1"))
            .bind(work_plan_id)
            .fetch_optional(&self.pool)
            .await?;

        row.map(row_to_work_plan).transpose()
    }

    pub async fn get_work_plan_by_remediation_plan(
        &self,
        remediation_plan_id: &str,
    ) -> Result<Option<StoredWorkPlan>, StoreError> {
        let row = sqlx::query(work_plan_select_sql("WHERE remediation_plan_id = ?1"))
            .bind(remediation_plan_id)
            .fetch_optional(&self.pool)
            .await?;

        row.map(row_to_work_plan).transpose()
    }

    pub async fn get_work_plan_by_work_item(
        &self,
        work_item_id: &str,
    ) -> Result<Option<StoredWorkPlan>, StoreError> {
        let row = sqlx::query(work_plan_select_sql("WHERE work_item_id = ?1"))
            .bind(work_item_id)
            .fetch_optional(&self.pool)
            .await?;

        row.map(row_to_work_plan).transpose()
    }

    pub async fn list_work_plans(
        &self,
        filter: WorkPlanListFilter,
    ) -> Result<Vec<StoredWorkPlan>, StoreError> {
        let limit = i64::from(filter.limit.clamp(1, 200));
        let offset = i64::from(filter.offset);
        let rows = sqlx::query(
            r#"
            SELECT id, work_item_id, remediation_plan_id, incident_id, session_id, run_id,
                   status, title, summary, risk_level, requires_approval, resource_namespace,
                   resource_kind, resource_name, work_plan_json, created_at, updated_at,
                   revision, status_changed_at, status_changed_by, status_reason
            FROM work_plans
            WHERE (?1 IS NULL OR work_item_id = ?1)
              AND (?2 IS NULL OR remediation_plan_id = ?2)
              AND (?3 IS NULL OR incident_id = ?3)
              AND (?4 IS NULL OR run_id = ?4)
              AND (?5 IS NULL OR status = ?5)
              AND (?6 IS NULL OR risk_level = ?6)
              AND (?7 IS NULL OR resource_namespace = ?7)
              AND (?8 IS NULL OR resource_kind = ?8)
              AND (?9 IS NULL OR resource_name = ?9)
              AND (?10 IS NULL OR CAST(created_at AS INTEGER) >= ?10)
              AND (?11 IS NULL OR CAST(created_at AS INTEGER) <= ?11)
            ORDER BY created_at DESC, id DESC
            LIMIT ?12 OFFSET ?13
            "#,
        )
        .bind(filter.work_item_id)
        .bind(filter.remediation_plan_id)
        .bind(filter.incident_id)
        .bind(filter.run_id.as_ref().map(RunId::as_str))
        .bind(filter.status)
        .bind(filter.risk_level)
        .bind(filter.resource_namespace)
        .bind(filter.resource_kind)
        .bind(filter.resource_name)
        .bind(filter.created_after_ms)
        .bind(filter.created_before_ms)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_work_plan).collect()
    }

    pub async fn update_work_plan_status(
        &self,
        work_plan_id: &str,
        status: &str,
        actor: Option<String>,
        reason: Option<String>,
    ) -> Result<StoredWorkPlan, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            UPDATE work_plans
            SET status = ?2,
                updated_at = ?3,
                status_changed_at = ?3,
                status_changed_by = ?4,
                status_reason = ?5
            WHERE id = ?1
            "#,
        )
        .bind(work_plan_id)
        .bind(status)
        .bind(now)
        .bind(actor)
        .bind(reason)
        .execute(&self.pool)
        .await?;

        self.get_work_plan(work_plan_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "work_plan".to_string(),
                id: work_plan_id.to_string(),
            })
    }

    pub async fn revise_work_plan(
        &self,
        work_plan_id: &str,
        revision: UpdateWorkPlanRevision,
    ) -> Result<StoredWorkPlan, StoreError> {
        let now = now_string();
        let work_plan_json = serde_json::to_string(&revision.work_plan_json)?;
        sqlx::query(
            r#"
            UPDATE work_plans
            SET title = COALESCE(?2, title),
                summary = COALESCE(?3, summary),
                risk_level = COALESCE(?4, risk_level),
                requires_approval = COALESCE(?5, requires_approval),
                work_plan_json = ?6,
                status = 'draft',
                updated_at = ?7,
                revision = revision + 1,
                status_changed_at = ?7,
                status_changed_by = ?8,
                status_reason = ?9
            WHERE id = ?1
            "#,
        )
        .bind(work_plan_id)
        .bind(revision.title)
        .bind(revision.summary)
        .bind(revision.risk_level)
        .bind(
            revision
                .requires_approval
                .map(|value| if value { 1 } else { 0 }),
        )
        .bind(work_plan_json)
        .bind(now)
        .bind(revision.actor)
        .bind(revision.reason)
        .execute(&self.pool)
        .await?;

        self.get_work_plan(work_plan_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "work_plan".to_string(),
                id: work_plan_id.to_string(),
            })
    }

    pub async fn create_change_set(
        &self,
        change_set: CreateChangeSet,
    ) -> Result<StoredChangeSet, StoreError> {
        let now = now_string();
        let change_set_json = serde_json::to_string(&change_set.change_set_json)?;
        sqlx::query(
            r#"
            INSERT INTO change_sets (
              id, work_plan_id, remediation_plan_id, incident_id, session_id, run_id,
              status, title, summary, risk_level, material_hash, resource_namespace,
              resource_kind, resource_name, change_set_json, created_at, updated_at,
              status_changed_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
            "#,
        )
        .bind(&change_set.id)
        .bind(&change_set.work_plan_id)
        .bind(&change_set.remediation_plan_id)
        .bind(&change_set.incident_id)
        .bind(change_set.session_id.as_str())
        .bind(change_set.run_id.as_ref().map(RunId::as_str))
        .bind(&change_set.status)
        .bind(&change_set.title)
        .bind(&change_set.summary)
        .bind(&change_set.risk_level)
        .bind(&change_set.material_hash)
        .bind(change_set.resource_namespace)
        .bind(change_set.resource_kind)
        .bind(change_set.resource_name)
        .bind(change_set_json)
        .bind(now.clone())
        .bind(now.clone())
        .bind(now)
        .execute(&self.pool)
        .await?;

        self.get_change_set(&change_set.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "change_set".to_string(),
                id: change_set.id,
            })
    }

    pub async fn get_change_set(
        &self,
        change_set_id: &str,
    ) -> Result<Option<StoredChangeSet>, StoreError> {
        let row = sqlx::query(change_set_select_sql("WHERE id = ?1"))
            .bind(change_set_id)
            .fetch_optional(&self.pool)
            .await?;

        row.map(row_to_change_set).transpose()
    }

    pub async fn get_change_set_by_work_plan(
        &self,
        work_plan_id: &str,
    ) -> Result<Option<StoredChangeSet>, StoreError> {
        let row = sqlx::query(change_set_select_sql("WHERE work_plan_id = ?1"))
            .bind(work_plan_id)
            .fetch_optional(&self.pool)
            .await?;

        row.map(row_to_change_set).transpose()
    }

    pub async fn list_change_sets(
        &self,
        filter: ChangeSetListFilter,
    ) -> Result<Vec<StoredChangeSet>, StoreError> {
        let limit = i64::from(filter.limit.clamp(1, 200));
        let offset = i64::from(filter.offset);
        let rows = sqlx::query(
            r#"
            SELECT id, work_plan_id, remediation_plan_id, incident_id, session_id, run_id,
                   status, title, summary, risk_level, material_hash, revision,
                   resource_namespace, resource_kind, resource_name, change_set_json, created_at,
                   updated_at, status_changed_at, status_changed_by, status_reason
            FROM change_sets
            WHERE (?1 IS NULL OR work_plan_id = ?1)
              AND (?2 IS NULL OR remediation_plan_id = ?2)
              AND (?3 IS NULL OR incident_id = ?3)
              AND (?4 IS NULL OR run_id = ?4)
              AND (?5 IS NULL OR status = ?5)
              AND (?6 IS NULL OR risk_level = ?6)
              AND (?7 IS NULL OR resource_namespace = ?7)
              AND (?8 IS NULL OR resource_kind = ?8)
              AND (?9 IS NULL OR resource_name = ?9)
              AND (?10 IS NULL OR CAST(created_at AS INTEGER) >= ?10)
              AND (?11 IS NULL OR CAST(created_at AS INTEGER) <= ?11)
            ORDER BY created_at DESC, id DESC
            LIMIT ?12 OFFSET ?13
            "#,
        )
        .bind(filter.work_plan_id)
        .bind(filter.remediation_plan_id)
        .bind(filter.incident_id)
        .bind(filter.run_id.as_ref().map(RunId::as_str))
        .bind(filter.status)
        .bind(filter.risk_level)
        .bind(filter.resource_namespace)
        .bind(filter.resource_kind)
        .bind(filter.resource_name)
        .bind(filter.created_after_ms)
        .bind(filter.created_before_ms)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_change_set).collect()
    }

    pub async fn update_change_set_status(
        &self,
        change_set_id: &str,
        status: &str,
        actor: Option<String>,
        reason: Option<String>,
    ) -> Result<StoredChangeSet, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            UPDATE change_sets
            SET status = ?2,
                updated_at = ?3,
                status_changed_at = ?3,
                status_changed_by = ?4,
                status_reason = ?5
            WHERE id = ?1
            "#,
        )
        .bind(change_set_id)
        .bind(status)
        .bind(now)
        .bind(actor)
        .bind(reason)
        .execute(&self.pool)
        .await?;

        self.get_change_set(change_set_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "change_set".to_string(),
                id: change_set_id.to_string(),
            })
    }

    pub async fn revise_change_set(
        &self,
        change_set_id: &str,
        revision: UpdateChangeSetRevision,
    ) -> Result<StoredChangeSet, StoreError> {
        let now = now_string();
        let change_set_json = serde_json::to_string(&revision.change_set_json)?;
        sqlx::query(
            r#"
            UPDATE change_sets
            SET title = COALESCE(?2, title),
                summary = COALESCE(?3, summary),
                risk_level = COALESCE(?4, risk_level),
                material_hash = ?5,
                change_set_json = ?6,
                status = 'draft',
                updated_at = ?7,
                revision = revision + 1,
                status_changed_at = ?7,
                status_changed_by = ?8,
                status_reason = ?9
            WHERE id = ?1
            "#,
        )
        .bind(change_set_id)
        .bind(revision.title)
        .bind(revision.summary)
        .bind(revision.risk_level)
        .bind(revision.material_hash)
        .bind(change_set_json)
        .bind(now)
        .bind(revision.actor)
        .bind(revision.reason)
        .execute(&self.pool)
        .await?;

        self.get_change_set(change_set_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "change_set".to_string(),
                id: change_set_id.to_string(),
            })
    }

    pub async fn create_pipeline_intent(
        &self,
        intent: CreatePipelineIntent,
    ) -> Result<StoredPipelineIntent, StoreError> {
        let now = now_string();
        let intent_json = serde_json::to_string(&intent.intent_json)?;
        sqlx::query(
            r#"
            INSERT INTO pipeline_intents (
              id, change_set_id, work_plan_id, remediation_plan_id, incident_id, session_id, run_id,
              status, title, summary, risk_level, intent_kind, resource_namespace, resource_kind,
              resource_name, intent_json, created_at, updated_at, status_changed_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
            "#,
        )
        .bind(&intent.id)
        .bind(&intent.change_set_id)
        .bind(&intent.work_plan_id)
        .bind(&intent.remediation_plan_id)
        .bind(&intent.incident_id)
        .bind(intent.session_id.as_str())
        .bind(intent.run_id.as_ref().map(RunId::as_str))
        .bind(&intent.status)
        .bind(&intent.title)
        .bind(&intent.summary)
        .bind(&intent.risk_level)
        .bind(&intent.intent_kind)
        .bind(intent.resource_namespace)
        .bind(intent.resource_kind)
        .bind(intent.resource_name)
        .bind(intent_json)
        .bind(now.clone())
        .bind(now.clone())
        .bind(now)
        .execute(&self.pool)
        .await?;

        self.get_pipeline_intent(&intent.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "pipeline_intent".to_string(),
                id: intent.id,
            })
    }

    pub async fn get_pipeline_intent(
        &self,
        intent_id: &str,
    ) -> Result<Option<StoredPipelineIntent>, StoreError> {
        let row = sqlx::query(pipeline_intent_select_sql("WHERE id = ?1"))
            .bind(intent_id)
            .fetch_optional(&self.pool)
            .await?;

        row.map(row_to_pipeline_intent).transpose()
    }

    pub async fn get_pipeline_intent_by_change_set(
        &self,
        change_set_id: &str,
    ) -> Result<Option<StoredPipelineIntent>, StoreError> {
        let row = sqlx::query(pipeline_intent_select_sql("WHERE change_set_id = ?1"))
            .bind(change_set_id)
            .fetch_optional(&self.pool)
            .await?;

        row.map(row_to_pipeline_intent).transpose()
    }

    pub async fn list_pipeline_intents(
        &self,
        filter: PipelineIntentListFilter,
    ) -> Result<Vec<StoredPipelineIntent>, StoreError> {
        let limit = i64::from(filter.limit.clamp(1, 200));
        let offset = i64::from(filter.offset);
        let rows = sqlx::query(
            r#"
            SELECT id, change_set_id, work_plan_id, remediation_plan_id, incident_id, session_id,
                   run_id, status, title, summary, risk_level, intent_kind, resource_namespace,
                   resource_kind, resource_name, intent_json, created_at, updated_at,
                   status_changed_at, status_changed_by, status_reason
            FROM pipeline_intents
            WHERE (?1 IS NULL OR change_set_id = ?1)
              AND (?2 IS NULL OR work_plan_id = ?2)
              AND (?3 IS NULL OR remediation_plan_id = ?3)
              AND (?4 IS NULL OR incident_id = ?4)
              AND (?5 IS NULL OR run_id = ?5)
              AND (?6 IS NULL OR status = ?6)
              AND (?7 IS NULL OR intent_kind = ?7)
              AND (?8 IS NULL OR risk_level = ?8)
              AND (?9 IS NULL OR resource_namespace = ?9)
              AND (?10 IS NULL OR resource_kind = ?10)
              AND (?11 IS NULL OR resource_name = ?11)
              AND (?12 IS NULL OR CAST(created_at AS INTEGER) >= ?12)
              AND (?13 IS NULL OR CAST(created_at AS INTEGER) <= ?13)
            ORDER BY created_at DESC, id DESC
            LIMIT ?14 OFFSET ?15
            "#,
        )
        .bind(filter.change_set_id)
        .bind(filter.work_plan_id)
        .bind(filter.remediation_plan_id)
        .bind(filter.incident_id)
        .bind(filter.run_id.as_ref().map(RunId::as_str))
        .bind(filter.status)
        .bind(filter.intent_kind)
        .bind(filter.risk_level)
        .bind(filter.resource_namespace)
        .bind(filter.resource_kind)
        .bind(filter.resource_name)
        .bind(filter.created_after_ms)
        .bind(filter.created_before_ms)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_pipeline_intent).collect()
    }

    pub async fn create_pipeline_contract(
        &self,
        contract: CreatePipelineContract,
    ) -> Result<StoredPipelineContract, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            INSERT INTO pipeline_contracts (
              id, status, namespace, pipeline_ref, version, contract_json, created_at,
              updated_at, status_changed_at, status_changed_by, status_reason
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?7, ?8, ?9)
            "#,
        )
        .bind(&contract.id)
        .bind(&contract.status)
        .bind(&contract.namespace)
        .bind(&contract.pipeline_ref)
        .bind(&contract.version)
        .bind(serde_json::to_string(&contract.contract_json)?)
        .bind(now)
        .bind(contract.actor)
        .bind(contract.reason)
        .execute(&self.pool)
        .await?;

        self.get_pipeline_contract(&contract.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "pipeline_contract".to_string(),
                id: contract.id,
            })
    }

    pub async fn get_pipeline_contract(
        &self,
        contract_id: &str,
    ) -> Result<Option<StoredPipelineContract>, StoreError> {
        let row = sqlx::query(pipeline_contract_select_sql("WHERE id = ?1"))
            .bind(contract_id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_pipeline_contract).transpose()
    }

    pub async fn list_pipeline_contracts(
        &self,
        filter: PipelineContractListFilter,
    ) -> Result<Vec<StoredPipelineContract>, StoreError> {
        let limit = i64::from(filter.limit.clamp(1, 200));
        let offset = i64::from(filter.offset);
        let rows = sqlx::query(
            r#"
            SELECT id, status, namespace, pipeline_ref, version, contract_json, created_at,
                   updated_at, status_changed_at, status_changed_by, status_reason
            FROM pipeline_contracts
            WHERE (?1 IS NULL OR namespace = ?1)
              AND (?2 IS NULL OR pipeline_ref = ?2)
              AND (?3 IS NULL OR status = ?3)
            ORDER BY created_at DESC, id DESC
            LIMIT ?4 OFFSET ?5
            "#,
        )
        .bind(filter.namespace)
        .bind(filter.pipeline_ref)
        .bind(filter.status)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_pipeline_contract).collect()
    }

    pub async fn update_pipeline_contract_status(
        &self,
        contract_id: &str,
        status: &str,
        actor: Option<String>,
        reason: Option<String>,
    ) -> Result<StoredPipelineContract, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            UPDATE pipeline_contracts
            SET status = ?2,
                updated_at = ?3,
                status_changed_at = ?3,
                status_changed_by = ?4,
                status_reason = ?5
            WHERE id = ?1
            "#,
        )
        .bind(contract_id)
        .bind(status)
        .bind(now)
        .bind(actor)
        .bind(reason)
        .execute(&self.pool)
        .await?;
        self.get_pipeline_contract(contract_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "pipeline_contract".to_string(),
                id: contract_id.to_string(),
            })
    }

    /// Atomically retire one active contract and activate its replacement.
    /// No committed state contains two active versions or an empty contract slot.
    pub async fn replace_pipeline_contract(
        &self,
        current_contract_id: &str,
        replacement: ReplacePipelineContract,
    ) -> Result<(StoredPipelineContract, StoredPipelineContract), StoreError> {
        let now = now_string();
        let mut transaction = self.pool.begin().await?;
        let retired = sqlx::query(
            r#"
            UPDATE pipeline_contracts
            SET status = 'retired',
                updated_at = ?2,
                status_changed_at = ?2,
                status_changed_by = ?3,
                status_reason = ?4
            WHERE id = ?1 AND status = 'active'
            "#,
        )
        .bind(current_contract_id)
        .bind(&now)
        .bind(&replacement.actor)
        .bind(&replacement.reason)
        .execute(&mut *transaction)
        .await?;
        if retired.rows_affected() != 1 {
            return Err(StoreError::NotFound {
                entity: "active pipeline_contract".to_string(),
                id: current_contract_id.to_string(),
            });
        }
        sqlx::query(
            r#"
            INSERT INTO pipeline_contracts (
              id, status, namespace, pipeline_ref, version, contract_json, created_at,
              updated_at, status_changed_at, status_changed_by, status_reason
            )
            VALUES (?1, 'active', ?2, ?3, ?4, ?5, ?6, ?6, ?6, ?7, ?8)
            "#,
        )
        .bind(&replacement.id)
        .bind(&replacement.namespace)
        .bind(&replacement.pipeline_ref)
        .bind(&replacement.version)
        .bind(serde_json::to_string(&replacement.contract_json)?)
        .bind(&now)
        .bind(&replacement.actor)
        .bind(&replacement.reason)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;

        let retired = self
            .get_pipeline_contract(current_contract_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "pipeline_contract".to_string(),
                id: current_contract_id.to_string(),
            })?;
        let active = self
            .get_pipeline_contract(&replacement.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "pipeline_contract".to_string(),
                id: replacement.id,
            })?;
        Ok((retired, active))
    }

    pub async fn update_pipeline_intent_status(
        &self,
        intent_id: &str,
        status: &str,
        actor: Option<String>,
        reason: Option<String>,
    ) -> Result<StoredPipelineIntent, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            UPDATE pipeline_intents
            SET status = ?2,
                updated_at = ?3,
                status_changed_at = ?3,
                status_changed_by = ?4,
                status_reason = ?5
            WHERE id = ?1
            "#,
        )
        .bind(intent_id)
        .bind(status)
        .bind(now)
        .bind(actor)
        .bind(reason)
        .execute(&self.pool)
        .await?;

        self.get_pipeline_intent(intent_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "pipeline_intent".to_string(),
                id: intent_id.to_string(),
            })
    }

    pub async fn revise_pipeline_intent_draft(
        &self,
        intent_id: &str,
        revision: UpdatePipelineIntentDraft,
    ) -> Result<StoredPipelineIntent, StoreError> {
        let now = now_string();
        let intent_json = serde_json::to_string(&revision.intent_json)?;
        sqlx::query(
            r#"
            UPDATE pipeline_intents
            SET status = 'proposed',
                title = ?2,
                summary = ?3,
                risk_level = ?4,
                intent_kind = ?5,
                resource_namespace = ?6,
                resource_kind = ?7,
                resource_name = ?8,
                intent_json = ?9,
                updated_at = ?10,
                status_changed_at = ?10,
                status_changed_by = ?11,
                status_reason = ?12
            WHERE id = ?1
            "#,
        )
        .bind(intent_id)
        .bind(revision.title)
        .bind(revision.summary)
        .bind(revision.risk_level)
        .bind(revision.intent_kind)
        .bind(revision.resource_namespace)
        .bind(revision.resource_kind)
        .bind(revision.resource_name)
        .bind(intent_json)
        .bind(now)
        .bind(revision.actor)
        .bind(revision.reason)
        .execute(&self.pool)
        .await?;

        self.get_pipeline_intent(intent_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "pipeline_intent".to_string(),
                id: intent_id.to_string(),
            })
    }

    pub async fn update_pipeline_intent_evidence(
        &self,
        intent_id: &str,
        update: UpdatePipelineIntentEvidence,
    ) -> Result<StoredPipelineIntent, StoreError> {
        let now = now_string();
        let intent_json = serde_json::to_string(&update.intent_json)?;
        sqlx::query(
            r#"
            UPDATE pipeline_intents
            SET intent_json = ?2,
                updated_at = ?3,
                status_changed_at = ?3,
                status_changed_by = ?4,
                status_reason = ?5
            WHERE id = ?1
            "#,
        )
        .bind(intent_id)
        .bind(intent_json)
        .bind(now)
        .bind(update.actor)
        .bind(update.reason)
        .execute(&self.pool)
        .await?;

        self.get_pipeline_intent(intent_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "pipeline_intent".to_string(),
                id: intent_id.to_string(),
            })
    }

    pub async fn update_pipeline_intent_execution(
        &self,
        intent_id: &str,
        update: UpdatePipelineIntentExecution,
    ) -> Result<StoredPipelineIntent, StoreError> {
        let now = now_string();
        let intent_json = serde_json::to_string(&update.intent_json)?;
        sqlx::query(
            r#"
            UPDATE pipeline_intents
            SET status = ?2,
                intent_json = ?3,
                updated_at = ?4,
                status_changed_at = ?4,
                status_changed_by = ?5,
                status_reason = ?6
            WHERE id = ?1
            "#,
        )
        .bind(intent_id)
        .bind(update.status)
        .bind(intent_json)
        .bind(now)
        .bind(update.actor)
        .bind(update.reason)
        .execute(&self.pool)
        .await?;

        self.get_pipeline_intent(intent_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "pipeline_intent".to_string(),
                id: intent_id.to_string(),
            })
    }

    pub async fn create_deployment_contract(
        &self,
        contract: CreateDeploymentContract,
    ) -> Result<StoredDeploymentContract, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            INSERT INTO deployment_contracts (
              id, status, target_environment, target_namespace, argo_application, version,
              contract_json, created_at, updated_at, status_changed_at, status_changed_by,
              status_reason
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?8, ?9, ?10)
            "#,
        )
        .bind(&contract.id)
        .bind(&contract.status)
        .bind(&contract.target_environment)
        .bind(&contract.target_namespace)
        .bind(&contract.argo_application)
        .bind(&contract.version)
        .bind(serde_json::to_string(&contract.contract_json)?)
        .bind(now)
        .bind(contract.actor)
        .bind(contract.reason)
        .execute(&self.pool)
        .await?;
        self.get_deployment_contract(&contract.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "deployment_contract".to_string(),
                id: contract.id,
            })
    }

    pub async fn get_deployment_contract(
        &self,
        contract_id: &str,
    ) -> Result<Option<StoredDeploymentContract>, StoreError> {
        let row = sqlx::query(deployment_contract_select_sql("WHERE id = ?1"))
            .bind(contract_id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_deployment_contract).transpose()
    }

    pub async fn list_deployment_contracts(
        &self,
        filter: DeploymentContractListFilter,
    ) -> Result<Vec<StoredDeploymentContract>, StoreError> {
        let limit = i64::from(filter.limit.clamp(1, 200));
        let offset = i64::from(filter.offset);
        let rows = sqlx::query(
            r#"
            SELECT id, status, target_environment, target_namespace, argo_application, version,
                   contract_json, created_at, updated_at, status_changed_at, status_changed_by,
                   status_reason
            FROM deployment_contracts
            WHERE (?1 IS NULL OR target_environment = ?1)
              AND (?2 IS NULL OR target_namespace = ?2)
              AND (?3 IS NULL OR argo_application = ?3)
              AND (?4 IS NULL OR status = ?4)
            ORDER BY created_at DESC, id DESC
            LIMIT ?5 OFFSET ?6
            "#,
        )
        .bind(filter.target_environment)
        .bind(filter.target_namespace)
        .bind(filter.argo_application)
        .bind(filter.status)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_deployment_contract).collect()
    }

    pub async fn update_deployment_contract_status(
        &self,
        contract_id: &str,
        status: &str,
        actor: Option<String>,
        reason: Option<String>,
    ) -> Result<StoredDeploymentContract, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            UPDATE deployment_contracts
            SET status = ?2, updated_at = ?3, status_changed_at = ?3,
                status_changed_by = ?4, status_reason = ?5
            WHERE id = ?1
            "#,
        )
        .bind(contract_id)
        .bind(status)
        .bind(now)
        .bind(actor)
        .bind(reason)
        .execute(&self.pool)
        .await?;
        self.get_deployment_contract(contract_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "deployment_contract".to_string(),
                id: contract_id.to_string(),
            })
    }

    pub async fn create_deployment_intent(
        &self,
        intent: CreateDeploymentIntent,
    ) -> Result<StoredDeploymentIntent, StoreError> {
        let now = now_string();
        let intent_json = serde_json::to_string(&intent.intent_json)?;
        sqlx::query(
            r#"
            INSERT INTO deployment_intents (
              id, pipeline_intent_id, change_set_id, work_plan_id, remediation_plan_id,
              incident_id, session_id, run_id, status, title, summary, risk_level, intent_kind,
              target_environment, target_namespace, argo_application, resource_namespace,
              resource_kind, resource_name, intent_json, created_at, updated_at, status_changed_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)
            "#,
        )
        .bind(&intent.id)
        .bind(&intent.pipeline_intent_id)
        .bind(&intent.change_set_id)
        .bind(&intent.work_plan_id)
        .bind(&intent.remediation_plan_id)
        .bind(&intent.incident_id)
        .bind(intent.session_id.as_str())
        .bind(intent.run_id.as_ref().map(RunId::as_str))
        .bind(&intent.status)
        .bind(&intent.title)
        .bind(&intent.summary)
        .bind(&intent.risk_level)
        .bind(&intent.intent_kind)
        .bind(intent.target_environment)
        .bind(intent.target_namespace)
        .bind(intent.argo_application)
        .bind(intent.resource_namespace)
        .bind(intent.resource_kind)
        .bind(intent.resource_name)
        .bind(intent_json)
        .bind(now.clone())
        .bind(now.clone())
        .bind(now)
        .execute(&self.pool)
        .await?;

        self.get_deployment_intent(&intent.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "deployment_intent".to_string(),
                id: intent.id,
            })
    }

    pub async fn get_deployment_intent(
        &self,
        intent_id: &str,
    ) -> Result<Option<StoredDeploymentIntent>, StoreError> {
        let row = sqlx::query(deployment_intent_select_sql("WHERE id = ?1"))
            .bind(intent_id)
            .fetch_optional(&self.pool)
            .await?;

        row.map(row_to_deployment_intent).transpose()
    }

    pub async fn get_deployment_intent_by_pipeline_intent(
        &self,
        pipeline_intent_id: &str,
    ) -> Result<Option<StoredDeploymentIntent>, StoreError> {
        let row = sqlx::query(deployment_intent_select_sql(
            "WHERE pipeline_intent_id = ?1",
        ))
        .bind(pipeline_intent_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(row_to_deployment_intent).transpose()
    }

    pub async fn list_deployment_intents(
        &self,
        filter: DeploymentIntentListFilter,
    ) -> Result<Vec<StoredDeploymentIntent>, StoreError> {
        let limit = i64::from(filter.limit.clamp(1, 200));
        let offset = i64::from(filter.offset);
        let rows = sqlx::query(
            r#"
            SELECT id, pipeline_intent_id, change_set_id, work_plan_id, remediation_plan_id,
                   incident_id, session_id, run_id, status, title, summary, risk_level,
                   intent_kind, target_environment, target_namespace, argo_application,
                   resource_namespace, resource_kind, resource_name, intent_json, created_at,
                   updated_at, status_changed_at, status_changed_by, status_reason
            FROM deployment_intents
            WHERE (?1 IS NULL OR pipeline_intent_id = ?1)
              AND (?2 IS NULL OR change_set_id = ?2)
              AND (?3 IS NULL OR work_plan_id = ?3)
              AND (?4 IS NULL OR remediation_plan_id = ?4)
              AND (?5 IS NULL OR incident_id = ?5)
              AND (?6 IS NULL OR run_id = ?6)
              AND (?7 IS NULL OR status = ?7)
              AND (?8 IS NULL OR intent_kind = ?8)
              AND (?9 IS NULL OR risk_level = ?9)
              AND (?10 IS NULL OR target_environment = ?10)
              AND (?11 IS NULL OR target_namespace = ?11)
              AND (?12 IS NULL OR argo_application = ?12)
              AND (?13 IS NULL OR resource_namespace = ?13)
              AND (?14 IS NULL OR resource_kind = ?14)
              AND (?15 IS NULL OR resource_name = ?15)
              AND (?16 IS NULL OR CAST(created_at AS INTEGER) >= ?16)
              AND (?17 IS NULL OR CAST(created_at AS INTEGER) <= ?17)
            ORDER BY created_at DESC, id DESC
            LIMIT ?18 OFFSET ?19
            "#,
        )
        .bind(filter.pipeline_intent_id)
        .bind(filter.change_set_id)
        .bind(filter.work_plan_id)
        .bind(filter.remediation_plan_id)
        .bind(filter.incident_id)
        .bind(filter.run_id.as_ref().map(RunId::as_str))
        .bind(filter.status)
        .bind(filter.intent_kind)
        .bind(filter.risk_level)
        .bind(filter.target_environment)
        .bind(filter.target_namespace)
        .bind(filter.argo_application)
        .bind(filter.resource_namespace)
        .bind(filter.resource_kind)
        .bind(filter.resource_name)
        .bind(filter.created_after_ms)
        .bind(filter.created_before_ms)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_deployment_intent).collect()
    }

    pub async fn update_deployment_intent_status(
        &self,
        intent_id: &str,
        status: &str,
        actor: Option<String>,
        reason: Option<String>,
    ) -> Result<StoredDeploymentIntent, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            UPDATE deployment_intents
            SET status = ?2,
                updated_at = ?3,
                status_changed_at = ?3,
                status_changed_by = ?4,
                status_reason = ?5
            WHERE id = ?1
            "#,
        )
        .bind(intent_id)
        .bind(status)
        .bind(now)
        .bind(actor)
        .bind(reason)
        .execute(&self.pool)
        .await?;

        self.get_deployment_intent(intent_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "deployment_intent".to_string(),
                id: intent_id.to_string(),
            })
    }

    pub async fn revise_deployment_intent_draft(
        &self,
        intent_id: &str,
        revision: UpdateDeploymentIntentDraft,
    ) -> Result<StoredDeploymentIntent, StoreError> {
        let now = now_string();
        let intent_json = serde_json::to_string(&revision.intent_json)?;
        sqlx::query(
            r#"
            UPDATE deployment_intents
            SET status = 'proposed',
                title = ?2,
                summary = ?3,
                risk_level = ?4,
                intent_kind = ?5,
                target_environment = ?6,
                target_namespace = ?7,
                argo_application = ?8,
                resource_namespace = ?9,
                resource_kind = ?10,
                resource_name = ?11,
                intent_json = ?12,
                updated_at = ?13,
                status_changed_at = ?13,
                status_changed_by = ?14,
                status_reason = ?15
            WHERE id = ?1
            "#,
        )
        .bind(intent_id)
        .bind(revision.title)
        .bind(revision.summary)
        .bind(revision.risk_level)
        .bind(revision.intent_kind)
        .bind(revision.target_environment)
        .bind(revision.target_namespace)
        .bind(revision.argo_application)
        .bind(revision.resource_namespace)
        .bind(revision.resource_kind)
        .bind(revision.resource_name)
        .bind(intent_json)
        .bind(now)
        .bind(revision.actor)
        .bind(revision.reason)
        .execute(&self.pool)
        .await?;

        self.get_deployment_intent(intent_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "deployment_intent".to_string(),
                id: intent_id.to_string(),
            })
    }

    pub async fn update_deployment_intent_evidence(
        &self,
        intent_id: &str,
        update: UpdateDeploymentIntentEvidence,
    ) -> Result<StoredDeploymentIntent, StoreError> {
        let now = now_string();
        let intent_json = serde_json::to_string(&update.intent_json)?;
        sqlx::query(
            r#"
            UPDATE deployment_intents
            SET intent_json = ?2,
                updated_at = ?3,
                status_changed_at = ?3,
                status_changed_by = ?4,
                status_reason = ?5
            WHERE id = ?1
            "#,
        )
        .bind(intent_id)
        .bind(intent_json)
        .bind(now)
        .bind(update.actor)
        .bind(update.reason)
        .execute(&self.pool)
        .await?;

        self.get_deployment_intent(intent_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "deployment_intent".to_string(),
                id: intent_id.to_string(),
            })
    }

    pub async fn create_release(
        &self,
        release: CreateRelease,
    ) -> Result<StoredRelease, StoreError> {
        let now = now_string();
        let release_json = serde_json::to_string(&release.release_json)?;
        sqlx::query(
            r#"
            INSERT INTO releases (
              id, deployment_intent_id, pipeline_intent_id, change_set_id, work_plan_id,
              remediation_plan_id, incident_id, session_id, run_id, status, title, summary,
              risk_level, release_kind, target_environment, target_namespace, argo_application,
              version, commit_sha, image_digest, rollback_ref, release_json, created_at,
              updated_at, status_changed_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)
            "#,
        )
        .bind(&release.id)
        .bind(&release.deployment_intent_id)
        .bind(&release.pipeline_intent_id)
        .bind(&release.change_set_id)
        .bind(&release.work_plan_id)
        .bind(&release.remediation_plan_id)
        .bind(&release.incident_id)
        .bind(release.session_id.as_str())
        .bind(release.run_id.as_ref().map(RunId::as_str))
        .bind(&release.status)
        .bind(&release.title)
        .bind(&release.summary)
        .bind(&release.risk_level)
        .bind(&release.release_kind)
        .bind(release.target_environment)
        .bind(release.target_namespace)
        .bind(release.argo_application)
        .bind(release.version)
        .bind(release.commit_sha)
        .bind(release.image_digest)
        .bind(release.rollback_ref)
        .bind(release_json)
        .bind(now.clone())
        .bind(now.clone())
        .bind(now)
        .execute(&self.pool)
        .await?;

        self.get_release(&release.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "release".to_string(),
                id: release.id,
            })
    }

    pub async fn get_release(&self, release_id: &str) -> Result<Option<StoredRelease>, StoreError> {
        let row = sqlx::query(release_select_sql("WHERE id = ?1"))
            .bind(release_id)
            .fetch_optional(&self.pool)
            .await?;

        row.map(row_to_release).transpose()
    }

    pub async fn get_release_by_deployment_intent(
        &self,
        deployment_intent_id: &str,
    ) -> Result<Option<StoredRelease>, StoreError> {
        let row = sqlx::query(release_select_sql("WHERE deployment_intent_id = ?1"))
            .bind(deployment_intent_id)
            .fetch_optional(&self.pool)
            .await?;

        row.map(row_to_release).transpose()
    }

    pub async fn list_releases(
        &self,
        filter: ReleaseListFilter,
    ) -> Result<Vec<StoredRelease>, StoreError> {
        let limit = i64::from(filter.limit.clamp(1, 200));
        let offset = i64::from(filter.offset);
        let rows = sqlx::query(
            r#"
            SELECT id, deployment_intent_id, pipeline_intent_id, change_set_id, work_plan_id,
                   remediation_plan_id, incident_id, session_id, run_id, status, title, summary,
                   risk_level, release_kind, target_environment, target_namespace,
                   argo_application, version, commit_sha, image_digest, rollback_ref,
                   release_json, created_at, updated_at, status_changed_at, status_changed_by,
                   status_reason
            FROM releases
            WHERE (?1 IS NULL OR deployment_intent_id = ?1)
              AND (?2 IS NULL OR pipeline_intent_id = ?2)
              AND (?3 IS NULL OR change_set_id = ?3)
              AND (?4 IS NULL OR work_plan_id = ?4)
              AND (?5 IS NULL OR remediation_plan_id = ?5)
              AND (?6 IS NULL OR incident_id = ?6)
              AND (?7 IS NULL OR run_id = ?7)
              AND (?8 IS NULL OR status = ?8)
              AND (?9 IS NULL OR release_kind = ?9)
              AND (?10 IS NULL OR risk_level = ?10)
              AND (?11 IS NULL OR target_environment = ?11)
              AND (?12 IS NULL OR target_namespace = ?12)
              AND (?13 IS NULL OR argo_application = ?13)
              AND (?14 IS NULL OR version = ?14)
              AND (?15 IS NULL OR commit_sha = ?15)
              AND (?16 IS NULL OR image_digest = ?16)
              AND (?17 IS NULL OR CAST(created_at AS INTEGER) >= ?17)
              AND (?18 IS NULL OR CAST(created_at AS INTEGER) <= ?18)
            ORDER BY created_at DESC, id DESC
            LIMIT ?19 OFFSET ?20
            "#,
        )
        .bind(filter.deployment_intent_id)
        .bind(filter.pipeline_intent_id)
        .bind(filter.change_set_id)
        .bind(filter.work_plan_id)
        .bind(filter.remediation_plan_id)
        .bind(filter.incident_id)
        .bind(filter.run_id.as_ref().map(RunId::as_str))
        .bind(filter.status)
        .bind(filter.release_kind)
        .bind(filter.risk_level)
        .bind(filter.target_environment)
        .bind(filter.target_namespace)
        .bind(filter.argo_application)
        .bind(filter.version)
        .bind(filter.commit_sha)
        .bind(filter.image_digest)
        .bind(filter.created_after_ms)
        .bind(filter.created_before_ms)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_release).collect()
    }

    pub async fn update_release_status(
        &self,
        release_id: &str,
        status: &str,
        actor: Option<String>,
        reason: Option<String>,
    ) -> Result<StoredRelease, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            UPDATE releases
            SET status = ?2,
                updated_at = ?3,
                status_changed_at = ?3,
                status_changed_by = ?4,
                status_reason = ?5
            WHERE id = ?1
            "#,
        )
        .bind(release_id)
        .bind(status)
        .bind(now)
        .bind(actor)
        .bind(reason)
        .execute(&self.pool)
        .await?;

        self.get_release(release_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "release".to_string(),
                id: release_id.to_string(),
            })
    }

    pub async fn revise_release_draft(
        &self,
        release_id: &str,
        revision: UpdateReleaseDraft,
    ) -> Result<StoredRelease, StoreError> {
        let now = now_string();
        let release_json = serde_json::to_string(&revision.release_json)?;
        sqlx::query(
            r#"
            UPDATE releases
            SET status = 'proposed',
                title = ?2,
                summary = ?3,
                risk_level = ?4,
                release_kind = ?5,
                target_environment = ?6,
                target_namespace = ?7,
                argo_application = ?8,
                version = ?9,
                commit_sha = ?10,
                image_digest = ?11,
                rollback_ref = ?12,
                release_json = ?13,
                updated_at = ?14,
                status_changed_at = ?14,
                status_changed_by = ?15,
                status_reason = ?16
            WHERE id = ?1
            "#,
        )
        .bind(release_id)
        .bind(revision.title)
        .bind(revision.summary)
        .bind(revision.risk_level)
        .bind(revision.release_kind)
        .bind(revision.target_environment)
        .bind(revision.target_namespace)
        .bind(revision.argo_application)
        .bind(revision.version)
        .bind(revision.commit_sha)
        .bind(revision.image_digest)
        .bind(revision.rollback_ref)
        .bind(release_json)
        .bind(now)
        .bind(revision.actor)
        .bind(revision.reason)
        .execute(&self.pool)
        .await?;

        self.get_release(release_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "release".to_string(),
                id: release_id.to_string(),
            })
    }

    pub async fn update_release_evidence(
        &self,
        release_id: &str,
        update: UpdateReleaseEvidence,
    ) -> Result<StoredRelease, StoreError> {
        let now = now_string();
        let release_json = serde_json::to_string(&update.release_json)?;
        sqlx::query(
            r#"
            UPDATE releases
            SET release_json = ?2,
                updated_at = ?3,
                status_changed_at = ?3,
                status_changed_by = ?4,
                status_reason = ?5
            WHERE id = ?1
            "#,
        )
        .bind(release_id)
        .bind(release_json)
        .bind(now)
        .bind(update.actor)
        .bind(update.reason)
        .execute(&self.pool)
        .await?;

        self.get_release(release_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "release".to_string(),
                id: release_id.to_string(),
            })
    }

    pub async fn create_registry_evidence(
        &self,
        evidence: CreateRegistryEvidence,
    ) -> Result<StoredRegistryEvidence, StoreError> {
        let now = now_string();
        let evidence_json = serde_json::to_string(&evidence.evidence_json)?;
        sqlx::query(
            r#"
            INSERT INTO registry_evidence (
              id, release_id, deployment_intent_id, pipeline_intent_id, change_set_id,
              work_plan_id, remediation_plan_id, incident_id, session_id, run_id, status,
              title, summary, risk_level, registry, repository, image_ref, image_digest, tag,
              source, verification_status, evidence_json, created_at, updated_at,
              status_changed_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)
            "#,
        )
        .bind(&evidence.id)
        .bind(&evidence.release_id)
        .bind(&evidence.deployment_intent_id)
        .bind(&evidence.pipeline_intent_id)
        .bind(&evidence.change_set_id)
        .bind(&evidence.work_plan_id)
        .bind(&evidence.remediation_plan_id)
        .bind(&evidence.incident_id)
        .bind(evidence.session_id.as_str())
        .bind(evidence.run_id.as_ref().map(RunId::as_str))
        .bind(&evidence.status)
        .bind(&evidence.title)
        .bind(&evidence.summary)
        .bind(&evidence.risk_level)
        .bind(evidence.registry)
        .bind(evidence.repository)
        .bind(evidence.image_ref)
        .bind(evidence.image_digest)
        .bind(evidence.tag)
        .bind(&evidence.source)
        .bind(&evidence.verification_status)
        .bind(evidence_json)
        .bind(now.clone())
        .bind(now.clone())
        .bind(now)
        .execute(&self.pool)
        .await?;

        self.get_registry_evidence(&evidence.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "registry_evidence".to_string(),
                id: evidence.id,
            })
    }

    pub async fn get_registry_evidence(
        &self,
        evidence_id: &str,
    ) -> Result<Option<StoredRegistryEvidence>, StoreError> {
        let row = sqlx::query(registry_evidence_select_sql("WHERE id = ?1"))
            .bind(evidence_id)
            .fetch_optional(&self.pool)
            .await?;

        row.map(row_to_registry_evidence).transpose()
    }

    pub async fn get_registry_evidence_by_release(
        &self,
        release_id: &str,
    ) -> Result<Option<StoredRegistryEvidence>, StoreError> {
        let row = sqlx::query(registry_evidence_select_sql("WHERE release_id = ?1"))
            .bind(release_id)
            .fetch_optional(&self.pool)
            .await?;

        row.map(row_to_registry_evidence).transpose()
    }

    pub async fn list_registry_evidence(
        &self,
        filter: RegistryEvidenceListFilter,
    ) -> Result<Vec<StoredRegistryEvidence>, StoreError> {
        let limit = i64::from(filter.limit.clamp(1, 200));
        let offset = i64::from(filter.offset);
        let rows = sqlx::query(
            r#"
            SELECT id, release_id, deployment_intent_id, pipeline_intent_id, change_set_id,
                   work_plan_id, remediation_plan_id, incident_id, session_id, run_id, status,
                   title, summary, risk_level, registry, repository, image_ref, image_digest,
                   tag, source, verification_status, evidence_json, created_at, updated_at,
                   status_changed_at, status_changed_by, status_reason
            FROM registry_evidence
            WHERE (?1 IS NULL OR release_id = ?1)
              AND (?2 IS NULL OR deployment_intent_id = ?2)
              AND (?3 IS NULL OR pipeline_intent_id = ?3)
              AND (?4 IS NULL OR change_set_id = ?4)
              AND (?5 IS NULL OR work_plan_id = ?5)
              AND (?6 IS NULL OR remediation_plan_id = ?6)
              AND (?7 IS NULL OR incident_id = ?7)
              AND (?8 IS NULL OR run_id = ?8)
              AND (?9 IS NULL OR status = ?9)
              AND (?10 IS NULL OR risk_level = ?10)
              AND (?11 IS NULL OR registry = ?11)
              AND (?12 IS NULL OR repository = ?12)
              AND (?13 IS NULL OR image_ref = ?13)
              AND (?14 IS NULL OR image_digest = ?14)
              AND (?15 IS NULL OR tag = ?15)
              AND (?16 IS NULL OR source = ?16)
              AND (?17 IS NULL OR verification_status = ?17)
              AND (?18 IS NULL OR CAST(created_at AS INTEGER) >= ?18)
              AND (?19 IS NULL OR CAST(created_at AS INTEGER) <= ?19)
            ORDER BY created_at DESC, id DESC
            LIMIT ?20 OFFSET ?21
            "#,
        )
        .bind(filter.release_id)
        .bind(filter.deployment_intent_id)
        .bind(filter.pipeline_intent_id)
        .bind(filter.change_set_id)
        .bind(filter.work_plan_id)
        .bind(filter.remediation_plan_id)
        .bind(filter.incident_id)
        .bind(filter.run_id.as_ref().map(RunId::as_str))
        .bind(filter.status)
        .bind(filter.risk_level)
        .bind(filter.registry)
        .bind(filter.repository)
        .bind(filter.image_ref)
        .bind(filter.image_digest)
        .bind(filter.tag)
        .bind(filter.source)
        .bind(filter.verification_status)
        .bind(filter.created_after_ms)
        .bind(filter.created_before_ms)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_registry_evidence).collect()
    }

    pub async fn update_registry_evidence_status(
        &self,
        evidence_id: &str,
        status: &str,
        actor: Option<String>,
        reason: Option<String>,
    ) -> Result<StoredRegistryEvidence, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            UPDATE registry_evidence
            SET status = ?2,
                updated_at = ?3,
                status_changed_at = ?3,
                status_changed_by = ?4,
                status_reason = ?5
            WHERE id = ?1
            "#,
        )
        .bind(evidence_id)
        .bind(status)
        .bind(now)
        .bind(actor)
        .bind(reason)
        .execute(&self.pool)
        .await?;

        self.get_registry_evidence(evidence_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "registry_evidence".to_string(),
                id: evidence_id.to_string(),
            })
    }

    pub async fn revise_registry_evidence_draft(
        &self,
        evidence_id: &str,
        revision: UpdateRegistryEvidenceDraft,
    ) -> Result<StoredRegistryEvidence, StoreError> {
        let now = now_string();
        let evidence_json = serde_json::to_string(&revision.evidence_json)?;
        sqlx::query(
            r#"
            UPDATE registry_evidence
            SET status = 'proposed',
                title = ?2,
                summary = ?3,
                risk_level = ?4,
                registry = ?5,
                repository = ?6,
                image_ref = ?7,
                image_digest = ?8,
                tag = ?9,
                source = ?10,
                verification_status = ?11,
                evidence_json = ?12,
                updated_at = ?13,
                status_changed_at = ?13,
                status_changed_by = ?14,
                status_reason = ?15
            WHERE id = ?1
            "#,
        )
        .bind(evidence_id)
        .bind(revision.title)
        .bind(revision.summary)
        .bind(revision.risk_level)
        .bind(revision.registry)
        .bind(revision.repository)
        .bind(revision.image_ref)
        .bind(revision.image_digest)
        .bind(revision.tag)
        .bind(revision.source)
        .bind(revision.verification_status)
        .bind(evidence_json)
        .bind(now)
        .bind(revision.actor)
        .bind(revision.reason)
        .execute(&self.pool)
        .await?;

        self.get_registry_evidence(evidence_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "registry_evidence".to_string(),
                id: evidence_id.to_string(),
            })
    }

    pub async fn create_approval_gate(
        &self,
        gate: CreateApprovalGate,
    ) -> Result<StoredApprovalGate, StoreError> {
        let now = now_string();
        let gate_json = serde_json::to_string(&gate.gate_json)?;
        sqlx::query(
            r#"
            INSERT INTO approval_gates (
              id, remediation_plan_id, incident_id, session_id, run_id, status, gate_kind,
              gate_order, title, summary, risk_level, resource_namespace, resource_kind,
              resource_name, gate_json, created_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
            "#,
        )
        .bind(&gate.id)
        .bind(&gate.remediation_plan_id)
        .bind(&gate.incident_id)
        .bind(gate.session_id.as_str())
        .bind(gate.run_id.as_ref().map(RunId::as_str))
        .bind(&gate.status)
        .bind(&gate.gate_kind)
        .bind(gate.gate_order)
        .bind(&gate.title)
        .bind(&gate.summary)
        .bind(&gate.risk_level)
        .bind(gate.resource_namespace)
        .bind(gate.resource_kind)
        .bind(gate.resource_name)
        .bind(gate_json)
        .bind(now)
        .execute(&self.pool)
        .await?;

        self.get_approval_gate(&gate.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "approval_gate".to_string(),
                id: gate.id,
            })
    }

    pub async fn get_approval_gate(
        &self,
        gate_id: &str,
    ) -> Result<Option<StoredApprovalGate>, StoreError> {
        let row = sqlx::query(approval_gate_select_sql("WHERE id = ?1"))
            .bind(gate_id)
            .fetch_optional(&self.pool)
            .await?;

        row.map(row_to_approval_gate).transpose()
    }

    pub async fn decide_approval_gate(
        &self,
        gate_id: &str,
        status: &str,
        decided_by: Option<String>,
        decision_reason: Option<String>,
    ) -> Result<StoredApprovalGate, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            UPDATE approval_gates
            SET status = ?2,
                decided_at = ?3,
                decided_by = ?4,
                decision_reason = ?5
            WHERE id = ?1
            "#,
        )
        .bind(gate_id)
        .bind(status)
        .bind(now)
        .bind(decided_by)
        .bind(decision_reason)
        .execute(&self.pool)
        .await?;

        self.get_approval_gate(gate_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "approval_gate".to_string(),
                id: gate_id.to_string(),
            })
    }

    pub async fn list_approval_gates(
        &self,
        filter: ApprovalGateListFilter,
    ) -> Result<Vec<StoredApprovalGate>, StoreError> {
        let limit = i64::from(filter.limit.clamp(1, 200));
        let offset = i64::from(filter.offset);
        let rows = sqlx::query(
            r#"
            SELECT id, remediation_plan_id, incident_id, session_id, run_id, status, gate_kind,
                   gate_order, title, summary, risk_level, resource_namespace, resource_kind,
                   resource_name, gate_json, created_at, decided_at, decided_by, decision_reason,
                   stale_at, stale_by, stale_reason
            FROM approval_gates
            WHERE (?1 IS NULL OR remediation_plan_id = ?1)
              AND (?2 IS NULL OR incident_id = ?2)
              AND (?3 IS NULL OR run_id = ?3)
              AND (?4 IS NULL OR status = ?4)
              AND (?5 IS NULL OR gate_kind = ?5)
              AND (?6 IS NULL OR risk_level = ?6)
              AND (?7 IS NULL OR resource_namespace = ?7)
              AND (?8 IS NULL OR resource_kind = ?8)
              AND (?9 IS NULL OR resource_name = ?9)
              AND (?10 IS NULL OR CAST(created_at AS INTEGER) >= ?10)
              AND (?11 IS NULL OR CAST(created_at AS INTEGER) <= ?11)
            ORDER BY created_at DESC, remediation_plan_id DESC, gate_order ASC, id ASC
            LIMIT ?12 OFFSET ?13
            "#,
        )
        .bind(filter.remediation_plan_id)
        .bind(filter.incident_id)
        .bind(filter.run_id.as_ref().map(RunId::as_str))
        .bind(filter.status)
        .bind(filter.gate_kind)
        .bind(filter.risk_level)
        .bind(filter.resource_namespace)
        .bind(filter.resource_kind)
        .bind(filter.resource_name)
        .bind(filter.created_after_ms)
        .bind(filter.created_before_ms)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_approval_gate).collect()
    }

    pub async fn stale_approval_gates_for_remediation_plan(
        &self,
        remediation_plan_id: &str,
        stale_by: Option<String>,
        stale_reason: Option<String>,
    ) -> Result<Vec<StoredApprovalGate>, StoreError> {
        let gates = self
            .list_approval_gates(ApprovalGateListFilter {
                remediation_plan_id: Some(remediation_plan_id.to_string()),
                status: None,
                limit: 200,
                ..ApprovalGateListFilter::default()
            })
            .await?
            .into_iter()
            .filter(|gate| matches!(gate.status.as_str(), "satisfied" | "waived"))
            .collect::<Vec<_>>();

        let mut staled = Vec::with_capacity(gates.len());
        for gate in gates {
            staled.push(
                self.stale_approval_gate(&gate.id, stale_by.clone(), stale_reason.clone())
                    .await?,
            );
        }

        Ok(staled)
    }

    async fn stale_approval_gate(
        &self,
        gate_id: &str,
        stale_by: Option<String>,
        stale_reason: Option<String>,
    ) -> Result<StoredApprovalGate, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            UPDATE approval_gates
            SET status = 'stale',
                stale_at = ?2,
                stale_by = ?3,
                stale_reason = ?4
            WHERE id = ?1
            "#,
        )
        .bind(gate_id)
        .bind(now)
        .bind(stale_by)
        .bind(stale_reason)
        .execute(&self.pool)
        .await?;

        self.get_approval_gate(gate_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "approval_gate".to_string(),
                id: gate_id.to_string(),
            })
    }

    pub async fn approval_gate_summary(
        &self,
        filter: ApprovalGateSummaryFilter,
    ) -> Result<ApprovalGateSummary, StoreError> {
        let total = approval_gate_summary_total(&self.pool, &filter).await?;
        let by_status = approval_gate_summary_text_buckets(&self.pool, &filter, "status").await?;
        let by_gate_kind =
            approval_gate_summary_text_buckets(&self.pool, &filter, "gate_kind").await?;
        let by_risk_level =
            approval_gate_summary_text_buckets(&self.pool, &filter, "risk_level").await?;
        let by_age_bucket = approval_gate_summary_age_buckets(&self.pool, &filter).await?;
        let by_resource_namespace =
            approval_gate_summary_text_buckets(&self.pool, &filter, "resource_namespace").await?;
        let by_resource_kind =
            approval_gate_summary_text_buckets(&self.pool, &filter, "resource_kind").await?;
        let by_resource_name =
            approval_gate_summary_text_buckets(&self.pool, &filter, "resource_name").await?;
        let by_incident_id =
            approval_gate_summary_text_buckets(&self.pool, &filter, "incident_id").await?;
        let by_remediation_plan_id =
            approval_gate_summary_text_buckets(&self.pool, &filter, "remediation_plan_id").await?;

        Ok(ApprovalGateSummary {
            total,
            by_status,
            by_gate_kind,
            by_risk_level,
            by_age_bucket,
            by_resource_namespace,
            by_resource_kind,
            by_resource_name,
            by_incident_id,
            by_remediation_plan_id,
        })
    }

    pub async fn list_events(&self, run_id: &RunId) -> Result<Vec<AgentEvent>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, session_id, run_id, seq, type, payload_json
            FROM events
            WHERE run_id = ?1
            ORDER BY seq ASC
            "#,
        )
        .bind(run_id.as_str())
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_event).collect()
    }

    pub async fn create_permission_grant(
        &self,
        grant: CreatePermissionGrant,
    ) -> Result<StoredPermissionGrant, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            INSERT INTO permission_grants (
              id, subject, status, reason, scope_json, policy_json, created_at, expires_at
            )
            VALUES (?1, ?2, 'active', ?3, ?4, ?5, ?6, ?7)
            "#,
        )
        .bind(&grant.id)
        .bind(&grant.subject)
        .bind(&grant.reason)
        .bind(serde_json::to_string(&grant.scope_json)?)
        .bind(serde_json::to_string(&grant.policy_json)?)
        .bind(now)
        .bind(grant.expires_at)
        .execute(&self.pool)
        .await?;

        self.get_permission_grant(&grant.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "permission grant".to_string(),
                id: grant.id,
            })
    }

    pub async fn get_permission_grant(
        &self,
        grant_id: &str,
    ) -> Result<Option<StoredPermissionGrant>, StoreError> {
        let row = sqlx::query(permission_grant_select_sql("WHERE id = ?1"))
            .bind(grant_id)
            .fetch_optional(&self.pool)
            .await?;

        row.map(row_to_permission_grant).transpose()
    }

    pub async fn list_permission_grants(
        &self,
        status: Option<&str>,
        limit: u32,
    ) -> Result<Vec<StoredPermissionGrant>, StoreError> {
        let limit = i64::from(limit.clamp(1, 200));
        let rows = match status {
            Some(status) => {
                sqlx::query(permission_grant_select_sql(
                    "WHERE status = ?1 ORDER BY created_at DESC LIMIT ?2",
                ))
                .bind(status)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx::query(permission_grant_select_sql(
                    "ORDER BY created_at DESC LIMIT ?1",
                ))
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
        };

        rows.into_iter().map(row_to_permission_grant).collect()
    }

    pub async fn revoke_permission_grant(
        &self,
        grant_id: &str,
        revoked_by: Option<String>,
        revoke_reason: Option<String>,
    ) -> Result<StoredPermissionGrant, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            UPDATE permission_grants
            SET status = 'revoked',
                revoked_at = ?2,
                revoked_by = ?3,
                revoke_reason = ?4
            WHERE id = ?1
            "#,
        )
        .bind(grant_id)
        .bind(now)
        .bind(revoked_by)
        .bind(revoke_reason)
        .execute(&self.pool)
        .await?;

        self.get_permission_grant(grant_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "permission grant".to_string(),
                id: grant_id.to_string(),
            })
    }

    pub async fn stale_permission_grant(
        &self,
        grant_id: &str,
        stale_by: Option<String>,
        stale_reason: Option<String>,
    ) -> Result<StoredPermissionGrant, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            UPDATE permission_grants
            SET status = 'stale',
                revoked_at = ?2,
                revoked_by = ?3,
                revoke_reason = ?4
            WHERE id = ?1
            "#,
        )
        .bind(grant_id)
        .bind(now)
        .bind(stale_by)
        .bind(stale_reason)
        .execute(&self.pool)
        .await?;

        self.get_permission_grant(grant_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "permission grant".to_string(),
                id: grant_id.to_string(),
            })
    }

    pub async fn create_audit_event(
        &self,
        event: CreateAuditEvent,
    ) -> Result<StoredAuditEvent, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            INSERT INTO audit_events (
              id, kind, actor, resource_kind, resource_id, run_id, payload_json, created_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
        )
        .bind(&event.id)
        .bind(&event.kind)
        .bind(event.actor)
        .bind(&event.resource_kind)
        .bind(&event.resource_id)
        .bind(event.run_id.as_ref().map(RunId::as_str))
        .bind(serde_json::to_string(&event.payload_json)?)
        .bind(now)
        .execute(&self.pool)
        .await?;

        self.get_audit_event(&event.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "audit event".to_string(),
                id: event.id,
            })
    }

    pub async fn get_audit_event(
        &self,
        event_id: &str,
    ) -> Result<Option<StoredAuditEvent>, StoreError> {
        let row = sqlx::query(audit_event_select_sql("WHERE id = ?1"))
            .bind(event_id)
            .fetch_optional(&self.pool)
            .await?;

        row.map(row_to_audit_event).transpose()
    }

    pub async fn list_audit_events(
        &self,
        resource_kind: Option<&str>,
        resource_id: Option<&str>,
        run_id: Option<&RunId>,
        limit: u32,
    ) -> Result<Vec<StoredAuditEvent>, StoreError> {
        let limit = i64::from(limit.clamp(1, 200));
        let rows = match (resource_kind, resource_id, run_id) {
            (Some(resource_kind), Some(resource_id), _) => sqlx::query(audit_event_select_sql(
                "WHERE resource_kind = ?1 AND resource_id = ?2 ORDER BY created_at DESC LIMIT ?3",
            ))
            .bind(resource_kind)
            .bind(resource_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?,
            (_, _, Some(run_id)) => {
                sqlx::query(audit_event_select_sql(
                    "WHERE run_id = ?1 ORDER BY created_at DESC LIMIT ?2",
                ))
                .bind(run_id.as_str())
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
            _ => {
                sqlx::query(audit_event_select_sql("ORDER BY created_at DESC LIMIT ?1"))
                    .bind(limit)
                    .fetch_all(&self.pool)
                    .await?
            }
        };

        rows.into_iter().map(row_to_audit_event).collect()
    }

    pub async fn query_audit_events(
        &self,
        filter: AuditEventListFilter,
    ) -> Result<Vec<StoredAuditEvent>, StoreError> {
        let limit = i64::from(filter.limit.clamp(1, 200));
        let production_impacting = filter.production_impacting.map(i64::from);
        let rows = sqlx::query(
            r#"
            SELECT ae.id, ae.kind, ae.actor, ae.resource_kind, ae.resource_id, ae.run_id,
                   ae.payload_json, ae.created_at
            FROM audit_events ae
            LEFT JOIN runs r ON r.id = ae.run_id
            WHERE (?1 IS NULL OR ae.kind = ?1)
              AND (?2 IS NULL OR ae.actor = ?2)
              AND (?3 IS NULL OR ae.resource_kind = ?3)
              AND (?4 IS NULL OR ae.resource_id = ?4)
              AND (?5 IS NULL OR ae.run_id = ?5)
              AND (?6 IS NULL OR COALESCE(
                    json_extract(ae.payload_json, '$.run_scope.namespace'),
                    json_extract(ae.payload_json, '$.scope.namespace'),
                    json_extract(ae.payload_json, '$.resource_namespace'),
                    json_extract(r.execution_target_json, '$.scope.namespace'),
                    CASE ae.resource_kind
                      WHEN 'work_plan' THEN (SELECT resource_namespace FROM work_plans WHERE id = ae.resource_id)
                      WHEN 'change_set' THEN (SELECT resource_namespace FROM change_sets WHERE id = ae.resource_id)
                      WHEN 'pipeline_intent' THEN (SELECT resource_namespace FROM pipeline_intents WHERE id = ae.resource_id)
                      WHEN 'deployment_intent' THEN (SELECT COALESCE(target_namespace, resource_namespace) FROM deployment_intents WHERE id = ae.resource_id)
                      WHEN 'release' THEN (SELECT target_namespace FROM releases WHERE id = ae.resource_id)
                      WHEN 'approval_gate' THEN (SELECT resource_namespace FROM approval_gates WHERE id = ae.resource_id)
                      WHEN 'observation' THEN (SELECT resource_namespace FROM observations WHERE id = ae.resource_id)
                      WHEN 'incident' THEN (SELECT resource_namespace FROM incidents WHERE id = ae.resource_id)
                      WHEN 'remediation_plan' THEN (SELECT resource_namespace FROM remediation_plans WHERE id = ae.resource_id)
                      WHEN 'permission_grant' THEN (SELECT json_extract(scope_json, '$.namespaces[0]') FROM permission_grants WHERE id = ae.resource_id)
                    END
                  ) = ?6)
              AND (?7 IS NULL OR COALESCE(
                    json_extract(ae.payload_json, '$.run_scope.repo'),
                    json_extract(ae.payload_json, '$.scope.repo'),
                    json_extract(r.execution_target_json, '$.scope.repo'),
                    CASE ae.resource_kind
                      WHEN 'permission_grant' THEN (SELECT json_extract(scope_json, '$.repos[0]') FROM permission_grants WHERE id = ae.resource_id)
                    END
                  ) = ?7)
              AND (?8 IS NULL OR COALESCE(
                    json_extract(ae.payload_json, '$.run_scope.branch'),
                    json_extract(ae.payload_json, '$.scope.branch'),
                    json_extract(r.execution_target_json, '$.scope.branch'),
                    CASE ae.resource_kind
                      WHEN 'permission_grant' THEN (SELECT json_extract(scope_json, '$.branches[0]') FROM permission_grants WHERE id = ae.resource_id)
                    END
                  ) = ?8)
              AND (?9 IS NULL OR COALESCE(
                    json_extract(ae.payload_json, '$.run_scope.production_impacting'),
                    json_extract(ae.payload_json, '$.scope.production_impacting'),
                    json_extract(r.execution_target_json, '$.scope.production_impacting'),
                    CASE ae.resource_kind
                      WHEN 'permission_grant' THEN (SELECT json_extract(scope_json, '$.production_impacting') FROM permission_grants WHERE id = ae.resource_id)
                    END
                  ) = ?9)
              AND (?10 IS NULL OR ae.kind LIKE '%' || ?10 || '%' COLLATE NOCASE
                    OR COALESCE(ae.actor, '') LIKE '%' || ?10 || '%' COLLATE NOCASE
                    OR ae.resource_kind LIKE '%' || ?10 || '%' COLLATE NOCASE
                    OR ae.resource_id LIKE '%' || ?10 || '%' COLLATE NOCASE
                    OR COALESCE(ae.run_id, '') LIKE '%' || ?10 || '%' COLLATE NOCASE
                    OR ae.payload_json LIKE '%' || ?10 || '%' COLLATE NOCASE)
            ORDER BY ae.created_at DESC, ae.id DESC
            LIMIT ?11
            "#,
        )
        .bind(filter.kind)
        .bind(filter.actor)
        .bind(filter.resource_kind)
        .bind(filter.resource_id)
        .bind(filter.run_id.as_ref().map(RunId::as_str))
        .bind(filter.namespace)
        .bind(filter.repo)
        .bind(filter.branch)
        .bind(production_impacting)
        .bind(filter.search)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_audit_event).collect()
    }
}

fn row_to_file_change(row: sqlx::sqlite::SqliteRow) -> Result<StoredFileChange, StoreError> {
    Ok(StoredFileChange {
        id: row.try_get("id")?,
        session_id: SessionId::new(row.try_get::<String, _>("session_id")?),
        run_id: RunId::new(row.try_get::<String, _>("run_id")?),
        path: row.try_get("path")?,
        before_hash: row.try_get("before_hash")?,
        after_hash: row.try_get("after_hash")?,
        diff: row.try_get("diff")?,
        created_at: row.try_get("created_at")?,
    })
}

fn row_to_artifact(row: sqlx::sqlite::SqliteRow) -> Result<StoredArtifact, StoreError> {
    let run_id: Option<String> = row.try_get("run_id")?;
    let content_json: Option<String> = row.try_get("content_json")?;
    Ok(StoredArtifact {
        id: row.try_get("id")?,
        session_id: SessionId::new(row.try_get::<String, _>("session_id")?),
        run_id: run_id.map(RunId::new),
        kind: row.try_get("kind")?,
        label: row.try_get("label")?,
        mime_type: row.try_get("mime_type")?,
        path: row.try_get("path")?,
        content_text: row.try_get("content_text")?,
        content_json: content_json
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
        created_at: row.try_get("created_at")?,
    })
}

fn row_to_observation(row: sqlx::sqlite::SqliteRow) -> Result<StoredObservation, StoreError> {
    let run_id: Option<String> = row.try_get("run_id")?;
    let resource_ref_json: Option<String> = row.try_get("resource_ref_json")?;
    let data_json: String = row.try_get("data_json")?;
    Ok(StoredObservation {
        id: row.try_get("id")?,
        session_id: SessionId::new(row.try_get::<String, _>("session_id")?),
        run_id: run_id.map(RunId::new),
        source: row.try_get("source")?,
        kind: row.try_get("kind")?,
        subject: row.try_get("subject")?,
        summary: row.try_get("summary")?,
        resource_namespace: row.try_get("resource_namespace")?,
        resource_kind: row.try_get("resource_kind")?,
        resource_name: row.try_get("resource_name")?,
        resource_ref_json: resource_ref_json
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
        artifact_id: row.try_get("artifact_id")?,
        data_json: serde_json::from_str(&data_json)?,
        observed_at: row.try_get("observed_at")?,
    })
}

fn row_to_incident(row: sqlx::sqlite::SqliteRow) -> Result<StoredIncident, StoreError> {
    let run_id: Option<String> = row.try_get("run_id")?;
    let data_json: String = row.try_get("data_json")?;
    Ok(StoredIncident {
        id: row.try_get("id")?,
        observation_id: row.try_get("observation_id")?,
        session_id: SessionId::new(row.try_get::<String, _>("session_id")?),
        run_id: run_id.map(RunId::new),
        status: row.try_get("status")?,
        severity: row.try_get("severity")?,
        title: row.try_get("title")?,
        summary: row.try_get("summary")?,
        resource_namespace: row.try_get("resource_namespace")?,
        resource_kind: row.try_get("resource_kind")?,
        resource_name: row.try_get("resource_name")?,
        data_json: serde_json::from_str(&data_json)?,
        created_at: row.try_get("created_at")?,
    })
}

fn row_to_remediation_plan(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredRemediationPlan, StoreError> {
    let run_id: Option<String> = row.try_get("run_id")?;
    let plan_json: String = row.try_get("plan_json")?;
    Ok(StoredRemediationPlan {
        id: row.try_get("id")?,
        incident_id: row.try_get("incident_id")?,
        session_id: SessionId::new(row.try_get::<String, _>("session_id")?),
        run_id: run_id.map(RunId::new),
        status: row.try_get("status")?,
        title: row.try_get("title")?,
        summary: row.try_get("summary")?,
        risk_level: row.try_get("risk_level")?,
        requires_approval: row.try_get::<i64, _>("requires_approval")? != 0,
        resource_namespace: row.try_get("resource_namespace")?,
        resource_kind: row.try_get("resource_kind")?,
        resource_name: row.try_get("resource_name")?,
        plan_json: serde_json::from_str(&plan_json)?,
        created_at: row.try_get("created_at")?,
    })
}

fn row_to_work_plan(row: sqlx::sqlite::SqliteRow) -> Result<StoredWorkPlan, StoreError> {
    let run_id: Option<String> = row.try_get("run_id")?;
    let work_plan_json: String = row.try_get("work_plan_json")?;
    Ok(StoredWorkPlan {
        id: row.try_get("id")?,
        work_item_id: row.try_get("work_item_id")?,
        remediation_plan_id: row.try_get("remediation_plan_id")?,
        incident_id: row.try_get("incident_id")?,
        session_id: SessionId::new(row.try_get::<String, _>("session_id")?),
        run_id: run_id.map(RunId::new),
        status: row.try_get("status")?,
        title: row.try_get("title")?,
        summary: row.try_get("summary")?,
        risk_level: row.try_get("risk_level")?,
        requires_approval: row.try_get::<i64, _>("requires_approval")? != 0,
        resource_namespace: row.try_get("resource_namespace")?,
        resource_kind: row.try_get("resource_kind")?,
        resource_name: row.try_get("resource_name")?,
        work_plan_json: serde_json::from_str(&work_plan_json)?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        revision: row.try_get("revision")?,
        status_changed_at: row.try_get("status_changed_at")?,
        status_changed_by: row.try_get("status_changed_by")?,
        status_reason: row.try_get("status_reason")?,
    })
}

fn row_to_work_item(row: sqlx::sqlite::SqliteRow) -> Result<StoredWorkItem, StoreError> {
    let acceptance_criteria_json: String = row.try_get("acceptance_criteria_json")?;
    let current_run_id: Option<String> = row.try_get("current_run_id")?;
    let max_attempts: i64 = row.try_get("max_attempts")?;
    let max_elapsed_seconds: i64 = row.try_get("max_elapsed_seconds")?;
    let attempt_count: i64 = row.try_get("attempt_count")?;
    Ok(StoredWorkItem {
        id: row.try_get("id")?,
        status: row.try_get("status")?,
        title: row.try_get("title")?,
        intent: row.try_get("intent")?,
        acceptance_criteria: serde_json::from_str(&acceptance_criteria_json)?,
        source_repo: row.try_get("source_repo")?,
        source_ref: row.try_get("source_ref")?,
        gitops_repo: row.try_get("gitops_repo")?,
        gitops_ref: row.try_get("gitops_ref")?,
        target_environment: row.try_get("target_environment")?,
        target_namespace: row.try_get("target_namespace")?,
        argo_application: row.try_get("argo_application")?,
        production_impacting: row.try_get::<i64, _>("production_impacting")? != 0,
        max_attempts: u32::try_from(max_attempts)
            .map_err(|error| StoreError::InvalidData(error.to_string()))?,
        max_elapsed_seconds: u64::try_from(max_elapsed_seconds)
            .map_err(|error| StoreError::InvalidData(error.to_string()))?,
        attempt_count: u32::try_from(attempt_count)
            .map_err(|error| StoreError::InvalidData(error.to_string()))?,
        current_run_id: current_run_id.map(RunId::new),
        created_by: row.try_get("created_by")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        status_changed_at: row.try_get("status_changed_at")?,
        status_changed_by: row.try_get("status_changed_by")?,
        status_reason: row.try_get("status_reason")?,
    })
}

fn row_to_workspace(row: sqlx::sqlite::SqliteRow) -> Result<StoredWorkspace, StoreError> {
    let run_id: Option<String> = row.try_get("run_id")?;
    Ok(StoredWorkspace {
        id: row.try_get("id")?,
        work_item_id: row.try_get("work_item_id")?,
        run_id: run_id.map(RunId::new),
        status: row.try_get("status")?,
        source_repo: row.try_get("source_repo")?,
        source_ref: row.try_get("source_ref")?,
        resolved_commit: row.try_get("resolved_commit")?,
        branch: row.try_get("branch")?,
        retention_status: row.try_get("retention_status")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        status_changed_at: row.try_get("status_changed_at")?,
        status_changed_by: row.try_get("status_changed_by")?,
        status_reason: row.try_get("status_reason")?,
    })
}

fn row_to_change_set(row: sqlx::sqlite::SqliteRow) -> Result<StoredChangeSet, StoreError> {
    let run_id: Option<String> = row.try_get("run_id")?;
    let change_set_json: String = row.try_get("change_set_json")?;
    Ok(StoredChangeSet {
        id: row.try_get("id")?,
        work_plan_id: row.try_get("work_plan_id")?,
        remediation_plan_id: row.try_get("remediation_plan_id")?,
        incident_id: row.try_get("incident_id")?,
        session_id: SessionId::new(row.try_get::<String, _>("session_id")?),
        run_id: run_id.map(RunId::new),
        status: row.try_get("status")?,
        title: row.try_get("title")?,
        summary: row.try_get("summary")?,
        risk_level: row.try_get("risk_level")?,
        material_hash: row.try_get("material_hash")?,
        revision: row.try_get("revision")?,
        resource_namespace: row.try_get("resource_namespace")?,
        resource_kind: row.try_get("resource_kind")?,
        resource_name: row.try_get("resource_name")?,
        change_set_json: serde_json::from_str(&change_set_json)?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        status_changed_at: row.try_get("status_changed_at")?,
        status_changed_by: row.try_get("status_changed_by")?,
        status_reason: row.try_get("status_reason")?,
    })
}

fn row_to_pipeline_intent(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredPipelineIntent, StoreError> {
    let run_id: Option<String> = row.try_get("run_id")?;
    let intent_json: String = row.try_get("intent_json")?;
    Ok(StoredPipelineIntent {
        id: row.try_get("id")?,
        change_set_id: row.try_get("change_set_id")?,
        work_plan_id: row.try_get("work_plan_id")?,
        remediation_plan_id: row.try_get("remediation_plan_id")?,
        incident_id: row.try_get("incident_id")?,
        session_id: SessionId::new(row.try_get::<String, _>("session_id")?),
        run_id: run_id.map(RunId::new),
        status: row.try_get("status")?,
        title: row.try_get("title")?,
        summary: row.try_get("summary")?,
        risk_level: row.try_get("risk_level")?,
        intent_kind: row.try_get("intent_kind")?,
        resource_namespace: row.try_get("resource_namespace")?,
        resource_kind: row.try_get("resource_kind")?,
        resource_name: row.try_get("resource_name")?,
        intent_json: serde_json::from_str(&intent_json)?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        status_changed_at: row.try_get("status_changed_at")?,
        status_changed_by: row.try_get("status_changed_by")?,
        status_reason: row.try_get("status_reason")?,
    })
}

fn row_to_pipeline_contract(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredPipelineContract, StoreError> {
    let contract_json: String = row.try_get("contract_json")?;
    Ok(StoredPipelineContract {
        id: row.try_get("id")?,
        status: row.try_get("status")?,
        namespace: row.try_get("namespace")?,
        pipeline_ref: row.try_get("pipeline_ref")?,
        version: row.try_get("version")?,
        contract_json: serde_json::from_str(&contract_json)?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        status_changed_at: row.try_get("status_changed_at")?,
        status_changed_by: row.try_get("status_changed_by")?,
        status_reason: row.try_get("status_reason")?,
    })
}

fn row_to_deployment_contract(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredDeploymentContract, StoreError> {
    let contract_json: String = row.try_get("contract_json")?;
    Ok(StoredDeploymentContract {
        id: row.try_get("id")?,
        status: row.try_get("status")?,
        target_environment: row.try_get("target_environment")?,
        target_namespace: row.try_get("target_namespace")?,
        argo_application: row.try_get("argo_application")?,
        version: row.try_get("version")?,
        contract_json: serde_json::from_str(&contract_json)?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        status_changed_at: row.try_get("status_changed_at")?,
        status_changed_by: row.try_get("status_changed_by")?,
        status_reason: row.try_get("status_reason")?,
    })
}

fn row_to_deployment_intent(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredDeploymentIntent, StoreError> {
    let run_id: Option<String> = row.try_get("run_id")?;
    let intent_json: String = row.try_get("intent_json")?;
    Ok(StoredDeploymentIntent {
        id: row.try_get("id")?,
        pipeline_intent_id: row.try_get("pipeline_intent_id")?,
        change_set_id: row.try_get("change_set_id")?,
        work_plan_id: row.try_get("work_plan_id")?,
        remediation_plan_id: row.try_get("remediation_plan_id")?,
        incident_id: row.try_get("incident_id")?,
        session_id: SessionId::new(row.try_get::<String, _>("session_id")?),
        run_id: run_id.map(RunId::new),
        status: row.try_get("status")?,
        title: row.try_get("title")?,
        summary: row.try_get("summary")?,
        risk_level: row.try_get("risk_level")?,
        intent_kind: row.try_get("intent_kind")?,
        target_environment: row.try_get("target_environment")?,
        target_namespace: row.try_get("target_namespace")?,
        argo_application: row.try_get("argo_application")?,
        resource_namespace: row.try_get("resource_namespace")?,
        resource_kind: row.try_get("resource_kind")?,
        resource_name: row.try_get("resource_name")?,
        intent_json: serde_json::from_str(&intent_json)?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        status_changed_at: row.try_get("status_changed_at")?,
        status_changed_by: row.try_get("status_changed_by")?,
        status_reason: row.try_get("status_reason")?,
    })
}

fn row_to_release(row: sqlx::sqlite::SqliteRow) -> Result<StoredRelease, StoreError> {
    let run_id: Option<String> = row.try_get("run_id")?;
    let release_json: String = row.try_get("release_json")?;
    Ok(StoredRelease {
        id: row.try_get("id")?,
        deployment_intent_id: row.try_get("deployment_intent_id")?,
        pipeline_intent_id: row.try_get("pipeline_intent_id")?,
        change_set_id: row.try_get("change_set_id")?,
        work_plan_id: row.try_get("work_plan_id")?,
        remediation_plan_id: row.try_get("remediation_plan_id")?,
        incident_id: row.try_get("incident_id")?,
        session_id: SessionId::new(row.try_get::<String, _>("session_id")?),
        run_id: run_id.map(RunId::new),
        status: row.try_get("status")?,
        title: row.try_get("title")?,
        summary: row.try_get("summary")?,
        risk_level: row.try_get("risk_level")?,
        release_kind: row.try_get("release_kind")?,
        target_environment: row.try_get("target_environment")?,
        target_namespace: row.try_get("target_namespace")?,
        argo_application: row.try_get("argo_application")?,
        version: row.try_get("version")?,
        commit_sha: row.try_get("commit_sha")?,
        image_digest: row.try_get("image_digest")?,
        rollback_ref: row.try_get("rollback_ref")?,
        release_json: serde_json::from_str(&release_json)?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        status_changed_at: row.try_get("status_changed_at")?,
        status_changed_by: row.try_get("status_changed_by")?,
        status_reason: row.try_get("status_reason")?,
    })
}

fn row_to_registry_evidence(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredRegistryEvidence, StoreError> {
    let run_id: Option<String> = row.try_get("run_id")?;
    let evidence_json: String = row.try_get("evidence_json")?;
    Ok(StoredRegistryEvidence {
        id: row.try_get("id")?,
        release_id: row.try_get("release_id")?,
        deployment_intent_id: row.try_get("deployment_intent_id")?,
        pipeline_intent_id: row.try_get("pipeline_intent_id")?,
        change_set_id: row.try_get("change_set_id")?,
        work_plan_id: row.try_get("work_plan_id")?,
        remediation_plan_id: row.try_get("remediation_plan_id")?,
        incident_id: row.try_get("incident_id")?,
        session_id: SessionId::new(row.try_get::<String, _>("session_id")?),
        run_id: run_id.map(RunId::new),
        status: row.try_get("status")?,
        title: row.try_get("title")?,
        summary: row.try_get("summary")?,
        risk_level: row.try_get("risk_level")?,
        registry: row.try_get("registry")?,
        repository: row.try_get("repository")?,
        image_ref: row.try_get("image_ref")?,
        image_digest: row.try_get("image_digest")?,
        tag: row.try_get("tag")?,
        source: row.try_get("source")?,
        verification_status: row.try_get("verification_status")?,
        evidence_json: serde_json::from_str(&evidence_json)?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        status_changed_at: row.try_get("status_changed_at")?,
        status_changed_by: row.try_get("status_changed_by")?,
        status_reason: row.try_get("status_reason")?,
    })
}

fn row_to_approval_gate(row: sqlx::sqlite::SqliteRow) -> Result<StoredApprovalGate, StoreError> {
    let run_id: Option<String> = row.try_get("run_id")?;
    let gate_json: String = row.try_get("gate_json")?;
    Ok(StoredApprovalGate {
        id: row.try_get("id")?,
        remediation_plan_id: row.try_get("remediation_plan_id")?,
        incident_id: row.try_get("incident_id")?,
        session_id: SessionId::new(row.try_get::<String, _>("session_id")?),
        run_id: run_id.map(RunId::new),
        status: row.try_get("status")?,
        gate_kind: row.try_get("gate_kind")?,
        gate_order: row.try_get("gate_order")?,
        title: row.try_get("title")?,
        summary: row.try_get("summary")?,
        risk_level: row.try_get("risk_level")?,
        resource_namespace: row.try_get("resource_namespace")?,
        resource_kind: row.try_get("resource_kind")?,
        resource_name: row.try_get("resource_name")?,
        gate_json: serde_json::from_str(&gate_json)?,
        created_at: row.try_get("created_at")?,
        decided_at: row.try_get("decided_at")?,
        decided_by: row.try_get("decided_by")?,
        decision_reason: row.try_get("decision_reason")?,
        stale_at: row.try_get("stale_at")?,
        stale_by: row.try_get("stale_by")?,
        stale_reason: row.try_get("stale_reason")?,
    })
}

fn artifact_select_sql(where_clause: &str) -> &'static str {
    match where_clause {
        "WHERE id = ?1" => {
            r#"
            SELECT id, session_id, run_id, kind, label, mime_type, path,
                   content_text, content_json, created_at
            FROM artifacts
            WHERE id = ?1
            "#
        }
        "WHERE run_id = ?1 ORDER BY created_at ASC, id ASC" => {
            r#"
            SELECT id, session_id, run_id, kind, label, mime_type, path,
                   content_text, content_json, created_at
            FROM artifacts
            WHERE run_id = ?1
            ORDER BY created_at ASC, id ASC
            "#
        }
        _ => unreachable!("artifact select SQL only supports known static clauses"),
    }
}

fn observation_select_sql(where_clause: &str) -> &'static str {
    match where_clause {
        "WHERE id = ?1" => {
            r#"
            SELECT id, session_id, run_id, source, kind, subject, summary,
                   resource_namespace, resource_kind, resource_name,
                   resource_ref_json, artifact_id, data_json, observed_at
            FROM observations
            WHERE id = ?1
            "#
        }
        "WHERE run_id = ?1 ORDER BY observed_at ASC, id ASC" => {
            r#"
            SELECT id, session_id, run_id, source, kind, subject, summary,
                   resource_namespace, resource_kind, resource_name,
                   resource_ref_json, artifact_id, data_json, observed_at
            FROM observations
            WHERE run_id = ?1
            ORDER BY observed_at ASC, id ASC
            "#
        }
        _ => unreachable!("observation select SQL only supports known static clauses"),
    }
}

fn incident_select_sql(where_clause: &str) -> &'static str {
    match where_clause {
        "WHERE id = ?1" => {
            r#"
            SELECT id, observation_id, session_id, run_id, status, severity, title, summary,
                   resource_namespace, resource_kind, resource_name, data_json, created_at
            FROM incidents
            WHERE id = ?1
            "#
        }
        _ => unreachable!("incident select SQL only supports known static clauses"),
    }
}

fn remediation_plan_select_sql(where_clause: &str) -> &'static str {
    match where_clause {
        "WHERE id = ?1" => {
            r#"
            SELECT id, incident_id, session_id, run_id, status, title, summary, risk_level,
                   requires_approval, resource_namespace, resource_kind, resource_name,
                   plan_json, created_at
            FROM remediation_plans
            WHERE id = ?1
            "#
        }
        _ => unreachable!("remediation plan select SQL only supports known static clauses"),
    }
}

fn work_item_select_sql(where_clause: &str) -> &'static str {
    match where_clause {
        "WHERE id = ?1" => {
            r#"
            SELECT id, status, title, intent, acceptance_criteria_json, source_repo, source_ref,
                   gitops_repo, gitops_ref, target_environment, target_namespace,
                   argo_application, production_impacting, max_attempts, max_elapsed_seconds,
                   attempt_count, current_run_id, created_by, created_at, updated_at,
                   status_changed_at, status_changed_by, status_reason
            FROM work_items
            WHERE id = ?1
            "#
        }
        _ => unreachable!("work item select SQL only supports known static clauses"),
    }
}

fn workspace_select_sql(where_clause: &str) -> &'static str {
    match where_clause {
        "WHERE id = ?1" => {
            r#"
            SELECT id, work_item_id, run_id, status, source_repo, source_ref, resolved_commit,
                   branch, retention_status, created_at, updated_at, status_changed_at,
                   status_changed_by, status_reason
            FROM workspaces
            WHERE id = ?1
            "#
        }
        _ => unreachable!("workspace select SQL only supports known static clauses"),
    }
}

fn work_plan_select_sql(where_clause: &str) -> &'static str {
    match where_clause {
        "WHERE id = ?1" => {
            r#"
            SELECT id, work_item_id, remediation_plan_id, incident_id, session_id, run_id,
                   status, title, summary, risk_level, requires_approval, resource_namespace,
                   resource_kind, resource_name, work_plan_json, created_at, updated_at,
                   revision, status_changed_at, status_changed_by, status_reason
            FROM work_plans
            WHERE id = ?1
            "#
        }
        "WHERE remediation_plan_id = ?1" => {
            r#"
            SELECT id, work_item_id, remediation_plan_id, incident_id, session_id, run_id,
                   status, title, summary, risk_level, requires_approval, resource_namespace,
                   resource_kind, resource_name, work_plan_json, created_at, updated_at,
                   revision, status_changed_at, status_changed_by, status_reason
            FROM work_plans
            WHERE remediation_plan_id = ?1
            "#
        }
        "WHERE work_item_id = ?1" => {
            r#"
            SELECT id, work_item_id, remediation_plan_id, incident_id, session_id, run_id,
                   status, title, summary, risk_level, requires_approval, resource_namespace,
                   resource_kind, resource_name, work_plan_json, created_at, updated_at,
                   revision, status_changed_at, status_changed_by, status_reason
            FROM work_plans
            WHERE work_item_id = ?1
            "#
        }
        _ => unreachable!("work plan select SQL only supports known static clauses"),
    }
}

fn change_set_select_sql(where_clause: &str) -> &'static str {
    match where_clause {
        "WHERE id = ?1" => {
            r#"
            SELECT id, work_plan_id, remediation_plan_id, incident_id, session_id, run_id,
                   status, title, summary, risk_level, material_hash, revision,
                   resource_namespace, resource_kind, resource_name, change_set_json, created_at,
                   updated_at, status_changed_at, status_changed_by, status_reason
            FROM change_sets
            WHERE id = ?1
            "#
        }
        "WHERE work_plan_id = ?1" => {
            r#"
            SELECT id, work_plan_id, remediation_plan_id, incident_id, session_id, run_id,
                   status, title, summary, risk_level, material_hash, revision,
                   resource_namespace, resource_kind, resource_name, change_set_json, created_at,
                   updated_at, status_changed_at, status_changed_by, status_reason
            FROM change_sets
            WHERE work_plan_id = ?1
            "#
        }
        _ => unreachable!("change set select SQL only supports known static clauses"),
    }
}

fn pipeline_intent_select_sql(where_clause: &str) -> &'static str {
    match where_clause {
        "WHERE id = ?1" => {
            r#"
            SELECT id, change_set_id, work_plan_id, remediation_plan_id, incident_id, session_id,
                   run_id, status, title, summary, risk_level, intent_kind, resource_namespace,
                   resource_kind, resource_name, intent_json, created_at, updated_at,
                   status_changed_at, status_changed_by, status_reason
            FROM pipeline_intents
            WHERE id = ?1
            "#
        }
        "WHERE change_set_id = ?1" => {
            r#"
            SELECT id, change_set_id, work_plan_id, remediation_plan_id, incident_id, session_id,
                   run_id, status, title, summary, risk_level, intent_kind, resource_namespace,
                   resource_kind, resource_name, intent_json, created_at, updated_at,
                   status_changed_at, status_changed_by, status_reason
            FROM pipeline_intents
            WHERE change_set_id = ?1
            "#
        }
        _ => unreachable!("pipeline intent select SQL only supports known static clauses"),
    }
}

fn pipeline_contract_select_sql(where_clause: &str) -> &'static str {
    match where_clause {
        "WHERE id = ?1" => {
            r#"
            SELECT id, status, namespace, pipeline_ref, version, contract_json, created_at,
                   updated_at, status_changed_at, status_changed_by, status_reason
            FROM pipeline_contracts
            WHERE id = ?1
            "#
        }
        _ => unreachable!("pipeline contract select SQL only supports known static clauses"),
    }
}

fn deployment_contract_select_sql(where_clause: &str) -> &'static str {
    match where_clause {
        "WHERE id = ?1" => {
            r#"
            SELECT id, status, target_environment, target_namespace, argo_application, version,
                   contract_json, created_at, updated_at, status_changed_at, status_changed_by,
                   status_reason
            FROM deployment_contracts
            WHERE id = ?1
        "#
        }
        _ => unreachable!("deployment contract select SQL only supports known static clauses"),
    }
}

fn deployment_intent_select_sql(where_clause: &str) -> &'static str {
    match where_clause {
        "WHERE id = ?1" => {
            r#"
            SELECT id, pipeline_intent_id, change_set_id, work_plan_id, remediation_plan_id,
                   incident_id, session_id, run_id, status, title, summary, risk_level,
                   intent_kind, target_environment, target_namespace, argo_application,
                   resource_namespace, resource_kind, resource_name, intent_json, created_at,
                   updated_at, status_changed_at, status_changed_by, status_reason
            FROM deployment_intents
            WHERE id = ?1
            "#
        }
        "WHERE pipeline_intent_id = ?1" => {
            r#"
            SELECT id, pipeline_intent_id, change_set_id, work_plan_id, remediation_plan_id,
                   incident_id, session_id, run_id, status, title, summary, risk_level,
                   intent_kind, target_environment, target_namespace, argo_application,
                   resource_namespace, resource_kind, resource_name, intent_json, created_at,
                   updated_at, status_changed_at, status_changed_by, status_reason
            FROM deployment_intents
            WHERE pipeline_intent_id = ?1
            "#
        }
        _ => unreachable!("deployment intent select SQL only supports known static clauses"),
    }
}

fn release_select_sql(where_clause: &str) -> &'static str {
    match where_clause {
        "WHERE id = ?1" => {
            r#"
            SELECT id, deployment_intent_id, pipeline_intent_id, change_set_id, work_plan_id,
                   remediation_plan_id, incident_id, session_id, run_id, status, title, summary,
                   risk_level, release_kind, target_environment, target_namespace,
                   argo_application, version, commit_sha, image_digest, rollback_ref,
                   release_json, created_at, updated_at, status_changed_at, status_changed_by,
                   status_reason
            FROM releases
            WHERE id = ?1
            "#
        }
        "WHERE deployment_intent_id = ?1" => {
            r#"
            SELECT id, deployment_intent_id, pipeline_intent_id, change_set_id, work_plan_id,
                   remediation_plan_id, incident_id, session_id, run_id, status, title, summary,
                   risk_level, release_kind, target_environment, target_namespace,
                   argo_application, version, commit_sha, image_digest, rollback_ref,
                   release_json, created_at, updated_at, status_changed_at, status_changed_by,
                   status_reason
            FROM releases
            WHERE deployment_intent_id = ?1
            "#
        }
        _ => unreachable!("release select SQL only supports known static clauses"),
    }
}

fn registry_evidence_select_sql(where_clause: &str) -> &'static str {
    match where_clause {
        "WHERE id = ?1" => {
            r#"
            SELECT id, release_id, deployment_intent_id, pipeline_intent_id, change_set_id,
                   work_plan_id, remediation_plan_id, incident_id, session_id, run_id, status,
                   title, summary, risk_level, registry, repository, image_ref, image_digest,
                   tag, source, verification_status, evidence_json, created_at, updated_at,
                   status_changed_at, status_changed_by, status_reason
            FROM registry_evidence
            WHERE id = ?1
            "#
        }
        "WHERE release_id = ?1" => {
            r#"
            SELECT id, release_id, deployment_intent_id, pipeline_intent_id, change_set_id,
                   work_plan_id, remediation_plan_id, incident_id, session_id, run_id, status,
                   title, summary, risk_level, registry, repository, image_ref, image_digest,
                   tag, source, verification_status, evidence_json, created_at, updated_at,
                   status_changed_at, status_changed_by, status_reason
            FROM registry_evidence
            WHERE release_id = ?1
            "#
        }
        _ => unreachable!("registry evidence select SQL only supports known static clauses"),
    }
}

fn approval_gate_select_sql(where_clause: &str) -> &'static str {
    match where_clause {
        "WHERE id = ?1" => {
            r#"
            SELECT id, remediation_plan_id, incident_id, session_id, run_id, status, gate_kind,
                   gate_order, title, summary, risk_level, resource_namespace, resource_kind,
                   resource_name, gate_json, created_at, decided_at, decided_by, decision_reason,
                   stale_at, stale_by, stale_reason
            FROM approval_gates
            WHERE id = ?1
            "#
        }
        _ => unreachable!("approval gate select SQL only supports known static clauses"),
    }
}

fn row_to_run(row: sqlx::sqlite::SqliteRow) -> Result<StoredRun, StoreError> {
    let execution_target_json: String = row.try_get("execution_target_json")?;
    let result_json: Option<String> = row.try_get("result_json")?;
    Ok(StoredRun {
        id: RunId::new(row.try_get::<String, _>("id")?),
        session_id: SessionId::new(row.try_get::<String, _>("session_id")?),
        cwd: row.try_get("cwd")?,
        status: row.try_get("status")?,
        user_task: row.try_get("user_task")?,
        max_turns: row.try_get::<i64, _>("max_turns")? as u32,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
        cancel_requested_at: row.try_get("cancel_requested_at")?,
        error: row.try_get("error")?,
        result_json: result_json
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
        execution_target_json: serde_json::from_str(&execution_target_json)?,
    })
}

const RUN_SUMMARY_WHERE: &str = r#"
WHERE (?1 IS NULL OR status = ?1)
  AND (?2 IS NULL OR json_extract(execution_target_json, '$.run_scope.namespace') = ?2)
  AND (?3 IS NULL OR json_extract(execution_target_json, '$.run_scope.repo') = ?3)
  AND (?4 IS NULL OR json_extract(execution_target_json, '$.run_scope.branch') = ?4)
  AND (?5 IS NULL OR json_extract(execution_target_json, '$.run_scope.production_impacting') = ?5)
  AND (?6 IS NULL OR CAST(started_at AS INTEGER) >= ?6)
  AND (?7 IS NULL OR CAST(started_at AS INTEGER) <= ?7)
"#;

const RUN_AGE_BUCKET_CASE: &str = r#"
CASE
  WHEN (?8 - CAST(started_at AS INTEGER)) < 300000 THEN 'lt_5m'
  WHEN (?8 - CAST(started_at AS INTEGER)) < 3600000 THEN '5m_to_1h'
  WHEN (?8 - CAST(started_at AS INTEGER)) < 86400000 THEN '1h_to_24h'
  ELSE 'gte_24h'
END
"#;

const RUN_AGE_BUCKET_ORDER_CASE: &str = r#"
CASE
  WHEN (?8 - CAST(started_at AS INTEGER)) < 300000 THEN 0
  WHEN (?8 - CAST(started_at AS INTEGER)) < 3600000 THEN 1
  WHEN (?8 - CAST(started_at AS INTEGER)) < 86400000 THEN 2
  ELSE 3
END
"#;

async fn run_summary_total(
    pool: &SqlitePool,
    filter: &RunSummaryFilter,
) -> Result<u64, StoreError> {
    let sql = format!("SELECT COUNT(*) AS count FROM runs {RUN_SUMMARY_WHERE}");
    let count: i64 = sqlx::query_scalar(&sql)
        .bind(filter.status.clone())
        .bind(filter.namespace.clone())
        .bind(filter.repo.clone())
        .bind(filter.branch.clone())
        .bind(run_summary_production_impacting(filter))
        .bind(filter.started_after_ms)
        .bind(filter.started_before_ms)
        .fetch_one(pool)
        .await?;

    Ok(count.max(0) as u64)
}

async fn run_summary_text_buckets(
    pool: &SqlitePool,
    filter: &RunSummaryFilter,
    field_expr: &str,
) -> Result<Vec<CountBucket>, StoreError> {
    let sql = format!(
        r#"
        SELECT {field_expr} AS value, COUNT(*) AS count
        FROM runs
        {RUN_SUMMARY_WHERE}
        GROUP BY value
        ORDER BY count DESC, value ASC
        "#
    );
    let rows = sqlx::query(&sql)
        .bind(filter.status.clone())
        .bind(filter.namespace.clone())
        .bind(filter.repo.clone())
        .bind(filter.branch.clone())
        .bind(run_summary_production_impacting(filter))
        .bind(filter.started_after_ms)
        .bind(filter.started_before_ms)
        .fetch_all(pool)
        .await?;

    rows.into_iter()
        .map(|row| {
            Ok(CountBucket {
                value: row.try_get("value")?,
                count: row.try_get::<i64, _>("count")?.max(0) as u64,
            })
        })
        .collect()
}

async fn run_summary_age_buckets(
    pool: &SqlitePool,
    filter: &RunSummaryFilter,
) -> Result<Vec<CountBucket>, StoreError> {
    let now = now_string();
    let sql = format!(
        r#"
        SELECT age_bucket AS value, COUNT(*) AS count
        FROM (
            SELECT
                {RUN_AGE_BUCKET_CASE} AS age_bucket,
                {RUN_AGE_BUCKET_ORDER_CASE} AS sort_order
            FROM runs
            {RUN_SUMMARY_WHERE}
        )
        GROUP BY age_bucket, sort_order
        ORDER BY sort_order ASC
        "#
    );
    let rows = sqlx::query(&sql)
        .bind(filter.status.clone())
        .bind(filter.namespace.clone())
        .bind(filter.repo.clone())
        .bind(filter.branch.clone())
        .bind(run_summary_production_impacting(filter))
        .bind(filter.started_after_ms)
        .bind(filter.started_before_ms)
        .bind(now)
        .fetch_all(pool)
        .await?;

    rows.into_iter()
        .map(|row| {
            Ok(CountBucket {
                value: row.try_get("value")?,
                count: row.try_get::<i64, _>("count")?.max(0) as u64,
            })
        })
        .collect()
}

async fn run_summary_bool_buckets(
    pool: &SqlitePool,
    filter: &RunSummaryFilter,
    field_expr: &str,
) -> Result<Vec<BooleanCountBucket>, StoreError> {
    let sql = format!(
        r#"
        SELECT {field_expr} AS value, COUNT(*) AS count
        FROM runs
        {RUN_SUMMARY_WHERE}
        GROUP BY value
        ORDER BY count DESC, value ASC
        "#
    );
    let rows = sqlx::query(&sql)
        .bind(filter.status.clone())
        .bind(filter.namespace.clone())
        .bind(filter.repo.clone())
        .bind(filter.branch.clone())
        .bind(run_summary_production_impacting(filter))
        .bind(filter.started_after_ms)
        .bind(filter.started_before_ms)
        .fetch_all(pool)
        .await?;

    rows.into_iter()
        .map(|row| {
            let value = row
                .try_get::<Option<i64>, _>("value")?
                .map(|value| value != 0);
            Ok(BooleanCountBucket {
                value,
                count: row.try_get::<i64, _>("count")?.max(0) as u64,
            })
        })
        .collect()
}

fn run_summary_production_impacting(filter: &RunSummaryFilter) -> Option<i64> {
    filter
        .production_impacting
        .map(|value| if value { 1_i64 } else { 0_i64 })
}

fn row_to_approval(row: sqlx::sqlite::SqliteRow) -> Result<StoredApproval, StoreError> {
    let action_json: Option<String> = row.try_get("action_json")?;
    let preview_json: Option<String> = row.try_get("preview_json")?;
    let resume_messages_json: Option<String> = row.try_get("resume_messages_json")?;
    let run_scope_json: Option<String> = row.try_get("run_scope_json")?;
    Ok(StoredApproval {
        id: row.try_get("id")?,
        session_id: SessionId::new(row.try_get::<String, _>("session_id")?),
        run_id: RunId::new(row.try_get::<String, _>("run_id")?),
        status: row.try_get("status")?,
        kind: row.try_get("kind")?,
        summary: row.try_get("summary")?,
        risk_level: row.try_get("risk_level")?,
        requested_at: row.try_get("requested_at")?,
        decided_at: row.try_get("decided_at")?,
        decided_by: row.try_get("decided_by")?,
        decision_reason: row.try_get("decision_reason")?,
        run_scope_json: run_scope_json
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
        action_json: action_json
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
        preview_json: preview_json
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
        resume_messages_json: resume_messages_json
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
        turns_completed: row.try_get::<i64, _>("turns_completed")? as u32,
    })
}

fn approval_select_sql(where_clause: &str) -> String {
    format!(
        r#"
        SELECT id, session_id, run_id, status, kind, summary, risk_level,
               requested_at, decided_at, decided_by, decision_reason,
               run_scope_json, action_json, preview_json, resume_messages_json, turns_completed
        FROM approvals
        {where_clause}
        "#
    )
}

const APPROVAL_SUMMARY_WHERE: &str = r#"
WHERE (?1 IS NULL OR status = ?1)
  AND (?2 IS NULL OR json_extract(run_scope_json, '$.namespace') = ?2)
  AND (?3 IS NULL OR json_extract(run_scope_json, '$.repo') = ?3)
  AND (?4 IS NULL OR json_extract(run_scope_json, '$.branch') = ?4)
  AND (?5 IS NULL OR json_extract(run_scope_json, '$.production_impacting') = ?5)
  AND (?6 IS NULL OR CAST(requested_at AS INTEGER) >= ?6)
  AND (?7 IS NULL OR CAST(requested_at AS INTEGER) <= ?7)
"#;

const APPROVAL_AGE_BUCKET_CASE: &str = r#"
CASE
  WHEN (?8 - CAST(requested_at AS INTEGER)) < 300000 THEN 'lt_5m'
  WHEN (?8 - CAST(requested_at AS INTEGER)) < 3600000 THEN '5m_to_1h'
  WHEN (?8 - CAST(requested_at AS INTEGER)) < 86400000 THEN '1h_to_24h'
  ELSE 'gte_24h'
END
"#;

const APPROVAL_AGE_BUCKET_ORDER_CASE: &str = r#"
CASE
  WHEN (?8 - CAST(requested_at AS INTEGER)) < 300000 THEN 0
  WHEN (?8 - CAST(requested_at AS INTEGER)) < 3600000 THEN 1
  WHEN (?8 - CAST(requested_at AS INTEGER)) < 86400000 THEN 2
  ELSE 3
END
"#;

async fn approval_summary_total(
    pool: &SqlitePool,
    filter: &ApprovalSummaryFilter,
) -> Result<u64, StoreError> {
    let sql = format!("SELECT COUNT(*) AS count FROM approvals {APPROVAL_SUMMARY_WHERE}");
    let count: i64 = sqlx::query_scalar(&sql)
        .bind(filter.status.clone())
        .bind(filter.namespace.clone())
        .bind(filter.repo.clone())
        .bind(filter.branch.clone())
        .bind(summary_production_impacting(filter))
        .bind(filter.requested_after_ms)
        .bind(filter.requested_before_ms)
        .fetch_one(pool)
        .await?;

    Ok(count.max(0) as u64)
}

async fn approval_summary_text_buckets(
    pool: &SqlitePool,
    filter: &ApprovalSummaryFilter,
    field_expr: &str,
) -> Result<Vec<ApprovalCountBucket>, StoreError> {
    let sql = format!(
        r#"
        SELECT {field_expr} AS value, COUNT(*) AS count
        FROM approvals
        {APPROVAL_SUMMARY_WHERE}
        GROUP BY value
        ORDER BY count DESC, value ASC
        "#
    );
    let rows = sqlx::query(&sql)
        .bind(filter.status.clone())
        .bind(filter.namespace.clone())
        .bind(filter.repo.clone())
        .bind(filter.branch.clone())
        .bind(summary_production_impacting(filter))
        .bind(filter.requested_after_ms)
        .bind(filter.requested_before_ms)
        .fetch_all(pool)
        .await?;

    rows.into_iter()
        .map(|row| {
            Ok(ApprovalCountBucket {
                value: row.try_get("value")?,
                count: row.try_get::<i64, _>("count")?.max(0) as u64,
            })
        })
        .collect()
}

async fn approval_summary_age_buckets(
    pool: &SqlitePool,
    filter: &ApprovalSummaryFilter,
) -> Result<Vec<ApprovalCountBucket>, StoreError> {
    let now = now_string();
    let sql = format!(
        r#"
        SELECT age_bucket AS value, COUNT(*) AS count
        FROM (
            SELECT
                {APPROVAL_AGE_BUCKET_CASE} AS age_bucket,
                {APPROVAL_AGE_BUCKET_ORDER_CASE} AS sort_order
            FROM approvals
            {APPROVAL_SUMMARY_WHERE}
        )
        GROUP BY age_bucket, sort_order
        ORDER BY sort_order ASC
        "#
    );
    let rows = sqlx::query(&sql)
        .bind(filter.status.clone())
        .bind(filter.namespace.clone())
        .bind(filter.repo.clone())
        .bind(filter.branch.clone())
        .bind(summary_production_impacting(filter))
        .bind(filter.requested_after_ms)
        .bind(filter.requested_before_ms)
        .bind(now)
        .fetch_all(pool)
        .await?;

    rows.into_iter()
        .map(|row| {
            Ok(ApprovalCountBucket {
                value: row.try_get("value")?,
                count: row.try_get::<i64, _>("count")?.max(0) as u64,
            })
        })
        .collect()
}

async fn approval_summary_bool_buckets(
    pool: &SqlitePool,
    filter: &ApprovalSummaryFilter,
    field_expr: &str,
) -> Result<Vec<ApprovalBooleanCountBucket>, StoreError> {
    let sql = format!(
        r#"
        SELECT {field_expr} AS value, COUNT(*) AS count
        FROM approvals
        {APPROVAL_SUMMARY_WHERE}
        GROUP BY value
        ORDER BY count DESC, value ASC
        "#
    );
    let rows = sqlx::query(&sql)
        .bind(filter.status.clone())
        .bind(filter.namespace.clone())
        .bind(filter.repo.clone())
        .bind(filter.branch.clone())
        .bind(summary_production_impacting(filter))
        .bind(filter.requested_after_ms)
        .bind(filter.requested_before_ms)
        .fetch_all(pool)
        .await?;

    rows.into_iter()
        .map(|row| {
            let value = row
                .try_get::<Option<i64>, _>("value")?
                .map(|value| value != 0);
            Ok(ApprovalBooleanCountBucket {
                value,
                count: row.try_get::<i64, _>("count")?.max(0) as u64,
            })
        })
        .collect()
}

fn summary_production_impacting(filter: &ApprovalSummaryFilter) -> Option<i64> {
    filter
        .production_impacting
        .map(|value| if value { 1_i64 } else { 0_i64 })
}

const APPROVAL_GATE_SUMMARY_WHERE: &str = r#"
WHERE (?1 IS NULL OR remediation_plan_id = ?1)
  AND (?2 IS NULL OR incident_id = ?2)
  AND (?3 IS NULL OR run_id = ?3)
  AND (?4 IS NULL OR status = ?4)
  AND (?5 IS NULL OR gate_kind = ?5)
  AND (?6 IS NULL OR risk_level = ?6)
  AND (?7 IS NULL OR resource_namespace = ?7)
  AND (?8 IS NULL OR resource_kind = ?8)
  AND (?9 IS NULL OR resource_name = ?9)
  AND (?10 IS NULL OR CAST(created_at AS INTEGER) >= ?10)
  AND (?11 IS NULL OR CAST(created_at AS INTEGER) <= ?11)
"#;

const APPROVAL_GATE_AGE_BUCKET_CASE: &str = r#"
CASE
  WHEN (?12 - CAST(created_at AS INTEGER)) < 300000 THEN 'lt_5m'
  WHEN (?12 - CAST(created_at AS INTEGER)) < 3600000 THEN '5m_to_1h'
  WHEN (?12 - CAST(created_at AS INTEGER)) < 86400000 THEN '1h_to_24h'
  ELSE 'gte_24h'
END
"#;

const APPROVAL_GATE_AGE_BUCKET_ORDER_CASE: &str = r#"
CASE
  WHEN (?12 - CAST(created_at AS INTEGER)) < 300000 THEN 0
  WHEN (?12 - CAST(created_at AS INTEGER)) < 3600000 THEN 1
  WHEN (?12 - CAST(created_at AS INTEGER)) < 86400000 THEN 2
  ELSE 3
END
"#;

async fn approval_gate_summary_total(
    pool: &SqlitePool,
    filter: &ApprovalGateSummaryFilter,
) -> Result<u64, StoreError> {
    let sql = format!("SELECT COUNT(*) AS count FROM approval_gates {APPROVAL_GATE_SUMMARY_WHERE}");
    let count: i64 = sqlx::query_scalar(&sql)
        .bind(filter.remediation_plan_id.clone())
        .bind(filter.incident_id.clone())
        .bind(filter.run_id.as_ref().map(RunId::as_str))
        .bind(filter.status.clone())
        .bind(filter.gate_kind.clone())
        .bind(filter.risk_level.clone())
        .bind(filter.resource_namespace.clone())
        .bind(filter.resource_kind.clone())
        .bind(filter.resource_name.clone())
        .bind(filter.created_after_ms)
        .bind(filter.created_before_ms)
        .fetch_one(pool)
        .await?;

    Ok(count.max(0) as u64)
}

async fn approval_gate_summary_text_buckets(
    pool: &SqlitePool,
    filter: &ApprovalGateSummaryFilter,
    field_expr: &str,
) -> Result<Vec<ApprovalGateCountBucket>, StoreError> {
    let sql = format!(
        r#"
        SELECT {field_expr} AS value, COUNT(*) AS count
        FROM approval_gates
        {APPROVAL_GATE_SUMMARY_WHERE}
        GROUP BY value
        ORDER BY count DESC, value ASC
        "#
    );
    let rows = sqlx::query(&sql)
        .bind(filter.remediation_plan_id.clone())
        .bind(filter.incident_id.clone())
        .bind(filter.run_id.as_ref().map(RunId::as_str))
        .bind(filter.status.clone())
        .bind(filter.gate_kind.clone())
        .bind(filter.risk_level.clone())
        .bind(filter.resource_namespace.clone())
        .bind(filter.resource_kind.clone())
        .bind(filter.resource_name.clone())
        .bind(filter.created_after_ms)
        .bind(filter.created_before_ms)
        .fetch_all(pool)
        .await?;

    rows.into_iter()
        .map(|row| {
            Ok(ApprovalGateCountBucket {
                value: row.try_get("value")?,
                count: row.try_get::<i64, _>("count")?.max(0) as u64,
            })
        })
        .collect()
}

async fn approval_gate_summary_age_buckets(
    pool: &SqlitePool,
    filter: &ApprovalGateSummaryFilter,
) -> Result<Vec<ApprovalGateCountBucket>, StoreError> {
    let now = now_string();
    let sql = format!(
        r#"
        SELECT age_bucket AS value, COUNT(*) AS count
        FROM (
            SELECT
                {APPROVAL_GATE_AGE_BUCKET_CASE} AS age_bucket,
                {APPROVAL_GATE_AGE_BUCKET_ORDER_CASE} AS sort_order
            FROM approval_gates
            {APPROVAL_GATE_SUMMARY_WHERE}
        )
        GROUP BY age_bucket, sort_order
        ORDER BY sort_order ASC
        "#
    );
    let rows = sqlx::query(&sql)
        .bind(filter.remediation_plan_id.clone())
        .bind(filter.incident_id.clone())
        .bind(filter.run_id.as_ref().map(RunId::as_str))
        .bind(filter.status.clone())
        .bind(filter.gate_kind.clone())
        .bind(filter.risk_level.clone())
        .bind(filter.resource_namespace.clone())
        .bind(filter.resource_kind.clone())
        .bind(filter.resource_name.clone())
        .bind(filter.created_after_ms)
        .bind(filter.created_before_ms)
        .bind(now)
        .fetch_all(pool)
        .await?;

    rows.into_iter()
        .map(|row| {
            Ok(ApprovalGateCountBucket {
                value: row.try_get("value")?,
                count: row.try_get::<i64, _>("count")?.max(0) as u64,
            })
        })
        .collect()
}

fn row_to_event(row: sqlx::sqlite::SqliteRow) -> Result<AgentEvent, StoreError> {
    let kind_text: String = row.try_get("type")?;
    let payload_json: String = row.try_get("payload_json")?;
    Ok(AgentEvent {
        event_id: EventId::new(row.try_get::<String, _>("id")?),
        session_id: SessionId::new(row.try_get::<String, _>("session_id")?),
        run_id: RunId::new(row.try_get::<String, _>("run_id")?),
        seq: row.try_get::<i64, _>("seq")? as u64,
        kind: serde_json::from_value(serde_json::Value::String(kind_text))?,
        payload: serde_json::from_str(&payload_json)?,
    })
}

fn row_to_permission_grant(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredPermissionGrant, StoreError> {
    let scope_json: String = row.try_get("scope_json")?;
    let policy_json: String = row.try_get("policy_json")?;
    Ok(StoredPermissionGrant {
        id: row.try_get("id")?,
        subject: row.try_get("subject")?,
        status: row.try_get("status")?,
        reason: row.try_get("reason")?,
        scope_json: serde_json::from_str(&scope_json)?,
        policy_json: serde_json::from_str(&policy_json)?,
        created_at: row.try_get("created_at")?,
        expires_at: row.try_get("expires_at")?,
        revoked_at: row.try_get("revoked_at")?,
        revoked_by: row.try_get("revoked_by")?,
        revoke_reason: row.try_get("revoke_reason")?,
    })
}

fn row_to_audit_event(row: sqlx::sqlite::SqliteRow) -> Result<StoredAuditEvent, StoreError> {
    let run_id: Option<String> = row.try_get("run_id")?;
    let payload_json: String = row.try_get("payload_json")?;
    Ok(StoredAuditEvent {
        id: row.try_get("id")?,
        kind: row.try_get("kind")?,
        actor: row.try_get("actor")?,
        resource_kind: row.try_get("resource_kind")?,
        resource_id: row.try_get("resource_id")?,
        run_id: run_id.map(RunId::new),
        payload_json: serde_json::from_str(&payload_json)?,
        created_at: row.try_get("created_at")?,
    })
}

fn permission_grant_select_sql(where_clause: &str) -> &'static str {
    match where_clause {
        "WHERE id = ?1" => {
            r#"
            SELECT id, subject, status, reason, scope_json, policy_json, created_at,
                   expires_at, revoked_at, revoked_by, revoke_reason
            FROM permission_grants
            WHERE id = ?1
            "#
        }
        "WHERE status = ?1 ORDER BY created_at DESC LIMIT ?2" => {
            r#"
            SELECT id, subject, status, reason, scope_json, policy_json, created_at,
                   expires_at, revoked_at, revoked_by, revoke_reason
            FROM permission_grants
            WHERE status = ?1
            ORDER BY created_at DESC
            LIMIT ?2
            "#
        }
        "ORDER BY created_at DESC LIMIT ?1" => {
            r#"
            SELECT id, subject, status, reason, scope_json, policy_json, created_at,
                   expires_at, revoked_at, revoked_by, revoke_reason
            FROM permission_grants
            ORDER BY created_at DESC
            LIMIT ?1
            "#
        }
        _ => unreachable!("permission grant select SQL only supports known static clauses"),
    }
}

fn audit_event_select_sql(where_clause: &str) -> &'static str {
    match where_clause {
        "WHERE id = ?1" => {
            r#"
            SELECT id, kind, actor, resource_kind, resource_id, run_id, payload_json, created_at
            FROM audit_events
            WHERE id = ?1
            "#
        }
        "WHERE resource_kind = ?1 AND resource_id = ?2 ORDER BY created_at DESC LIMIT ?3" => {
            r#"
            SELECT id, kind, actor, resource_kind, resource_id, run_id, payload_json, created_at
            FROM audit_events
            WHERE resource_kind = ?1 AND resource_id = ?2
            ORDER BY created_at DESC
            LIMIT ?3
            "#
        }
        "WHERE run_id = ?1 ORDER BY created_at DESC LIMIT ?2" => {
            r#"
            SELECT id, kind, actor, resource_kind, resource_id, run_id, payload_json, created_at
            FROM audit_events
            WHERE run_id = ?1
            ORDER BY created_at DESC
            LIMIT ?2
            "#
        }
        "ORDER BY created_at DESC LIMIT ?1" => {
            r#"
            SELECT id, kind, actor, resource_kind, resource_id, run_id, payload_json, created_at
            FROM audit_events
            ORDER BY created_at DESC
            LIMIT ?1
            "#
        }
        _ => unreachable!("audit event select SQL only supports known static clauses"),
    }
}

fn now_string() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    millis.to_string()
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid stored data: {0}")]
    InvalidData(String),
    #[error("{entity} not found: {id}")]
    NotFound { entity: String, id: String },
}

#[cfg(test)]
mod tests {
    use super::SqliteStore;
    use crate::{
        ApprovalListFilter, ApprovalSummaryFilter, CreateAuditEvent, CreateRun, CreateSession,
        RunListFilter, RunSummaryFilter,
    };
    use pharness_core::{AgentEvent, EventId, EventKind, RunId, SessionId};

    #[tokio::test]
    async fn persists_runs_and_events() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let session_id = SessionId::new("ses_test");
        let run_id = RunId::new("run_test");

        store
            .create_session(CreateSession {
                id: session_id.clone(),
                title: "test".to_string(),
                cwd: ".".to_string(),
            })
            .await
            .unwrap();
        store
            .create_run(CreateRun {
                id: run_id.clone(),
                session_id: session_id.clone(),
                user_task: "test task".to_string(),
                cwd: ".".to_string(),
                max_turns: 10,
                initial_status: "queued".to_string(),
                execution_target_json: serde_json::json!({"kind":"local_process"}),
            })
            .await
            .unwrap();

        store
            .append_event(&AgentEvent {
                event_id: EventId::new("evt_1"),
                session_id: session_id.clone(),
                run_id: run_id.clone(),
                seq: 1,
                kind: EventKind::RunStarted,
                payload: serde_json::json!({"ok": true}),
            })
            .await
            .unwrap();

        let run = store.get_run(&run_id).await.unwrap().unwrap();
        let events = store.list_events(&run_id).await.unwrap();

        assert_eq!(run.status, "queued");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, EventKind::RunStarted);
    }

    #[tokio::test]
    async fn lists_runs_with_status_scope_and_time_filters() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let session_id = SessionId::new("ses_run_list");
        let stale_run_id = RunId::new("run_stale");
        let fresh_run_id = RunId::new("run_fresh");

        store
            .create_session(CreateSession {
                id: session_id.clone(),
                title: "run list".to_string(),
                cwd: ".".to_string(),
            })
            .await
            .unwrap();

        for (run_id, namespace, status) in [
            (&stale_run_id, "apps-dev", "completed"),
            (&fresh_run_id, "apps-prod", "approval_required"),
        ] {
            store
                .create_run(CreateRun {
                    id: run_id.clone(),
                    session_id: session_id.clone(),
                    user_task: format!("inspect {namespace}"),
                    cwd: ".".to_string(),
                    max_turns: 10,
                    initial_status: status.to_string(),
                    execution_target_json: serde_json::json!({
                        "kind": "local_process",
                        "run_scope": {
                            "namespace": namespace,
                            "repo": "git@example.test/team/app.git",
                            "branch": "feature/pharness",
                            "production_impacting": namespace == "apps-prod"
                        }
                    }),
                })
                .await
                .unwrap();
        }

        let stale_started_at = super::now_string()
            .parse::<i64>()
            .unwrap()
            .saturating_sub(2 * 60 * 60 * 1000)
            .to_string();
        sqlx::query("UPDATE runs SET started_at = ?1 WHERE id = ?2")
            .bind(stale_started_at)
            .bind(stale_run_id.as_str())
            .execute(&store.pool)
            .await
            .unwrap();
        let cutoff = super::now_string()
            .parse::<i64>()
            .unwrap()
            .saturating_sub(30 * 60 * 1000);

        let approval_required = store
            .list_runs(RunListFilter {
                status: Some("approval_required".to_string()),
                namespace: Some("apps-prod".to_string()),
                production_impacting: Some(true),
                started_after_ms: Some(cutoff),
                limit: 50,
                ..RunListFilter::default()
            })
            .await
            .unwrap();
        let stale = store
            .list_runs(RunListFilter {
                started_before_ms: Some(cutoff),
                limit: 50,
                ..RunListFilter::default()
            })
            .await
            .unwrap();

        assert_eq!(approval_required.len(), 1);
        assert_eq!(approval_required[0].id, fresh_run_id);
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].id, stale_run_id);

        let summary = store
            .run_summary(RunSummaryFilter {
                started_after_ms: Some(cutoff),
                ..RunSummaryFilter::default()
            })
            .await
            .unwrap();

        assert_eq!(summary.total, 1);
        assert_eq!(
            summary.by_status[0].value.as_deref(),
            Some("approval_required")
        );
        assert_eq!(summary.by_namespace[0].value.as_deref(), Some("apps-prod"));
        assert_eq!(summary.by_production_impacting[0].value, Some(true));
        assert_eq!(summary.by_age_bucket[0].value.as_deref(), Some("lt_5m"));
    }

    #[tokio::test]
    async fn completes_run_with_structured_result() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let session_id = SessionId::new("ses_result");
        let run_id = RunId::new("run_result");

        store
            .create_session(CreateSession {
                id: session_id.clone(),
                title: "result".to_string(),
                cwd: ".".to_string(),
            })
            .await
            .unwrap();
        store
            .create_run(CreateRun {
                id: run_id.clone(),
                session_id,
                user_task: "result task".to_string(),
                cwd: ".".to_string(),
                max_turns: 10,
                initial_status: "queued".to_string(),
                execution_target_json: serde_json::json!({"kind":"local_process"}),
            })
            .await
            .unwrap();

        let run = store
            .complete_run(
                &run_id,
                "completed",
                serde_json::json!({"summary": "done", "turns": 1}),
                None,
            )
            .await
            .unwrap();

        assert_eq!(run.status, "completed");
        assert_eq!(run.result_json.unwrap()["summary"], "done");
    }

    #[tokio::test]
    async fn persists_and_decides_pending_approval() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let session_id = SessionId::new("ses_approval");
        let run_id = RunId::new("run_approval");

        store
            .create_session(CreateSession {
                id: session_id.clone(),
                title: "approval".to_string(),
                cwd: ".".to_string(),
            })
            .await
            .unwrap();
        store
            .create_run(CreateRun {
                id: run_id.clone(),
                session_id: session_id.clone(),
                user_task: "approval task".to_string(),
                cwd: ".".to_string(),
                max_turns: 10,
                initial_status: "approval_required".to_string(),
                execution_target_json: serde_json::json!({"kind":"local_process"}),
            })
            .await
            .unwrap();

        let approval = store
            .create_approval(crate::CreateApproval {
                id: "appr_1".to_string(),
                session_id,
                run_id: run_id.clone(),
                status: "pending".to_string(),
                kind: "file_write".to_string(),
                summary: "write file".to_string(),
                risk_level: "medium".to_string(),
                run_scope_json: Some(serde_json::json!({
                    "namespace": "apps-dev",
                    "repo": "git@example.test/team/app.git",
                    "branch": "feature/pharness",
                    "production_impacting": false
                })),
                action_json: Some(serde_json::json!({"action":"write_file"})),
                preview_json: Some(serde_json::json!({
                    "kind": "file_write",
                    "action": "write_file",
                    "status": "ok"
                })),
                resume_messages_json: Some(serde_json::json!([])),
                turns_completed: 1,
            })
            .await
            .unwrap();
        assert_eq!(approval.status, "pending");

        let pending = store
            .pending_approval_for_run(&run_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(pending.id, "appr_1");
        assert_eq!(pending.turns_completed, 1);
        assert_eq!(
            pending.run_scope_json.as_ref().unwrap()["namespace"],
            "apps-dev"
        );
        assert_eq!(
            pending.preview_json.as_ref().unwrap()["action"],
            "write_file"
        );

        let decided = store
            .decide_pending_approval(
                &run_id,
                "approved",
                Some("tester".to_string()),
                Some("ok".to_string()),
            )
            .await
            .unwrap();
        assert_eq!(decided.status, "approved");
        assert_eq!(decided.decided_by.as_deref(), Some("tester"));
        assert!(store
            .pending_approval_for_run(&run_id)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn lists_pending_approvals() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let session_id = SessionId::new("ses_approval_list");
        let run_id = RunId::new("run_approval_list");

        store
            .create_session(CreateSession {
                id: session_id.clone(),
                title: "approval list".to_string(),
                cwd: ".".to_string(),
            })
            .await
            .unwrap();
        store
            .create_run(CreateRun {
                id: run_id.clone(),
                session_id: session_id.clone(),
                user_task: "write".to_string(),
                cwd: ".".to_string(),
                max_turns: 10,
                initial_status: "approval_required".to_string(),
                execution_target_json: serde_json::json!({"kind":"local_process"}),
            })
            .await
            .unwrap();
        store
            .create_approval(crate::CreateApproval {
                id: "appr_list".to_string(),
                session_id,
                run_id,
                status: "pending".to_string(),
                kind: "file_write".to_string(),
                summary: "write file".to_string(),
                risk_level: "medium".to_string(),
                run_scope_json: None,
                action_json: None,
                preview_json: None,
                resume_messages_json: None,
                turns_completed: 1,
            })
            .await
            .unwrap();

        let approvals = store
            .list_approvals(ApprovalListFilter {
                status: Some("pending".to_string()),
                limit: 50,
                ..ApprovalListFilter::default()
            })
            .await
            .unwrap();

        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].id, "appr_list");

        store
            .create_approval(crate::CreateApproval {
                id: "appr_scoped".to_string(),
                session_id: SessionId::new("ses_approval_list"),
                run_id: RunId::new("run_approval_list"),
                status: "pending".to_string(),
                kind: "file_write".to_string(),
                summary: "write scoped file".to_string(),
                risk_level: "medium".to_string(),
                run_scope_json: Some(serde_json::json!({
                    "namespace": "apps-dev",
                    "repo": "git@example.test/team/app.git",
                    "branch": "feature/pharness",
                    "production_impacting": false
                })),
                action_json: None,
                preview_json: None,
                resume_messages_json: None,
                turns_completed: 1,
            })
            .await
            .unwrap();

        let scoped = store
            .list_approvals(ApprovalListFilter {
                status: Some("pending".to_string()),
                namespace: Some("apps-dev".to_string()),
                repo: Some("git@example.test/team/app.git".to_string()),
                branch: Some("feature/pharness".to_string()),
                production_impacting: Some(false),
                requested_after_ms: None,
                requested_before_ms: None,
                limit: 50,
                offset: 0,
            })
            .await
            .unwrap();
        let second_page = store
            .list_approvals(ApprovalListFilter {
                status: Some("pending".to_string()),
                limit: 1,
                offset: 1,
                ..ApprovalListFilter::default()
            })
            .await
            .unwrap();

        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].id, "appr_scoped");
        assert_eq!(second_page.len(), 1);

        let summary = store
            .approval_summary(ApprovalSummaryFilter {
                status: Some("pending".to_string()),
                namespace: Some("apps-dev".to_string()),
                repo: Some("git@example.test/team/app.git".to_string()),
                branch: Some("feature/pharness".to_string()),
                production_impacting: Some(false),
                requested_after_ms: None,
                requested_before_ms: None,
            })
            .await
            .unwrap();

        assert_eq!(summary.total, 1);
        assert_eq!(summary.by_status[0].value.as_deref(), Some("pending"));
        assert_eq!(summary.by_kind[0].value.as_deref(), Some("file_write"));
        assert_eq!(summary.by_risk_level[0].value.as_deref(), Some("medium"));
        assert_eq!(summary.by_age_bucket[0].value.as_deref(), Some("lt_5m"));
        assert_eq!(summary.by_namespace[0].value.as_deref(), Some("apps-dev"));
        assert_eq!(summary.by_production_impacting[0].value, Some(false));
    }

    #[tokio::test]
    async fn summarizes_approval_age_buckets() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let session_id = SessionId::new("ses_approval_age");
        let run_id = RunId::new("run_approval_age");

        store
            .create_session(CreateSession {
                id: session_id.clone(),
                title: "approval age".to_string(),
                cwd: ".".to_string(),
            })
            .await
            .unwrap();
        store
            .create_run(CreateRun {
                id: run_id.clone(),
                session_id: session_id.clone(),
                user_task: "write".to_string(),
                cwd: ".".to_string(),
                max_turns: 10,
                initial_status: "approval_required".to_string(),
                execution_target_json: serde_json::json!({"kind":"local_process"}),
            })
            .await
            .unwrap();

        for id in ["appr_fresh", "appr_stale"] {
            store
                .create_approval(crate::CreateApproval {
                    id: id.to_string(),
                    session_id: session_id.clone(),
                    run_id: run_id.clone(),
                    status: "pending".to_string(),
                    kind: "file_write".to_string(),
                    summary: "write file".to_string(),
                    risk_level: "medium".to_string(),
                    run_scope_json: None,
                    action_json: None,
                    preview_json: None,
                    resume_messages_json: None,
                    turns_completed: 1,
                })
                .await
                .unwrap();
        }

        let stale_requested_at = super::now_string()
            .parse::<i128>()
            .unwrap()
            .saturating_sub(2 * 60 * 60 * 1000)
            .to_string();
        sqlx::query("UPDATE approvals SET requested_at = ?1 WHERE id = ?2")
            .bind(stale_requested_at)
            .bind("appr_stale")
            .execute(&store.pool)
            .await
            .unwrap();
        let cutoff = super::now_string()
            .parse::<i64>()
            .unwrap()
            .saturating_sub(30 * 60 * 1000);

        let fresh = store
            .list_approvals(ApprovalListFilter {
                status: Some("pending".to_string()),
                requested_after_ms: Some(cutoff),
                limit: 50,
                ..ApprovalListFilter::default()
            })
            .await
            .unwrap();
        let stale = store
            .list_approvals(ApprovalListFilter {
                status: Some("pending".to_string()),
                requested_before_ms: Some(cutoff),
                limit: 50,
                ..ApprovalListFilter::default()
            })
            .await
            .unwrap();

        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].id, "appr_fresh");
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].id, "appr_stale");

        let summary = store
            .approval_summary(ApprovalSummaryFilter {
                status: Some("pending".to_string()),
                ..ApprovalSummaryFilter::default()
            })
            .await
            .unwrap();

        assert_eq!(
            summary
                .by_age_bucket
                .iter()
                .map(|bucket| (bucket.value.as_deref(), bucket.count))
                .collect::<Vec<_>>(),
            vec![(Some("lt_5m"), 1), (Some("1h_to_24h"), 1)]
        );

        let stale_summary = store
            .approval_summary(ApprovalSummaryFilter {
                status: Some("pending".to_string()),
                requested_before_ms: Some(cutoff),
                ..ApprovalSummaryFilter::default()
            })
            .await
            .unwrap();

        assert_eq!(stale_summary.total, 1);
        assert_eq!(
            stale_summary.by_age_bucket[0].value.as_deref(),
            Some("1h_to_24h")
        );
    }

    #[tokio::test]
    async fn persists_file_changes() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let session_id = SessionId::new("ses_file_change");
        let run_id = RunId::new("run_file_change");

        store
            .create_session(CreateSession {
                id: session_id.clone(),
                title: "file change".to_string(),
                cwd: ".".to_string(),
            })
            .await
            .unwrap();
        store
            .create_run(CreateRun {
                id: run_id.clone(),
                session_id: session_id.clone(),
                user_task: "write".to_string(),
                cwd: ".".to_string(),
                max_turns: 10,
                initial_status: "queued".to_string(),
                execution_target_json: serde_json::json!({"kind":"local_process"}),
            })
            .await
            .unwrap();

        store
            .create_file_change(crate::CreateFileChange {
                id: "chg_test".to_string(),
                session_id,
                run_id: run_id.clone(),
                path: "README.md".to_string(),
                before_hash: None,
                after_hash: None,
                diff: "--- before\n+++ after".to_string(),
            })
            .await
            .unwrap();

        let changes = store.list_file_changes(&run_id).await.unwrap();

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, "README.md");
    }

    #[tokio::test]
    async fn persists_artifacts() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let session_id = SessionId::new("ses_artifact");
        let run_id = RunId::new("run_artifact");

        store
            .create_session(CreateSession {
                id: session_id.clone(),
                title: "artifact".to_string(),
                cwd: ".".to_string(),
            })
            .await
            .unwrap();
        store
            .create_run(CreateRun {
                id: run_id.clone(),
                session_id: session_id.clone(),
                user_task: "observe".to_string(),
                cwd: ".".to_string(),
                max_turns: 10,
                initial_status: "queued".to_string(),
                execution_target_json: serde_json::json!({"kind":"local_process"}),
            })
            .await
            .unwrap();

        let artifact = store
            .create_artifact(crate::CreateArtifact {
                id: "art_test".to_string(),
                session_id,
                run_id: Some(run_id.clone()),
                kind: "tool_result".to_string(),
                label: "Prometheus query".to_string(),
                mime_type: Some("application/json".to_string()),
                path: None,
                content_text: None,
                content_json: Some(serde_json::json!({"result_count": 2})),
            })
            .await
            .unwrap();

        let fetched = store.get_artifact(&artifact.id).await.unwrap().unwrap();
        let artifacts = store.list_artifacts(&run_id).await.unwrap();

        assert_eq!(fetched.label, "Prometheus query");
        assert_eq!(artifacts.len(), 1);
        assert_eq!(
            artifacts[0].content_json.as_ref().unwrap()["result_count"],
            2
        );
    }

    #[tokio::test]
    async fn persists_observations() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let session_id = SessionId::new("ses_observation");
        let run_id = RunId::new("run_observation");

        store
            .create_session(CreateSession {
                id: session_id.clone(),
                title: "observation".to_string(),
                cwd: ".".to_string(),
            })
            .await
            .unwrap();
        store
            .create_run(CreateRun {
                id: run_id.clone(),
                session_id: session_id.clone(),
                user_task: "observe".to_string(),
                cwd: ".".to_string(),
                max_turns: 10,
                initial_status: "queued".to_string(),
                execution_target_json: serde_json::json!({"kind":"local_process"}),
            })
            .await
            .unwrap();

        let observation = store
            .create_observation(crate::CreateObservation {
                id: "obs_test".to_string(),
                session_id,
                run_id: Some(run_id.clone()),
                source: "prometheus".to_string(),
                kind: "query".to_string(),
                subject: "up".to_string(),
                summary: "read Prometheus instant query".to_string(),
                resource_namespace: None,
                resource_kind: Some("query".to_string()),
                resource_name: Some("up".to_string()),
                resource_ref_json: Some(serde_json::json!({
                    "provider": "prometheus",
                    "kind": "query",
                    "name": "up"
                })),
                artifact_id: None,
                data_json: serde_json::json!({"result_count": 2}),
            })
            .await
            .unwrap();

        let fetched = store
            .get_observation(&observation.id)
            .await
            .unwrap()
            .unwrap();
        let observations = store.list_run_observations(&run_id).await.unwrap();
        let filtered = store
            .list_observations(crate::ObservationListFilter {
                run_id: Some(run_id.clone()),
                source: Some("prometheus".to_string()),
                kind: Some("query".to_string()),
                subject: Some("up".to_string()),
                resource_namespace: None,
                resource_kind: Some("query".to_string()),
                resource_name: Some("up".to_string()),
                observed_after_ms: Some(0),
                observed_before_ms: None,
                limit: 10,
                offset: 0,
            })
            .await
            .unwrap();

        assert_eq!(fetched.subject, "up");
        assert_eq!(fetched.resource_kind.as_deref(), Some("query"));
        assert_eq!(fetched.resource_name.as_deref(), Some("up"));
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].data_json["result_count"], 2);
        assert_eq!(filtered.len(), 1);
    }

    #[tokio::test]
    async fn persists_incidents() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let session_id = SessionId::new("ses_incident");
        let run_id = RunId::new("run_incident");

        store
            .create_session(CreateSession {
                id: session_id.clone(),
                title: "incident".to_string(),
                cwd: ".".to_string(),
            })
            .await
            .unwrap();
        store
            .create_run(CreateRun {
                id: run_id.clone(),
                session_id: session_id.clone(),
                user_task: "observe incident".to_string(),
                cwd: ".".to_string(),
                max_turns: 10,
                initial_status: "queued".to_string(),
                execution_target_json: serde_json::json!({"kind":"local_process"}),
            })
            .await
            .unwrap();
        store
            .create_observation(crate::CreateObservation {
                id: "obs_incident".to_string(),
                session_id: session_id.clone(),
                run_id: Some(run_id.clone()),
                source: "tekton".to_string(),
                kind: "pipeline_run_analysis".to_string(),
                subject: "build-app".to_string(),
                summary: "analyzed Tekton PipelineRun ci/build-app".to_string(),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-app".to_string()),
                resource_ref_json: None,
                artifact_id: None,
                data_json: serde_json::json!({"status": "failed"}),
            })
            .await
            .unwrap();

        let incident = store
            .create_incident(crate::CreateIncident {
                id: "inc_test".to_string(),
                observation_id: "obs_incident".to_string(),
                session_id: session_id.clone(),
                run_id: Some(run_id.clone()),
                status: "candidate".to_string(),
                severity: "high".to_string(),
                title: "Tekton PipelineRun issue: ci/build-app".to_string(),
                summary: "PipelineRun status is failed".to_string(),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-app".to_string()),
                data_json: serde_json::json!({"reasons": ["pipeline_status=failed"]}),
            })
            .await
            .unwrap();

        let fetched = store.get_incident(&incident.id).await.unwrap().unwrap();
        let incidents = store
            .list_incidents(crate::IncidentListFilter {
                run_id: Some(run_id.clone()),
                status: Some("candidate".to_string()),
                severity: Some("high".to_string()),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-app".to_string()),
                created_after_ms: Some(0),
                created_before_ms: None,
                limit: 10,
                offset: 0,
            })
            .await
            .unwrap();

        assert_eq!(fetched.observation_id, "obs_incident");
        assert_eq!(fetched.severity, "high");
        assert_eq!(incidents.len(), 1);
        assert_eq!(incidents[0].id, "inc_test");

        let plan = store
            .create_remediation_plan(crate::CreateRemediationPlan {
                id: "rplan_test".to_string(),
                incident_id: incident.id.clone(),
                session_id: session_id.clone(),
                run_id: Some(run_id.clone()),
                status: "draft".to_string(),
                title: "Draft remediation for ci/build-app".to_string(),
                summary: "Review failed PipelineRun before any mutation".to_string(),
                risk_level: "high".to_string(),
                requires_approval: true,
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-app".to_string()),
                plan_json: serde_json::json!({
                    "approval_gates": ["before_pipeline_rerun"],
                    "steps": [{"kind": "read_only", "action": "inspect_taskruns"}]
                }),
            })
            .await
            .unwrap();
        let fetched_plan = store.get_remediation_plan(&plan.id).await.unwrap().unwrap();
        let plans = store
            .list_remediation_plans(crate::RemediationPlanListFilter {
                incident_id: Some("inc_test".to_string()),
                run_id: Some(run_id.clone()),
                status: Some("draft".to_string()),
                risk_level: Some("high".to_string()),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-app".to_string()),
                created_after_ms: Some(0),
                created_before_ms: None,
                limit: 10,
                offset: 0,
            })
            .await
            .unwrap();

        assert!(fetched_plan.requires_approval);
        assert_eq!(fetched_plan.incident_id, "inc_test");
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].id, "rplan_test");

        let work_plan = store
            .create_work_plan(crate::CreateWorkPlan {
                id: "wplan_test".to_string(),
                work_item_id: None,
                remediation_plan_id: Some(plan.id.clone()),
                incident_id: Some(incident.id.clone()),
                session_id: session_id.clone(),
                run_id: Some(run_id.clone()),
                status: "draft".to_string(),
                title: "WorkPlan for ci/build-app".to_string(),
                summary: "Review failed PipelineRun before execution".to_string(),
                risk_level: "high".to_string(),
                requires_approval: true,
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-app".to_string()),
                work_plan_json: serde_json::json!({
                    "source": {"kind": "remediation_plan", "id": "rplan_test"},
                    "execution": {"enabled": false},
                    "steps": [{"kind": "read_only", "action": "inspect_taskruns"}]
                }),
            })
            .await
            .unwrap();
        let fetched_work_plan = store.get_work_plan(&work_plan.id).await.unwrap().unwrap();
        let idempotency_lookup = store
            .get_work_plan_by_remediation_plan("rplan_test")
            .await
            .unwrap()
            .unwrap();
        let work_plans = store
            .list_work_plans(crate::WorkPlanListFilter {
                work_item_id: None,
                remediation_plan_id: Some("rplan_test".to_string()),
                incident_id: Some("inc_test".to_string()),
                run_id: Some(run_id.clone()),
                status: Some("draft".to_string()),
                risk_level: Some("high".to_string()),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-app".to_string()),
                created_after_ms: Some(0),
                created_before_ms: None,
                limit: 10,
                offset: 0,
            })
            .await
            .unwrap();

        assert_eq!(
            fetched_work_plan.remediation_plan_id.as_deref(),
            Some("rplan_test")
        );
        assert_eq!(idempotency_lookup.id, "wplan_test");
        assert_eq!(work_plans.len(), 1);
        assert_eq!(work_plans[0].id, "wplan_test");
        assert!(!work_plans[0].work_plan_json["execution"]["enabled"]
            .as_bool()
            .unwrap());

        let change_set = store
            .create_change_set(crate::CreateChangeSet {
                id: "cset_test".to_string(),
                work_plan_id: work_plan.id.clone(),
                remediation_plan_id: plan.id.clone(),
                incident_id: incident.id.clone(),
                session_id: session_id.clone(),
                run_id: Some(run_id.clone()),
                status: "approved".to_string(),
                title: "ChangeSet: build config".to_string(),
                summary: "Review build config changes".to_string(),
                risk_level: "medium".to_string(),
                material_hash: "hash_test".to_string(),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-app".to_string()),
                change_set_json: serde_json::json!({
                    "changes": [{"path": "tekton/pipeline.yaml"}]
                }),
            })
            .await
            .unwrap();
        let pipeline_intent = store
            .create_pipeline_intent(crate::CreatePipelineIntent {
                id: "pint_test".to_string(),
                change_set_id: change_set.id.clone(),
                work_plan_id: work_plan.id.clone(),
                remediation_plan_id: plan.id.clone(),
                incident_id: incident.id.clone(),
                session_id: session_id.clone(),
                run_id: Some(run_id.clone()),
                status: "proposed".to_string(),
                title: "Run build/test/package".to_string(),
                summary: "Propose Tekton build/test/package for approved ChangeSet".to_string(),
                risk_level: "medium".to_string(),
                intent_kind: "tekton_build_test_package".to_string(),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-app".to_string()),
                intent_json: serde_json::json!({
                    "execution": {"enabled": false},
                    "pipeline": {"tasks": ["test", "build", "package"]}
                }),
            })
            .await
            .unwrap();
        let listed_pipeline_intents = store
            .list_pipeline_intents(crate::PipelineIntentListFilter {
                change_set_id: Some(change_set.id.clone()),
                work_plan_id: Some(work_plan.id.clone()),
                status: Some("proposed".to_string()),
                intent_kind: Some("tekton_build_test_package".to_string()),
                limit: 10,
                offset: 0,
                ..crate::PipelineIntentListFilter::default()
            })
            .await
            .unwrap();
        let approved_pipeline_intent = store
            .update_pipeline_intent_status(
                &pipeline_intent.id,
                "approved",
                Some("tester".to_string()),
                Some("pipeline intent reviewed".to_string()),
            )
            .await
            .unwrap();
        let stale_pipeline_intent = store
            .update_pipeline_intent_status(
                &pipeline_intent.id,
                "stale",
                Some("tester".to_string()),
                Some("source changed".to_string()),
            )
            .await
            .unwrap();
        let reproposed_pipeline_intent = store
            .revise_pipeline_intent_draft(
                &pipeline_intent.id,
                crate::UpdatePipelineIntentDraft {
                    title: "Run build/test/package again".to_string(),
                    summary: "Re-propose Tekton build/test/package after source change".to_string(),
                    risk_level: "medium".to_string(),
                    intent_kind: "tekton_build_test_package".to_string(),
                    resource_namespace: Some("ci".to_string()),
                    resource_kind: Some("PipelineRun".to_string()),
                    resource_name: Some("build-app".to_string()),
                    intent_json: serde_json::json!({
                        "execution": {"enabled": false},
                        "source": {"material_hash": "hash_test_2"},
                        "pipeline": {"tasks": ["test", "build", "package"]}
                    }),
                    actor: Some("tester".to_string()),
                    reason: Some("pipeline intent reproposed".to_string()),
                },
            )
            .await
            .unwrap();

        assert_eq!(pipeline_intent.change_set_id, "cset_test");
        assert!(!pipeline_intent.intent_json["execution"]["enabled"]
            .as_bool()
            .unwrap());
        assert_eq!(listed_pipeline_intents.len(), 1);
        assert_eq!(listed_pipeline_intents[0].id, "pint_test");
        assert_eq!(approved_pipeline_intent.status, "approved");
        assert_eq!(
            approved_pipeline_intent.status_changed_by.as_deref(),
            Some("tester")
        );
        assert_eq!(stale_pipeline_intent.status, "stale");
        assert_eq!(reproposed_pipeline_intent.status, "proposed");
        assert_eq!(
            reproposed_pipeline_intent.intent_json["source"]["material_hash"],
            serde_json::json!("hash_test_2")
        );

        let deployment_intent = store
            .create_deployment_intent(crate::CreateDeploymentIntent {
                id: "dint_test".to_string(),
                pipeline_intent_id: pipeline_intent.id.clone(),
                change_set_id: change_set.id.clone(),
                work_plan_id: work_plan.id.clone(),
                remediation_plan_id: plan.id.clone(),
                incident_id: incident.id.clone(),
                session_id: session_id.clone(),
                run_id: Some(run_id.clone()),
                status: "proposed".to_string(),
                title: "Deploy checkout-api".to_string(),
                summary: "Propose Argo sync for approved PipelineIntent".to_string(),
                risk_level: "medium".to_string(),
                intent_kind: "argo_sync_deploy".to_string(),
                target_environment: Some("dev".to_string()),
                target_namespace: Some("apps-dev".to_string()),
                argo_application: Some("checkout-api".to_string()),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-app".to_string()),
                intent_json: serde_json::json!({
                    "execution": {"enabled": false},
                    "deployment": {"provider": "argo_cd", "operation": "sync"}
                }),
            })
            .await
            .unwrap();
        let listed_deployment_intents = store
            .list_deployment_intents(crate::DeploymentIntentListFilter {
                pipeline_intent_id: Some(pipeline_intent.id.clone()),
                change_set_id: Some(change_set.id.clone()),
                work_plan_id: Some(work_plan.id.clone()),
                status: Some("proposed".to_string()),
                intent_kind: Some("argo_sync_deploy".to_string()),
                target_environment: Some("dev".to_string()),
                target_namespace: Some("apps-dev".to_string()),
                argo_application: Some("checkout-api".to_string()),
                limit: 10,
                offset: 0,
                ..crate::DeploymentIntentListFilter::default()
            })
            .await
            .unwrap();
        let approved_deployment_intent = store
            .update_deployment_intent_status(
                &deployment_intent.id,
                "approved",
                Some("tester".to_string()),
                Some("deployment intent reviewed".to_string()),
            )
            .await
            .unwrap();
        let stale_deployment_intent = store
            .update_deployment_intent_status(
                &deployment_intent.id,
                "stale",
                Some("tester".to_string()),
                Some("pipeline intent changed".to_string()),
            )
            .await
            .unwrap();
        let reproposed_deployment_intent = store
            .revise_deployment_intent_draft(
                &deployment_intent.id,
                crate::UpdateDeploymentIntentDraft {
                    title: "Deploy checkout-api again".to_string(),
                    summary: "Re-propose Argo sync after pipeline intent changed".to_string(),
                    risk_level: "medium".to_string(),
                    intent_kind: "argo_sync_deploy".to_string(),
                    target_environment: Some("dev".to_string()),
                    target_namespace: Some("apps-dev".to_string()),
                    argo_application: Some("checkout-api".to_string()),
                    resource_namespace: Some("ci".to_string()),
                    resource_kind: Some("PipelineRun".to_string()),
                    resource_name: Some("build-app".to_string()),
                    intent_json: serde_json::json!({
                        "execution": {"enabled": false},
                        "source": {"pipeline_intent_id": pipeline_intent.id},
                    }),
                    actor: Some("tester".to_string()),
                    reason: Some("deployment intent reproposed".to_string()),
                },
            )
            .await
            .unwrap();

        assert_eq!(deployment_intent.pipeline_intent_id, "pint_test");
        assert!(!deployment_intent.intent_json["execution"]["enabled"]
            .as_bool()
            .unwrap());
        assert_eq!(listed_deployment_intents.len(), 1);
        assert_eq!(listed_deployment_intents[0].id, "dint_test");
        assert_eq!(approved_deployment_intent.status, "approved");
        assert_eq!(
            approved_deployment_intent.status_changed_by.as_deref(),
            Some("tester")
        );
        assert_eq!(stale_deployment_intent.status, "stale");
        assert_eq!(reproposed_deployment_intent.status, "proposed");
        assert_eq!(
            reproposed_deployment_intent.intent_json["source"]["pipeline_intent_id"],
            serde_json::json!("pint_test")
        );

        let release = store
            .create_release(crate::CreateRelease {
                id: "rel_test".to_string(),
                deployment_intent_id: deployment_intent.id.clone(),
                pipeline_intent_id: pipeline_intent.id.clone(),
                change_set_id: change_set.id.clone(),
                work_plan_id: work_plan.id.clone(),
                remediation_plan_id: plan.id.clone(),
                incident_id: incident.id.clone(),
                session_id: session_id.clone(),
                run_id: Some(run_id.clone()),
                status: "proposed".to_string(),
                title: "Release checkout-api".to_string(),
                summary: "Propose release after approved deployment intent".to_string(),
                risk_level: "medium".to_string(),
                release_kind: "gitops_release".to_string(),
                target_environment: Some("dev".to_string()),
                target_namespace: Some("apps-dev".to_string()),
                argo_application: Some("checkout-api".to_string()),
                version: Some("v0.1.0-smoke".to_string()),
                commit_sha: Some("abc1234".to_string()),
                image_digest: Some("sha256:deadbeef".to_string()),
                rollback_ref: Some("previous-release".to_string()),
                release_json: serde_json::json!({
                    "execution": {"enabled": false},
                    "verification": {"required": ["argo", "lgtm"]}
                }),
            })
            .await
            .unwrap();
        let listed_releases = store
            .list_releases(crate::ReleaseListFilter {
                deployment_intent_id: Some(deployment_intent.id.clone()),
                pipeline_intent_id: Some(pipeline_intent.id.clone()),
                change_set_id: Some(change_set.id.clone()),
                work_plan_id: Some(work_plan.id.clone()),
                status: Some("proposed".to_string()),
                release_kind: Some("gitops_release".to_string()),
                target_environment: Some("dev".to_string()),
                target_namespace: Some("apps-dev".to_string()),
                argo_application: Some("checkout-api".to_string()),
                version: Some("v0.1.0-smoke".to_string()),
                commit_sha: Some("abc1234".to_string()),
                image_digest: Some("sha256:deadbeef".to_string()),
                limit: 10,
                offset: 0,
                ..crate::ReleaseListFilter::default()
            })
            .await
            .unwrap();
        let approved_release = store
            .update_release_status(
                &release.id,
                "approved",
                Some("tester".to_string()),
                Some("release reviewed".to_string()),
            )
            .await
            .unwrap();
        let stale_release = store
            .update_release_status(
                &release.id,
                "stale",
                Some("tester".to_string()),
                Some("deployment intent changed".to_string()),
            )
            .await
            .unwrap();
        let reproposed_release = store
            .revise_release_draft(
                &release.id,
                crate::UpdateReleaseDraft {
                    title: "Release checkout-api again".to_string(),
                    summary: "Re-propose release after deployment intent changed".to_string(),
                    risk_level: "medium".to_string(),
                    release_kind: "gitops_release".to_string(),
                    target_environment: Some("dev".to_string()),
                    target_namespace: Some("apps-dev".to_string()),
                    argo_application: Some("checkout-api".to_string()),
                    version: Some("v0.1.1-smoke".to_string()),
                    commit_sha: Some("def5678".to_string()),
                    image_digest: Some("sha256:feedface".to_string()),
                    rollback_ref: Some("rel_test".to_string()),
                    release_json: serde_json::json!({
                        "execution": {"enabled": false},
                        "source": {"deployment_intent_id": deployment_intent.id},
                    }),
                    actor: Some("tester".to_string()),
                    reason: Some("release reproposed".to_string()),
                },
            )
            .await
            .unwrap();

        assert_eq!(release.deployment_intent_id, "dint_test");
        assert!(!release.release_json["execution"]["enabled"]
            .as_bool()
            .unwrap());
        assert_eq!(listed_releases.len(), 1);
        assert_eq!(listed_releases[0].id, "rel_test");
        assert_eq!(approved_release.status, "approved");
        assert_eq!(
            approved_release.status_changed_by.as_deref(),
            Some("tester")
        );
        assert_eq!(stale_release.status, "stale");
        assert_eq!(reproposed_release.status, "proposed");
        assert_eq!(reproposed_release.version.as_deref(), Some("v0.1.1-smoke"));

        let registry_evidence = store
            .create_registry_evidence(crate::CreateRegistryEvidence {
                id: "regev_test".to_string(),
                release_id: release.id.clone(),
                deployment_intent_id: deployment_intent.id.clone(),
                pipeline_intent_id: pipeline_intent.id.clone(),
                change_set_id: change_set.id.clone(),
                work_plan_id: work_plan.id.clone(),
                remediation_plan_id: plan.id.clone(),
                incident_id: incident.id.clone(),
                session_id: session_id.clone(),
                run_id: Some(run_id.clone()),
                status: "proposed".to_string(),
                title: "Registry evidence checkout-api".to_string(),
                summary: "Manual image verification evidence".to_string(),
                risk_level: "medium".to_string(),
                registry: Some("registry.example.test".to_string()),
                repository: Some("checkout-api".to_string()),
                image_ref: Some("registry.example.test/checkout-api:v0.1.0-smoke".to_string()),
                image_digest: Some("sha256:deadbeef".to_string()),
                tag: Some("v0.1.0-smoke".to_string()),
                source: "manual".to_string(),
                verification_status: "verified".to_string(),
                evidence_json: serde_json::json!({
                    "image": {"digest": "sha256:deadbeef"},
                    "verification": {"status": "verified"}
                }),
            })
            .await
            .unwrap();
        let listed_registry_evidence = store
            .list_registry_evidence(crate::RegistryEvidenceListFilter {
                release_id: Some(release.id.clone()),
                deployment_intent_id: Some(deployment_intent.id.clone()),
                pipeline_intent_id: Some(pipeline_intent.id.clone()),
                change_set_id: Some(change_set.id.clone()),
                work_plan_id: Some(work_plan.id.clone()),
                status: Some("proposed".to_string()),
                registry: Some("registry.example.test".to_string()),
                repository: Some("checkout-api".to_string()),
                image_digest: Some("sha256:deadbeef".to_string()),
                source: Some("manual".to_string()),
                verification_status: Some("verified".to_string()),
                limit: 10,
                offset: 0,
                ..crate::RegistryEvidenceListFilter::default()
            })
            .await
            .unwrap();
        let verified_registry_evidence = store
            .update_registry_evidence_status(
                &registry_evidence.id,
                "verified",
                Some("tester".to_string()),
                Some("image metadata verified".to_string()),
            )
            .await
            .unwrap();
        let stale_registry_evidence = store
            .update_registry_evidence_status(
                &registry_evidence.id,
                "stale",
                Some("tester".to_string()),
                Some("release changed".to_string()),
            )
            .await
            .unwrap();
        let reproposed_registry_evidence = store
            .revise_registry_evidence_draft(
                &registry_evidence.id,
                crate::UpdateRegistryEvidenceDraft {
                    title: "Registry evidence checkout-api again".to_string(),
                    summary: "Re-propose image verification evidence".to_string(),
                    risk_level: "medium".to_string(),
                    registry: Some("registry.example.test".to_string()),
                    repository: Some("checkout-api".to_string()),
                    image_ref: Some("registry.example.test/checkout-api:v0.1.1-smoke".to_string()),
                    image_digest: Some("sha256:feedface".to_string()),
                    tag: Some("v0.1.1-smoke".to_string()),
                    source: "manual".to_string(),
                    verification_status: "unverified".to_string(),
                    evidence_json: serde_json::json!({
                        "source": {"release_id": release.id},
                        "verification": {"status": "unverified"}
                    }),
                    actor: Some("tester".to_string()),
                    reason: Some("registry evidence reproposed".to_string()),
                },
            )
            .await
            .unwrap();

        assert_eq!(registry_evidence.release_id, "rel_test");
        assert_eq!(listed_registry_evidence.len(), 1);
        assert_eq!(listed_registry_evidence[0].id, "regev_test");
        assert_eq!(verified_registry_evidence.status, "verified");
        assert_eq!(
            verified_registry_evidence.status_changed_by.as_deref(),
            Some("tester")
        );
        assert_eq!(stale_registry_evidence.status, "stale");
        assert_eq!(reproposed_registry_evidence.status, "proposed");
        assert_eq!(
            reproposed_registry_evidence.image_digest.as_deref(),
            Some("sha256:feedface")
        );

        let gate = store
            .create_approval_gate(crate::CreateApprovalGate {
                id: "agate_test".to_string(),
                remediation_plan_id: plan.id,
                incident_id: incident.id,
                session_id,
                run_id: Some(run_id.clone()),
                status: "pending".to_string(),
                gate_kind: "pipeline_mutation".to_string(),
                gate_order: 1,
                title: "Approve pipeline mutation".to_string(),
                summary: "Require approval before rerunning Tekton resources".to_string(),
                risk_level: "high".to_string(),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-app".to_string()),
                gate_json: serde_json::json!({
                    "required_before": "rerunning PipelineRun"
                }),
            })
            .await
            .unwrap();
        let fetched_gate = store.get_approval_gate(&gate.id).await.unwrap().unwrap();
        let gates = store
            .list_approval_gates(crate::ApprovalGateListFilter {
                remediation_plan_id: Some("rplan_test".to_string()),
                incident_id: Some("inc_test".to_string()),
                run_id: Some(run_id.clone()),
                status: Some("pending".to_string()),
                gate_kind: Some("pipeline_mutation".to_string()),
                risk_level: Some("high".to_string()),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-app".to_string()),
                created_after_ms: Some(0),
                created_before_ms: None,
                limit: 10,
                offset: 0,
            })
            .await
            .unwrap();

        assert_eq!(fetched_gate.remediation_plan_id, "rplan_test");
        assert_eq!(fetched_gate.gate_kind, "pipeline_mutation");
        assert_eq!(gates.len(), 1);
        assert_eq!(gates[0].id, "agate_test");

        let gate_summary = store
            .approval_gate_summary(crate::ApprovalGateSummaryFilter {
                remediation_plan_id: Some("rplan_test".to_string()),
                incident_id: Some("inc_test".to_string()),
                run_id: Some(run_id.clone()),
                status: Some("pending".to_string()),
                gate_kind: Some("pipeline_mutation".to_string()),
                risk_level: Some("high".to_string()),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-app".to_string()),
                created_after_ms: Some(0),
                created_before_ms: None,
            })
            .await
            .unwrap();

        assert_eq!(gate_summary.total, 1);
        assert_eq!(gate_summary.by_status[0].value.as_deref(), Some("pending"));
        assert_eq!(
            gate_summary.by_gate_kind[0].value.as_deref(),
            Some("pipeline_mutation")
        );
        assert_eq!(
            gate_summary.by_age_bucket[0].value.as_deref(),
            Some("lt_5m")
        );
        assert_eq!(
            gate_summary.by_resource_namespace[0].value.as_deref(),
            Some("ci")
        );

        let satisfied_gate = store
            .decide_approval_gate(
                "agate_test",
                "satisfied",
                Some("lucas".to_string()),
                Some("reviewed smoke evidence".to_string()),
            )
            .await
            .unwrap();
        let satisfied_gates = store
            .list_approval_gates(crate::ApprovalGateListFilter {
                remediation_plan_id: Some("rplan_test".to_string()),
                incident_id: Some("inc_test".to_string()),
                run_id: Some(run_id),
                status: Some("satisfied".to_string()),
                gate_kind: Some("pipeline_mutation".to_string()),
                risk_level: Some("high".to_string()),
                resource_namespace: Some("ci".to_string()),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: Some("build-app".to_string()),
                created_after_ms: Some(0),
                created_before_ms: None,
                limit: 10,
                offset: 0,
            })
            .await
            .unwrap();

        assert_eq!(satisfied_gate.status, "satisfied");
        assert_eq!(satisfied_gate.decided_by.as_deref(), Some("lucas"));
        assert_eq!(
            satisfied_gate.decision_reason.as_deref(),
            Some("reviewed smoke evidence")
        );
        assert!(satisfied_gate.decided_at.is_some());
        assert_eq!(satisfied_gates.len(), 1);
    }

    #[tokio::test]
    async fn persists_lists_and_revokes_permission_grants() {
        let store = SqliteStore::connect_in_memory().await.unwrap();

        let grant = store
            .create_permission_grant(crate::CreatePermissionGrant {
                id: "grant_test".to_string(),
                subject: "agent:local-worker".to_string(),
                reason: "allow local write smoke".to_string(),
                scope_json: serde_json::json!({
                    "environment": "local",
                    "capability_kinds": ["filesystem"]
                }),
                policy_json: serde_json::json!({
                    "policy_mode": "trusted_writes"
                }),
                expires_at: Some("9999999999999".to_string()),
            })
            .await
            .unwrap();

        let listed = store
            .list_permission_grants(Some("active"), 50)
            .await
            .unwrap();
        let revoked = store
            .revoke_permission_grant(
                &grant.id,
                Some("tester".to_string()),
                Some("done".to_string()),
            )
            .await
            .unwrap();

        assert_eq!(grant.status, "active");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "grant_test");
        assert_eq!(revoked.status, "revoked");
        assert_eq!(revoked.revoked_by.as_deref(), Some("tester"));
        assert!(store
            .list_permission_grants(Some("active"), 50)
            .await
            .unwrap()
            .is_empty());

        let stale_seed = store
            .create_permission_grant(crate::CreatePermissionGrant {
                id: "grant_stale".to_string(),
                subject: "agent:local-worker".to_string(),
                reason: "allow bounded plan".to_string(),
                scope_json: serde_json::json!({
                    "environment": "local",
                    "capability_kinds": ["filesystem"],
                    "work_plan_ids": ["wplan_1"]
                }),
                policy_json: serde_json::json!({
                    "policy_mode": "trusted_writes"
                }),
                expires_at: None,
            })
            .await
            .unwrap();
        let staled = store
            .stale_permission_grant(
                &stale_seed.id,
                Some("planner".to_string()),
                Some("work plan changed".to_string()),
            )
            .await
            .unwrap();
        let stale_grants = store
            .list_permission_grants(Some("stale"), 50)
            .await
            .unwrap();

        assert_eq!(staled.status, "stale");
        assert_eq!(staled.revoked_by.as_deref(), Some("planner"));
        assert_eq!(staled.revoke_reason.as_deref(), Some("work plan changed"));
        assert_eq!(stale_grants.len(), 1);
        assert_eq!(stale_grants[0].id, "grant_stale");
    }

    #[tokio::test]
    async fn persists_and_filters_audit_events() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let session_id = SessionId::new("ses_audit");
        let run_id = RunId::new("run_audit");

        store
            .create_session(CreateSession {
                id: session_id.clone(),
                title: "audit".to_string(),
                cwd: ".".to_string(),
            })
            .await
            .unwrap();
        store
            .create_run(CreateRun {
                id: run_id.clone(),
                session_id,
                user_task: "audit task".to_string(),
                cwd: ".".to_string(),
                max_turns: 10,
                initial_status: "queued".to_string(),
                execution_target_json: serde_json::json!({"kind":"local_process"}),
            })
            .await
            .unwrap();

        let created = store
            .create_audit_event(CreateAuditEvent {
                id: "aud_1".to_string(),
                kind: "permission_grant.used".to_string(),
                actor: Some("agent:local-worker".to_string()),
                resource_kind: "permission_grant".to_string(),
                resource_id: "pgrant_1".to_string(),
                run_id: Some(run_id.clone()),
                payload_json: serde_json::json!({
                    "grant_id": "pgrant_1",
                    "run_scope": {
                        "namespace": "apps-dev",
                        "repo": "team/pharness",
                        "branch": "main",
                        "production_impacting": false
                    }
                }),
            })
            .await
            .unwrap();
        let by_resource = store
            .list_audit_events(Some("permission_grant"), Some("pgrant_1"), None, 50)
            .await
            .unwrap();
        let by_run = store
            .list_audit_events(None, None, Some(&run_id), 50)
            .await
            .unwrap();
        let searched = store
            .query_audit_events(crate::AuditEventListFilter {
                actor: Some("agent:local-worker".to_string()),
                namespace: Some("apps-dev".to_string()),
                repo: Some("team/pharness".to_string()),
                branch: Some("main".to_string()),
                production_impacting: Some(false),
                search: Some("pgrant_1".to_string()),
                limit: 50,
                ..crate::AuditEventListFilter::default()
            })
            .await
            .unwrap();

        assert_eq!(created.kind, "permission_grant.used");
        assert_eq!(by_resource.len(), 1);
        assert_eq!(by_run[0].payload_json["grant_id"], "pgrant_1");
        assert_eq!(searched.len(), 1);
    }

    #[tokio::test]
    async fn persists_work_item_workspace_and_work_item_backed_work_plan() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let item = store
            .create_work_item(crate::CreateWorkItem {
                id: "witem_test".to_string(),
                status: "planning".to_string(),
                title: "Test work item".to_string(),
                intent: "Make a focused change".to_string(),
                acceptance_criteria: vec!["test passes".to_string()],
                source_repo: "team/finance-api".to_string(),
                source_ref: "main".to_string(),
                gitops_repo: None,
                gitops_ref: None,
                target_environment: "dev".to_string(),
                target_namespace: Some("apps-dev".to_string()),
                argo_application: Some("finance-api".to_string()),
                production_impacting: false,
                max_attempts: 2,
                max_elapsed_seconds: 900,
                created_by: Some("operator".to_string()),
            })
            .await
            .unwrap();
        let session_id = SessionId::new("ses_work_item_test");
        store
            .create_session(CreateSession {
                id: session_id.clone(),
                title: "work item test".to_string(),
                cwd: "work-item/witem_test".to_string(),
            })
            .await
            .unwrap();
        let work_plan = store
            .create_work_plan(crate::CreateWorkPlan {
                id: "wplan_work_item_test".to_string(),
                work_item_id: Some(item.id.clone()),
                remediation_plan_id: None,
                incident_id: None,
                session_id,
                run_id: None,
                status: "draft".to_string(),
                title: "work item plan".to_string(),
                summary: "test".to_string(),
                risk_level: "medium".to_string(),
                requires_approval: true,
                resource_namespace: Some("apps-dev".to_string()),
                resource_kind: Some("application".to_string()),
                resource_name: Some("finance-api".to_string()),
                work_plan_json: serde_json::json!({}),
            })
            .await
            .unwrap();
        let workspace = store
            .create_workspace(crate::CreateWorkspace {
                id: "ws_test".to_string(),
                work_item_id: item.id.clone(),
                run_id: None,
                status: "declared".to_string(),
                source_repo: item.source_repo.clone(),
                source_ref: item.source_ref.clone(),
                resolved_commit: None,
                branch: None,
                retention_status: "ephemeral".to_string(),
                actor: Some("operator".to_string()),
                reason: Some("test".to_string()),
            })
            .await
            .unwrap();

        assert_eq!(
            store
                .get_work_plan_by_work_item(&item.id)
                .await
                .unwrap()
                .unwrap()
                .id,
            work_plan.id
        );
        assert_eq!(
            store
                .list_workspaces(crate::WorkspaceListFilter {
                    work_item_id: Some(item.id),
                    limit: 10,
                    ..crate::WorkspaceListFilter::default()
                })
                .await
                .unwrap()[0]
                .id,
            workspace.id
        );
    }
}

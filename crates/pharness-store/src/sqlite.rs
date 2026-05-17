use crate::{
    CreateApproval, CreateArtifact, CreateFileChange, CreateRun, CreateSession, StoredApproval,
    StoredArtifact, StoredFileChange, StoredRun,
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
        let resume_messages_json = approval
            .resume_messages_json
            .map(|value| serde_json::to_string(&value))
            .transpose()?;

        sqlx::query(
            r#"
            INSERT INTO approvals (
              id, session_id, run_id, status, kind, summary, risk_level,
              requested_at, action_json, resume_messages_json, turns_completed
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
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
        .bind(action_json)
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
        let row = sqlx::query(
            r#"
            SELECT id, session_id, run_id, status, kind, summary, risk_level,
                   requested_at, decided_at, decided_by, decision_reason,
                   action_json, resume_messages_json, turns_completed
            FROM approvals
            WHERE run_id = ?1 AND status = 'pending'
            ORDER BY requested_at DESC
            LIMIT 1
            "#,
        )
        .bind(run_id.as_str())
        .fetch_optional(&self.pool)
        .await?;

        row.map(row_to_approval).transpose()
    }

    pub async fn list_approvals(
        &self,
        status: Option<&str>,
        limit: u32,
    ) -> Result<Vec<StoredApproval>, StoreError> {
        let limit = i64::from(limit.clamp(1, 200));
        let rows = match status {
            Some(status) => {
                let sql = format!(
                    "{} ORDER BY requested_at DESC LIMIT ?2",
                    approval_select_sql("WHERE status = ?1")
                );
                sqlx::query(&sql)
                    .bind(status)
                    .bind(limit)
                    .fetch_all(&self.pool)
                    .await?
            }
            None => {
                let sql = format!(
                    "{} ORDER BY requested_at DESC LIMIT ?1",
                    approval_select_sql("")
                );
                sqlx::query(&sql).bind(limit).fetch_all(&self.pool).await?
            }
        };

        rows.into_iter().map(row_to_approval).collect()
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

fn row_to_approval(row: sqlx::sqlite::SqliteRow) -> Result<StoredApproval, StoreError> {
    let action_json: Option<String> = row.try_get("action_json")?;
    let resume_messages_json: Option<String> = row.try_get("resume_messages_json")?;
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
        action_json: action_json
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
               action_json, resume_messages_json, turns_completed
        FROM approvals
        {where_clause}
        "#
    )
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
    #[error("{entity} not found: {id}")]
    NotFound { entity: String, id: String },
}

#[cfg(test)]
mod tests {
    use super::SqliteStore;
    use crate::{CreateRun, CreateSession};
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
                session_id,
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
                action_json: Some(serde_json::json!({"action":"write_file"})),
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
                action_json: None,
                resume_messages_json: None,
                turns_completed: 1,
            })
            .await
            .unwrap();

        let approvals = store.list_approvals(Some("pending"), 50).await.unwrap();

        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].id, "appr_list");
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
}

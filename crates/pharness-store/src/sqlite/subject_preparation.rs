use super::{now_string, SqliteStore, StoreError};
use crate::{
    CompleteSubjectEnvironmentPreparation, CreateSubjectEnvironmentPreparation,
    CreateSubjectWorkspace, StoredSubjectEnvironmentPreparation, StoredSubjectWorkspace,
};
use pharness_core::RunId;
use sqlx::Row;

impl SqliteStore {
    pub async fn create_subject_workspace(
        &self,
        workspace: CreateSubjectWorkspace,
    ) -> Result<StoredSubjectWorkspace, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            INSERT INTO subject_workspaces (
              id, subject_kind, subject_id, run_id, status, source_repo, source_ref,
              source_commit, branch, retention_status, created_at, updated_at,
              status_changed_at, status_changed_by, status_reason
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11, ?11, ?12, ?13)
            "#,
        )
        .bind(&workspace.id)
        .bind(&workspace.subject_kind)
        .bind(&workspace.subject_id)
        .bind(workspace.run_id.as_ref().map(RunId::as_str))
        .bind(&workspace.status)
        .bind(&workspace.source_repo)
        .bind(&workspace.source_ref)
        .bind(&workspace.source_commit)
        .bind(&workspace.branch)
        .bind(&workspace.retention_status)
        .bind(&now)
        .bind(&workspace.actor)
        .bind(&workspace.reason)
        .execute(&self.pool)
        .await?;
        self.get_subject_workspace(&workspace.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "subject_workspace".into(),
                id: workspace.id,
            })
    }

    pub async fn get_subject_workspace(
        &self,
        id: &str,
    ) -> Result<Option<StoredSubjectWorkspace>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, subject_kind, subject_id, run_id, status, source_repo, source_ref,
                   source_commit, resolved_commit, branch, retention_status, created_at,
                   updated_at, status_changed_at, status_changed_by, status_reason
            FROM subject_workspaces WHERE id = ?1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_subject_workspace).transpose()
    }

    pub async fn create_subject_environment_preparation(
        &self,
        preparation: CreateSubjectEnvironmentPreparation,
    ) -> Result<StoredSubjectEnvironmentPreparation, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            INSERT INTO subject_environment_preparations (
              id, subject_kind, subject_id, workspace_id, run_id, status,
              environment_profile_id, source_commit, input_hash, input_json,
              started_at, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11, ?11)
            "#,
        )
        .bind(&preparation.id)
        .bind(&preparation.subject_kind)
        .bind(&preparation.subject_id)
        .bind(&preparation.workspace_id)
        .bind(preparation.run_id.as_ref().map(RunId::as_str))
        .bind(&preparation.status)
        .bind(&preparation.environment_profile_id)
        .bind(&preparation.source_commit)
        .bind(&preparation.input_hash)
        .bind(serde_json::to_string(&preparation.input)?)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        self.get_subject_environment_preparation(&preparation.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "subject_environment_preparation".into(),
                id: preparation.id,
            })
    }

    pub async fn get_subject_environment_preparation(
        &self,
        id: &str,
    ) -> Result<Option<StoredSubjectEnvironmentPreparation>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, subject_kind, subject_id, workspace_id, run_id, status,
                   environment_profile_id, source_commit, input_hash, input_json,
                   repository_contract_json, repository_contract_hash,
                   environment_snapshot_json, acceptance_results_json, logs_json,
                   error_code, started_at, finished_at, created_at, updated_at
            FROM subject_environment_preparations WHERE id = ?1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_subject_preparation).transpose()
    }

    pub async fn latest_subject_environment_preparation(
        &self,
        subject_kind: &str,
        subject_id: &str,
    ) -> Result<Option<StoredSubjectEnvironmentPreparation>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, subject_kind, subject_id, workspace_id, run_id, status,
                   environment_profile_id, source_commit, input_hash, input_json,
                   repository_contract_json, repository_contract_hash,
                   environment_snapshot_json, acceptance_results_json, logs_json,
                   error_code, started_at, finished_at, created_at, updated_at
            FROM subject_environment_preparations
            WHERE subject_kind = ?1 AND subject_id = ?2
            ORDER BY created_at DESC, id DESC LIMIT 1
            "#,
        )
        .bind(subject_kind)
        .bind(subject_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_subject_preparation).transpose()
    }

    pub async fn complete_subject_environment_preparation(
        &self,
        outcome: CompleteSubjectEnvironmentPreparation,
    ) -> Result<StoredSubjectEnvironmentPreparation, StoreError> {
        if !matches!(outcome.status.as_str(), "succeeded" | "failed") {
            return Err(StoreError::Conflict(
                "subject environment preparation must finish succeeded or failed".into(),
            ));
        }
        let now = now_string();
        let result = sqlx::query(
            r#"
            UPDATE subject_environment_preparations
            SET status = ?2, repository_contract_json = ?3,
                repository_contract_hash = ?4, environment_snapshot_json = ?5,
                acceptance_results_json = ?6, logs_json = ?7, error_code = ?8,
                finished_at = ?9, updated_at = ?9
            WHERE id = ?1 AND status IN ('queued', 'running')
            "#,
        )
        .bind(&outcome.id)
        .bind(&outcome.status)
        .bind(
            outcome
                .repository_contract
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
        )
        .bind(&outcome.repository_contract_hash)
        .bind(
            outcome
                .environment_snapshot
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
        )
        .bind(serde_json::to_string(&outcome.acceptance_results)?)
        .bind(serde_json::to_string(&outcome.logs)?)
        .bind(&outcome.error_code)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "subject environment preparation is already terminal".into(),
            ));
        }
        let preparation = self
            .get_subject_environment_preparation(&outcome.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "subject_environment_preparation".into(),
                id: outcome.id,
            })?;
        sqlx::query(
            r#"
            UPDATE subject_workspaces
            SET status = ?2, resolved_commit = COALESCE(?3, resolved_commit),
                updated_at = ?4, status_changed_at = ?4,
                status_changed_by = 'controller', status_reason = ?5
            WHERE id = ?1
            "#,
        )
        .bind(&preparation.workspace_id)
        .bind(if preparation.status == "succeeded" {
            "prepared"
        } else {
            "failed"
        })
        .bind(outcome.resolved_commit)
        .bind(&now)
        .bind(if preparation.status == "succeeded" {
            "subject environment preparation succeeded"
        } else {
            "subject environment preparation failed"
        })
        .execute(&self.pool)
        .await?;
        Ok(preparation)
    }
}

fn row_to_subject_workspace(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredSubjectWorkspace, StoreError> {
    Ok(StoredSubjectWorkspace {
        id: row.try_get("id")?,
        subject_kind: row.try_get("subject_kind")?,
        subject_id: row.try_get("subject_id")?,
        run_id: row.try_get::<Option<String>, _>("run_id")?.map(RunId::new),
        status: row.try_get("status")?,
        source_repo: row.try_get("source_repo")?,
        source_ref: row.try_get("source_ref")?,
        source_commit: row.try_get("source_commit")?,
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

fn row_to_subject_preparation(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredSubjectEnvironmentPreparation, StoreError> {
    Ok(StoredSubjectEnvironmentPreparation {
        id: row.try_get("id")?,
        subject_kind: row.try_get("subject_kind")?,
        subject_id: row.try_get("subject_id")?,
        workspace_id: row.try_get("workspace_id")?,
        run_id: row.try_get::<Option<String>, _>("run_id")?.map(RunId::new),
        status: row.try_get("status")?,
        environment_profile_id: row.try_get("environment_profile_id")?,
        source_commit: row.try_get("source_commit")?,
        input_hash: row.try_get("input_hash")?,
        input: serde_json::from_str(&row.try_get::<String, _>("input_json")?)?,
        repository_contract: row
            .try_get::<Option<String>, _>("repository_contract_json")?
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
        repository_contract_hash: row.try_get("repository_contract_hash")?,
        environment_snapshot: row
            .try_get::<Option<String>, _>("environment_snapshot_json")?
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
        acceptance_results: serde_json::from_str(
            &row.try_get::<String, _>("acceptance_results_json")?,
        )?,
        logs: serde_json::from_str(&row.try_get::<String, _>("logs_json")?)?,
        error_code: row.try_get("error_code")?,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::SqliteStore;
    use crate::{
        CompleteSubjectEnvironmentPreparation, CreateSubjectEnvironmentPreparation,
        CreateSubjectWorkspace,
    };
    use serde_json::json;
    use sqlx::Row;

    #[tokio::test]
    async fn repository_readiness_uses_subject_scoped_workspace_and_preparation() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let workspace = store
            .create_subject_workspace(CreateSubjectWorkspace {
                id: "sws_ready".into(),
                subject_kind: "repository_readiness".into(),
                subject_id: "repo_test:aaaaaaaa".into(),
                run_id: None,
                status: "provisioning".into(),
                source_repo: "https://github.com/example/repo.git".into(),
                source_ref: "main".into(),
                source_commit: "a".repeat(40),
                branch: None,
                retention_status: "ephemeral".into(),
                actor: "operator".into(),
                reason: "assess readiness".into(),
            })
            .await
            .unwrap();
        let preparation = store
            .create_subject_environment_preparation(CreateSubjectEnvironmentPreparation {
                id: "sprep_ready".into(),
                subject_kind: workspace.subject_kind.clone(),
                subject_id: workspace.subject_id.clone(),
                workspace_id: workspace.id.clone(),
                run_id: None,
                status: "queued".into(),
                environment_profile_id: "python-3.11".into(),
                source_commit: workspace.source_commit.clone(),
                input_hash: "sha256:input".into(),
                input: json!({"repository_id":"repo_test"}),
            })
            .await
            .unwrap();
        let completed = store
            .complete_subject_environment_preparation(CompleteSubjectEnvironmentPreparation {
                id: preparation.id,
                status: "succeeded".into(),
                resolved_commit: Some(workspace.source_commit.clone()),
                repository_contract: Some(json!({"api_version":"pharness.dev/v1alpha1"})),
                repository_contract_hash: Some("sha256:contract".into()),
                environment_snapshot: Some(json!({"source_sha":workspace.source_commit})),
                acceptance_results: json!([{"name":"unit","status":"passed"}]),
                logs: json!([{"step":"preparation","status":"succeeded"}]),
                error_code: None,
            })
            .await
            .unwrap();
        assert_eq!(completed.status, "succeeded");
        assert_eq!(
            store
                .get_subject_workspace(&workspace.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            "prepared"
        );
        let refreshed = store
            .create_subject_environment_preparation(CreateSubjectEnvironmentPreparation {
                id: "sprep_ready_refresh".into(),
                subject_kind: workspace.subject_kind.clone(),
                subject_id: workspace.subject_id.clone(),
                workspace_id: workspace.id.clone(),
                run_id: None,
                status: "queued".into(),
                environment_profile_id: "python-3.11".into(),
                source_commit: workspace.source_commit.clone(),
                input_hash: "sha256:refreshed-input".into(),
                input: json!({"repository_id":"repo_test","reason":"runner_refresh"}),
            })
            .await
            .unwrap();
        assert_eq!(
            store
                .latest_subject_environment_preparation(
                    &workspace.subject_kind,
                    &workspace.subject_id,
                )
                .await
                .unwrap()
                .unwrap()
                .id,
            refreshed.id
        );
        for table in [
            "environment_preparations",
            "subject_environment_preparations",
        ] {
            let rows = sqlx::query(&format!("PRAGMA index_list('{table}')"))
                .fetch_all(&store.pool)
                .await
                .unwrap();
            let index = rows
                .iter()
                .find(|row| {
                    row.try_get::<String, _>("name")
                        .unwrap()
                        .ends_with("environment_preparations_workspace")
                })
                .unwrap();
            assert_eq!(index.try_get::<i64, _>("unique").unwrap(), 0);
        }
        assert!(store
            .complete_subject_environment_preparation(CompleteSubjectEnvironmentPreparation {
                id: completed.id,
                status: "failed".into(),
                resolved_commit: None,
                repository_contract: None,
                repository_contract_hash: None,
                environment_snapshot: None,
                acceptance_results: json!([]),
                logs: json!([]),
                error_code: Some("stale".into()),
            },)
            .await
            .is_err());
    }
}

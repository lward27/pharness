use super::{now_string, SqliteStore, StoreError};
use crate::{CreateRepositoryOnboarding, StoredRepositoryDiscovery, StoredRepositoryOnboarding};
use sqlx::{Row, Sqlite, Transaction};

impl SqliteStore {
    pub async fn create_repository_onboarding(
        &self,
        onboarding: CreateRepositoryOnboarding,
    ) -> Result<StoredRepositoryOnboarding, StoreError> {
        let now = now_string();
        let mut tx = self.pool.begin().await?;
        insert_repository_onboarding(&mut tx, &onboarding, &now).await?;
        tx.commit().await?;
        self.get_repository_onboarding(&onboarding.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "repository_onboarding".into(),
                id: onboarding.id,
            })
    }

    pub async fn get_repository_onboarding(
        &self,
        id: &str,
    ) -> Result<Option<StoredRepositoryOnboarding>, StoreError> {
        let row = sqlx::query(&onboarding_select_sql("WHERE id = ?1"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_onboarding).transpose()
    }

    pub async fn list_repository_onboardings(
        &self,
        repository_id: &str,
    ) -> Result<Vec<StoredRepositoryOnboarding>, StoreError> {
        let rows = sqlx::query(&onboarding_select_sql(
            "WHERE repository_id = ?1 ORDER BY created_at DESC, id DESC",
        ))
        .bind(repository_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_onboarding).collect()
    }

    pub async fn create_repository_discovery(
        &self,
        id: &str,
        onboarding_id: &str,
        source_commit: &str,
    ) -> Result<StoredRepositoryDiscovery, StoreError> {
        let now = now_string();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO repository_discoveries (
              id, onboarding_id, source_commit, status, schema_version, created_at, updated_at
            ) VALUES (?1, ?2, ?3, 'queued', 'pharness.dev/repository-discovery/v1alpha1', ?4, ?4)
            "#,
        )
        .bind(id)
        .bind(onboarding_id)
        .bind(source_commit)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        let updated = sqlx::query(
            r#"
            UPDATE repository_onboardings
            SET current_discovery_id = ?2,
                status = 'discovery_queued',
                state_version = state_version + 1,
                updated_at = ?3,
                status_changed_at = ?3,
                status_changed_by = 'controller',
                status_reason = 'deterministic discovery queued'
            WHERE id = ?1 AND status IN ('registered', 'discovery_failed')
            "#,
        )
        .bind(onboarding_id)
        .bind(id)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "repository onboarding is not ready for discovery".into(),
            ));
        }
        tx.commit().await?;
        self.get_repository_discovery(id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "repository_discovery".into(),
                id: id.into(),
            })
    }

    pub async fn get_repository_discovery(
        &self,
        id: &str,
    ) -> Result<Option<StoredRepositoryDiscovery>, StoreError> {
        let row = sqlx::query(&discovery_select_sql("WHERE id = ?1"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_discovery).transpose()
    }

    pub async fn finish_repository_discovery(
        &self,
        id: &str,
        resolved_commit: &str,
        inventory: &serde_json::Value,
        content_hash: &str,
    ) -> Result<StoredRepositoryDiscovery, StoreError> {
        let now = now_string();
        let discovery =
            self.get_repository_discovery(id)
                .await?
                .ok_or_else(|| StoreError::NotFound {
                    entity: "repository_discovery".into(),
                    id: id.into(),
                })?;
        if discovery.source_commit != resolved_commit {
            return Err(StoreError::Conflict(
                "repository discovery resolved a different source commit".into(),
            ));
        }
        let mut tx = self.pool.begin().await?;
        let updated = sqlx::query(
            r#"
            UPDATE repository_discoveries
            SET resolved_commit = ?2,
                status = 'succeeded',
                inventory_json = ?3,
                content_hash = ?4,
                started_at = COALESCE(started_at, ?5),
                finished_at = ?5,
                updated_at = ?5
            WHERE id = ?1 AND status IN ('queued', 'running')
            "#,
        )
        .bind(id)
        .bind(resolved_commit)
        .bind(serde_json::to_string(inventory)?)
        .bind(content_hash)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "repository discovery is already terminal".into(),
            ));
        }
        sqlx::query(
            r#"
            UPDATE repository_onboardings
            SET resolved_commit = ?2,
                status = 'discovered',
                state_version = state_version + 1,
                updated_at = ?3,
                status_changed_at = ?3,
                status_changed_by = 'controller',
                status_reason = 'deterministic discovery completed'
            WHERE id = ?1 AND current_discovery_id = ?4
            "#,
        )
        .bind(&discovery.onboarding_id)
        .bind(resolved_commit)
        .bind(&now)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.get_repository_discovery(id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "repository_discovery".into(),
                id: id.into(),
            })
    }

    pub async fn fail_repository_discovery(
        &self,
        id: &str,
        error_code: &str,
        error_summary: &str,
    ) -> Result<StoredRepositoryDiscovery, StoreError> {
        let now = now_string();
        let discovery =
            self.get_repository_discovery(id)
                .await?
                .ok_or_else(|| StoreError::NotFound {
                    entity: "repository_discovery".into(),
                    id: id.into(),
                })?;
        let mut tx = self.pool.begin().await?;
        let updated = sqlx::query(
            r#"
            UPDATE repository_discoveries
            SET status = 'failed', error_code = ?2, error_summary = ?3,
                started_at = COALESCE(started_at, ?4), finished_at = ?4, updated_at = ?4
            WHERE id = ?1 AND status IN ('queued', 'running')
            "#,
        )
        .bind(id)
        .bind(error_code)
        .bind(error_summary)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "repository discovery is already terminal".into(),
            ));
        }
        sqlx::query(
            r#"
            UPDATE repository_onboardings
            SET status = 'discovery_failed', state_version = state_version + 1,
                blockers_json = json_array(json_object('code', ?2, 'summary', ?3)),
                updated_at = ?4, status_changed_at = ?4,
                status_changed_by = 'controller', status_reason = ?3
            WHERE id = ?1 AND current_discovery_id = ?5
            "#,
        )
        .bind(&discovery.onboarding_id)
        .bind(error_code)
        .bind(error_summary)
        .bind(&now)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.get_repository_discovery(id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "repository_discovery".into(),
                id: id.into(),
            })
    }
}

pub(super) async fn insert_repository_onboarding(
    tx: &mut Transaction<'_, Sqlite>,
    onboarding: &CreateRepositoryOnboarding,
    now: &str,
) -> Result<(), StoreError> {
    sqlx::query(
        r#"
        INSERT INTO repository_onboardings (
          id, product_id, repository_id, binding_id, onboarding_kind, status,
          registered_commit, state_version, blockers_json, created_by, creation_reason,
          created_at, updated_at, status_changed_at, status_changed_by, status_reason
        ) VALUES (?1, ?2, ?3, ?4, ?5, 'registered', ?6, 1, '[]', ?7, ?8, ?9, ?9, ?9, ?7, ?8)
        "#,
    )
    .bind(&onboarding.id)
    .bind(&onboarding.product_id)
    .bind(&onboarding.repository_id)
    .bind(&onboarding.binding_id)
    .bind(&onboarding.onboarding_kind)
    .bind(&onboarding.registered_commit)
    .bind(&onboarding.actor)
    .bind(&onboarding.reason)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn onboarding_select_sql(where_clause: &str) -> String {
    format!(
        "SELECT id, product_id, repository_id, binding_id, onboarding_kind, status, \
         registered_commit, resolved_commit, current_discovery_id, current_proposal_revision, \
         approved_proposal_hash, source_delivery_intent_id, contract_version_id, state_version, \
         blockers_json, created_by, creation_reason, created_at, updated_at, status_changed_at, \
         status_changed_by, status_reason FROM repository_onboardings {where_clause}"
    )
}

fn discovery_select_sql(where_clause: &str) -> String {
    format!(
        "SELECT id, onboarding_id, source_commit, resolved_commit, status, schema_version, \
         inventory_json, content_hash, error_code, error_summary, started_at, finished_at, \
         created_at, updated_at FROM repository_discoveries {where_clause}"
    )
}

fn row_to_onboarding(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredRepositoryOnboarding, StoreError> {
    let blockers: String = row.try_get("blockers_json")?;
    Ok(StoredRepositoryOnboarding {
        id: row.try_get("id")?,
        product_id: row.try_get("product_id")?,
        repository_id: row.try_get("repository_id")?,
        binding_id: row.try_get("binding_id")?,
        onboarding_kind: row.try_get("onboarding_kind")?,
        status: row.try_get("status")?,
        registered_commit: row.try_get("registered_commit")?,
        resolved_commit: row.try_get("resolved_commit")?,
        current_discovery_id: row.try_get("current_discovery_id")?,
        current_proposal_revision: row.try_get::<i64, _>("current_proposal_revision")? as u64,
        approved_proposal_hash: row.try_get("approved_proposal_hash")?,
        source_delivery_intent_id: row.try_get("source_delivery_intent_id")?,
        contract_version_id: row.try_get("contract_version_id")?,
        state_version: row.try_get::<i64, _>("state_version")? as u64,
        blockers: serde_json::from_str(&blockers)?,
        created_by: row.try_get("created_by")?,
        creation_reason: row.try_get("creation_reason")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        status_changed_at: row.try_get("status_changed_at")?,
        status_changed_by: row.try_get("status_changed_by")?,
        status_reason: row.try_get("status_reason")?,
    })
}

fn row_to_discovery(row: sqlx::sqlite::SqliteRow) -> Result<StoredRepositoryDiscovery, StoreError> {
    let inventory: Option<String> = row.try_get("inventory_json")?;
    Ok(StoredRepositoryDiscovery {
        id: row.try_get("id")?,
        onboarding_id: row.try_get("onboarding_id")?,
        source_commit: row.try_get("source_commit")?,
        resolved_commit: row.try_get("resolved_commit")?,
        status: row.try_get("status")?,
        schema_version: row.try_get("schema_version")?,
        inventory_json: inventory
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
        content_hash: row.try_get("content_hash")?,
        error_code: row.try_get("error_code")?,
        error_summary: row.try_get("error_summary")?,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

use super::{now_string, SqliteStore, StoreError};
use crate::{
    ApproveRepositoryOnboardingProposal, CreateRepositoryContractVersion,
    CreateRepositoryOnboarding, CreateRepositoryOnboardingProposal,
    CreateRepositoryReadinessAssessment, StoredRepositoryContractVersion,
    StoredRepositoryDiscovery, StoredRepositoryOnboarding, StoredRepositoryOnboardingProposal,
    StoredRepositoryReadinessAssessment,
};
use sqlx::{Row, Sqlite, Transaction};

impl SqliteStore {
    pub async fn start_repository_onboarding_proposer(
        &self,
        onboarding_id: &str,
        expected_state_version: u64,
        run_id: &str,
        profile_hash: &str,
        actor: &str,
        reason: &str,
    ) -> Result<StoredRepositoryOnboarding, StoreError> {
        let now = now_string();
        let result = sqlx::query(
            r#"
            UPDATE repository_onboardings
            SET status = 'proposal_running', proposer_run_id = ?3,
                proposer_profile_hash = ?4, proposer_stop_reason = NULL,
                blockers_json = '[]',
                state_version = state_version + 1, updated_at = ?5,
                status_changed_at = ?5, status_changed_by = ?6, status_reason = ?7
            WHERE id = ?1 AND state_version = ?2 AND status IN ('discovered', 'proposal_failed')
            "#,
        )
        .bind(onboarding_id)
        .bind(i64::try_from(expected_state_version).unwrap_or(i64::MAX))
        .bind(run_id)
        .bind(profile_hash)
        .bind(&now)
        .bind(actor)
        .bind(reason)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "repository onboarding is no longer ready for its proposer".into(),
            ));
        }
        self.get_repository_onboarding(onboarding_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "repository_onboarding".into(),
                id: onboarding_id.into(),
            })
    }

    pub async fn fail_repository_onboarding_proposer(
        &self,
        onboarding_id: &str,
        run_id: &str,
        stop_reason: &str,
    ) -> Result<StoredRepositoryOnboarding, StoreError> {
        let now = now_string();
        let result = sqlx::query(
            r#"
            UPDATE repository_onboardings
            SET status = 'proposal_failed', proposer_stop_reason = ?3,
                blockers_json = json_array(json_object(
                  'code', 'onboarding_proposer_failed', 'summary', ?3
                )), state_version = state_version + 1, updated_at = ?4,
                status_changed_at = ?4, status_changed_by = 'controller', status_reason = ?3
            WHERE id = ?1 AND proposer_run_id = ?2 AND status = 'proposal_running'
            "#,
        )
        .bind(onboarding_id)
        .bind(run_id)
        .bind(stop_reason)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "repository onboarding proposer is no longer current".into(),
            ));
        }
        self.get_repository_onboarding(onboarding_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "repository_onboarding".into(),
                id: onboarding_id.into(),
            })
    }

    pub async fn start_repository_onboarding_patch(
        &self,
        onboarding_id: &str,
        expected_state_version: u64,
        execution_id: &str,
        actor: &str,
        reason: &str,
    ) -> Result<StoredRepositoryOnboarding, StoreError> {
        let now = now_string();
        let result = sqlx::query(
            r#"
            UPDATE repository_onboardings
            SET status = 'patch_queued', patch_execution_id = ?3,
                patch_artifact_id = NULL, patch_hash = NULL,
                state_version = state_version + 1, updated_at = ?4,
                status_changed_at = ?4, status_changed_by = ?5, status_reason = ?6
            WHERE id = ?1 AND state_version = ?2 AND status IN ('proposal_approved', 'patch_failed')
            "#,
        )
        .bind(onboarding_id)
        .bind(i64::try_from(expected_state_version).unwrap_or(i64::MAX))
        .bind(execution_id)
        .bind(&now)
        .bind(actor)
        .bind(reason)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "repository onboarding is no longer ready to materialize its patch".into(),
            ));
        }
        self.get_repository_onboarding(onboarding_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "repository_onboarding".into(),
                id: onboarding_id.into(),
            })
    }

    pub async fn finish_repository_onboarding_patch(
        &self,
        onboarding_id: &str,
        execution_id: &str,
        artifact_id: &str,
        patch_hash: &str,
    ) -> Result<StoredRepositoryOnboarding, StoreError> {
        let now = now_string();
        let result = sqlx::query(
            r#"
            UPDATE repository_onboardings
            SET status = 'delivery_ready', patch_artifact_id = ?3, patch_hash = ?4,
                state_version = state_version + 1, blockers_json = '[]', updated_at = ?5,
                status_changed_at = ?5, status_changed_by = 'controller',
                status_reason = 'controller materialized the approved onboarding patch'
            WHERE id = ?1 AND patch_execution_id = ?2 AND status = 'patch_queued'
            "#,
        )
        .bind(onboarding_id)
        .bind(execution_id)
        .bind(artifact_id)
        .bind(patch_hash)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "repository onboarding patch execution is no longer current".into(),
            ));
        }
        self.get_repository_onboarding(onboarding_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "repository_onboarding".into(),
                id: onboarding_id.into(),
            })
    }

    pub async fn fail_repository_onboarding_patch(
        &self,
        onboarding_id: &str,
        execution_id: &str,
        error_code: &str,
    ) -> Result<StoredRepositoryOnboarding, StoreError> {
        let now = now_string();
        let result = sqlx::query(
            r#"
            UPDATE repository_onboardings
            SET status = 'patch_failed', blockers_json = json_array(json_object(
                  'code', ?3, 'summary', 'approved onboarding patch materialization failed'
                )), state_version = state_version + 1, updated_at = ?4,
                status_changed_at = ?4, status_changed_by = 'controller',
                status_reason = 'approved onboarding patch materialization failed'
            WHERE id = ?1 AND patch_execution_id = ?2 AND status = 'patch_queued'
            "#,
        )
        .bind(onboarding_id)
        .bind(execution_id)
        .bind(error_code)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "repository onboarding patch execution is no longer current".into(),
            ));
        }
        self.get_repository_onboarding(onboarding_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "repository_onboarding".into(),
                id: onboarding_id.into(),
            })
    }

    pub async fn bind_repository_onboarding_source_delivery(
        &self,
        onboarding_id: &str,
        expected_state_version: u64,
        intent_id: &str,
        actor: &str,
        reason: &str,
    ) -> Result<StoredRepositoryOnboarding, StoreError> {
        let now = now_string();
        let result = sqlx::query(
            r#"
            UPDATE repository_onboardings
            SET status = 'delivery_authorized', source_delivery_intent_id = ?3,
                state_version = state_version + 1, updated_at = ?4,
                status_changed_at = ?4, status_changed_by = ?5, status_reason = ?6
            WHERE id = ?1 AND state_version = ?2 AND status = 'delivery_ready'
                  AND source_delivery_intent_id IS NULL
            "#,
        )
        .bind(onboarding_id)
        .bind(i64::try_from(expected_state_version).unwrap_or(i64::MAX))
        .bind(intent_id)
        .bind(&now)
        .bind(actor)
        .bind(reason)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "repository onboarding source delivery is no longer authorizable".into(),
            ));
        }
        self.get_repository_onboarding(onboarding_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "repository_onboarding".into(),
                id: onboarding_id.into(),
            })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_repository_onboarding_source_delivery(
        &self,
        onboarding_id: &str,
        intent_id: &str,
        status: &str,
        resolved_commit: Option<&str>,
        actor: &str,
        reason: &str,
    ) -> Result<StoredRepositoryOnboarding, StoreError> {
        if !matches!(
            status,
            "writer_dispatched"
                | "waiting_external"
                | "observer_dispatched"
                | "waiting_checks"
                | "waiting_merge"
                | "blocked"
                | "merge_observed"
                | "delivery_failed"
        ) {
            return Err(StoreError::Conflict(
                "invalid repository onboarding source delivery status".into(),
            ));
        }
        let now = now_string();
        let result = sqlx::query(
            r#"
            UPDATE repository_onboardings
            SET status = ?3, resolved_commit = COALESCE(?4, resolved_commit),
                state_version = state_version + 1, updated_at = ?5,
                status_changed_at = ?5, status_changed_by = ?6, status_reason = ?7
            WHERE id = ?1 AND source_delivery_intent_id = ?2
            "#,
        )
        .bind(onboarding_id)
        .bind(intent_id)
        .bind(status)
        .bind(resolved_commit)
        .bind(&now)
        .bind(actor)
        .bind(reason)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "repository onboarding source delivery is no longer current".into(),
            ));
        }
        self.get_repository_onboarding(onboarding_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "repository_onboarding".into(),
                id: onboarding_id.into(),
            })
    }

    pub async fn start_repository_onboarding_contract_validation(
        &self,
        onboarding_id: &str,
        expected_state_version: u64,
        execution_id: &str,
        actor: &str,
        reason: &str,
    ) -> Result<StoredRepositoryOnboarding, StoreError> {
        let now = now_string();
        let result = sqlx::query(
            r#"
            UPDATE repository_onboardings
            SET status = 'validation_queued', validation_execution_id = ?3,
                validation_stop_reason = NULL, state_version = state_version + 1,
                updated_at = ?4, status_changed_at = ?4,
                status_changed_by = ?5, status_reason = ?6
            WHERE id = ?1 AND state_version = ?2
                  AND status IN ('merge_observed', 'validation_failed')
                  AND resolved_commit IS NOT NULL
            "#,
        )
        .bind(onboarding_id)
        .bind(i64::try_from(expected_state_version).unwrap_or(i64::MAX))
        .bind(execution_id)
        .bind(&now)
        .bind(actor)
        .bind(reason)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "repository onboarding is no longer ready for merged contract validation".into(),
            ));
        }
        self.get_repository_onboarding(onboarding_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "repository_onboarding".into(),
                id: onboarding_id.into(),
            })
    }

    pub async fn fail_repository_onboarding_contract_validation(
        &self,
        onboarding_id: &str,
        execution_id: &str,
        stop_reason: &str,
    ) -> Result<StoredRepositoryOnboarding, StoreError> {
        let now = now_string();
        let result = sqlx::query(
            r#"
            UPDATE repository_onboardings
            SET status = 'validation_failed', validation_stop_reason = ?3,
                blockers_json = json_array(json_object(
                  'code', 'merged_contract_validation_failed', 'summary', ?3
                )), state_version = state_version + 1, updated_at = ?4,
                status_changed_at = ?4, status_changed_by = 'controller',
                status_reason = ?3
            WHERE id = ?1 AND validation_execution_id = ?2
                  AND status = 'validation_queued'
            "#,
        )
        .bind(onboarding_id)
        .bind(execution_id)
        .bind(stop_reason)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "repository onboarding contract validation is no longer current".into(),
            ));
        }
        self.get_repository_onboarding(onboarding_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "repository_onboarding".into(),
                id: onboarding_id.into(),
            })
    }

    pub async fn complete_repository_onboarding_contract_validation(
        &self,
        execution_id: &str,
        version: CreateRepositoryContractVersion,
    ) -> Result<StoredRepositoryOnboarding, StoreError> {
        let now = now_string();
        let api_version = version
            .contract
            .get("api_version")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| StoreError::InvalidData("contract has no api_version".into()))?;
        let mut tx = self.pool.begin().await?;
        let current = sqlx::query(
            "SELECT repository_id, resolved_commit, status, validation_execution_id FROM repository_onboardings WHERE id = ?1",
        )
        .bind(&version.onboarding_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StoreError::NotFound {
            entity: "repository_onboarding".into(),
            id: version.onboarding_id.clone(),
        })?;
        if current.try_get::<String, _>("repository_id")? != version.repository_id
            || current
                .try_get::<Option<String>, _>("resolved_commit")?
                .as_deref()
                != Some(version.source_commit.as_str())
            || current.try_get::<String, _>("status")? != "validation_queued"
            || current
                .try_get::<Option<String>, _>("validation_execution_id")?
                .as_deref()
                != Some(execution_id)
        {
            return Err(StoreError::Conflict(
                "repository onboarding contract validation provenance changed".into(),
            ));
        }
        sqlx::query(
            r#"
            INSERT INTO repository_contract_versions (
              id, repository_id, onboarding_id, source_commit, contract_path, api_version,
              contract_json, content_hash, merge_provenance_json, status, created_at
            ) VALUES (?1, ?2, ?3, ?4, '.pharness/repository.yaml', ?5, ?6, ?7, ?8, 'active', ?9)
            "#,
        )
        .bind(&version.id)
        .bind(&version.repository_id)
        .bind(&version.onboarding_id)
        .bind(&version.source_commit)
        .bind(api_version)
        .bind(serde_json::to_string(&version.contract)?)
        .bind(&version.content_hash)
        .bind(serde_json::to_string(&version.merge_provenance)?)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            UPDATE repository_onboardings
            SET status = 'contract_ready', contract_version_id = ?3,
                validation_stop_reason = NULL, blockers_json = '[]',
                state_version = state_version + 1, updated_at = ?4,
                status_changed_at = ?4, status_changed_by = 'controller',
                status_reason = 'merged canonical RepositoryContract validated'
            WHERE id = ?1 AND validation_execution_id = ?2 AND status = 'validation_queued'
            "#,
        )
        .bind(&version.onboarding_id)
        .bind(execution_id)
        .bind(&version.id)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            UPDATE repositories
            SET registered_commit = ?2, state_version = state_version + 1, updated_at = ?3
            WHERE id = ?1
            "#,
        )
        .bind(&version.repository_id)
        .bind(&version.source_commit)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.get_repository_onboarding(&version.onboarding_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "repository_onboarding".into(),
                id: version.onboarding_id,
            })
    }

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

    pub async fn list_repository_onboardings_awaiting_isolated_job(
        &self,
    ) -> Result<Vec<StoredRepositoryOnboarding>, StoreError> {
        let rows = sqlx::query(&onboarding_select_sql(
            "WHERE status IN ('patch_queued', 'validation_queued') ORDER BY updated_at, id",
        ))
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

    pub async fn latest_successful_repository_discovery(
        &self,
        repository_id: &str,
        source_commit: &str,
    ) -> Result<Option<StoredRepositoryDiscovery>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT d.id, d.onboarding_id, d.source_commit, d.resolved_commit, d.status,
                   d.schema_version, d.inventory_json, d.content_hash, d.error_code,
                   d.error_summary, d.started_at, d.finished_at, d.created_at, d.updated_at
            FROM repository_discoveries d
            JOIN repository_onboardings o ON o.id = d.onboarding_id
            WHERE o.repository_id = ?1 AND d.source_commit = ?2 AND d.status = 'succeeded'
            ORDER BY d.finished_at DESC, d.id DESC LIMIT 1
            "#,
        )
        .bind(repository_id)
        .bind(source_commit)
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
                blockers_json = '[]',
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

impl SqliteStore {
    pub async fn create_repository_onboarding_proposal(
        &self,
        proposal: CreateRepositoryOnboardingProposal,
    ) -> Result<StoredRepositoryOnboardingProposal, StoreError> {
        let now = now_string();
        let mut tx = self.pool.begin().await?;
        let onboarding = sqlx::query(
            "SELECT state_version, current_discovery_id, current_proposal_revision, status FROM repository_onboardings WHERE id = ?1",
        )
        .bind(&proposal.onboarding_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StoreError::NotFound {
            entity: "repository_onboarding".into(),
            id: proposal.onboarding_id.clone(),
        })?;
        let state_version = onboarding.try_get::<i64, _>("state_version")? as u64;
        let discovery_id: Option<String> = onboarding.try_get("current_discovery_id")?;
        let status: String = onboarding.try_get("status")?;
        if state_version != proposal.expected_state_version
            || discovery_id.as_deref() != Some(proposal.discovery_id.as_str())
            || !matches!(
                status.as_str(),
                "discovered" | "proposal_running" | "proposal_ready"
            )
        {
            return Err(StoreError::Conflict(
                "repository onboarding changed after proposal preview".into(),
            ));
        }
        let discovery =
            sqlx::query("SELECT status, content_hash FROM repository_discoveries WHERE id = ?1")
                .bind(&proposal.discovery_id)
                .fetch_one(&mut *tx)
                .await?;
        let discovery_status: String = discovery.try_get("status")?;
        let discovery_hash: Option<String> = discovery.try_get("content_hash")?;
        if discovery_status != "succeeded"
            || discovery_hash.as_deref() != Some(proposal.discovery_hash.as_str())
        {
            return Err(StoreError::Conflict(
                "proposal does not reference current validated discovery evidence".into(),
            ));
        }
        let revision = onboarding.try_get::<i64, _>("current_proposal_revision")? + 1;
        sqlx::query(
            r#"
            INSERT INTO repository_onboarding_proposals (
              id, onboarding_id, revision, status, proposal_json, content_hash,
              discovery_id, discovery_hash, created_by, origin, created_at
            ) VALUES (?1, ?2, ?3, 'proposed', ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
        )
        .bind(&proposal.id)
        .bind(&proposal.onboarding_id)
        .bind(revision)
        .bind(serde_json::to_string(&proposal.proposal)?)
        .bind(&proposal.content_hash)
        .bind(&proposal.discovery_id)
        .bind(&proposal.discovery_hash)
        .bind(&proposal.actor)
        .bind(&proposal.origin)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            UPDATE repository_onboardings
            SET status = 'proposal_ready', current_proposal_revision = ?2,
                state_version = state_version + 1, updated_at = ?3,
                status_changed_at = ?3, status_changed_by = ?4,
                status_reason = 'onboarding proposal revision created'
            WHERE id = ?1 AND state_version = ?5
            "#,
        )
        .bind(&proposal.onboarding_id)
        .bind(revision)
        .bind(&now)
        .bind(&proposal.actor)
        .bind(proposal.expected_state_version as i64)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.get_repository_onboarding_proposal(&proposal.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "repository_onboarding_proposal".into(),
                id: proposal.id,
            })
    }

    pub async fn get_repository_onboarding_proposal(
        &self,
        id: &str,
    ) -> Result<Option<StoredRepositoryOnboardingProposal>, StoreError> {
        let row = sqlx::query(&proposal_select_sql("WHERE id = ?1"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_proposal).transpose()
    }

    pub async fn get_current_repository_onboarding_proposal(
        &self,
        onboarding_id: &str,
    ) -> Result<Option<StoredRepositoryOnboardingProposal>, StoreError> {
        let row = sqlx::query(&proposal_select_sql(
            "WHERE onboarding_id = ?1 ORDER BY revision DESC LIMIT 1",
        ))
        .bind(onboarding_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_proposal).transpose()
    }

    pub async fn approve_repository_onboarding_proposal(
        &self,
        approval: ApproveRepositoryOnboardingProposal,
    ) -> Result<StoredRepositoryOnboarding, StoreError> {
        let now = now_string();
        let mut tx = self.pool.begin().await?;
        let proposal = sqlx::query(
            "SELECT onboarding_id, status, content_hash FROM repository_onboarding_proposals WHERE id = ?1",
        )
        .bind(&approval.proposal_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StoreError::NotFound {
            entity: "repository_onboarding_proposal".into(),
            id: approval.proposal_id.clone(),
        })?;
        if proposal.try_get::<String, _>("onboarding_id")? != approval.onboarding_id
            || proposal.try_get::<String, _>("status")? != "proposed"
            || proposal.try_get::<String, _>("content_hash")? != approval.proposal_hash
        {
            return Err(StoreError::Conflict(
                "onboarding proposal approval does not match the proposed revision".into(),
            ));
        }
        if let Some(change) = approval.model_change {
            let onboarding = sqlx::query(
                "SELECT product_id, binding_id FROM repository_onboardings WHERE id = ?1",
            )
            .bind(&approval.onboarding_id)
            .fetch_one(&mut *tx)
            .await?;
            if onboarding.try_get::<String, _>("product_id")? != change.product_id
                || onboarding.try_get::<String, _>("binding_id")? != change.binding_id
            {
                return Err(StoreError::Conflict(
                    "onboarding product-model change is outside the approved subject".into(),
                ));
            }
            for service in &change.services {
                sqlx::query(
                    r#"
                    INSERT INTO services (
                      id, product_id, service_key, display_name, description, status,
                      created_at, updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?6)
                    "#,
                )
                .bind(&service.id)
                .bind(&change.product_id)
                .bind(&service.service_key)
                .bind(&service.display_name)
                .bind(&service.description)
                .bind(&now)
                .execute(&mut *tx)
                .await?;
            }
            if let Some(revision_id) = &change.binding_revision_id {
                let content_hash = change.binding_content_hash.as_deref().ok_or_else(|| {
                    StoreError::InvalidData(
                        "binding revision change has no canonical content hash".into(),
                    )
                })?;
                let next_revision = sqlx::query(
                    "SELECT COALESCE(MAX(revision), 0) + 1 AS revision FROM repository_binding_revisions WHERE binding_id = ?1",
                )
                .bind(&change.binding_id)
                .fetch_one(&mut *tx)
                .await?
                .try_get::<i64, _>("revision")?;
                sqlx::query(
                    r#"
                    INSERT INTO repository_binding_revisions (
                      id, binding_id, revision, service_ids_json, scopes_json, status,
                      evidence_json, content_hash, reviewed_by, review_reason, created_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, 'reviewed', ?6, ?7, ?8, ?9, ?10)
                    "#,
                )
                .bind(revision_id)
                .bind(&change.binding_id)
                .bind(next_revision)
                .bind(serde_json::to_string(&change.binding_service_ids)?)
                .bind(serde_json::to_string(&change.binding_scopes)?)
                .bind(serde_json::to_string(&change.binding_evidence)?)
                .bind(content_hash)
                .bind(&approval.actor)
                .bind(&approval.reason)
                .bind(&now)
                .execute(&mut *tx)
                .await?;
                let updated = sqlx::query(
                    "UPDATE repository_bindings SET current_revision_id = ?2, updated_at = ?3 WHERE id = ?1 AND product_id = ?4",
                )
                .bind(&change.binding_id)
                .bind(revision_id)
                .bind(&now)
                .bind(&change.product_id)
                .execute(&mut *tx)
                .await?;
                if updated.rows_affected() != 1 {
                    return Err(StoreError::Conflict(
                        "onboarding repository binding changed outside its Product".into(),
                    ));
                }
            } else if change.binding_content_hash.is_some()
                || !change.binding_service_ids.is_empty()
                || !change.binding_scopes.is_empty()
            {
                return Err(StoreError::InvalidData(
                    "binding revision material was supplied without a revision ID".into(),
                ));
            }
            sqlx::query(
                r#"
                INSERT INTO product_model_snapshots (
                  id, product_id, version, model_json, content_hash,
                  created_by, creation_reason, created_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                "#,
            )
            .bind(&change.snapshot_id)
            .bind(&change.product_id)
            .bind((change.expected_product_state_version + 1) as i64)
            .bind(serde_json::to_string(&change.snapshot)?)
            .bind(&change.snapshot_hash)
            .bind(&approval.actor)
            .bind(&approval.reason)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
            let updated = sqlx::query(
                r#"
                UPDATE products
                SET state_version = state_version + 1,
                    current_model_snapshot_id = ?3,
                    updated_at = ?4
                WHERE id = ?1 AND state_version = ?2
                "#,
            )
            .bind(&change.product_id)
            .bind(change.expected_product_state_version as i64)
            .bind(&change.snapshot_id)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
            if updated.rows_affected() != 1 {
                return Err(StoreError::Conflict(
                    "Product changed after onboarding proposal preview".into(),
                ));
            }
        }
        let updated = sqlx::query(
            r#"
            UPDATE repository_onboardings
            SET status = 'proposal_approved', approved_proposal_hash = ?2,
                state_version = state_version + 1, updated_at = ?3,
                status_changed_at = ?3, status_changed_by = ?4, status_reason = ?5
            WHERE id = ?1 AND state_version = ?6 AND status = 'proposal_ready'
            "#,
        )
        .bind(&approval.onboarding_id)
        .bind(&approval.proposal_hash)
        .bind(&now)
        .bind(&approval.actor)
        .bind(&approval.reason)
        .bind(approval.expected_state_version as i64)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "repository onboarding changed after approval preview".into(),
            ));
        }
        sqlx::query("UPDATE repository_onboarding_proposals SET status = 'approved' WHERE id = ?1")
            .bind(&approval.proposal_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        self.get_repository_onboarding(&approval.onboarding_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "repository_onboarding".into(),
                id: approval.onboarding_id,
            })
    }

    pub async fn create_repository_contract_version(
        &self,
        version: CreateRepositoryContractVersion,
    ) -> Result<StoredRepositoryContractVersion, StoreError> {
        let now = now_string();
        let api_version = version
            .contract
            .get("api_version")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| StoreError::InvalidData("contract has no api_version".into()))?;
        sqlx::query(
            r#"
            INSERT INTO repository_contract_versions (
              id, repository_id, onboarding_id, source_commit, contract_path, api_version,
              contract_json, content_hash, merge_provenance_json, status, created_at
            ) VALUES (?1, ?2, ?3, ?4, '.pharness/repository.yaml', ?5, ?6, ?7, ?8, 'active', ?9)
            "#,
        )
        .bind(&version.id)
        .bind(&version.repository_id)
        .bind(&version.onboarding_id)
        .bind(&version.source_commit)
        .bind(api_version)
        .bind(serde_json::to_string(&version.contract)?)
        .bind(&version.content_hash)
        .bind(serde_json::to_string(&version.merge_provenance)?)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        self.get_repository_contract_version(&version.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "repository_contract_version".into(),
                id: version.id,
            })
    }

    pub async fn get_repository_contract_version(
        &self,
        id: &str,
    ) -> Result<Option<StoredRepositoryContractVersion>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, repository_id, onboarding_id, source_commit, contract_path, api_version,
                   contract_json, content_hash, merge_provenance_json, status, created_at
            FROM repository_contract_versions WHERE id = ?1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_contract_version).transpose()
    }

    pub async fn latest_repository_contract_version(
        &self,
        repository_id: &str,
        source_commit: &str,
    ) -> Result<Option<StoredRepositoryContractVersion>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, repository_id, onboarding_id, source_commit, contract_path, api_version,
                   contract_json, content_hash, merge_provenance_json, status, created_at
            FROM repository_contract_versions
            WHERE repository_id = ?1 AND source_commit = ?2 AND status = 'active'
            ORDER BY created_at DESC, id DESC LIMIT 1
            "#,
        )
        .bind(repository_id)
        .bind(source_commit)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_contract_version).transpose()
    }

    pub async fn create_repository_readiness_assessment(
        &self,
        assessment: CreateRepositoryReadinessAssessment,
    ) -> Result<StoredRepositoryReadinessAssessment, StoreError> {
        let now = now_string();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO repository_readiness_assessments (
              id, repository_id, source_commit, contract_version_id, contract_hash,
              dependency_lock_hash, environment_profile_id, environment_profile_revision,
              runner_image_digest, validation_policy_version, contract_status, coding_status,
              checks_json, blockers_json, warnings_json, evidence_refs_json, input_hash,
              content_hash, assessed_at, expires_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                      ?14, ?15, ?16, ?17, ?18, ?19, ?20)
            "#,
        )
        .bind(&assessment.id)
        .bind(&assessment.repository_id)
        .bind(&assessment.source_commit)
        .bind(&assessment.contract_version_id)
        .bind(&assessment.contract_hash)
        .bind(&assessment.dependency_lock_hash)
        .bind(&assessment.environment_profile_id)
        .bind(&assessment.environment_profile_revision)
        .bind(&assessment.runner_image_digest)
        .bind(&assessment.validation_policy_version)
        .bind(&assessment.contract_status)
        .bind(&assessment.coding_status)
        .bind(serde_json::to_string(&assessment.checks)?)
        .bind(serde_json::to_string(&assessment.blockers)?)
        .bind(serde_json::to_string(&assessment.warnings)?)
        .bind(serde_json::to_string(&assessment.evidence_refs)?)
        .bind(&assessment.input_hash)
        .bind(&assessment.content_hash)
        .bind(&now)
        .bind(&assessment.expires_at)
        .execute(&mut *tx)
        .await?;
        if assessment.contract_status == "ready" && assessment.coding_status == "ready" {
            sqlx::query(
                r#"
                UPDATE repository_onboardings
                SET status = 'ready', readiness_assessment_id = ?1,
                    state_version = state_version + 1, updated_at = ?2,
                    status_changed_at = ?2, status_changed_by = 'controller',
                    status_reason = 'exact repository contract and coding readiness validated'
                WHERE id = (
                  SELECT id FROM repository_onboardings
                  WHERE repository_id = ?3 AND resolved_commit = ?4
                    AND contract_version_id = ?5 AND status = 'contract_ready'
                  ORDER BY created_at DESC, id DESC LIMIT 1
                )
                "#,
            )
            .bind(&assessment.id)
            .bind(&now)
            .bind(&assessment.repository_id)
            .bind(&assessment.source_commit)
            .bind(&assessment.contract_version_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        self.get_repository_readiness_assessment(&assessment.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "repository_readiness_assessment".into(),
                id: assessment.id,
            })
    }

    pub async fn get_repository_readiness_assessment(
        &self,
        id: &str,
    ) -> Result<Option<StoredRepositoryReadinessAssessment>, StoreError> {
        let row = sqlx::query(&readiness_select_sql("WHERE id = ?1"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_readiness).transpose()
    }

    pub async fn latest_repository_readiness_assessment(
        &self,
        repository_id: &str,
        source_commit: &str,
    ) -> Result<Option<StoredRepositoryReadinessAssessment>, StoreError> {
        let row = sqlx::query(&readiness_select_sql(
            "WHERE repository_id = ?1 AND source_commit = ?2 ORDER BY assessed_at DESC, id DESC LIMIT 1",
        ))
        .bind(repository_id)
        .bind(source_commit)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_readiness).transpose()
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
         approved_proposal_hash, source_delivery_intent_id, contract_version_id, readiness_assessment_id, proposer_run_id, \
         proposer_profile_hash, proposer_stop_reason, patch_execution_id, patch_artifact_id, \
         patch_hash, validation_execution_id, validation_stop_reason, state_version, \
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

fn proposal_select_sql(where_clause: &str) -> String {
    format!(
        "SELECT id, onboarding_id, revision, status, proposal_json, content_hash, discovery_id, \
         discovery_hash, created_by, origin, created_at FROM repository_onboarding_proposals {where_clause}"
    )
}

fn readiness_select_sql(where_clause: &str) -> String {
    format!(
        "SELECT id, repository_id, source_commit, contract_version_id, contract_hash, \
         dependency_lock_hash, environment_profile_id, environment_profile_revision, \
         runner_image_digest, validation_policy_version, contract_status, coding_status, \
         checks_json, blockers_json, warnings_json, evidence_refs_json, input_hash, content_hash, \
         assessed_at, expires_at FROM repository_readiness_assessments {where_clause}"
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
        readiness_assessment_id: row.try_get("readiness_assessment_id")?,
        proposer_run_id: row.try_get("proposer_run_id")?,
        proposer_profile_hash: row.try_get("proposer_profile_hash")?,
        proposer_stop_reason: row.try_get("proposer_stop_reason")?,
        patch_execution_id: row.try_get("patch_execution_id")?,
        patch_artifact_id: row.try_get("patch_artifact_id")?,
        patch_hash: row.try_get("patch_hash")?,
        validation_execution_id: row.try_get("validation_execution_id")?,
        validation_stop_reason: row.try_get("validation_stop_reason")?,
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

fn row_to_proposal(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredRepositoryOnboardingProposal, StoreError> {
    Ok(StoredRepositoryOnboardingProposal {
        id: row.try_get("id")?,
        onboarding_id: row.try_get("onboarding_id")?,
        revision: row.try_get::<i64, _>("revision")? as u64,
        status: row.try_get("status")?,
        proposal: serde_json::from_str(&row.try_get::<String, _>("proposal_json")?)?,
        content_hash: row.try_get("content_hash")?,
        discovery_id: row.try_get("discovery_id")?,
        discovery_hash: row.try_get("discovery_hash")?,
        created_by: row.try_get("created_by")?,
        origin: row.try_get("origin")?,
        created_at: row.try_get("created_at")?,
    })
}

fn row_to_contract_version(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredRepositoryContractVersion, StoreError> {
    Ok(StoredRepositoryContractVersion {
        id: row.try_get("id")?,
        repository_id: row.try_get("repository_id")?,
        onboarding_id: row.try_get("onboarding_id")?,
        source_commit: row.try_get("source_commit")?,
        contract_path: row.try_get("contract_path")?,
        api_version: row.try_get("api_version")?,
        contract: serde_json::from_str(&row.try_get::<String, _>("contract_json")?)?,
        content_hash: row.try_get("content_hash")?,
        merge_provenance: serde_json::from_str(
            &row.try_get::<String, _>("merge_provenance_json")?,
        )?,
        status: row.try_get("status")?,
        created_at: row.try_get("created_at")?,
    })
}

fn row_to_readiness(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredRepositoryReadinessAssessment, StoreError> {
    Ok(StoredRepositoryReadinessAssessment {
        id: row.try_get("id")?,
        repository_id: row.try_get("repository_id")?,
        source_commit: row.try_get("source_commit")?,
        contract_version_id: row.try_get("contract_version_id")?,
        contract_hash: row.try_get("contract_hash")?,
        dependency_lock_hash: row.try_get("dependency_lock_hash")?,
        environment_profile_id: row.try_get("environment_profile_id")?,
        environment_profile_revision: row.try_get("environment_profile_revision")?,
        runner_image_digest: row.try_get("runner_image_digest")?,
        validation_policy_version: row.try_get("validation_policy_version")?,
        contract_status: row.try_get("contract_status")?,
        coding_status: row.try_get("coding_status")?,
        checks: serde_json::from_str(&row.try_get::<String, _>("checks_json")?)?,
        blockers: serde_json::from_str(&row.try_get::<String, _>("blockers_json")?)?,
        warnings: serde_json::from_str(&row.try_get::<String, _>("warnings_json")?)?,
        evidence_refs: serde_json::from_str(&row.try_get::<String, _>("evidence_refs_json")?)?,
        input_hash: row.try_get("input_hash")?,
        content_hash: row.try_get("content_hash")?,
        assessed_at: row.try_get("assessed_at")?,
        expires_at: row.try_get("expires_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::SqliteStore;
    use crate::{
        ApproveRepositoryOnboardingProposal, ApprovedOnboardingProductModelChange,
        ApprovedOnboardingService, CreateRepositoryContractVersion, CreateRepositoryOnboarding,
        CreateRepositoryReadinessAssessment, CreateRun, CreateSession,
    };
    use pharness_core::{RunId, SessionId};
    use serde_json::json;

    #[tokio::test]
    async fn successful_retries_clear_stale_onboarding_blockers() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let now = "2026-08-24T00:00:00Z";
        let source_commit = "a".repeat(40);
        sqlx::query("INSERT INTO organizations (id, organization_key, display_name, created_at, updated_at) VALUES ('org_discovery', 'discovery', 'Discovery', ?1, ?1)")
            .bind(now).execute(&store.pool).await.unwrap();
        sqlx::query("INSERT INTO products (id, organization_id, product_key, display_name, description, owner_principal, state_version, created_at, updated_at) VALUES ('prod_discovery', 'org_discovery', 'product', 'Product', '', 'operator', 1, ?1, ?1)")
            .bind(now).execute(&store.pool).await.unwrap();
        sqlx::query("INSERT INTO repositories (id, provider, external_id, canonical_url, default_branch, registered_commit, created_at, updated_at) VALUES ('repo_discovery', 'github', '3', 'https://github.com/example/discovery.git', 'main', ?1, ?2, ?2)")
            .bind(&source_commit).bind(now).execute(&store.pool).await.unwrap();
        sqlx::query("INSERT INTO repository_bindings (id, product_id, repository_id, status, created_at, updated_at) VALUES ('rbind_discovery', 'prod_discovery', 'repo_discovery', 'active', ?1, ?1)")
            .bind(now).execute(&store.pool).await.unwrap();
        let onboarding = store
            .create_repository_onboarding(CreateRepositoryOnboarding {
                id: "ronb_discovery".into(),
                product_id: "prod_discovery".into(),
                repository_id: "repo_discovery".into(),
                binding_id: "rbind_discovery".into(),
                onboarding_kind: "initial".into(),
                registered_commit: source_commit.clone(),
                actor: "operator".into(),
                reason: "register".into(),
            })
            .await
            .unwrap();

        store
            .create_repository_discovery("rdisc_failed", &onboarding.id, &source_commit)
            .await
            .unwrap();
        store
            .fail_repository_discovery("rdisc_failed", "git_fetch_failed", "fetch failed")
            .await
            .unwrap();
        store
            .create_repository_discovery("rdisc_success", &onboarding.id, &source_commit)
            .await
            .unwrap();
        store
            .finish_repository_discovery(
                "rdisc_success",
                &source_commit,
                &json!({"inventory": []}),
                "sha256:discovery",
            )
            .await
            .unwrap();

        let discovered = store
            .get_repository_onboarding(&onboarding.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(discovered.status, "discovered");
        assert!(discovered.blockers.is_empty());

        let session_id = SessionId::new("ses_discovery");
        store
            .create_session(CreateSession {
                id: session_id.clone(),
                title: "Discovery proposer".into(),
                cwd: "/workspace".into(),
            })
            .await
            .unwrap();
        for run_id in ["run_failed", "run_retry"] {
            store
                .create_run(CreateRun {
                    id: RunId::new(run_id),
                    session_id: session_id.clone(),
                    user_task: "propose onboarding".into(),
                    cwd: "/workspace".into(),
                    max_turns: 16,
                    initial_status: "queued".into(),
                    execution_target_json: json!({}),
                })
                .await
                .unwrap();
        }
        store
            .start_repository_onboarding_proposer(
                &discovered.id,
                discovered.state_version,
                "run_failed",
                "sha256:profile",
                "operator",
                "start proposer",
            )
            .await
            .unwrap();
        let failed = store
            .fail_repository_onboarding_proposer(
                &discovered.id,
                "run_failed",
                "worker startup failed",
            )
            .await
            .unwrap();
        assert!(!failed.blockers.is_empty());
        let retried = store
            .start_repository_onboarding_proposer(
                &discovered.id,
                failed.state_version,
                "run_retry",
                "sha256:profile",
                "operator",
                "retry proposer",
            )
            .await
            .unwrap();
        assert_eq!(retried.status, "proposal_running");
        assert!(retried.blockers.is_empty());
    }

    #[tokio::test]
    async fn onboarding_approval_atomically_applies_reviewed_product_model_changes() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let now = "2026-08-24T00:00:00Z";
        sqlx::query("INSERT INTO organizations (id, organization_key, display_name, created_at, updated_at) VALUES ('org_model', 'model', 'Model', ?1, ?1)")
            .bind(now).execute(&store.pool).await.unwrap();
        sqlx::query("INSERT INTO products (id, organization_id, product_key, display_name, description, owner_principal, state_version, created_at, updated_at) VALUES ('prod_model', 'org_model', 'product', 'Product', '', 'operator', 1, ?1, ?1)")
            .bind(now).execute(&store.pool).await.unwrap();
        sqlx::query("INSERT INTO repositories (id, provider, external_id, canonical_url, default_branch, registered_commit, created_at, updated_at) VALUES ('repo_model', 'github', '2', 'https://github.com/example/model.git', 'main', ?1, ?2, ?2)")
            .bind("a".repeat(40)).bind(now).execute(&store.pool).await.unwrap();
        sqlx::query("INSERT INTO repository_bindings (id, product_id, repository_id, status, created_at, updated_at) VALUES ('rbind_model', 'prod_model', 'repo_model', 'active', ?1, ?1)")
            .bind(now).execute(&store.pool).await.unwrap();
        let onboarding = store
            .create_repository_onboarding(CreateRepositoryOnboarding {
                id: "ronb_model".into(),
                product_id: "prod_model".into(),
                repository_id: "repo_model".into(),
                binding_id: "rbind_model".into(),
                onboarding_kind: "initial".into(),
                registered_commit: "a".repeat(40),
                actor: "operator".into(),
                reason: "register".into(),
            })
            .await
            .unwrap();
        sqlx::query("INSERT INTO repository_discoveries (id, onboarding_id, source_commit, resolved_commit, status, schema_version, inventory_json, content_hash, created_at, updated_at) VALUES ('rdisc_model', ?1, ?2, ?2, 'succeeded', 'pharness.dev/repository-discovery/v1alpha1', '{}', 'sha256:discovery', ?3, ?3)")
            .bind(&onboarding.id).bind("a".repeat(40)).bind(now).execute(&store.pool).await.unwrap();
        sqlx::query("INSERT INTO repository_onboarding_proposals (id, onboarding_id, revision, status, proposal_json, content_hash, discovery_id, discovery_hash, created_by, origin, created_at) VALUES ('rprop_model', ?1, 1, 'proposed', '{}', 'sha256:proposal', 'rdisc_model', 'sha256:discovery', 'operator', 'operator', ?2)")
            .bind(&onboarding.id).bind(now).execute(&store.pool).await.unwrap();
        sqlx::query("UPDATE repository_onboardings SET status = 'proposal_ready', current_discovery_id = 'rdisc_model', current_proposal_revision = 1 WHERE id = ?1")
            .bind(&onboarding.id).execute(&store.pool).await.unwrap();

        let approved = store
            .approve_repository_onboarding_proposal(ApproveRepositoryOnboardingProposal {
                onboarding_id: onboarding.id.clone(),
                proposal_id: "rprop_model".into(),
                proposal_hash: "sha256:proposal".into(),
                expected_state_version: onboarding.state_version,
                actor: "operator".into(),
                reason: "approve model".into(),
                model_change: Some(ApprovedOnboardingProductModelChange {
                    product_id: "prod_model".into(),
                    expected_product_state_version: 1,
                    services: vec![ApprovedOnboardingService {
                        id: "svc_model".into(),
                        service_key: "api".into(),
                        display_name: "API".into(),
                        description: "API service".into(),
                    }],
                    binding_id: "rbind_model".into(),
                    binding_revision_id: Some("rbrev_model".into()),
                    binding_service_ids: vec!["svc_model".into()],
                    binding_scopes: vec!["src/**".into()],
                    binding_evidence: json!({"proposal_id":"rprop_model"}),
                    binding_content_hash: Some("sha256:binding".into()),
                    snapshot_id: "pmodel_model".into(),
                    snapshot: json!({"schema_version":"pharness.dev/product-model/v1alpha1"}),
                    snapshot_hash: "sha256:snapshot".into(),
                }),
            })
            .await
            .unwrap();

        assert_eq!(approved.status, "proposal_approved");
        assert_eq!(
            store
                .list_product_services("prod_model")
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            store
                .get_repository_binding("prod_model", "repo_model")
                .await
                .unwrap()
                .unwrap()
                .current_revision_id,
            "rbrev_model"
        );
        let product = store.get_product("prod_model").await.unwrap().unwrap();
        assert_eq!(product.state_version, 2);
        assert_eq!(product.current_model_snapshot_id, "pmodel_model");
        assert_eq!(
            store
                .get_repository_onboarding_proposal("rprop_model")
                .await
                .unwrap()
                .unwrap()
                .status,
            "approved"
        );
    }

    #[tokio::test]
    async fn onboarding_source_delivery_binds_once_and_tracks_external_wait() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let now = "2026-08-24T00:00:00Z";
        sqlx::query("INSERT INTO organizations (id, organization_key, display_name, created_at, updated_at) VALUES ('org_test', 'test', 'Test', ?1, ?1)")
            .bind(now).execute(&store.pool).await.unwrap();
        sqlx::query("INSERT INTO products (id, organization_id, product_key, display_name, description, owner_principal, created_at, updated_at) VALUES ('prod_test', 'org_test', 'product', 'Product', '', 'operator', ?1, ?1)")
            .bind(now).execute(&store.pool).await.unwrap();
        sqlx::query("INSERT INTO repositories (id, provider, external_id, canonical_url, default_branch, registered_commit, created_at, updated_at) VALUES ('repo_test', 'github', '1', 'https://github.com/example/repo.git', 'main', ?1, ?2, ?2)")
            .bind("a".repeat(40)).bind(now).execute(&store.pool).await.unwrap();
        sqlx::query("INSERT INTO repository_bindings (id, product_id, repository_id, status, created_at, updated_at) VALUES ('rbind_test', 'prod_test', 'repo_test', 'active', ?1, ?1)")
            .bind(now).execute(&store.pool).await.unwrap();
        let onboarding = store
            .create_repository_onboarding(CreateRepositoryOnboarding {
                id: "ronb_test".into(),
                product_id: "prod_test".into(),
                repository_id: "repo_test".into(),
                binding_id: "rbind_test".into(),
                onboarding_kind: "initial".into(),
                registered_commit: "a".repeat(40),
                actor: "operator".into(),
                reason: "register repository".into(),
            })
            .await
            .unwrap();
        sqlx::query("UPDATE repository_onboardings SET status = 'delivery_ready' WHERE id = ?1")
            .bind(&onboarding.id)
            .execute(&store.pool)
            .await
            .unwrap();
        let bound = store
            .bind_repository_onboarding_source_delivery(
                &onboarding.id,
                onboarding.state_version,
                "srcintent_test",
                "operator",
                "deliver approved proposal",
            )
            .await
            .unwrap();
        assert_eq!(bound.status, "delivery_authorized");
        assert_eq!(
            bound.source_delivery_intent_id.as_deref(),
            Some("srcintent_test")
        );
        assert!(store
            .bind_repository_onboarding_source_delivery(
                &onboarding.id,
                onboarding.state_version,
                "srcintent_other",
                "operator",
                "stale duplicate",
            )
            .await
            .is_err());
        let waiting = store
            .update_repository_onboarding_source_delivery(
                &onboarding.id,
                "srcintent_test",
                "waiting_external",
                None,
                "controller:repo-mode",
                "manual merge pending",
            )
            .await
            .unwrap();
        assert_eq!(waiting.status, "waiting_external");
        assert!(store
            .update_repository_onboarding_source_delivery(
                &onboarding.id,
                "srcintent_test",
                "completed",
                None,
                "controller:repo-mode",
                "invalid state",
            )
            .await
            .is_err());

        let merged_commit = "b".repeat(40);
        let merged = store
            .update_repository_onboarding_source_delivery(
                &onboarding.id,
                "srcintent_test",
                "merge_observed",
                Some(&merged_commit),
                "controller:repo-mode",
                "merge provenance verified",
            )
            .await
            .unwrap();
        let validating = store
            .start_repository_onboarding_contract_validation(
                &onboarding.id,
                merged.state_version,
                "onbvalidate_test",
                "operator",
                "validate merged contract",
            )
            .await
            .unwrap();
        assert_eq!(validating.status, "validation_queued");
        assert_eq!(
            store
                .list_repository_onboardings_awaiting_isolated_job()
                .await
                .unwrap()
                .into_iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>(),
            vec![onboarding.id.clone()]
        );
        let completed = store
            .complete_repository_onboarding_contract_validation(
                "onbvalidate_test",
                CreateRepositoryContractVersion {
                    id: "rcontract_test".into(),
                    repository_id: "repo_test".into(),
                    onboarding_id: onboarding.id.clone(),
                    source_commit: merged_commit.clone(),
                    contract: json!({"api_version":"pharness.dev/v1alpha1"}),
                    content_hash: format!("sha256:{}", "c".repeat(64)),
                    merge_provenance: json!({"merge_commit_sha":merged_commit}),
                },
            )
            .await
            .unwrap();
        assert_eq!(completed.status, "contract_ready");
        assert!(store
            .list_repository_onboardings_awaiting_isolated_job()
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            completed.contract_version_id.as_deref(),
            Some("rcontract_test")
        );
        assert_eq!(
            store
                .get_repository("repo_test")
                .await
                .unwrap()
                .unwrap()
                .registered_commit,
            "b".repeat(40)
        );
        let assessment = store
            .create_repository_readiness_assessment(CreateRepositoryReadinessAssessment {
                id: "rready_test".into(),
                repository_id: "repo_test".into(),
                source_commit: "b".repeat(40),
                contract_version_id: Some("rcontract_test".into()),
                contract_hash: Some(format!("sha256:{}", "c".repeat(64))),
                dependency_lock_hash: Some(format!("sha256:{}", "d".repeat(64))),
                environment_profile_id: Some("python-3.11".into()),
                environment_profile_revision: Some("e".repeat(40)),
                runner_image_digest: Some(format!("sha256:{}", "f".repeat(64))),
                validation_policy_version: "repo-mode-v1".into(),
                contract_status: "ready".into(),
                coding_status: "ready".into(),
                checks: json!([]),
                blockers: json!([]),
                warnings: json!([]),
                evidence_refs: json!([]),
                input_hash: format!("sha256:{}", "1".repeat(64)),
                content_hash: format!("sha256:{}", "2".repeat(64)),
                expires_at: None,
            })
            .await
            .unwrap();
        assert_eq!(assessment.coding_status, "ready");
        let ready = store
            .get_repository_onboarding(&onboarding.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ready.status, "ready");
        assert_eq!(
            ready.readiness_assessment_id.as_deref(),
            Some("rready_test")
        );
    }
}

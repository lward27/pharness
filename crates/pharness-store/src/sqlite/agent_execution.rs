use super::{now_string, SqliteStore, StoreError};
use crate::{
    CreateAgentExecutionPolicyQualification, CreateAgentExecutionSelection,
    CreateAgentHostCapabilitySnapshot, CreateAgentHostEnrollment, CreateAgentLease,
    EnrollAgentHost, StoredAgentExecutionPolicyQualification, StoredAgentExecutionSelection,
    StoredAgentHost, StoredAgentHostCapabilitySnapshot, StoredAgentHostEnrollment,
    StoredAgentLease,
};
use pharness_core::RunId;
use sqlx::Row;

impl SqliteStore {
    pub async fn create_agent_execution_policy_qualification(
        &self,
        qualification: CreateAgentExecutionPolicyQualification,
    ) -> Result<StoredAgentExecutionPolicyQualification, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            INSERT INTO agent_execution_policy_qualifications (
              id, policy_id, policy_revision, policy_hash, runtime_revision,
              suite_id, suite_hash, attempts, metrics_json, verdict,
              evidence_artifact_id, actor, reason, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
            "#,
        )
        .bind(&qualification.id)
        .bind(&qualification.policy_id)
        .bind(&qualification.policy_revision)
        .bind(&qualification.policy_hash)
        .bind(&qualification.runtime_revision)
        .bind(&qualification.suite_id)
        .bind(&qualification.suite_hash)
        .bind(i64::from(qualification.attempts))
        .bind(serde_json::to_string(&qualification.metrics)?)
        .bind(&qualification.verdict)
        .bind(&qualification.evidence_artifact_id)
        .bind(&qualification.actor)
        .bind(&qualification.reason)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        self.get_agent_execution_policy_qualification(&qualification.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "agent_execution_policy_qualification".into(),
                id: qualification.id,
            })
    }

    pub async fn get_agent_execution_policy_qualification(
        &self,
        id: &str,
    ) -> Result<Option<StoredAgentExecutionPolicyQualification>, StoreError> {
        let row = sqlx::query("SELECT * FROM agent_execution_policy_qualifications WHERE id = ?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_execution_policy_qualification).transpose()
    }

    pub async fn list_agent_execution_policy_qualifications(
        &self,
        policy_id: &str,
        policy_revision: &str,
    ) -> Result<Vec<StoredAgentExecutionPolicyQualification>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT * FROM agent_execution_policy_qualifications
            WHERE policy_id = ?1 AND policy_revision = ?2
            ORDER BY CAST(created_at AS INTEGER) DESC, id DESC
            "#,
        )
        .bind(policy_id)
        .bind(policy_revision)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(row_to_execution_policy_qualification)
            .collect()
    }

    pub async fn create_agent_execution_selection(
        &self,
        selection: CreateAgentExecutionSelection,
    ) -> Result<StoredAgentExecutionSelection, StoreError> {
        selection
            .resolved_binding
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        let now = now_string();
        let binding = &selection.resolved_binding;
        sqlx::query(
            r#"
            INSERT INTO agent_execution_selections (
              id, subject_kind, subject_id, stage_key, policy_id, policy_revision,
              policy_hash, resolved_binding_json, binding_hash, actor, reason,
              state_hash, supersedes_selection_id, stage_execution_id, run_id, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
            "#,
        )
        .bind(&selection.id)
        .bind(&selection.subject_kind)
        .bind(&selection.subject_id)
        .bind(&selection.stage_key)
        .bind(&binding.policy.policy_id)
        .bind(&binding.policy.revision)
        .bind(&binding.policy.policy_hash)
        .bind(serde_json::to_string(binding)?)
        .bind(&binding.binding_hash)
        .bind(&selection.actor)
        .bind(&selection.reason)
        .bind(&selection.state_hash)
        .bind(&selection.supersedes_selection_id)
        .bind(&selection.stage_execution_id)
        .bind(selection.run_id.as_ref().map(RunId::as_str))
        .bind(&now)
        .execute(&self.pool)
        .await?;
        self.get_agent_execution_selection(&selection.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "agent_execution_selection".into(),
                id: selection.id,
            })
    }

    pub async fn get_agent_execution_selection(
        &self,
        id: &str,
    ) -> Result<Option<StoredAgentExecutionSelection>, StoreError> {
        let row = sqlx::query("SELECT * FROM agent_execution_selections WHERE id = ?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_execution_selection).transpose()
    }

    pub async fn get_agent_execution_selection_for_run(
        &self,
        run_id: &RunId,
    ) -> Result<Option<StoredAgentExecutionSelection>, StoreError> {
        let row = sqlx::query("SELECT * FROM agent_execution_selections WHERE run_id = ?1")
            .bind(run_id.as_str())
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_execution_selection).transpose()
    }

    pub async fn list_agent_execution_selections(
        &self,
        subject_kind: &str,
        subject_id: &str,
    ) -> Result<Vec<StoredAgentExecutionSelection>, StoreError> {
        let rows = sqlx::query(
            "SELECT * FROM agent_execution_selections WHERE subject_kind = ?1 AND subject_id = ?2 ORDER BY created_at, id",
        )
        .bind(subject_kind)
        .bind(subject_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_execution_selection).collect()
    }

    pub async fn create_agent_host_enrollment(
        &self,
        enrollment: CreateAgentHostEnrollment,
    ) -> Result<StoredAgentHostEnrollment, StoreError> {
        let now = now_string();
        let result = sqlx::query(
            r#"
            INSERT INTO agent_host_enrollments (
              id, display_name, host_pool, token_hash, actor, reason, created_at, expires_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
        )
        .bind(&enrollment.id)
        .bind(&enrollment.display_name)
        .bind(&enrollment.host_pool)
        .bind(&enrollment.token_hash)
        .bind(&enrollment.actor)
        .bind(&enrollment.reason)
        .bind(&now)
        .bind(&enrollment.expires_at)
        .execute(&self.pool)
        .await;
        match result {
            Ok(_) => {}
            Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
                return Err(StoreError::Conflict(
                    "agent-host enrollment token was already registered".into(),
                ));
            }
            Err(error) => return Err(error.into()),
        }
        self.get_agent_host_enrollment(&enrollment.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "agent_host_enrollment".into(),
                id: enrollment.id,
            })
    }

    pub async fn get_agent_host_enrollment(
        &self,
        id: &str,
    ) -> Result<Option<StoredAgentHostEnrollment>, StoreError> {
        let row = sqlx::query("SELECT * FROM agent_host_enrollments WHERE id = ?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_host_enrollment).transpose()
    }

    pub async fn enroll_agent_host(
        &self,
        host: EnrollAgentHost,
    ) -> Result<StoredAgentHost, StoreError> {
        let now = now_string();
        let mut tx = self.pool.begin().await?;
        let enrollment = sqlx::query(
            r#"
            SELECT display_name, host_pool
            FROM agent_host_enrollments
            WHERE id = ?1 AND token_hash = ?2 AND consumed_at IS NULL
              AND CAST(expires_at AS INTEGER) > CAST(?3 AS INTEGER)
            "#,
        )
        .bind(&host.enrollment_id)
        .bind(&host.enrollment_token_hash)
        .bind(&now)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            StoreError::Conflict("agent-host enrollment is invalid, expired, or consumed".into())
        })?;
        let display_name: String = enrollment.try_get("display_name")?;
        let host_pool: String = enrollment.try_get("host_pool")?;
        sqlx::query(
            r#"
            INSERT INTO agent_hosts (
              id, display_name, host_pool, lifecycle_state, credential_hash,
              enrollment_id, platform, architecture, created_at, updated_at, last_contact_at
            ) VALUES (?1, ?2, ?3, 'ready', ?4, ?5, ?6, ?7, ?8, ?8, ?8)
            "#,
        )
        .bind(&host.id)
        .bind(display_name)
        .bind(host_pool)
        .bind(&host.credential_hash)
        .bind(&host.enrollment_id)
        .bind(&host.platform)
        .bind(&host.architecture)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        let consumed = sqlx::query(
            r#"
            UPDATE agent_host_enrollments
            SET consumed_at = ?2, consumed_by_host_id = ?3
            WHERE id = ?1 AND consumed_at IS NULL
            "#,
        )
        .bind(&host.enrollment_id)
        .bind(&now)
        .bind(&host.id)
        .execute(&mut *tx)
        .await?;
        if consumed.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "agent-host enrollment was consumed concurrently".into(),
            ));
        }
        tx.commit().await?;
        self.get_agent_host(&host.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "agent_host".into(),
                id: host.id,
            })
    }

    pub async fn get_agent_host(&self, id: &str) -> Result<Option<StoredAgentHost>, StoreError> {
        let row = sqlx::query("SELECT * FROM agent_hosts WHERE id = ?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_host).transpose()
    }

    pub async fn list_agent_hosts(&self) -> Result<Vec<StoredAgentHost>, StoreError> {
        let rows = sqlx::query("SELECT * FROM agent_hosts ORDER BY created_at, id")
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter().map(row_to_host).collect()
    }

    pub async fn agent_host_credential_matches(
        &self,
        id: &str,
        credential_hash: &str,
    ) -> Result<bool, StoreError> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM agent_hosts WHERE id = ?1 AND credential_hash = ?2 AND lifecycle_state != 'retired'",
        )
        .bind(id)
        .bind(credential_hash)
        .fetch_one(&self.pool)
        .await?;
        Ok(count == 1)
    }

    pub async fn heartbeat_agent_host(&self, id: &str) -> Result<StoredAgentHost, StoreError> {
        let now = now_string();
        let result = sqlx::query(
            "UPDATE agent_hosts SET last_contact_at = ?2, updated_at = ?2 WHERE id = ?1 AND lifecycle_state != 'retired'",
        )
        .bind(id)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "agent host is retired or missing".into(),
            ));
        }
        self.get_agent_host(id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "agent_host".into(),
                id: id.into(),
            })
    }

    pub async fn set_agent_host_lifecycle_state(
        &self,
        id: &str,
        state: &str,
    ) -> Result<StoredAgentHost, StoreError> {
        if !matches!(state, "ready" | "draining" | "unavailable" | "retired") {
            return Err(StoreError::InvalidData(
                "invalid agent-host lifecycle state".into(),
            ));
        }
        let now = now_string();
        let retired_at = (state == "retired").then_some(now.as_str());
        let result = sqlx::query(
            r#"
            UPDATE agent_hosts
            SET lifecycle_state = ?2, updated_at = ?3, retired_at = ?4
            WHERE id = ?1
              AND (?2 != 'retired' OR NOT EXISTS (
                SELECT 1 FROM agent_leases
                WHERE host_id = ?1 AND state IN ('claimed', 'running', 'paused')
              ))
            "#,
        )
        .bind(id)
        .bind(state)
        .bind(&now)
        .bind(retired_at)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "agent host cannot be retired while it owns an active or resumable lease".into(),
            ));
        }
        self.get_agent_host(id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "agent_host".into(),
                id: id.into(),
            })
    }

    pub async fn record_agent_host_capability_snapshot(
        &self,
        snapshot: CreateAgentHostCapabilitySnapshot,
    ) -> Result<StoredAgentHostCapabilitySnapshot, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            INSERT INTO agent_host_capability_snapshots (
              id, host_id, platform, architecture, codex_version, podman_version,
              execution_mode, authentication_class, authentication_ready, supported_profiles_json,
              runner_images_json, available_slots, storage_json, status, blockers_json,
              content_hash, created_at, expires_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
            "#,
        )
        .bind(&snapshot.id)
        .bind(&snapshot.host_id)
        .bind(&snapshot.platform)
        .bind(&snapshot.architecture)
        .bind(&snapshot.codex_version)
        .bind(&snapshot.podman_version)
        .bind(&snapshot.execution_mode)
        .bind(&snapshot.authentication_class)
        .bind(snapshot.authentication_ready)
        .bind(serde_json::to_string(&snapshot.supported_profiles)?)
        .bind(serde_json::to_string(&snapshot.runner_images)?)
        .bind(i64::from(snapshot.available_slots))
        .bind(serde_json::to_string(&snapshot.storage)?)
        .bind(&snapshot.status)
        .bind(serde_json::to_string(&snapshot.blockers)?)
        .bind(&snapshot.content_hash)
        .bind(&now)
        .bind(&snapshot.expires_at)
        .execute(&self.pool)
        .await?;
        self.get_agent_host_capability_snapshot(&snapshot.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "agent_host_capability_snapshot".into(),
                id: snapshot.id,
            })
    }

    pub async fn get_agent_host_capability_snapshot(
        &self,
        id: &str,
    ) -> Result<Option<StoredAgentHostCapabilitySnapshot>, StoreError> {
        let row = sqlx::query("SELECT * FROM agent_host_capability_snapshots WHERE id = ?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_capability_snapshot).transpose()
    }

    pub async fn latest_agent_host_capability_snapshot(
        &self,
        host_id: &str,
    ) -> Result<Option<StoredAgentHostCapabilitySnapshot>, StoreError> {
        let row = sqlx::query(
            "SELECT * FROM agent_host_capability_snapshots WHERE host_id = ?1 ORDER BY CAST(created_at AS INTEGER) DESC, id DESC LIMIT 1",
        )
        .bind(host_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_capability_snapshot).transpose()
    }

    pub async fn create_agent_lease(
        &self,
        lease: CreateAgentLease,
    ) -> Result<StoredAgentLease, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            INSERT INTO agent_leases (
              id, run_id, stage_execution_id, host_pool, pinned_host_id, workspace_id,
              environment_profile_id, runner_image, binding_hash, state, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'queued', ?10)
            "#,
        )
        .bind(&lease.id)
        .bind(lease.run_id.as_str())
        .bind(&lease.stage_execution_id)
        .bind(&lease.host_pool)
        .bind(&lease.pinned_host_id)
        .bind(&lease.workspace_id)
        .bind(&lease.environment_profile_id)
        .bind(&lease.runner_image)
        .bind(&lease.binding_hash)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        self.get_agent_lease(&lease.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "agent_lease".into(),
                id: lease.id,
            })
    }

    pub async fn get_agent_lease(&self, id: &str) -> Result<Option<StoredAgentLease>, StoreError> {
        let row = sqlx::query("SELECT * FROM agent_leases WHERE id = ?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_lease).transpose()
    }

    pub async fn get_agent_lease_for_run(
        &self,
        run_id: &RunId,
    ) -> Result<Option<StoredAgentLease>, StoreError> {
        let row = sqlx::query("SELECT * FROM agent_leases WHERE run_id = ?1")
            .bind(run_id.as_str())
            .fetch_optional(&self.pool)
            .await?;
        row.map(row_to_lease).transpose()
    }

    pub async fn agent_lease_token_matches(
        &self,
        lease_id: &str,
        host_id: &str,
        lease_token_hash: &str,
    ) -> Result<bool, StoreError> {
        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) FROM agent_leases
            WHERE id = ?1 AND host_id = ?2 AND lease_token_hash = ?3
              AND state IN ('claimed', 'running', 'paused')
            "#,
        )
        .bind(lease_id)
        .bind(host_id)
        .bind(lease_token_hash)
        .fetch_one(&self.pool)
        .await?;
        Ok(count == 1)
    }

    pub async fn list_agent_leases_for_host(
        &self,
        host_id: &str,
    ) -> Result<Vec<StoredAgentLease>, StoreError> {
        let rows = sqlx::query(
            "SELECT * FROM agent_leases WHERE host_id = ?1 ORDER BY created_at DESC, id DESC",
        )
        .bind(host_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_lease).collect()
    }

    pub async fn latest_agent_lease_for_workspace(
        &self,
        workspace_id: &str,
    ) -> Result<Option<StoredAgentLease>, StoreError> {
        let row = sqlx::query(
            "SELECT * FROM agent_leases WHERE workspace_id = ?1 ORDER BY CAST(created_at AS INTEGER) DESC, id DESC LIMIT 1",
        )
        .bind(workspace_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_lease).transpose()
    }

    pub async fn claim_next_agent_lease(
        &self,
        host_id: &str,
        lease_token_hash: &str,
        expires_at: &str,
    ) -> Result<Option<StoredAgentLease>, StoreError> {
        let now = now_string();
        let mut tx = self.pool.begin().await?;
        let host = sqlx::query(
            "SELECT host_pool FROM agent_hosts WHERE id = ?1 AND lifecycle_state = 'ready'",
        )
        .bind(host_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(host) = host else {
            return Ok(None);
        };
        let host_pool: String = host.try_get("host_pool")?;
        let snapshot = sqlx::query(
            r#"
            SELECT supported_profiles_json, runner_images_json, available_slots
            FROM agent_host_capability_snapshots
            WHERE host_id = ?1 AND status = 'passed' AND authentication_ready = 1
              AND CAST(expires_at AS INTEGER) > CAST(?2 AS INTEGER)
            ORDER BY CAST(created_at AS INTEGER) DESC, id DESC LIMIT 1
            "#,
        )
        .bind(host_id)
        .bind(&now)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(snapshot) = snapshot else {
            return Ok(None);
        };
        let available_slots = snapshot.try_get::<i64, _>("available_slots")?;
        let active: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM agent_leases WHERE host_id = ?1 AND state IN ('claimed', 'running', 'paused')",
        )
        .bind(host_id)
        .fetch_one(&mut *tx)
        .await?;
        if active >= available_slots {
            return Ok(None);
        }
        let profiles: Vec<String> =
            serde_json::from_str(&snapshot.try_get::<String, _>("supported_profiles_json")?)?;
        let runner_images: serde_json::Value =
            serde_json::from_str(&snapshot.try_get::<String, _>("runner_images_json")?)?;
        let candidates = sqlx::query(
            r#"
            SELECT id, environment_profile_id, runner_image
            FROM agent_leases
            WHERE state = 'queued' AND host_id IS NULL AND host_pool = ?1
              AND (pinned_host_id IS NULL OR pinned_host_id = ?2)
            ORDER BY created_at, id
            "#,
        )
        .bind(&host_pool)
        .bind(host_id)
        .fetch_all(&mut *tx)
        .await?;
        for candidate in candidates {
            let lease_id: String = candidate.try_get("id")?;
            let profile: String = candidate.try_get("environment_profile_id")?;
            let image: String = candidate.try_get("runner_image")?;
            let image_matches = runner_images
                .get(&profile)
                .and_then(serde_json::Value::as_str)
                .is_some_and(|supported| supported == image);
            if !profiles.iter().any(|supported| supported == &profile) || !image_matches {
                continue;
            }
            let claimed = sqlx::query(
                r#"
                UPDATE agent_leases
                SET host_id = ?2, state = 'claimed', lease_token_hash = ?3,
                    claimed_at = ?4, heartbeat_at = ?4, expires_at = ?5
                WHERE id = ?1 AND state = 'queued' AND host_id IS NULL
                "#,
            )
            .bind(&lease_id)
            .bind(host_id)
            .bind(lease_token_hash)
            .bind(&now)
            .bind(expires_at)
            .execute(&mut *tx)
            .await?;
            if claimed.rows_affected() == 1 {
                tx.commit().await?;
                return self.get_agent_lease(&lease_id).await;
            }
        }
        tx.commit().await?;
        Ok(None)
    }

    pub async fn mark_agent_lease_running(
        &self,
        lease_id: &str,
        host_id: &str,
        lease_token_hash: &str,
        expires_at: &str,
    ) -> Result<StoredAgentLease, StoreError> {
        self.update_live_agent_lease(lease_id, host_id, lease_token_hash, "running", expires_at)
            .await
    }

    pub async fn heartbeat_agent_lease(
        &self,
        lease_id: &str,
        host_id: &str,
        lease_token_hash: &str,
        expires_at: &str,
    ) -> Result<StoredAgentLease, StoreError> {
        self.update_live_agent_lease(lease_id, host_id, lease_token_hash, "heartbeat", expires_at)
            .await
    }

    pub async fn pause_agent_lease(
        &self,
        lease_id: &str,
        host_id: &str,
        lease_token_hash: &str,
        reason: &str,
    ) -> Result<StoredAgentLease, StoreError> {
        let now = now_string();
        let result = sqlx::query(
            r#"
            UPDATE agent_leases
            SET state = 'paused', error = ?4, heartbeat_at = ?5, expires_at = NULL
            WHERE id = ?1 AND host_id = ?2 AND lease_token_hash = ?3
              AND state IN ('claimed', 'running', 'paused')
            "#,
        )
        .bind(lease_id)
        .bind(host_id)
        .bind(lease_token_hash)
        .bind(reason)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "agent lease cannot be paused by this host".into(),
            ));
        }
        self.get_agent_lease(lease_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "agent_lease".into(),
                id: lease_id.into(),
            })
    }

    async fn update_live_agent_lease(
        &self,
        lease_id: &str,
        host_id: &str,
        lease_token_hash: &str,
        operation: &str,
        expires_at: &str,
    ) -> Result<StoredAgentLease, StoreError> {
        let now = now_string();
        let state_filter = if operation == "running" {
            "('claimed', 'paused', 'running')"
        } else {
            "('claimed', 'running')"
        };
        let query = format!(
            "UPDATE agent_leases SET state = ?4, heartbeat_at = ?5, expires_at = ?6 WHERE id = ?1 AND host_id = ?2 AND lease_token_hash = ?3 AND state IN {state_filter}"
        );
        let result = sqlx::query(&query)
            .bind(lease_id)
            .bind(host_id)
            .bind(lease_token_hash)
            .bind("running")
            .bind(&now)
            .bind(expires_at)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "agent lease is not owned by this host or is not live".into(),
            ));
        }
        self.get_agent_lease(lease_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "agent_lease".into(),
                id: lease_id.into(),
            })
    }

    pub async fn set_agent_lease_remote_thread(
        &self,
        lease_id: &str,
        host_id: &str,
        lease_token_hash: &str,
        remote_thread_id: &str,
    ) -> Result<StoredAgentLease, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE agent_leases SET remote_thread_id = ?4
            WHERE id = ?1 AND host_id = ?2 AND lease_token_hash = ?3
              AND state IN ('claimed', 'running', 'paused')
              AND (remote_thread_id IS NULL OR remote_thread_id = ?4)
            "#,
        )
        .bind(lease_id)
        .bind(host_id)
        .bind(lease_token_hash)
        .bind(remote_thread_id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "agent lease thread cannot be changed after it is bound".into(),
            ));
        }
        self.get_agent_lease(lease_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "agent_lease".into(),
                id: lease_id.into(),
            })
    }

    pub async fn complete_agent_lease(
        &self,
        lease_id: &str,
        host_id: &str,
        lease_token_hash: &str,
        terminal_state: &str,
        completion_hash: Option<&str>,
        error: Option<&str>,
    ) -> Result<StoredAgentLease, StoreError> {
        if !matches!(terminal_state, "completed" | "cancelled" | "failed") {
            return Err(StoreError::InvalidData(
                "invalid agent-lease terminal state".into(),
            ));
        }
        let now = now_string();
        let result = sqlx::query(
            r#"
            UPDATE agent_leases
            SET state = ?4, completion_hash = ?5, error = ?6, completed_at = ?7,
                heartbeat_at = ?7, expires_at = NULL
            WHERE id = ?1 AND host_id = ?2 AND lease_token_hash = ?3
              AND state IN ('claimed', 'running', 'paused')
            "#,
        )
        .bind(lease_id)
        .bind(host_id)
        .bind(lease_token_hash)
        .bind(terminal_state)
        .bind(completion_hash)
        .bind(error)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "agent lease is already terminal or is not owned by this host".into(),
            ));
        }
        self.get_agent_lease(lease_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "agent_lease".into(),
                id: lease_id.into(),
            })
    }

    pub async fn pause_expired_agent_leases(
        &self,
        now: &str,
    ) -> Result<Vec<StoredAgentLease>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id FROM agent_leases
            WHERE state IN ('claimed', 'running') AND expires_at IS NOT NULL
              AND CAST(expires_at AS INTEGER) <= CAST(?1 AS INTEGER)
            ORDER BY id
            "#,
        )
        .bind(now)
        .fetch_all(&self.pool)
        .await?;
        let ids = rows
            .into_iter()
            .map(|row| row.try_get::<String, _>("id"))
            .collect::<Result<Vec<_>, _>>()?;
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        sqlx::query(
            r#"
            UPDATE agent_leases SET state = 'paused', error = 'agent_host_unavailable'
            WHERE state IN ('claimed', 'running') AND expires_at IS NOT NULL
              AND CAST(expires_at AS INTEGER) <= CAST(?1 AS INTEGER)
            "#,
        )
        .bind(now)
        .execute(&self.pool)
        .await?;
        let mut leases = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(lease) = self.get_agent_lease(&id).await? {
                leases.push(lease);
            }
        }
        Ok(leases)
    }

    pub async fn abandon_agent_lease(
        &self,
        lease_id: &str,
        reason: &str,
    ) -> Result<StoredAgentLease, StoreError> {
        let now = now_string();
        let result = sqlx::query(
            r#"
            UPDATE agent_leases
            SET state = 'abandoned', error = ?2, completed_at = ?3, expires_at = NULL
            WHERE id = ?1 AND state = 'paused'
            "#,
        )
        .bind(lease_id)
        .bind(reason)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "only a paused agent lease may be abandoned".into(),
            ));
        }
        self.get_agent_lease(lease_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "agent_lease".into(),
                id: lease_id.into(),
            })
    }
}

fn row_to_execution_policy_qualification(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredAgentExecutionPolicyQualification, StoreError> {
    Ok(StoredAgentExecutionPolicyQualification {
        id: row.try_get("id")?,
        policy_id: row.try_get("policy_id")?,
        policy_revision: row.try_get("policy_revision")?,
        policy_hash: row.try_get("policy_hash")?,
        runtime_revision: row.try_get("runtime_revision")?,
        suite_id: row.try_get("suite_id")?,
        suite_hash: row.try_get("suite_hash")?,
        attempts: u32::try_from(row.try_get::<i64, _>("attempts")?)
            .map_err(|_| StoreError::InvalidData("invalid qualification attempts".into()))?,
        metrics: serde_json::from_str(&row.try_get::<String, _>("metrics_json")?)?,
        verdict: row.try_get("verdict")?,
        evidence_artifact_id: row.try_get("evidence_artifact_id")?,
        actor: row.try_get("actor")?,
        reason: row.try_get("reason")?,
        created_at: row.try_get("created_at")?,
    })
}

fn row_to_execution_selection(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredAgentExecutionSelection, StoreError> {
    Ok(StoredAgentExecutionSelection {
        id: row.try_get("id")?,
        subject_kind: row.try_get("subject_kind")?,
        subject_id: row.try_get("subject_id")?,
        stage_key: row.try_get("stage_key")?,
        policy_id: row.try_get("policy_id")?,
        policy_revision: row.try_get("policy_revision")?,
        policy_hash: row.try_get("policy_hash")?,
        resolved_binding: serde_json::from_str(
            &row.try_get::<String, _>("resolved_binding_json")?,
        )?,
        binding_hash: row.try_get("binding_hash")?,
        actor: row.try_get("actor")?,
        reason: row.try_get("reason")?,
        state_hash: row.try_get("state_hash")?,
        supersedes_selection_id: row.try_get("supersedes_selection_id")?,
        stage_execution_id: row.try_get("stage_execution_id")?,
        run_id: row.try_get::<Option<String>, _>("run_id")?.map(RunId::new),
        created_at: row.try_get("created_at")?,
    })
}

fn row_to_host_enrollment(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredAgentHostEnrollment, StoreError> {
    Ok(StoredAgentHostEnrollment {
        id: row.try_get("id")?,
        display_name: row.try_get("display_name")?,
        host_pool: row.try_get("host_pool")?,
        actor: row.try_get("actor")?,
        reason: row.try_get("reason")?,
        created_at: row.try_get("created_at")?,
        expires_at: row.try_get("expires_at")?,
        consumed_at: row.try_get("consumed_at")?,
        consumed_by_host_id: row.try_get("consumed_by_host_id")?,
    })
}

fn row_to_host(row: sqlx::sqlite::SqliteRow) -> Result<StoredAgentHost, StoreError> {
    Ok(StoredAgentHost {
        id: row.try_get("id")?,
        display_name: row.try_get("display_name")?,
        host_pool: row.try_get("host_pool")?,
        lifecycle_state: row.try_get("lifecycle_state")?,
        enrollment_id: row.try_get("enrollment_id")?,
        platform: row.try_get("platform")?,
        architecture: row.try_get("architecture")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        last_contact_at: row.try_get("last_contact_at")?,
        retired_at: row.try_get("retired_at")?,
    })
}

fn row_to_capability_snapshot(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredAgentHostCapabilitySnapshot, StoreError> {
    Ok(StoredAgentHostCapabilitySnapshot {
        id: row.try_get("id")?,
        host_id: row.try_get("host_id")?,
        platform: row.try_get("platform")?,
        architecture: row.try_get("architecture")?,
        codex_version: row.try_get("codex_version")?,
        podman_version: row.try_get("podman_version")?,
        execution_mode: row.try_get("execution_mode")?,
        authentication_class: row.try_get("authentication_class")?,
        authentication_ready: row.try_get::<i64, _>("authentication_ready")? != 0,
        supported_profiles: serde_json::from_str(
            &row.try_get::<String, _>("supported_profiles_json")?,
        )?,
        runner_images: serde_json::from_str(&row.try_get::<String, _>("runner_images_json")?)?,
        available_slots: u32::try_from(row.try_get::<i64, _>("available_slots")?)
            .map_err(|_| StoreError::InvalidData("invalid agent-host slot count".into()))?,
        storage: serde_json::from_str(&row.try_get::<String, _>("storage_json")?)?,
        status: row.try_get("status")?,
        blockers: serde_json::from_str(&row.try_get::<String, _>("blockers_json")?)?,
        content_hash: row.try_get("content_hash")?,
        created_at: row.try_get("created_at")?,
        expires_at: row.try_get("expires_at")?,
    })
}

fn row_to_lease(row: sqlx::sqlite::SqliteRow) -> Result<StoredAgentLease, StoreError> {
    Ok(StoredAgentLease {
        id: row.try_get("id")?,
        run_id: RunId::new(row.try_get::<String, _>("run_id")?),
        stage_execution_id: row.try_get("stage_execution_id")?,
        host_pool: row.try_get("host_pool")?,
        pinned_host_id: row.try_get("pinned_host_id")?,
        host_id: row.try_get("host_id")?,
        workspace_id: row.try_get("workspace_id")?,
        environment_profile_id: row.try_get("environment_profile_id")?,
        runner_image: row.try_get("runner_image")?,
        binding_hash: row.try_get("binding_hash")?,
        state: row.try_get("state")?,
        remote_thread_id: row.try_get("remote_thread_id")?,
        completion_hash: row.try_get("completion_hash")?,
        error: row.try_get("error")?,
        created_at: row.try_get("created_at")?,
        claimed_at: row.try_get("claimed_at")?,
        heartbeat_at: row.try_get("heartbeat_at")?,
        expires_at: row.try_get("expires_at")?,
        completed_at: row.try_get("completed_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CreateAgentHostEnrollment, CreateAgentLease, EnrollAgentHost};

    async fn enrolled_host(store: &SqliteStore, suffix: &str) -> StoredAgentHost {
        let enrollment_id = format!("hostenroll_{suffix}");
        store
            .create_agent_host_enrollment(CreateAgentHostEnrollment {
                id: enrollment_id.clone(),
                display_name: format!("host-{suffix}"),
                host_pool: "codex-reliability".into(),
                token_hash: format!("token-{suffix}"),
                actor: "operator".into(),
                reason: "test enrollment".into(),
                expires_at: "9999999999999".into(),
            })
            .await
            .unwrap();
        store
            .enroll_agent_host(EnrollAgentHost {
                id: format!("agenthost_{suffix}"),
                enrollment_id,
                enrollment_token_hash: format!("token-{suffix}"),
                credential_hash: format!("credential-{suffix}"),
                platform: "linux".into(),
                architecture: "amd64".into(),
            })
            .await
            .unwrap()
    }

    async fn insert_lease_subjects(store: &SqliteStore) {
        sqlx::query("INSERT INTO sessions (id,title,cwd,created_at,updated_at) VALUES ('ses_host','host test','/workspace','1','1')")
            .execute(&store.pool).await.unwrap();
        sqlx::query("INSERT INTO runs (id,session_id,status,user_task,max_turns,started_at,execution_target_json) VALUES ('run_host','ses_host','queued','host test',10,'1','{}')")
            .execute(&store.pool).await.unwrap();
        sqlx::query("INSERT INTO work_items (id,status,title,intent,acceptance_criteria_json,source_repo,source_ref,target_environment,production_impacting,max_attempts,max_elapsed_seconds,created_at,updated_at,status_changed_at) VALUES ('witem_host','waiting','host test','exercise lease','[]','https://github.com/example/repo.git','main','development',0,2,3600,'1','1','1')")
            .execute(&store.pool).await.unwrap();
        sqlx::query("INSERT INTO workspaces (id,work_item_id,run_id,status,source_repo,source_ref,retention_status,created_at,updated_at,status_changed_at) VALUES ('ws_host','witem_host','run_host','ready','https://github.com/example/repo.git','main','retained','1','1','1')")
            .execute(&store.pool).await.unwrap();
        sqlx::query("INSERT INTO stage_executions (id,work_item_id,stage_key,sequence,status,run_id,workspace_id,input_snapshot_json,input_hash,created_at) VALUES ('stageexec_host','witem_host','implement',1,'queued','run_host','ws_host','{}','sha256:test','1')")
            .execute(&store.pool).await.unwrap();
    }

    #[tokio::test]
    async fn enrollment_is_single_use_and_expiry_is_fail_closed() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let host = enrolled_host(&store, "one").await;
        assert_eq!(host.lifecycle_state, "ready");
        assert!(store
            .agent_host_credential_matches(&host.id, "credential-one")
            .await
            .unwrap());
        let duplicate = store
            .enroll_agent_host(EnrollAgentHost {
                id: "agenthost_duplicate".into(),
                enrollment_id: "hostenroll_one".into(),
                enrollment_token_hash: "token-one".into(),
                credential_hash: "credential-duplicate".into(),
                platform: "linux".into(),
                architecture: "amd64".into(),
            })
            .await;
        assert!(matches!(duplicate, Err(StoreError::Conflict(_))));

        store
            .create_agent_host_enrollment(CreateAgentHostEnrollment {
                id: "hostenroll_expired".into(),
                display_name: "expired".into(),
                host_pool: "codex-reliability".into(),
                token_hash: "expired-token".into(),
                actor: "operator".into(),
                reason: "expiry test".into(),
                expires_at: "1".into(),
            })
            .await
            .unwrap();
        let expired = store
            .enroll_agent_host(EnrollAgentHost {
                id: "agenthost_expired".into(),
                enrollment_id: "hostenroll_expired".into(),
                enrollment_token_hash: "expired-token".into(),
                credential_hash: "credential-expired".into(),
                platform: "linux".into(),
                architecture: "amd64".into(),
            })
            .await;
        assert!(matches!(expired, Err(StoreError::Conflict(_))));
    }

    #[tokio::test]
    async fn lease_claim_is_atomic_and_expiry_preserves_the_sticky_workspace() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let host = enrolled_host(&store, "lease").await;
        insert_lease_subjects(&store).await;
        store
            .record_agent_host_capability_snapshot(CreateAgentHostCapabilitySnapshot {
                id: "hostcap_lease".into(),
                host_id: host.id.clone(),
                platform: "linux".into(),
                architecture: "amd64".into(),
                codex_version: "0.150.1".into(),
                podman_version: Some("5.0.0".into()),
                execution_mode: "standalone".into(),
                authentication_class: "chatgpt_session".into(),
                authentication_ready: true,
                supported_profiles: vec!["python-3.11".into()],
                runner_images: serde_json::json!({"python-3.11":format!("registry.example/runner@sha256:{}", "a".repeat(64))}),
                available_slots: 1,
                storage: serde_json::json!({"state":"ready"}),
                status: "passed".into(),
                blockers: serde_json::json!([]),
                content_hash: "sha256:capability".into(),
                expires_at: "9999999999999".into(),
            })
            .await
            .unwrap();
        store
            .create_agent_lease(CreateAgentLease {
                id: "agentlease_host".into(),
                run_id: RunId::new("run_host"),
                stage_execution_id: "stageexec_host".into(),
                host_pool: "codex-reliability".into(),
                pinned_host_id: None,
                workspace_id: "ws_host".into(),
                environment_profile_id: "python-3.11".into(),
                runner_image: format!("registry.example/runner@sha256:{}", "a".repeat(64)),
                binding_hash: "sha256:binding".into(),
            })
            .await
            .unwrap();
        let claimed = store
            .claim_next_agent_lease(&host.id, "lease-token", "9999999999999")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(claimed.host_id.as_deref(), Some(host.id.as_str()));
        assert_eq!(claimed.workspace_id, "ws_host");
        assert!(store
            .claim_next_agent_lease(&host.id, "other-token", "9999999999999")
            .await
            .unwrap()
            .is_none());
        store
            .mark_agent_lease_running(&claimed.id, &host.id, "lease-token", "1")
            .await
            .unwrap();
        let paused = store
            .pause_expired_agent_leases("9999999999999")
            .await
            .unwrap();
        assert_eq!(paused.len(), 1);
        assert_eq!(paused[0].state, "paused");
        assert_eq!(paused[0].workspace_id, "ws_host");
        assert!(matches!(
            store
                .set_agent_host_lifecycle_state(&host.id, "retired")
                .await,
            Err(StoreError::Conflict(_))
        ));
        store
            .abandon_agent_lease(&claimed.id, "workspace permanently lost")
            .await
            .unwrap();
        let retired = store
            .set_agent_host_lifecycle_state(&host.id, "retired")
            .await
            .unwrap();
        assert_eq!(retired.lifecycle_state, "retired");
    }
}

use super::{now_string, SqliteStore, StoreError};
use crate::{
    CreateArchiveRecord, CreateRetentionHold, CreateRetentionPreview, DataInventory,
    DatabaseGeneration, DeleteArchiveRecord, StoredArchiveRecord, StoredRetentionHold,
    StoredRetentionPreview, StoredRetentionReceipt, RETENTION_POLICY_VERSION,
};
use pharness_core::RepositoryContract;
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::collections::HashSet;

impl SqliteStore {
    pub async fn ensure_database_generation(
        &self,
        expected_id: &str,
        initializing_revision: &str,
        purpose: &str,
    ) -> Result<DatabaseGeneration, StoreError> {
        if expected_id.trim().is_empty() {
            return Err(StoreError::InvalidData(
                "database generation ID must not be blank".into(),
            ));
        }
        let existing = self.get_database_generation().await?;
        if let Some(existing) = existing {
            if existing.id != expected_id {
                return Err(StoreError::Conflict(format!(
                    "mounted database generation {} does not match expected generation {expected_id}",
                    existing.id
                )));
            }
            return Ok(existing);
        }
        if self.database_has_operational_data().await? {
            return Err(StoreError::Conflict(
                "database generation is missing on a non-empty database; explicit generation adoption is required".into(),
            ));
        }
        self.insert_database_generation(expected_id, initializing_revision, purpose)
            .await
    }

    pub async fn adopt_existing_database_generation(
        &self,
        expected_id: &str,
        initializing_revision: &str,
        purpose: &str,
    ) -> Result<DatabaseGeneration, StoreError> {
        if let Some(existing) = self.get_database_generation().await? {
            if existing.id != expected_id {
                return Err(StoreError::Conflict(format!(
                    "mounted database generation {} does not match expected generation {expected_id}",
                    existing.id
                )));
            }
            return Ok(existing);
        }
        if !self.database_has_operational_data().await? {
            return Err(StoreError::Conflict(
                "generation adoption is reserved for a non-empty legacy database".into(),
            ));
        }
        self.insert_database_generation(
            expected_id,
            initializing_revision,
            &format!("adopted:{purpose}"),
        )
        .await
    }

    async fn database_has_operational_data(&self) -> Result<bool, StoreError> {
        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT
              (SELECT COUNT(*) FROM work_items) +
              (SELECT COUNT(*) FROM runs) +
              (SELECT COUNT(*) FROM products) +
              (SELECT COUNT(*) FROM repositories) +
              (SELECT COUNT(*) FROM approvals)
            "#,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
    }

    async fn insert_database_generation(
        &self,
        expected_id: &str,
        initializing_revision: &str,
        purpose: &str,
    ) -> Result<DatabaseGeneration, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            INSERT INTO database_generations (
              id, created_at, initializing_revision, schema_version, purpose
            ) VALUES (?1, ?2, ?3, '0049', ?4)
            "#,
        )
        .bind(expected_id)
        .bind(&now)
        .bind(initializing_revision)
        .bind(purpose)
        .execute(&self.pool)
        .await?;
        self.get_database_generation()
            .await?
            .ok_or_else(|| StoreError::InvalidData("database generation was not created".into()))
    }

    pub async fn get_database_generation(&self) -> Result<Option<DatabaseGeneration>, StoreError> {
        let row = sqlx::query(
            "SELECT id, created_at, initializing_revision, schema_version, purpose FROM database_generations ORDER BY created_at LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            Ok(DatabaseGeneration {
                id: row.try_get("id")?,
                created_at: row.try_get("created_at")?,
                initializing_revision: row.try_get("initializing_revision")?,
                schema_version: row.try_get("schema_version")?,
                purpose: row.try_get("purpose")?,
            })
        })
        .transpose()
    }

    pub async fn data_inventory(&self) -> Result<DataInventory, StoreError> {
        let tables = [
            "products",
            "repositories",
            "services",
            "repository_bindings",
            "work_items",
            "runs",
            "messages",
            "events",
            "tool_calls",
            "approvals",
            "artifacts",
            "file_changes",
            "stage_executions",
            "stage_outcomes",
            "evidence_validations",
        ];
        let mut counts = serde_json::Map::new();
        for table in tables {
            let query = format!("SELECT COUNT(*) AS count FROM {table}");
            let count: i64 = sqlx::query(&query)
                .fetch_one(&self.pool)
                .await?
                .try_get("count")?;
            counts.insert(table.into(), serde_json::json!(count));
        }
        let payload_bytes = sqlx::query(
            r#"
            SELECT
              COALESCE((SELECT SUM(length(content)) FROM messages), 0) AS messages,
              COALESCE((SELECT SUM(length(payload_json)) FROM events), 0) AS events,
              COALESCE((SELECT SUM(length(args_json) + length(COALESCE(result_json,'')) + length(policy_json)) FROM tool_calls), 0) AS tools,
              COALESCE((SELECT SUM(length(COALESCE(content_text,'')) + length(COALESCE(content_json,''))) FROM artifacts), 0) AS artifacts,
              COALESCE((SELECT SUM(length(diff)) FROM file_changes), 0) AS diffs,
              COALESCE((SELECT SUM(length(content)) FROM context_items), 0) AS context
            "#,
        )
        .fetch_one(&self.pool)
        .await?;
        let holds: i64 =
            sqlx::query("SELECT COUNT(*) AS count FROM retention_holds WHERE released_at IS NULL")
                .fetch_one(&self.pool)
                .await?
                .try_get("count")?;
        let archives: i64 = sqlx::query("SELECT COUNT(*) AS count FROM archive_records")
            .fetch_one(&self.pool)
            .await?
            .try_get("count")?;
        Ok(DataInventory {
            database_generation: self.get_database_generation().await?,
            table_counts: serde_json::Value::Object(counts),
            retained_bytes: serde_json::json!({
                "messages":payload_bytes.try_get::<i64,_>("messages")?,
                "events":payload_bytes.try_get::<i64,_>("events")?,
                "tools":payload_bytes.try_get::<i64,_>("tools")?,
                "artifacts":payload_bytes.try_get::<i64,_>("artifacts")?,
                "diffs":payload_bytes.try_get::<i64,_>("diffs")?,
                "context":payload_bytes.try_get::<i64,_>("context")?,
            }),
            active_holds: u64::try_from(holds).unwrap_or_default(),
            archives: u64::try_from(archives).unwrap_or_default(),
            as_of: now_string(),
        })
    }

    pub async fn create_retention_hold(
        &self,
        hold: CreateRetentionHold,
    ) -> Result<StoredRetentionHold, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            INSERT INTO retention_holds (
              id, subject_kind, subject_id, reason, actor, created_at, expires_at, state_hash
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
        )
        .bind(&hold.id)
        .bind(&hold.subject_kind)
        .bind(&hold.subject_id)
        .bind(&hold.reason)
        .bind(&hold.actor)
        .bind(&now)
        .bind(&hold.expires_at)
        .bind(&hold.state_hash)
        .execute(&self.pool)
        .await
        .map_err(|error| match &error {
            sqlx::Error::Database(database) if database.is_unique_violation() => {
                StoreError::Conflict("subject already has an active retention hold".into())
            }
            _ => StoreError::Sqlx(error),
        })?;
        self.get_retention_hold(&hold.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "retention_hold".into(),
                id: hold.id,
            })
    }

    pub async fn get_retention_hold(
        &self,
        id: &str,
    ) -> Result<Option<StoredRetentionHold>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, subject_kind, subject_id, reason, actor, created_at, expires_at,
                   released_at, released_by, release_reason, state_hash
            FROM retention_holds WHERE id = ?1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_hold).transpose()
    }

    pub async fn list_retention_holds(&self) -> Result<Vec<StoredRetentionHold>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, subject_kind, subject_id, reason, actor, created_at, expires_at,
                   released_at, released_by, release_reason, state_hash
            FROM retention_holds ORDER BY created_at DESC, id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_hold).collect()
    }

    pub async fn release_retention_hold(
        &self,
        id: &str,
        actor: &str,
        reason: &str,
        expected_state_hash: &str,
    ) -> Result<StoredRetentionHold, StoreError> {
        let now = now_string();
        let updated = sqlx::query(
            r#"
            UPDATE retention_holds
            SET released_at = ?2, released_by = ?3, release_reason = ?4
            WHERE id = ?1 AND released_at IS NULL AND state_hash = ?5
            "#,
        )
        .bind(id)
        .bind(&now)
        .bind(actor)
        .bind(reason)
        .bind(expected_state_hash)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "retention hold changed after review or is already released".into(),
            ));
        }
        self.get_retention_hold(id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "retention_hold".into(),
                id: id.into(),
            })
    }

    pub async fn create_retention_preview(
        &self,
        preview: CreateRetentionPreview,
    ) -> Result<StoredRetentionPreview, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            INSERT INTO retention_previews (
              id, database_generation_id, policy_version, status, preview_json,
              content_hash, state_hash, actor, reason, created_at, expires_at
            ) VALUES (?1, ?2, ?3, 'ready', ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
        )
        .bind(&preview.id)
        .bind(&preview.database_generation_id)
        .bind(RETENTION_POLICY_VERSION)
        .bind(serde_json::to_string(&preview.preview)?)
        .bind(&preview.content_hash)
        .bind(&preview.state_hash)
        .bind(&preview.actor)
        .bind(&preview.reason)
        .bind(&now)
        .bind(&preview.expires_at)
        .execute(&self.pool)
        .await?;
        self.get_retention_preview(&preview.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "retention_preview".into(),
                id: preview.id,
            })
    }

    pub async fn get_retention_preview(
        &self,
        id: &str,
    ) -> Result<Option<StoredRetentionPreview>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, database_generation_id, policy_version, status, preview_json,
                   content_hash, state_hash, actor, reason, created_at, expires_at, executed_at
            FROM retention_previews WHERE id = ?1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_preview).transpose()
    }

    pub async fn list_retention_previews(&self) -> Result<Vec<StoredRetentionPreview>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, database_generation_id, policy_version, status, preview_json,
                   content_hash, state_hash, actor, reason, created_at, expires_at, executed_at
            FROM retention_previews ORDER BY created_at DESC, id LIMIT 100
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_preview).collect()
    }

    pub async fn list_retention_receipts(&self) -> Result<Vec<StoredRetentionReceipt>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, preview_id, database_generation_id, policy_version, status,
                   receipt_json, content_hash, actor, reason, created_at
            FROM retention_receipts ORDER BY created_at DESC, id LIMIT 100
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_receipt).collect()
    }

    pub async fn create_archive_record(
        &self,
        archive: CreateArchiveRecord,
    ) -> Result<StoredArchiveRecord, StoreError> {
        let now = now_string();
        sqlx::query(
            r#"
            INSERT INTO archive_records (
              id, database_generation_id, archived_generation_id, database_claim,
              archive_claim, database_sha256, manifest_sha256, archive_json, status,
              created_at, deletion_eligible_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'retained', ?9, ?10)
            "#,
        )
        .bind(&archive.id)
        .bind(&archive.database_generation_id)
        .bind(&archive.archived_generation_id)
        .bind(&archive.database_claim)
        .bind(&archive.archive_claim)
        .bind(&archive.database_sha256)
        .bind(&archive.manifest_sha256)
        .bind(serde_json::to_string(&archive.archive)?)
        .bind(&now)
        .bind(&archive.deletion_eligible_at)
        .execute(&self.pool)
        .await?;
        self.get_archive_record(&archive.id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "archive_record".into(),
                id: archive.id,
            })
    }

    pub async fn get_archive_record(
        &self,
        id: &str,
    ) -> Result<Option<StoredArchiveRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, database_generation_id, archived_generation_id, database_claim,
                   archive_claim, database_sha256, manifest_sha256, archive_json, status,
                   created_at, deletion_eligible_at, deleted_at, deletion_receipt_id
            FROM archive_records WHERE id = ?1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_archive).transpose()
    }

    pub async fn list_archive_records(&self) -> Result<Vec<StoredArchiveRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, database_generation_id, archived_generation_id, database_claim,
                   archive_claim, database_sha256, manifest_sha256, archive_json, status,
                   created_at, deletion_eligible_at, deleted_at, deletion_receipt_id
            FROM archive_records ORDER BY created_at DESC, id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_archive).collect()
    }

    pub async fn mark_archive_deleted(
        &self,
        deletion: DeleteArchiveRecord,
    ) -> Result<StoredArchiveRecord, StoreError> {
        let archive = self
            .get_archive_record(&deletion.archive_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "archive_record".into(),
                id: deletion.archive_id.clone(),
            })?;
        if archive.status != "retained" || archive.deleted_at.is_some() {
            return Err(StoreError::Conflict(
                "archive record is not retained and deletion-eligible".into(),
            ));
        }
        let preview_json = serde_json::json!({
            "schema_version":"pharness.dev/archive-deletion-preview/v1alpha1",
            "archive_id":archive.id,
            "database_claim":archive.database_claim,
            "archive_claim":archive.archive_claim,
            "archived_generation_id":archive.archived_generation_id,
        });
        let receipt_json = serde_json::json!({
            "schema_version":"pharness.dev/archive-deletion-receipt/v1alpha1",
            "archive_id":archive.id,
            "database_claim":archive.database_claim,
            "archive_claim":archive.archive_claim,
            "deleted_or_already_absent_at":deletion.deleted_at,
        });
        let preview_hash = format!(
            "sha256:{:x}",
            Sha256::digest(serde_json::to_vec(&preview_json)?)
        );
        let receipt_hash = format!(
            "sha256:{:x}",
            Sha256::digest(serde_json::to_vec(&receipt_json)?)
        );
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO retention_previews (
              id,database_generation_id,policy_version,status,preview_json,content_hash,
              state_hash,actor,reason,created_at,expires_at,executed_at
            ) VALUES (?1,?2,?3,'executed',?4,?5,?6,?7,?8,?9,?9,?9)
            "#,
        )
        .bind(&deletion.preview_id)
        .bind(&archive.database_generation_id)
        .bind(RETENTION_POLICY_VERSION)
        .bind(serde_json::to_string(&preview_json)?)
        .bind(preview_hash)
        .bind(&deletion.state_hash)
        .bind(&deletion.actor)
        .bind(&deletion.reason)
        .bind(&deletion.deleted_at)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO retention_receipts (
              id,preview_id,database_generation_id,policy_version,status,receipt_json,
              content_hash,actor,reason,created_at
            ) VALUES (?1,?2,?3,?4,'succeeded',?5,?6,?7,?8,?9)
            "#,
        )
        .bind(&deletion.receipt_id)
        .bind(&deletion.preview_id)
        .bind(&archive.database_generation_id)
        .bind(RETENTION_POLICY_VERSION)
        .bind(serde_json::to_string(&receipt_json)?)
        .bind(receipt_hash)
        .bind(&deletion.actor)
        .bind(&deletion.reason)
        .bind(&deletion.deleted_at)
        .execute(&mut *tx)
        .await?;
        let updated = sqlx::query(
            "UPDATE archive_records SET status='deleted', deleted_at=?2, deletion_receipt_id=?3 WHERE id=?1 AND status='retained' AND deleted_at IS NULL",
        )
        .bind(&deletion.archive_id)
        .bind(&deletion.deleted_at)
        .bind(&deletion.receipt_id)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "archive changed after deletion review".into(),
            ));
        }
        tx.commit().await?;
        self.get_archive_record(&deletion.archive_id)
            .await?
            .ok_or_else(|| StoreError::NotFound {
                entity: "archive_record".into(),
                id: deletion.archive_id,
            })
    }

    pub async fn retention_candidates(
        &self,
        now_millis: u128,
    ) -> Result<serde_json::Value, StoreError> {
        let run_cutoff = now_millis
            .saturating_sub(30 * 24 * 60 * 60 * 1_000)
            .to_string();
        let workspace_cutoff = now_millis
            .saturating_sub(7 * 24 * 60 * 60 * 1_000)
            .to_string();
        let verification_cutoff = workspace_cutoff.clone();
        let run_rows = sqlx::query(
            r#"
            SELECT DISTINCT r.id
            FROM runs r
            JOIN stage_executions se ON se.run_id = r.id
            JOIN work_items wi ON wi.id = se.work_item_id
            WHERE wi.closed_at IS NOT NULL
              AND CAST(wi.closed_at AS INTEGER) <= CAST(?1 AS INTEGER)
              AND r.retention_state = 'retained'
              AND NOT EXISTS (
                SELECT 1 FROM retention_holds h
                WHERE h.released_at IS NULL
                  AND (h.expires_at IS NULL OR CAST(h.expires_at AS INTEGER) > CAST(?2 AS INTEGER))
                  AND h.subject_kind = 'work_item'
                  AND h.subject_id = wi.id
              )
            ORDER BY r.id
            "#,
        )
        .bind(&run_cutoff)
        .bind(now_millis.to_string())
        .fetch_all(&self.pool)
        .await?;
        let workspace_rows = sqlx::query(
            r#"
            SELECT DISTINCT w.id, w.work_item_id, w.run_id, wi.mode,
              (SELECT GROUP_CONCAT(DISTINCT se.run_id)
               FROM stage_executions se
               WHERE se.work_item_id = wi.id AND se.run_id IS NOT NULL) AS stage_run_ids
            FROM workspaces w
            JOIN work_items wi ON wi.id = w.work_item_id
            WHERE wi.closed_at IS NOT NULL
              AND CAST(wi.closed_at AS INTEGER) <= CAST(?1 AS INTEGER)
              AND w.retention_status = 'ephemeral'
              AND NOT EXISTS (
                SELECT 1 FROM retention_holds h
                WHERE h.released_at IS NULL
                  AND (h.expires_at IS NULL OR CAST(h.expires_at AS INTEGER) > CAST(?2 AS INTEGER))
                  AND h.subject_kind = 'work_item'
                  AND h.subject_id = wi.id
              )
            ORDER BY w.id
            "#,
        )
        .bind(&workspace_cutoff)
        .bind(now_millis.to_string())
        .fetch_all(&self.pool)
        .await?;
        let verification_rows = sqlx::query(
            r#"
            SELECT cv.id
            FROM capability_verifications cv
            WHERE CAST(cv.verified_at AS INTEGER) <= CAST(?1 AS INTEGER)
              AND NOT EXISTS (
                SELECT 1 FROM repository_readiness_assessments readiness
                WHERE readiness.evidence_refs_json LIKE '%' || cv.id || '%'
              )
              AND cv.id <> (
                SELECT newer.id FROM capability_verifications newer
                WHERE newer.capability = cv.capability
                ORDER BY CAST(newer.verified_at AS INTEGER) DESC, newer.id DESC LIMIT 1
              )
            ORDER BY cv.id
            "#,
        )
        .bind(&verification_cutoff)
        .fetch_all(&self.pool)
        .await?;
        let runs = run_rows
            .into_iter()
            .map(|row| row.try_get::<String, _>("id"))
            .collect::<Result<Vec<_>, _>>()?;
        let workspaces = workspace_rows
            .into_iter()
            .map(|row| {
                let id: String = row.try_get("id")?;
                let work_item_id: Option<String> = row.try_get("work_item_id")?;
                let run_id: Option<String> = row.try_get("run_id")?;
                let mode: Option<String> = row.try_get("mode")?;
                let mut run_ids = row
                    .try_get::<Option<String>, _>("stage_run_ids")?
                    .unwrap_or_default()
                    .split(',')
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>();
                if let Some(run_id) = &run_id {
                    run_ids.push(run_id.clone());
                }
                run_ids.sort();
                run_ids.dedup();
                let pvc_identity = if mode.as_deref() == Some("repo") {
                    id.clone()
                } else {
                    run_id.clone().unwrap_or_else(|| id.clone())
                };
                Ok(serde_json::json!({
                    "workspace_id":id,
                    "work_item_id":work_item_id,
                    "pvc_name":format!("pharness-{pvc_identity}-ws"),
                    "pvc_identity":pvc_identity,
                    "pvc_identity_kind":if mode.as_deref() == Some("repo") { "workspace" } else { "run" },
                    "run_ids":run_ids,
                }))
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?;
        let capability_verifications = verification_rows
            .into_iter()
            .map(|row| row.try_get::<String, _>("id"))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(serde_json::json!({
            "schema_version":RETENTION_POLICY_VERSION,
            "cutoffs":{
                "raw_run_payloads":run_cutoff,
                "ephemeral_workspaces":workspace_cutoff,
                "capability_verifications":verification_cutoff,
            },
            "runs":runs,
            "workspaces":workspaces,
            "capability_verifications":capability_verifications,
        }))
    }

    pub async fn execute_retention_preview(
        &self,
        preview: &StoredRetentionPreview,
        receipt_id: &str,
        actor: &str,
        reason: &str,
        now_millis: u128,
    ) -> Result<StoredRetentionReceipt, StoreError> {
        if preview.status != "ready" || preview.executed_at.is_some() {
            return Err(StoreError::Conflict(
                "retention preview is not executable".into(),
            ));
        }
        if preview.expires_at.parse::<u128>().unwrap_or_default() <= now_millis {
            return Err(StoreError::Conflict("retention preview expired".into()));
        }
        let runs = preview
            .preview
            .get("runs")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let verifications = preview
            .preview
            .get("capability_verifications")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let workspaces = preview
            .preview
            .get("workspaces")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let now = now_millis.to_string();
        let run_cutoff = now_millis
            .saturating_sub(30 * 24 * 60 * 60 * 1_000)
            .to_string();
        let mut tx = self.pool.begin().await?;
        let mut compacted_runs = Vec::new();
        for run in runs {
            let Some(run_id) = run.as_str() else { continue };
            let eligible = sqlx::query(
                r#"
                SELECT r.id, r.status, r.stop_reason, r.started_at, r.finished_at,
                       r.run_budget_json, r.budget_consumption_json, r.execution_target_json,
                       se.work_item_id, se.agent_profile_id,
                       (SELECT COUNT(*) FROM events e WHERE e.run_id = r.id) AS event_count,
                       (SELECT COUNT(*) FROM tool_calls t WHERE t.run_id = r.id) AS tool_count,
                       (SELECT COUNT(*) FROM approvals a WHERE a.run_id = r.id) AS approval_count,
                       (SELECT COUNT(*) FROM file_changes f WHERE f.run_id = r.id) AS changed_path_count
                FROM runs r
                JOIN stage_executions se ON se.run_id = r.id
                JOIN work_items wi ON wi.id = se.work_item_id
                WHERE r.id = ?1 AND r.retention_state = 'retained'
                  AND wi.closed_at IS NOT NULL
                  AND CAST(wi.closed_at AS INTEGER) <= CAST(?2 AS INTEGER)
                  AND NOT EXISTS (
                    SELECT 1 FROM retention_holds h
                    WHERE h.released_at IS NULL
                      AND (h.expires_at IS NULL OR CAST(h.expires_at AS INTEGER) > CAST(?3 AS INTEGER))
                      AND h.subject_kind = 'work_item' AND h.subject_id = wi.id
                  )
                "#,
            )
            .bind(run_id)
            .bind(&run_cutoff)
            .bind(&now)
            .fetch_optional(&mut *tx)
            .await?;
            let Some(row) = eligible else {
                return Err(StoreError::Conflict(format!(
                    "run {run_id} is no longer eligible for retention compaction"
                )));
            };
            let event_rows =
                sqlx::query("SELECT type, payload_json FROM events WHERE run_id = ?1 ORDER BY seq")
                    .bind(run_id)
                    .fetch_all(&mut *tx)
                    .await?;
            let acceptance_json: String = sqlx::query_scalar(
                r#"
                SELECT wi.acceptance_criteria_json
                FROM stage_executions se
                JOIN work_items wi ON wi.id = se.work_item_id
                WHERE se.run_id = ?1 LIMIT 1
                "#,
            )
            .bind(run_id)
            .fetch_one(&mut *tx)
            .await?;
            let acceptance_commands = serde_json::from_str::<Vec<String>>(&acceptance_json)?;
            let contract = row
                .try_get::<String, _>("execution_target_json")
                .ok()
                .and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok())
                .and_then(|value| value.get("repository_contract").cloned())
                .and_then(|value| serde_json::from_value::<RepositoryContract>(value).ok());
            let mut turns = 0_u64;
            let mut recoverable_failures = 0_u64;
            let mut estimated_context_tokens = 0_u64;
            let mut actual_prompt_tokens = 0_u64;
            let mut actual_completion_tokens = 0_u64;
            let mut actual_total_tokens = 0_u64;
            let mut compactions = 0_u64;
            let mut truncated_tool_results = 0_u64;
            let mut tools_started = 0_u64;
            let mut tools_completed = 0_u64;
            let mut tools_failed = 0_u64;
            let mut environment_discovery_turns = 0_u64;
            let mut test_commands = Vec::new();
            let mut test_results = Vec::new();
            let mut awaiting_test_result: Option<String> = None;
            for event_row in event_rows {
                let kind: String = event_row.try_get("type")?;
                let payload = serde_json::from_str::<serde_json::Value>(
                    &event_row.try_get::<String, _>("payload_json")?,
                )?;
                match kind.as_str() {
                    "model.request_started" => {
                        turns = turns.max(
                            payload
                                .get("turn")
                                .and_then(serde_json::Value::as_u64)
                                .unwrap_or(0)
                                + 1,
                        );
                        estimated_context_tokens += payload
                            .get("estimated_input_tokens")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(0);
                        compactions += payload
                            .get("compacted_exchanges")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(0);
                        truncated_tool_results += payload
                            .get("truncated_tool_results")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(0);
                    }
                    "model.response_finished" => {
                        actual_prompt_tokens += payload
                            .get("prompt_tokens")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(0);
                        actual_completion_tokens += payload
                            .get("completion_tokens")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(0);
                        actual_total_tokens += payload
                            .get("total_tokens")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(0);
                    }
                    "action.proposed" => {
                        awaiting_test_result = None;
                        if payload.get("action").and_then(serde_json::Value::as_str)
                            == Some("run_shell")
                        {
                            if let Some(command) =
                                payload.get("cmd").and_then(serde_json::Value::as_str)
                            {
                                if acceptance_commands.iter().any(|item| item == command) {
                                    test_commands.push(command.to_string());
                                    awaiting_test_result = Some(command.to_string());
                                }
                                if retention_environment_discovery_command(command) {
                                    environment_discovery_turns += 1;
                                }
                            }
                        } else if payload.get("action").and_then(serde_json::Value::as_str)
                            == Some("run_acceptance_command")
                        {
                            if let Some(command) = payload
                                .get("name")
                                .and_then(serde_json::Value::as_str)
                                .and_then(|name| {
                                    contract
                                        .as_ref()?
                                        .acceptance_commands
                                        .iter()
                                        .find(|item| item.name == name)
                                })
                                .map(|item| item.command.clone())
                                .filter(|command| {
                                    acceptance_commands.iter().any(|item| item == command)
                                })
                            {
                                test_commands.push(command.clone());
                                awaiting_test_result = Some(command);
                            }
                        }
                    }
                    "tool.started" => tools_started += 1,
                    "tool.finished" => {
                        tools_completed += 1;
                        let failed = payload.get("success").and_then(serde_json::Value::as_bool)
                            == Some(false)
                            || payload.get("status").and_then(serde_json::Value::as_str)
                                == Some("error")
                            || payload.get("error").is_some()
                            || payload.pointer("/content/error").is_some();
                        if failed {
                            tools_failed += 1;
                        }
                        if payload
                            .pointer("/content/recoverable")
                            .and_then(serde_json::Value::as_bool)
                            == Some(true)
                        {
                            recoverable_failures += 1;
                        }
                        if let Some(command) = awaiting_test_result.take() {
                            test_results.push(serde_json::json!({
                                "command":command,
                                "passed":!failed,
                                "result_hash":format!("sha256:{:x}", Sha256::digest(serde_json::to_vec(&payload)?)),
                            }));
                        }
                    }
                    _ => {}
                }
            }
            let path_rows = sqlx::query(
                "SELECT path, before_hash, after_hash FROM file_changes WHERE run_id = ?1 ORDER BY created_at, id",
            )
            .bind(run_id)
            .fetch_all(&mut *tx)
            .await?;
            let mut seen_paths = HashSet::new();
            let mut changed_paths = Vec::new();
            let mut changed_file_evidence = Vec::new();
            for path_row in path_rows {
                let path: String = path_row.try_get("path")?;
                changed_file_evidence.push(serde_json::json!({
                    "path":path,
                    "before_hash":path_row.try_get::<Option<String>,_>("before_hash")?,
                    "after_hash":path_row.try_get::<Option<String>,_>("after_hash")?,
                }));
                if seen_paths.insert(path.clone()) {
                    changed_paths.push(path);
                }
            }
            let approval_rows = sqlx::query(
                "SELECT status, requested_at, decided_at FROM approvals WHERE run_id = ?1 ORDER BY requested_at, id",
            )
            .bind(run_id)
            .fetch_all(&mut *tx)
            .await?;
            let mut approval_wait_ms = 0_u64;
            let mut pending_approvals = Vec::new();
            for approval_row in &approval_rows {
                let requested = approval_row
                    .try_get::<String, _>("requested_at")?
                    .parse::<u64>()
                    .unwrap_or(0);
                let decided = approval_row
                    .try_get::<Option<String>, _>("decided_at")?
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(now_millis as u64);
                approval_wait_ms += decided.saturating_sub(requested);
                if approval_row.try_get::<String, _>("status")? == "pending" {
                    pending_approvals.push("redacted_pending_approval".to_string());
                }
            }
            let preparation_duration_ms = sqlx::query(
                "SELECT started_at, finished_at FROM environment_preparations WHERE run_id = ?1 ORDER BY created_at DESC LIMIT 1",
            )
            .bind(run_id)
            .fetch_optional(&mut *tx)
            .await?
            .and_then(|preparation_row| {
                let started = preparation_row.try_get::<Option<String>, _>("started_at").ok()??;
                let finished = preparation_row.try_get::<Option<String>, _>("finished_at").ok()??;
                Some(
                    finished
                        .parse::<u64>()
                        .ok()?
                        .saturating_sub(started.parse::<u64>().ok()?),
                )
            });
            let budget_extensions: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM budget_extensions WHERE run_id = ?1 AND status = 'approved'",
            )
            .bind(run_id)
            .fetch_one(&mut *tx)
            .await?;
            let evidence_rows = sqlx::query(
                r#"
                SELECT evr.reference_kind, evr.reference_id, evr.reference_hash
                FROM evidence_validation_references evr
                JOIN evidence_validations ev ON ev.id = evr.evidence_validation_id
                JOIN stage_executions se ON se.id = ev.stage_execution_id
                WHERE se.work_item_id = ?1
                ORDER BY evr.reference_kind, evr.reference_id
                "#,
            )
            .bind(row.try_get::<String, _>("work_item_id")?)
            .fetch_all(&mut *tx)
            .await?;
            let evidence_references = evidence_rows
                .into_iter()
                .map(|evidence_row| {
                    Ok(serde_json::json!({
                        "kind":evidence_row.try_get::<String,_>("reference_kind")?,
                        "id":evidence_row.try_get::<String,_>("reference_id")?,
                        "hash":evidence_row.try_get::<String,_>("reference_hash")?,
                    }))
                })
                .collect::<Result<Vec<_>, sqlx::Error>>()?;
            let acceptance_evidence = test_results
                .iter()
                .filter(|result| {
                    result.get("passed").and_then(serde_json::Value::as_bool) == Some(true)
                })
                .cloned()
                .collect::<Vec<_>>();
            let summary = serde_json::json!({
                "schema_version":"pharness.dev/run-summary/v1alpha1",
                "run_id":run_id,
                "work_item_id":row.try_get::<String,_>("work_item_id")?,
                "status":row.try_get::<String,_>("status")?,
                "stop_reason":row.try_get::<Option<String>,_>("stop_reason")?,
                "agent_profile_id":row.try_get::<Option<String>,_>("agent_profile_id")?,
                "started_at":row.try_get::<String,_>("started_at")?,
                "finished_at":row.try_get::<Option<String>,_>("finished_at")?,
                "run_budget":serde_json::from_str::<serde_json::Value>(&row.try_get::<String,_>("run_budget_json")?)?,
                "budget_consumption":serde_json::from_str::<serde_json::Value>(&row.try_get::<String,_>("budget_consumption_json")?)?,
                "turns":turns,
                "recoverable_failures":recoverable_failures,
                "retries":recoverable_failures,
                "estimated_context_tokens":estimated_context_tokens,
                "actual_prompt_tokens":actual_prompt_tokens,
                "actual_completion_tokens":actual_completion_tokens,
                "actual_total_tokens":actual_total_tokens,
                "compactions":compactions,
                "truncated_tool_results":truncated_tool_results,
                "tools_started":tools_started,
                "tools_completed":tools_completed,
                "tools_failed":tools_failed,
                "changed_paths":changed_paths,
                "changed_file_evidence":changed_file_evidence,
                "diff_reference":format!("/api/runs/{run_id}/diff"),
                "test_commands":test_commands,
                "test_results":test_results,
                "acceptance_evidence":acceptance_evidence,
                "pending_approvals":pending_approvals,
                "environment_discovery_turns":environment_discovery_turns,
                "approval_wait_ms":approval_wait_ms,
                "preparation_duration_ms":preparation_duration_ms,
                "budget_extensions":budget_extensions,
                "evidence_references":evidence_references,
                "event_count":row.try_get::<i64,_>("event_count")?,
                "tool_count":row.try_get::<i64,_>("tool_count")?,
                "approval_count":row.try_get::<i64,_>("approval_count")?,
                "changed_path_count":row.try_get::<i64,_>("changed_path_count")?,
            });
            let bytes = serde_json::to_vec(&summary)?;
            let hash = format!("sha256:{:x}", Sha256::digest(bytes));
            sqlx::query(
                r#"
                INSERT INTO sealed_run_summaries (
                  id, run_id, work_item_id, summary_json, content_hash, sealed_at, compacted_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
                ON CONFLICT(run_id) DO NOTHING
                "#,
            )
            .bind(format!("runsum_{run_id}"))
            .bind(run_id)
            .bind(
                summary
                    .get("work_item_id")
                    .and_then(serde_json::Value::as_str),
            )
            .bind(serde_json::to_string(&summary)?)
            .bind(hash)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
            sqlx::query("DELETE FROM messages WHERE run_id = ?1")
                .bind(run_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM events WHERE run_id = ?1")
                .bind(run_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM context_items WHERE run_id = ?1")
                .bind(run_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                r#"
                UPDATE tool_calls
                SET args_json = '{"retention":"purged"}',
                    result_json = '{"retention":"purged"}',
                    policy_json = '{"retention":"purged"}',
                    purged_at = ?2
                WHERE run_id = ?1
                "#,
            )
            .bind(run_id)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "UPDATE file_changes SET diff = '[purged by retention policy]', purged_at = ?2 WHERE run_id = ?1",
            )
            .bind(run_id)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                r#"
                UPDATE artifacts
                SET content_text = NULL, content_json = NULL, purged_at = ?2
                WHERE run_id = ?1 AND retention_class <> 'evidence'
                  AND NOT EXISTS (
                    SELECT 1 FROM evidence_validation_references evr
                    WHERE evr.reference_kind = 'artifact' AND evr.reference_id = artifacts.id
                  )
                "#,
            )
            .bind(run_id)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
            sqlx::query("UPDATE runs SET retention_state = 'compacted' WHERE id = ?1")
                .bind(run_id)
                .execute(&mut *tx)
                .await?;
            compacted_runs.push(run_id.to_string());
        }
        let mut deleted_verifications = Vec::new();
        for verification in verifications {
            let Some(id) = verification.as_str() else {
                continue;
            };
            let deleted = sqlx::query(
                r#"
                DELETE FROM capability_verifications
                WHERE id = ?1
                  AND NOT EXISTS (
                    SELECT 1 FROM repository_readiness_assessments readiness
                    WHERE readiness.evidence_refs_json LIKE '%' || capability_verifications.id || '%'
                  )
                "#,
            )
            .bind(id)
            .execute(&mut *tx)
            .await?;
            if deleted.rows_affected() == 1 {
                deleted_verifications.push(id.to_string());
            }
        }
        let mut expired_workspaces = Vec::new();
        for workspace in &workspaces {
            let Some(workspace_id) = workspace
                .get("workspace_id")
                .and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            let updated = sqlx::query(
                "UPDATE workspaces SET retention_status='expired', updated_at=?2 WHERE id=?1 AND retention_status='ephemeral'",
            )
            .bind(workspace_id)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
            if updated.rows_affected() == 1 {
                expired_workspaces.push(workspace_id.to_string());
            }
        }
        let receipt = serde_json::json!({
            "schema_version":"pharness.dev/retention-receipt/v1alpha1",
            "preview_id":preview.id,
            "database_generation_id":preview.database_generation_id,
            "compacted_runs":compacted_runs,
            "deleted_capability_verifications":deleted_verifications,
            "expired_workspaces":expired_workspaces,
            "workspace_resources":workspaces,
            "executed_at":now,
        });
        let receipt_hash = format!("sha256:{:x}", Sha256::digest(serde_json::to_vec(&receipt)?));
        sqlx::query(
            r#"
            INSERT INTO retention_receipts (
              id, preview_id, database_generation_id, policy_version, status,
              receipt_json, content_hash, actor, reason, created_at
            ) VALUES (?1, ?2, ?3, ?4, 'succeeded', ?5, ?6, ?7, ?8, ?9)
            "#,
        )
        .bind(receipt_id)
        .bind(&preview.id)
        .bind(&preview.database_generation_id)
        .bind(RETENTION_POLICY_VERSION)
        .bind(serde_json::to_string(&receipt)?)
        .bind(receipt_hash)
        .bind(actor)
        .bind(reason)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE retention_previews SET status = 'executed', executed_at = ?2 WHERE id = ?1",
        )
        .bind(&preview.id)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.list_retention_receipts()
            .await?
            .into_iter()
            .find(|item| item.id == receipt_id)
            .ok_or_else(|| StoreError::NotFound {
                entity: "retention_receipt".into(),
                id: receipt_id.into(),
            })
    }
}

fn retention_environment_discovery_command(command: &str) -> bool {
    let normalized = command.to_ascii_lowercase();
    [
        "which python",
        "which docker",
        "python --version",
        "docker --version",
        "import httpx",
        "import requests",
        "import socket",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

fn row_to_hold(row: sqlx::sqlite::SqliteRow) -> Result<StoredRetentionHold, StoreError> {
    Ok(StoredRetentionHold {
        id: row.try_get("id")?,
        subject_kind: row.try_get("subject_kind")?,
        subject_id: row.try_get("subject_id")?,
        reason: row.try_get("reason")?,
        actor: row.try_get("actor")?,
        created_at: row.try_get("created_at")?,
        expires_at: row.try_get("expires_at")?,
        released_at: row.try_get("released_at")?,
        released_by: row.try_get("released_by")?,
        release_reason: row.try_get("release_reason")?,
        state_hash: row.try_get("state_hash")?,
    })
}

fn row_to_preview(row: sqlx::sqlite::SqliteRow) -> Result<StoredRetentionPreview, StoreError> {
    let preview: String = row.try_get("preview_json")?;
    Ok(StoredRetentionPreview {
        id: row.try_get("id")?,
        database_generation_id: row.try_get("database_generation_id")?,
        policy_version: row.try_get("policy_version")?,
        status: row.try_get("status")?,
        preview: serde_json::from_str(&preview)?,
        content_hash: row.try_get("content_hash")?,
        state_hash: row.try_get("state_hash")?,
        actor: row.try_get("actor")?,
        reason: row.try_get("reason")?,
        created_at: row.try_get("created_at")?,
        expires_at: row.try_get("expires_at")?,
        executed_at: row.try_get("executed_at")?,
    })
}

fn row_to_receipt(row: sqlx::sqlite::SqliteRow) -> Result<StoredRetentionReceipt, StoreError> {
    let receipt: String = row.try_get("receipt_json")?;
    Ok(StoredRetentionReceipt {
        id: row.try_get("id")?,
        preview_id: row.try_get("preview_id")?,
        database_generation_id: row.try_get("database_generation_id")?,
        policy_version: row.try_get("policy_version")?,
        status: row.try_get("status")?,
        receipt: serde_json::from_str(&receipt)?,
        content_hash: row.try_get("content_hash")?,
        actor: row.try_get("actor")?,
        reason: row.try_get("reason")?,
        created_at: row.try_get("created_at")?,
    })
}

fn row_to_archive(row: sqlx::sqlite::SqliteRow) -> Result<StoredArchiveRecord, StoreError> {
    let archive: String = row.try_get("archive_json")?;
    Ok(StoredArchiveRecord {
        id: row.try_get("id")?,
        database_generation_id: row.try_get("database_generation_id")?,
        archived_generation_id: row.try_get("archived_generation_id")?,
        database_claim: row.try_get("database_claim")?,
        archive_claim: row.try_get("archive_claim")?,
        database_sha256: row.try_get("database_sha256")?,
        manifest_sha256: row.try_get("manifest_sha256")?,
        archive: serde_json::from_str(&archive)?,
        status: row.try_get("status")?,
        created_at: row.try_get("created_at")?,
        deletion_eligible_at: row.try_get("deletion_eligible_at")?,
        deleted_at: row.try_get("deleted_at")?,
        deletion_receipt_id: row.try_get("deletion_receipt_id")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BootstrapOrganization, CreateRetentionHold, CreateRetentionPreview};
    use pharness_core::RunId;
    use serde_json::json;

    #[tokio::test]
    async fn empty_database_initializes_one_exact_generation() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let generation = store
            .ensure_database_generation("dbgen_test", "aabbcc", "test")
            .await
            .unwrap();
        assert_eq!(generation.id, "dbgen_test");
        let mismatch = store
            .ensure_database_generation("dbgen_other", "aabbcc", "test")
            .await
            .unwrap_err();
        assert!(matches!(mismatch, StoreError::Conflict(_)));
    }

    #[tokio::test]
    async fn non_empty_database_requires_explicit_generation_adoption() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        store
            .ensure_bootstrap_organization(&BootstrapOrganization {
                id: "org_adopt".into(),
                organization_key: "adopt".into(),
                display_name: "Adopt".into(),
            })
            .await
            .unwrap();
        sqlx::query(
            r#"
            INSERT INTO products (
              id, organization_id, product_key, display_name, description,
              owner_principal, state_version, current_model_snapshot_id, created_at, updated_at
            ) VALUES ('prod_adopt','org_adopt','adopt','Adopt','Legacy','operator',1,NULL,'1','1')
            "#,
        )
        .execute(&store.pool)
        .await
        .unwrap();
        assert!(matches!(
            store
                .ensure_database_generation("dbgen_adopt", "revision", "legacy")
                .await,
            Err(StoreError::Conflict(_))
        ));
        let adopted = store
            .adopt_existing_database_generation("dbgen_adopt", "revision", "legacy")
            .await
            .unwrap();
        assert_eq!(adopted.id, "dbgen_adopt");
        assert_eq!(adopted.purpose, "adopted:legacy");
    }

    #[tokio::test]
    async fn read_only_connection_does_not_run_migrations_or_accept_writes() {
        let database = std::env::temp_dir().join(format!(
            "pharness-read-only-{}-{}.db",
            std::process::id(),
            super::now_string()
        ));
        let store = SqliteStore::connect(&database).await.unwrap();
        store
            .ensure_database_generation("dbgen_read_only", "revision", "test")
            .await
            .unwrap();
        drop(store);
        let read_only = SqliteStore::connect_read_only(&database).await.unwrap();
        assert_eq!(
            read_only
                .get_database_generation()
                .await
                .unwrap()
                .unwrap()
                .id,
            "dbgen_read_only"
        );
        assert!(read_only
            .ensure_bootstrap_organization(&BootstrapOrganization {
                id: "org_read_only".into(),
                organization_key: "read-only".into(),
                display_name: "Read only".into(),
            })
            .await
            .is_err());
        drop(read_only);
        std::fs::remove_file(&database).unwrap();
        let wal = database.with_extension("db-wal");
        let shm = database.with_extension("db-shm");
        if wal.exists() {
            std::fs::remove_file(wal).unwrap();
        }
        if shm.exists() {
            std::fs::remove_file(shm).unwrap();
        }
    }

    #[tokio::test]
    async fn hold_uniqueness_and_cleanup_receipt_immutability_are_enforced() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        store
            .ensure_database_generation("dbgen_retention", "revision", "test")
            .await
            .unwrap();
        store
            .create_retention_hold(CreateRetentionHold {
                id: "hold_one".into(),
                subject_kind: "work_item".into(),
                subject_id: "witem_one".into(),
                reason: "acceptance evidence".into(),
                actor: "operator".into(),
                expires_at: None,
                state_hash: "sha256:hold".into(),
            })
            .await
            .unwrap();
        assert!(matches!(
            store
                .create_retention_hold(CreateRetentionHold {
                    id: "hold_two".into(),
                    subject_kind: "work_item".into(),
                    subject_id: "witem_one".into(),
                    reason: "duplicate".into(),
                    actor: "operator".into(),
                    expires_at: None,
                    state_hash: "sha256:other".into(),
                })
                .await,
            Err(StoreError::Conflict(_))
        ));
        let preview = store
            .create_retention_preview(CreateRetentionPreview {
                id: "preview_one".into(),
                database_generation_id: "dbgen_retention".into(),
                preview: json!({"runs":[],"workspaces":[],"capability_verifications":[]}),
                content_hash: "sha256:preview".into(),
                state_hash: "sha256:state".into(),
                actor: "operator".into(),
                reason: "test empty receipt".into(),
                expires_at: "2000".into(),
            })
            .await
            .unwrap();
        store
            .execute_retention_preview(&preview, "receipt_one", "operator", "test", 1000)
            .await
            .unwrap();
        let error = sqlx::query("UPDATE retention_receipts SET status='changed'")
            .execute(&store.pool)
            .await
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("retention receipts are immutable"));
    }

    #[tokio::test]
    async fn archive_deletion_is_receipted_once_and_preserves_the_record() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        store
            .ensure_database_generation("dbgen_clean", "revision", "test")
            .await
            .unwrap();
        store
            .create_archive_record(CreateArchiveRecord {
                id: "archive_legacy".into(),
                database_generation_id: "dbgen_clean".into(),
                archived_generation_id: "dbgen_legacy".into(),
                database_claim: "pharness-api-data".into(),
                archive_claim: "pharness-data-archive".into(),
                database_sha256: format!("sha256:{}", "a".repeat(64)),
                manifest_sha256: format!("sha256:{}", "b".repeat(64)),
                archive: json!({"manifest":"verified"}),
                deletion_eligible_at: "2000".into(),
            })
            .await
            .unwrap();

        let deleted = store
            .mark_archive_deleted(DeleteArchiveRecord {
                archive_id: "archive_legacy".into(),
                preview_id: "preview_archive".into(),
                receipt_id: "receipt_archive".into(),
                state_hash: "sha256:reviewed".into(),
                actor: "operator".into(),
                reason: "retention window complete".into(),
                deleted_at: "3000".into(),
            })
            .await
            .unwrap();
        assert_eq!(deleted.status, "deleted");
        assert_eq!(
            deleted.deletion_receipt_id.as_deref(),
            Some("receipt_archive")
        );
        assert!(store
            .mark_archive_deleted(DeleteArchiveRecord {
                archive_id: "archive_legacy".into(),
                preview_id: "preview_again".into(),
                receipt_id: "receipt_again".into(),
                state_hash: "sha256:reviewed".into(),
                actor: "operator".into(),
                reason: "retry".into(),
                deleted_at: "3001".into(),
            })
            .await
            .is_err());
        assert_eq!(store.list_retention_receipts().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn compaction_seals_operator_evidence_before_purging_raw_payloads() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        store
            .ensure_database_generation("dbgen_compact", "revision", "test")
            .await
            .unwrap();
        seed_closed_run(&store).await;
        let now = 4_000_000_000_000_u128;
        let candidates = store.retention_candidates(now).await.unwrap();
        assert_eq!(candidates["runs"], json!(["run_compact"]));
        let preview = store
            .create_retention_preview(CreateRetentionPreview {
                id: "preview_compact".into(),
                database_generation_id: "dbgen_compact".into(),
                preview: candidates,
                content_hash: "sha256:preview".into(),
                state_hash: "sha256:state".into(),
                actor: "operator".into(),
                reason: "compact eligible raw data".into(),
                expires_at: (now + 1_000).to_string(),
            })
            .await
            .unwrap();
        store
            .execute_retention_preview(&preview, "receipt_compact", "operator", "test", now)
            .await
            .unwrap();

        let run = store
            .get_run(&RunId::new("run_compact"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(run.retention_state, "compacted");
        let summary = run.sealed_summary.unwrap();
        assert_eq!(summary["turns"], 1);
        assert_eq!(summary["actual_total_tokens"], 15);
        assert_eq!(summary["tools_failed"], 0);
        assert_eq!(summary["changed_paths"], json!(["src/main.py"]));
        assert_eq!(summary["acceptance_evidence"][0]["passed"], true);
        let message_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE run_id='run_compact'")
                .fetch_one(&store.pool)
                .await
                .unwrap();
        let event_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE run_id='run_compact'")
                .fetch_one(&store.pool)
                .await
                .unwrap();
        let tool_args: String =
            sqlx::query_scalar("SELECT args_json FROM tool_calls WHERE run_id='run_compact'")
                .fetch_one(&store.pool)
                .await
                .unwrap();
        let diff: String =
            sqlx::query_scalar("SELECT diff FROM file_changes WHERE run_id='run_compact'")
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(message_count, 0);
        assert_eq!(event_count, 0);
        assert_eq!(tool_args, r#"{"retention":"purged"}"#);
        assert_eq!(diff, "[purged by retention policy]");
    }

    #[tokio::test]
    async fn active_hold_excludes_the_complete_work_item_aggregate() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        store
            .ensure_database_generation("dbgen_hold", "revision", "test")
            .await
            .unwrap();
        seed_closed_run(&store).await;
        store
            .create_retention_hold(CreateRetentionHold {
                id: "hold_compact".into(),
                subject_kind: "work_item".into(),
                subject_id: "witem_compact".into(),
                reason: "acceptance hold".into(),
                actor: "operator".into(),
                expires_at: Some("5000000000000".into()),
                state_hash: "sha256:hold".into(),
            })
            .await
            .unwrap();
        let candidates = store.retention_candidates(4_000_000_000_000).await.unwrap();
        assert_eq!(candidates["runs"], json!([]));
        assert_eq!(candidates["workspaces"], json!([]));
    }

    async fn seed_closed_run(store: &SqliteStore) {
        sqlx::query(
            "INSERT INTO sessions (id,title,cwd,created_at,updated_at) VALUES ('ses_compact','compact','.', '1','1')",
        )
        .execute(&store.pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO runs (
              id,session_id,status,user_task,max_turns,started_at,finished_at,
              execution_target_json,metadata_json,run_budget_json,budget_consumption_json,stop_reason
            ) VALUES (
              'run_compact','ses_compact','completed','compact evidence',48,'1','2',
              '{"kind":"local_process"}','{}',
              '{"initial_turns":48,"hard_turns":100,"initial_tokens":400000,"hard_tokens":1000000,"active_execution_seconds":3600,"recoverable_tool_errors":4,"identical_failures":2,"verification_reserve_turns":8}',
              '{"allowed_turns":48,"allowed_tokens":400000,"turns_used":1,"tokens_used":15,"active_execution_seconds_used":2,"extensions":0}',
              'completed'
            )
            "#,
        )
        .execute(&store.pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO work_items (
              id,status,title,intent,acceptance_criteria_json,source_repo,source_ref,
              target_environment,production_impacting,max_attempts,max_elapsed_seconds,
              attempt_count,current_run_id,created_at,updated_at,status_changed_at,
              mode,state_version,closed_at,closure_reason
            ) VALUES (
              'witem_compact','completed','compact','retain evidence',
              '["python -m unittest"]','https://github.com/example/repo','main',
              'dev',0,2,3600,1,'run_compact','1','1','1','repo',1,'1','merged'
            )
            "#,
        )
        .execute(&store.pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO stage_executions (
              id,work_item_id,stage_key,sequence,status,agent_profile_id,run_id,
              input_snapshot_json,input_hash,created_at,started_at,finished_at
            ) VALUES (
              'stage_compact','witem_compact','implement',1,'succeeded','repo-builder',
              'run_compact','{}','sha256:input','1','1','2'
            )
            "#,
        )
        .execute(&store.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO messages (id,session_id,run_id,role,content,created_at,metadata_json) VALUES ('msg_compact','ses_compact','run_compact','assistant','raw','1','{}')",
        )
        .execute(&store.pool)
        .await
        .unwrap();
        for (id, seq, kind, payload) in [
            (
                "evt_1",
                1,
                "model.request_started",
                json!({"turn":0,"estimated_input_tokens":10}),
            ),
            (
                "evt_2",
                2,
                "model.response_finished",
                json!({"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}),
            ),
            (
                "evt_3",
                3,
                "action.proposed",
                json!({"action":"run_shell","cmd":"python -m unittest"}),
            ),
            ("evt_4", 4, "tool.started", json!({"action":"run_shell"})),
            (
                "evt_5",
                5,
                "tool.finished",
                json!({"status":"ok","success":true}),
            ),
        ] {
            sqlx::query(
                "INSERT INTO events (id,session_id,run_id,seq,type,ts,payload_json) VALUES (?1,'ses_compact','run_compact',?2,?3,'1',?4)",
            )
            .bind(id)
            .bind(seq)
            .bind(kind)
            .bind(serde_json::to_string(&payload).unwrap())
            .execute(&store.pool)
            .await
            .unwrap();
        }
        sqlx::query(
            r#"
            INSERT INTO tool_calls (
              id,session_id,run_id,action_id,action_type,status,proposed_at,
              started_at,finished_at,args_json,result_json,policy_json
            ) VALUES (
              'tool_compact','ses_compact','run_compact','action','run_shell','completed',
              '1','1','2','{"cmd":"python -m unittest"}','{"status":"ok"}','{}'
            )
            "#,
        )
        .execute(&store.pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO file_changes (
              id,session_id,run_id,tool_call_id,path,before_hash,after_hash,diff,created_at
            ) VALUES (
              'change_compact','ses_compact','run_compact','tool_compact','src/main.py',
              'sha256:before','sha256:after','raw diff','1'
            )
            "#,
        )
        .execute(&store.pool)
        .await
        .unwrap();
    }
}

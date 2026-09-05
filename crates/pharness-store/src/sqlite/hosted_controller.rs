use super::{SqliteStore, StoreError};
use crate::{
    BeginWorkflowOperation, FinishWorkflowReconciliation, StoredWorkflowOperation,
    StoredWorkflowReconciliation,
};
use sqlx::{Row, Sqlite, Transaction};

impl SqliteStore {
    pub async fn get_workflow_reconciliation(
        &self,
        work_item_id: &str,
    ) -> Result<Option<StoredWorkflowReconciliation>, StoreError> {
        sqlx::query("SELECT * FROM hosted_reconciliations WHERE work_item_id = ?")
            .bind(work_item_id)
            .fetch_optional(&self.pool)
            .await?
            .map(reconciliation_from_row)
            .transpose()
    }

    /// A callback can bring a due time forward without invalidating the owner
    /// of an in-flight reconciliation or extending any execution allowance.
    pub async fn wake_workflow(&self, work_item_id: &str, now: i64) -> Result<(), StoreError> {
        sqlx::query("UPDATE hosted_reconciliations SET next_due_at = MIN(next_due_at, ?) WHERE work_item_id = ?")
            .bind(now).bind(work_item_id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn get_workflow_operation(
        &self,
        id: &str,
    ) -> Result<Option<StoredWorkflowOperation>, StoreError> {
        sqlx::query("SELECT * FROM hosted_operations WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .map(operation_from_row)
            .transpose()
    }

    /// Retain access to a source operation after its lock is released, so late
    /// callbacks can record evidence without reopening or rebinding an effect.
    pub async fn workflow_operation_for_source_intent(
        &self,
        intent_id: &str,
    ) -> Result<Option<StoredWorkflowOperation>, StoreError> {
        let rows = sqlx::query("SELECT * FROM hosted_operations WHERE json_extract(resource_refs_json, '$.source_delivery_intent_id') = ? LIMIT 2")
            .bind(intent_id).fetch_all(&self.pool).await?;
        if rows.len() > 1 {
            return Err(StoreError::Conflict(
                "source intent has ambiguous operation identity".into(),
            ));
        }
        rows.into_iter().next().map(operation_from_row).transpose()
    }

    /// One atomic statement arbitrates claims. A replacement claim increments
    /// its fence, so an expired owner cannot record a new dispatch or finish.
    pub async fn claim_due_workflow(
        &self,
        owner: &str,
        now: i64,
        lease_millis: i64,
    ) -> Result<Option<StoredWorkflowReconciliation>, StoreError> {
        if owner.is_empty() || !(1..=60_000).contains(&lease_millis) {
            return Err(StoreError::Conflict("invalid workflow claim".into()));
        }
        sqlx::query(
            "UPDATE hosted_reconciliations SET claim_owner = ?1, claim_fence = claim_fence + 1,
             claim_until = ?2, updated_at = ?3 WHERE work_item_id = (
               SELECT r.work_item_id FROM hosted_reconciliations r
               JOIN work_items w ON w.id = r.work_item_id
               WHERE r.next_due_at <= ?3 AND (r.claim_until IS NULL OR r.claim_until <= ?3)
                 AND (w.closed_at IS NULL OR EXISTS (
                   SELECT 1 FROM hosted_operations o WHERE o.work_item_id = w.id
                   AND o.status != 'succeeded'))
               ORDER BY r.next_due_at, r.work_item_id LIMIT 1
             ) RETURNING *",
        )
        .bind(owner)
        .bind(now.saturating_add(lease_millis))
        .bind(now)
        .fetch_optional(&self.pool)
        .await?
        .map(reconciliation_from_row)
        .transpose()
    }

    pub async fn finish_workflow_reconciliation(
        &self,
        claim: &StoredWorkflowReconciliation,
        finish: FinishWorkflowReconciliation<'_>,
        now: i64,
    ) -> Result<(), StoreError> {
        if finish.next_due_at <= now {
            return Err(StoreError::Conflict(
                "reconciliation must schedule a future due time".into(),
            ));
        }
        let changed = sqlx::query(
            "UPDATE hosted_reconciliations SET next_due_at = ?1, condition = ?2,
             condition_reason = ?3, unchanged_checks = CASE WHEN observed_state_hash IS ?4
             THEN unchanged_checks + 1 ELSE 0 END, observed_state_hash = ?4,
             claim_owner = NULL, claim_until = NULL, updated_at = ?5
             WHERE work_item_id = ?6 AND claim_owner = ?7 AND claim_fence = ?8
               AND claim_until > ?5 AND control_version = ?9",
        )
        .bind(finish.next_due_at)
        .bind(finish.condition)
        .bind(finish.reason)
        .bind(finish.observed_state_hash)
        .bind(now)
        .bind(&claim.work_item_id)
        .bind(&claim.claim_owner)
        .bind(claim.claim_fence)
        .bind(claim.control_version)
        .execute(&self.pool)
        .await?;
        require_one(changed.rows_affected())
    }

    pub async fn set_workflow_control(
        &self,
        work_item_id: &str,
        expected_version: i64,
        control: &str,
        actor: &str,
        reason: &str,
        now: i64,
    ) -> Result<StoredWorkflowReconciliation, StoreError> {
        if !matches!(control, "active" | "paused" | "cancelled")
            || actor.trim().is_empty()
            || reason.trim().is_empty()
        {
            return Err(StoreError::Conflict(
                "workflow control requires an actor, reason and valid action".into(),
            ));
        }
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "UPDATE hosted_reconciliations SET control = ?1, control_version = control_version + 1,
             next_due_at = ?2, claim_owner = NULL, claim_until = NULL, claim_fence = claim_fence + 1,
             condition = ?1, condition_reason = ?3, unchanged_checks = 0, updated_at = ?2
             WHERE work_item_id = ?4 AND control_version = ?5 AND control != 'cancelled'
             AND EXISTS (SELECT 1 FROM work_items w WHERE w.id = ?4 AND w.closed_at IS NULL)
             RETURNING *",
        )
        .bind(control).bind(now).bind(reason).bind(work_item_id).bind(expected_version)
        .fetch_optional(&mut *tx).await?
        .ok_or_else(|| StoreError::Conflict("workflow control changed or cancellation is terminal".into()))?;
        let state = reconciliation_from_row(row)?;
        sqlx::query(
            "INSERT INTO audit_events(id, kind, actor, resource_kind, resource_id, payload_json, created_at)
             VALUES (?1, 'hosted.workflow_control', ?2, 'work_item', ?3, ?4, ?5)",
        )
        .bind(format!("audit_workflow_control_{work_item_id}_{}", state.control_version))
        .bind(actor).bind(work_item_id)
        .bind(serde_json::json!({"control":control,"reason":reason,"control_version":state.control_version,
            "observation_and_authorized_recovery_continue":true}).to_string())
        .bind(now.to_string()).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(state)
    }

    pub async fn active_workflow_operation(
        &self,
        work_item_id: &str,
    ) -> Result<Option<StoredWorkflowOperation>, StoreError> {
        sqlx::query(
            "SELECT * FROM hosted_operations WHERE work_item_id = ? AND status != 'succeeded'",
        )
        .bind(work_item_id)
        .fetch_optional(&self.pool)
        .await?
        .map(operation_from_row)
        .transpose()
    }

    /// Record identity and acquire all resource locks in the same transaction.
    /// Callers must reconcile a returned existing operation before another send.
    pub async fn begin_workflow_operation(
        &self,
        claim: &StoredWorkflowReconciliation,
        operation: BeginWorkflowOperation<'_>,
        now: i64,
    ) -> Result<StoredWorkflowOperation, StoreError> {
        if operation.id.is_empty()
            || operation.action.is_empty()
            || operation.input_hash.is_empty()
            || !matches!(operation.effect, "development" | "observation" | "recovery")
        {
            return Err(StoreError::Conflict("invalid hosted operation".into()));
        }
        let mut requested_keys = operation.resource_keys.to_vec();
        requested_keys.sort_unstable();
        if requested_keys.iter().any(|key| key.is_empty())
            || requested_keys.windows(2).any(|pair| pair[0] == pair[1])
        {
            return Err(StoreError::Conflict(
                "resource locks must be distinct nonempty keys".into(),
            ));
        }
        let mut tx = self.pool.begin().await?;
        fence_claim(&mut tx, claim, now).await?;
        if claim.control != "active" && operation.effect == "development" {
            return Err(StoreError::Conflict(
                "new development and promotion are paused or cancelled".into(),
            ));
        }
        let existing = sqlx::query("SELECT * FROM hosted_operations WHERE work_item_id = ?1 AND action = ?2 AND input_hash = ?3")
            .bind(&claim.work_item_id).bind(operation.action).bind(operation.input_hash)
            .fetch_optional(&mut *tx).await?;
        if let Some(row) = existing {
            let existing = operation_from_row(row)?;
            if existing.id != operation.id
                || existing.effect != operation.effect
                || existing.resource_keys != requested_keys
            {
                return Err(StoreError::Conflict(
                    "hosted operation identity changed".into(),
                ));
            }
            if existing.status != "succeeded" {
                let keys: Vec<String> = sqlx::query_scalar(
                    "SELECT resource_key FROM hosted_operation_locks WHERE operation_id = ? ORDER BY resource_key",
                )
                .bind(operation.id)
                .fetch_all(&mut *tx)
                .await?;
                if keys.is_empty() && existing.status == "pending" {
                    for key in &requested_keys {
                        sqlx::query("INSERT INTO hosted_operation_locks(resource_key,operation_id) VALUES(?,?)")
                            .bind(key).bind(operation.id).execute(&mut *tx).await?;
                    }
                } else if keys != requested_keys {
                    return Err(StoreError::Conflict(
                        "hosted operation resource locks changed".into(),
                    ));
                }
            }
            tx.commit().await?;
            return Ok(existing);
        }
        if claim.control != "active" && operation.effect == "development" {
            return Err(StoreError::Conflict(
                "new development and promotion are paused or cancelled".into(),
            ));
        }
        sqlx::query("INSERT INTO hosted_operations(id,work_item_id,action,input_hash,effect,created_at,updated_at,resource_keys_json) VALUES(?1,?2,?3,?4,?5,?6,?6,?7)")
            .bind(operation.id).bind(&claim.work_item_id).bind(operation.action)
            .bind(operation.input_hash).bind(operation.effect).bind(now).bind(serde_json::to_string(&requested_keys)?).execute(&mut *tx).await?;
        for key in operation.resource_keys {
            if key.is_empty() {
                return Err(StoreError::Conflict("empty hosted resource lock".into()));
            }
            sqlx::query(
                "INSERT INTO hosted_operation_locks(resource_key,operation_id) VALUES(?1,?2)",
            )
            .bind(key)
            .bind(operation.id)
            .execute(&mut *tx)
            .await?;
        }
        let row = sqlx::query("SELECT * FROM hosted_operations WHERE id = ?")
            .bind(operation.id)
            .fetch_one(&mut *tx)
            .await?;
        tx.commit().await?;
        operation_from_row(row)
    }

    /// Only an operation that has never crossed its dispatch boundary may
    /// release locks without a terminal observation. Its intended keys survive.
    pub async fn release_pending_workflow_locks(
        &self,
        claim: &StoredWorkflowReconciliation,
        operation_id: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        fence_claim(&mut tx, claim, now).await?;
        let pending: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM hosted_operations WHERE id = ? AND work_item_id = ? AND status = 'pending' AND resource_refs_json = '{}'")
            .bind(operation_id).bind(&claim.work_item_id).fetch_one(&mut *tx).await?;
        require_one(pending as u64)?;
        sqlx::query("DELETE FROM hosted_operation_locks WHERE operation_id = ?")
            .bind(operation_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn record_workflow_operation(
        &self,
        claim: &StoredWorkflowReconciliation,
        operation_id: &str,
        status: &str,
        resource_refs: &serde_json::Value,
        reason: &str,
        now: i64,
    ) -> Result<StoredWorkflowOperation, StoreError> {
        if !matches!(status, "running" | "blocked" | "succeeded") || !resource_refs.is_object() {
            return Err(StoreError::Conflict(
                "invalid hosted operation outcome".into(),
            ));
        }
        let mut tx = self.pool.begin().await?;
        fence_claim(&mut tx, claim, now).await?;
        let row = sqlx::query("SELECT * FROM hosted_operations WHERE id = ? AND work_item_id = ?")
            .bind(operation_id)
            .bind(&claim.work_item_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| {
                StoreError::Conflict("hosted operation is not owned by this WorkItem".into())
            })?;
        let old = operation_from_row(row)?;
        // Existing references can be enriched, never silently rebound to a new
        // Run or external intent when a dispatch acknowledgment is lost.
        for (key, value) in old.resource_refs.as_object().into_iter().flatten() {
            if resource_refs.get(key) != Some(value) {
                return Err(StoreError::Conflict(
                    "hosted operation resource identity changed".into(),
                ));
            }
        }
        if old.status == "succeeded" {
            if status != old.status || *resource_refs != old.resource_refs {
                return Err(StoreError::Conflict(
                    "a completed hosted operation is immutable".into(),
                ));
            }
            tx.commit().await?;
            return Ok(old);
        }
        let row = sqlx::query("UPDATE hosted_operations SET status = ?1, resource_refs_json = ?2, status_reason = ?3, updated_at = ?4 WHERE id = ?5 RETURNING *")
            .bind(status).bind(resource_refs.to_string()).bind(reason).bind(now).bind(operation_id)
            .fetch_one(&mut *tx).await?;
        if status == "succeeded" {
            sqlx::query("DELETE FROM hosted_operation_locks WHERE operation_id = ?")
                .bind(operation_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        operation_from_row(row)
    }
}

async fn fence_claim(
    tx: &mut Transaction<'_, Sqlite>,
    claim: &StoredWorkflowReconciliation,
    now: i64,
) -> Result<(), StoreError> {
    let changed = sqlx::query("UPDATE hosted_reconciliations SET updated_at = ?1 WHERE work_item_id = ?2 AND claim_owner = ?3 AND claim_fence = ?4 AND claim_until > ?1 AND control_version = ?5 AND control = ?6")
        .bind(now).bind(&claim.work_item_id).bind(&claim.claim_owner).bind(claim.claim_fence)
        .bind(claim.control_version).bind(&claim.control).execute(&mut **tx).await?;
    require_one(changed.rows_affected())
}

fn require_one(rows: u64) -> Result<(), StoreError> {
    if rows == 1 {
        Ok(())
    } else {
        Err(StoreError::Conflict(
            "workflow claim expired or was superseded".into(),
        ))
    }
}

fn reconciliation_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<StoredWorkflowReconciliation, StoreError> {
    Ok(StoredWorkflowReconciliation {
        work_item_id: row.try_get("work_item_id")?,
        control: row.try_get("control")?,
        control_version: row.try_get("control_version")?,
        next_due_at: row.try_get("next_due_at")?,
        claim_owner: row.try_get("claim_owner")?,
        claim_fence: row.try_get("claim_fence")?,
        claim_until: row.try_get("claim_until")?,
        condition: row.try_get("condition")?,
        condition_reason: row.try_get("condition_reason")?,
        unchanged_checks: row.try_get("unchanged_checks")?,
        observed_state_hash: row.try_get("observed_state_hash")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn operation_from_row(row: sqlx::sqlite::SqliteRow) -> Result<StoredWorkflowOperation, StoreError> {
    Ok(StoredWorkflowOperation {
        id: row.try_get("id")?,
        work_item_id: row.try_get("work_item_id")?,
        action: row.try_get("action")?,
        input_hash: row.try_get("input_hash")?,
        effect: row.try_get("effect")?,
        status: row.try_get("status")?,
        resource_keys: serde_json::from_str(&row.try_get::<String, _>("resource_keys_json")?)?,
        resource_refs: serde_json::from_str(&row.try_get::<String, _>("resource_refs_json")?)?,
        status_reason: row.try_get("status_reason")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[cfg(test)]
mod tests;

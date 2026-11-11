use super::campus_journal::load_slot;
use super::SqliteStore;
use crate::{
    CampusCleanupBlockedSession, CampusJournalError, CampusLabSlotCondition, CampusLabSlotHoldKind,
    CampusLabSlotSync, CampusTenantLabMetrics, StoreError, StoredCampusLabSlot,
    StoredCampusLabSlotHold,
};
use pharness_workspaces::{CampusLabSessionState, CampusWorkloadRef};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};
use std::collections::BTreeMap;

type CampusResult<T> = Result<T, CampusJournalError>;

const MAX_HOLD_TEXT: usize = 512;
const ACTIVE_HOLD: &str = "EXISTS (SELECT 1 FROM campus_lab_slot_holds h
                                   WHERE h.slot_id = s.slot_id AND h.released_at IS NULL)";

impl SqliteStore {
    /// Make the registered slots of a tenant match its configuration:
    /// register missing slots, retire registered slots that are no longer
    /// configured, and restore retired ones that are configured again. A
    /// retired slot's occupant keeps running; the slot is not reused.
    pub async fn sync_campus_lab_slots(
        &self,
        tenant_id: &str,
        slot_ids: &[String],
        actor: &str,
        now: i64,
    ) -> CampusResult<CampusLabSlotSync> {
        if slot_ids.is_empty() || actor.trim().is_empty() {
            return Err(CampusJournalError::Invalid(
                "a slot sync needs slots and an actor".into(),
            ));
        }
        let mut tx = self.begin_campus_write().await?;
        let mut sync = CampusLabSlotSync::default();
        for slot_id in slot_ids {
            let inserted = sqlx::query(
                "INSERT INTO campus_lab_slots (slot_id, tenant_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?3) ON CONFLICT (slot_id) DO NOTHING",
            )
            .bind(slot_id)
            .bind(tenant_id)
            .bind(now)
            .execute(&mut *tx)
            .await?
            .rows_affected()
                == 1;
            let slot = load_slot(&mut tx, slot_id)
                .await?
                .ok_or_else(|| CampusJournalError::NotFound(slot_id.clone()))?;
            if slot.tenant_id != tenant_id {
                return Err(CampusJournalError::Conflict(format!(
                    "slot {slot_id} belongs to another tenant"
                )));
            }
            if inserted {
                sync.registered.push(slot_id.clone());
            }
            if release_hold(
                &mut tx,
                slot_id,
                CampusLabSlotHoldKind::Retired,
                None,
                actor,
                "the slot is configured again",
                now,
            )
            .await?
            {
                sync.restored.push(slot_id.clone());
            }
        }
        let unheld: Vec<String> = sqlx::query_scalar(&format!(
            "SELECT s.slot_id FROM campus_lab_slots s
             WHERE s.tenant_id = ?1 AND NOT {ACTIVE_HOLD} ORDER BY s.slot_id"
        ))
        .bind(tenant_id)
        .fetch_all(&mut *tx)
        .await?;
        for slot_id in unheld.into_iter().filter(|slot| !slot_ids.contains(slot)) {
            insert_hold(
                &mut tx,
                &slot_id,
                CampusLabSlotHoldKind::Retired,
                "the slot is no longer configured",
                actor,
                now,
            )
            .await?;
            sync.retired.push(slot_id);
        }
        tx.commit().await?;
        Ok(sync)
    }

    /// Hold a tenant's slot. Returns the new hold, or `None` when the slot
    /// already has an active hold (of either kind).
    pub async fn hold_campus_lab_slot(
        &self,
        slot_id: &str,
        tenant_id: &str,
        kind: CampusLabSlotHoldKind,
        reason: &str,
        actor: &str,
        now: i64,
    ) -> CampusResult<Option<StoredCampusLabSlotHold>> {
        if reason.trim().is_empty() || actor.trim().is_empty() {
            return Err(CampusJournalError::Invalid(
                "a slot hold needs a reason and an actor".into(),
            ));
        }
        let mut tx = self.begin_campus_write().await?;
        let slot = load_slot(&mut tx, slot_id)
            .await?
            .ok_or_else(|| CampusJournalError::NotFound(slot_id.into()))?;
        if slot.tenant_id != tenant_id {
            return Err(CampusJournalError::Conflict(format!(
                "slot {slot_id} belongs to another tenant"
            )));
        }
        if active_hold(&mut tx, slot_id).await?.is_some() {
            return Ok(None);
        }
        let hold = insert_hold(&mut tx, slot_id, kind, reason, actor, now).await?;
        tx.commit().await?;
        Ok(Some(hold))
    }

    /// Release the slot's active hold of `kind`, optionally only one placed
    /// by `held_by`. Returns whether a hold was released.
    pub async fn release_campus_lab_slot_hold(
        &self,
        slot_id: &str,
        kind: CampusLabSlotHoldKind,
        held_by: Option<&str>,
        actor: &str,
        reason: &str,
        now: i64,
    ) -> CampusResult<bool> {
        if reason.trim().is_empty() || actor.trim().is_empty() {
            return Err(CampusJournalError::Invalid(
                "a release needs a reason and an actor".into(),
            ));
        }
        let mut tx = self.begin_campus_write().await?;
        let released = release_hold(&mut tx, slot_id, kind, held_by, actor, reason, now).await?;
        tx.commit().await?;
        Ok(released)
    }

    /// Active holds on a tenant's slots, oldest first.
    pub async fn list_campus_lab_slot_holds(
        &self,
        tenant_id: &str,
    ) -> CampusResult<Vec<StoredCampusLabSlotHold>> {
        sqlx::query(
            "SELECT h.* FROM campus_lab_slot_holds h
             JOIN campus_lab_slots s ON s.slot_id = h.slot_id
             WHERE s.tenant_id = ?1 AND h.released_at IS NULL
             ORDER BY h.held_at, h.sequence",
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(hold_from_row)
        .collect()
    }

    /// Every hold a slot ever had, in order.
    pub async fn list_campus_lab_slot_hold_history(
        &self,
        slot_id: &str,
    ) -> CampusResult<Vec<StoredCampusLabSlotHold>> {
        sqlx::query("SELECT * FROM campus_lab_slot_holds WHERE slot_id = ?1 ORDER BY sequence")
            .bind(slot_id)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(hold_from_row)
            .collect()
    }

    pub async fn list_campus_lab_slots(
        &self,
        tenant_id: &str,
    ) -> CampusResult<Vec<StoredCampusLabSlot>> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT slot_id FROM campus_lab_slots WHERE tenant_id = ?1 ORDER BY slot_id",
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await?;
        let mut connection = self.pool.acquire().await?;
        let mut slots = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(slot) = load_slot(&mut connection, &id).await? {
                slots.push(slot);
            }
        }
        Ok(slots)
    }

    /// Quarantined sessions of a tenant with the number of their delete
    /// operations that were blocked, oldest quarantine first.
    pub async fn list_campus_cleanup_blocked_sessions(
        &self,
        tenant_id: &str,
    ) -> CampusResult<Vec<CampusCleanupBlockedSession>> {
        sqlx::query(
            "SELECT s.session_id, s.slot_id, s.blocked_reason,
                    COALESCE(s.quarantined_at, s.updated_at) AS quarantined_at,
                    (SELECT COUNT(*) FROM campus_session_operations o
                     WHERE o.session_id = s.session_id AND o.kind = 'delete'
                       AND o.status = 'blocked') AS blocked_attempts
             FROM campus_sessions_journal s
             WHERE s.tenant_id = ?1 AND s.state = 'cleanup_blocked'
             ORDER BY quarantined_at, s.session_id",
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(|row| {
            Ok(CampusCleanupBlockedSession {
                session_id: row.try_get("session_id")?,
                slot_id: row.try_get("slot_id")?,
                blocked_reason: row.try_get("blocked_reason")?,
                quarantined_at: row.try_get("quarantined_at")?,
                blocked_attempts: row.try_get("blocked_attempts")?,
            })
        })
        .collect()
    }

    /// Names of the teardown hooks that already succeeded for a session.
    pub async fn list_campus_teardown_hook_runs(
        &self,
        session_id: &str,
    ) -> CampusResult<Vec<String>> {
        Ok(sqlx::query_scalar(
            "SELECT hook FROM campus_teardown_hook_runs WHERE session_id = ?1
             ORDER BY completed_at, hook",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Record that a teardown hook succeeded for a session's workload.
    /// Returns whether this was the first record.
    pub async fn record_campus_teardown_hook_run(
        &self,
        session_id: &str,
        hook: &str,
        op_key: &str,
        workload: &CampusWorkloadRef,
        now: i64,
    ) -> CampusResult<bool> {
        if hook.is_empty() || hook.len() > 64 {
            return Err(CampusJournalError::Invalid(
                "a teardown hook name is 1 to 64 characters".into(),
            ));
        }
        Ok(sqlx::query(
            "INSERT INTO campus_teardown_hook_runs (
               session_id, hook, op_key, slot_id, slot_generation, completed_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (session_id, hook) DO NOTHING",
        )
        .bind(session_id)
        .bind(hook)
        .bind(op_key)
        .bind(&workload.slot_id)
        .bind(workload.slot_generation)
        .bind(now)
        .execute(&self.pool)
        .await?
        .rows_affected()
            == 1)
    }

    /// Campus lab gauges for every tenant with slots or sessions.
    pub async fn campus_lab_metrics(&self, now: i64) -> CampusResult<Vec<CampusTenantLabMetrics>> {
        let mut tenants: BTreeMap<String, CampusTenantLabMetrics> = BTreeMap::new();
        for row in sqlx::query(
            "SELECT tenant_id, state, COUNT(*) AS count FROM campus_sessions_journal
             WHERE state NOT IN ('terminated', 'failed') GROUP BY tenant_id, state",
        )
        .fetch_all(&self.pool)
        .await?
        {
            let state: String = row.try_get("state")?;
            let state = CampusLabSessionState::parse(&state)
                .ok_or_else(|| invalid(format!("unknown campus session state {state}")))?;
            metrics_for(&mut tenants, row.try_get("tenant_id")?)
                .sessions
                .insert(state, row.try_get("count")?);
        }
        for row in sqlx::query(
            "SELECT s.tenant_id,
                    CASE
                      WHEN j.state = 'cleanup_blocked' THEN 'cleanup_blocked'
                      WHEN h.kind = 'quarantined' THEN 'quarantined'
                      WHEN s.occupant_session_id IS NOT NULL THEN 'occupied'
                      WHEN h.kind = 'retired' THEN 'retired'
                      ELSE 'free'
                    END AS condition,
                    COUNT(*) AS count
             FROM campus_lab_slots s
             LEFT JOIN campus_sessions_journal j ON j.session_id = s.occupant_session_id
             LEFT JOIN campus_lab_slot_holds h ON h.slot_id = s.slot_id AND h.released_at IS NULL
             GROUP BY 1, 2",
        )
        .fetch_all(&self.pool)
        .await?
        {
            let condition: String = row.try_get("condition")?;
            let condition = CampusLabSlotCondition::parse(&condition)
                .ok_or_else(|| invalid(format!("unknown slot condition {condition}")))?;
            metrics_for(&mut tenants, row.try_get("tenant_id")?)
                .slots
                .insert(condition, row.try_get("count")?);
        }
        for row in sqlx::query(
            "SELECT tenant_id, MIN(since) AS since FROM (
               SELECT tenant_id, COALESCE(quarantined_at, updated_at) AS since
               FROM campus_sessions_journal WHERE state = 'cleanup_blocked'
               UNION ALL
               SELECT s.tenant_id, h.held_at AS since
               FROM campus_lab_slot_holds h JOIN campus_lab_slots s ON s.slot_id = h.slot_id
               WHERE h.released_at IS NULL AND h.kind = 'quarantined'
             ) GROUP BY tenant_id",
        )
        .fetch_all(&self.pool)
        .await?
        {
            metrics_for(&mut tenants, row.try_get("tenant_id")?).oldest_quarantined_at =
                row.try_get("since")?;
        }
        for row in sqlx::query(
            "SELECT tenant_id, COUNT(*) AS count FROM campus_sessions_journal
             WHERE expires_at <= ?1 AND state NOT IN ('terminated', 'failed', 'cleanup_blocked')
             GROUP BY tenant_id",
        )
        .bind(now)
        .fetch_all(&self.pool)
        .await?
        {
            metrics_for(&mut tenants, row.try_get("tenant_id")?).expired_live =
                row.try_get("count")?;
        }
        Ok(tenants.into_values().collect())
    }
}

fn metrics_for(
    tenants: &mut BTreeMap<String, CampusTenantLabMetrics>,
    tenant_id: String,
) -> &mut CampusTenantLabMetrics {
    tenants
        .entry(tenant_id.clone())
        .or_insert_with(|| CampusTenantLabMetrics {
            tenant_id,
            ..CampusTenantLabMetrics::default()
        })
}

fn invalid(message: String) -> CampusJournalError {
    CampusJournalError::Store(StoreError::InvalidData(message))
}

fn bounded(text: &str) -> &str {
    match text.char_indices().nth(MAX_HOLD_TEXT) {
        Some((index, _)) => &text[..index],
        None => text,
    }
}

async fn active_hold(
    connection: &mut SqliteConnection,
    slot_id: &str,
) -> CampusResult<Option<StoredCampusLabSlotHold>> {
    sqlx::query("SELECT * FROM campus_lab_slot_holds WHERE slot_id = ?1 AND released_at IS NULL")
        .bind(slot_id)
        .fetch_optional(&mut *connection)
        .await?
        .map(|row| hold_from_row(&row))
        .transpose()
}

async fn insert_hold(
    connection: &mut SqliteConnection,
    slot_id: &str,
    kind: CampusLabSlotHoldKind,
    reason: &str,
    actor: &str,
    now: i64,
) -> CampusResult<StoredCampusLabSlotHold> {
    let row = sqlx::query(
        "INSERT INTO campus_lab_slot_holds (slot_id, kind, reason, held_by, held_at)
         VALUES (?1, ?2, ?3, ?4, ?5) RETURNING *",
    )
    .bind(slot_id)
    .bind(kind.as_str())
    .bind(bounded(reason))
    .bind(bounded(actor))
    .bind(now)
    .fetch_one(&mut *connection)
    .await?;
    hold_from_row(&row)
}

async fn release_hold(
    connection: &mut SqliteConnection,
    slot_id: &str,
    kind: CampusLabSlotHoldKind,
    held_by: Option<&str>,
    actor: &str,
    reason: &str,
    now: i64,
) -> CampusResult<bool> {
    Ok(sqlx::query(
        "UPDATE campus_lab_slot_holds
         SET released_by = ?1, released_at = MAX(?2, held_at), release_reason = ?3
         WHERE slot_id = ?4 AND kind = ?5 AND released_at IS NULL
           AND (?6 IS NULL OR held_by = ?6)",
    )
    .bind(bounded(actor))
    .bind(now)
    .bind(bounded(reason))
    .bind(slot_id)
    .bind(kind.as_str())
    .bind(held_by)
    .execute(&mut *connection)
    .await?
    .rows_affected()
        == 1)
}

fn hold_from_row(row: &SqliteRow) -> CampusResult<StoredCampusLabSlotHold> {
    let kind: String = row.try_get("kind")?;
    Ok(StoredCampusLabSlotHold {
        sequence: row.try_get("sequence")?,
        slot_id: row.try_get("slot_id")?,
        kind: CampusLabSlotHoldKind::parse(&kind)
            .ok_or_else(|| invalid(format!("unknown campus slot hold kind {kind}")))?,
        reason: row.try_get("reason")?,
        held_by: row.try_get("held_by")?,
        held_at: row.try_get("held_at")?,
        released_by: row.try_get("released_by")?,
        released_at: row.try_get("released_at")?,
        release_reason: row.try_get("release_reason")?,
    })
}

#[cfg(test)]
mod tests;

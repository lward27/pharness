use super::SqliteStore;
use crate::{
    campus_operation_key, CampusJournalError, CampusLabOrphan, CampusLabTransition,
    CampusLabTransitionOutcome, CampusOperationLease, CampusOrphanReason, CreateCampusLabSession,
    StoredCampusLabEvent, StoredCampusLabOperation, StoredCampusLabSession, StoredCampusLabSlot,
};
use pharness_workspaces::{
    CampusLabDesiredState, CampusLabObservedState, CampusLabOperationKind,
    CampusLabOperationStatus, CampusLabSessionState, OwnedCampusResource,
};
use serde_json::json;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite, SqliteConnection, Transaction};

/// Upper bound on one operation lease. A provider call that outlives its
/// lease can still finish, but its journal write is then rejected.
pub const MAX_CAMPUS_LEASE_SECONDS: i64 = 3_600;

type CampusResult<T> = Result<T, CampusJournalError>;

impl SqliteStore {
    /// Every journal write runs under `BEGIN IMMEDIATE`, which takes SQLite's
    /// single write lock before anything is read. A read-validate-write
    /// sequence therefore cannot interleave with another writer, and a
    /// concurrent writer waits instead of failing a lock upgrade.
    pub(super) async fn begin_campus_write(&self) -> CampusResult<Transaction<'static, Sqlite>> {
        Ok(self.pool.begin_with("BEGIN IMMEDIATE").await?)
    }

    /// Register a lab slot for a tenant. Re-registering is a no-op; moving a
    /// slot to another tenant is refused.
    pub async fn register_campus_lab_slot(
        &self,
        slot_id: &str,
        tenant_id: &str,
        now: i64,
    ) -> CampusResult<StoredCampusLabSlot> {
        let mut tx = self.begin_campus_write().await?;
        sqlx::query(
            "INSERT INTO campus_lab_slots (slot_id, tenant_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?3) ON CONFLICT (slot_id) DO NOTHING",
        )
        .bind(slot_id)
        .bind(tenant_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        let slot = load_slot(&mut tx, slot_id)
            .await?
            .ok_or_else(|| CampusJournalError::NotFound(slot_id.into()))?;
        if slot.tenant_id != tenant_id {
            return Err(CampusJournalError::Conflict(format!(
                "slot {slot_id} belongs to another tenant"
            )));
        }
        tx.commit().await?;
        Ok(slot)
    }

    pub async fn get_campus_lab_slot(
        &self,
        slot_id: &str,
    ) -> CampusResult<Option<StoredCampusLabSlot>> {
        let mut connection = self.pool.acquire().await?;
        load_slot(&mut connection, slot_id).await
    }

    /// Record a requested lab session and its first audit event.
    ///
    /// Returns the session and whether it was created. A duplicate delivery
    /// with identical content returns the existing session unchanged.
    pub async fn create_campus_lab_session(
        &self,
        request: CreateCampusLabSession,
    ) -> CampusResult<(StoredCampusLabSession, bool)> {
        let mut tx = self.begin_campus_write().await?;
        let created = insert_campus_lab_session(&mut tx, &request).await?;
        tx.commit().await?;
        Ok(created)
    }

    pub async fn get_campus_lab_session(
        &self,
        session_id: &str,
    ) -> CampusResult<Option<StoredCampusLabSession>> {
        let mut connection = self.pool.acquire().await?;
        load_session(&mut connection, session_id).await
    }

    /// Ask a session to converge to `Terminated`. Idempotent; it changes the
    /// desired state only, so an in-flight operation keeps its fence and can
    /// still record what it did before the stop is acted on.
    pub async fn request_campus_lab_session_stop(
        &self,
        session_id: &str,
        actor: &str,
        reason: &str,
        now: i64,
    ) -> CampusResult<StoredCampusLabSession> {
        let mut tx = self.begin_campus_write().await?;
        let session = request_campus_lab_stop_in(&mut tx, session_id, actor, reason, now).await?;
        tx.commit().await?;
        Ok(session)
    }
    /// Record a provider observation at an exact generation. Observations do
    /// not advance the generation.
    pub async fn record_campus_lab_observation(
        &self,
        session_id: &str,
        expected_generation: i64,
        observed: CampusLabObservedState,
        now: i64,
    ) -> CampusResult<StoredCampusLabSession> {
        let mut tx = self.begin_campus_write().await?;
        let row = sqlx::query(
            "UPDATE campus_sessions_journal
             SET observed_state = ?1, observed_at = ?2, updated_at = ?2
             WHERE session_id = ?3 AND generation = ?4 AND state NOT IN ('terminated', 'failed')
             RETURNING *",
        )
        .bind(observed.as_str())
        .bind(now)
        .bind(session_id)
        .bind(expected_generation)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            return Err(stale(&mut tx, session_id, expected_generation).await);
        };
        let session = session_from_row(&row)?;
        tx.commit().await?;
        Ok(session)
    }

    /// Apply one state transition with its effects, its audit event and any
    /// operation it begins or completes, all in one transaction.
    ///
    /// Nothing is written unless the session is still at
    /// `expected_generation` in state `from`, any completed operation is held
    /// under a live lease with the latest fence, and the session still
    /// occupies its slot generation.
    pub async fn transition_campus_lab_session(
        &self,
        transition: CampusLabTransition,
    ) -> CampusResult<CampusLabTransitionOutcome> {
        use CampusLabSessionState as State;

        let t = transition;
        t.from.transition(t.to)?;
        if t.actor.trim().is_empty() {
            return Err(CampusJournalError::Invalid(
                "a transition needs an actor".into(),
            ));
        }
        let now = t.now;
        let mut tx = self.begin_campus_write().await?;
        let session = load_session(&mut tx, &t.session_id)
            .await?
            .ok_or_else(|| CampusJournalError::NotFound(t.session_id.clone()))?;
        if session.generation != t.expected_generation || session.state != t.from {
            return Err(fenced(&session, t.expected_generation, t.from));
        }
        let generation = session.generation + 1;
        let mut next = session.clone();
        let mut begins = None;
        let mut completed = None;
        match (t.from, t.to) {
            (State::Requested, State::Authorizing) => {}
            (State::Requested | State::Authorizing, State::Terminated) => {
                next.desired_state = CampusLabDesiredState::Terminated;
                next.observed_state = CampusLabObservedState::Absent;
                next.terminated_at = Some(now);
            }
            (State::Authorizing, State::Failed) => {
                next.failure_reason = Some(required(&t.failure, "a failure reason")?);
                next.desired_state = CampusLabDesiredState::Terminated;
                next.observed_state = CampusLabObservedState::Absent;
                next.terminated_at = Some(now);
            }
            (State::Authorizing, State::Provisioning) => {
                if session.desired_state == CampusLabDesiredState::Terminated
                    || now >= session.expires_at
                {
                    return Err(CampusJournalError::Conflict(
                        "a stopped or expired lab session cannot launch".into(),
                    ));
                }
                let slot = sqlx::query(
                    "UPDATE campus_lab_slots
                     SET occupant_session_id = ?1, occupant_generation = occupant_generation + 1,
                         occupied_at = ?2, updated_at = ?2
                     WHERE slot_id = (
                       SELECT s.slot_id FROM campus_lab_slots s
                       WHERE s.tenant_id = ?3 AND s.occupant_session_id IS NULL
                         AND NOT EXISTS (
                           SELECT 1 FROM campus_lab_slot_holds h
                           WHERE h.slot_id = s.slot_id AND h.released_at IS NULL
                         )
                       ORDER BY s.slot_id LIMIT 1
                     )
                     RETURNING slot_id, occupant_generation",
                )
                .bind(&session.session_id)
                .bind(now)
                .bind(&session.tenant_id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| CampusJournalError::NoFreeSlot(session.tenant_id.clone()))?;
                next.slot_id = Some(slot.try_get("slot_id")?);
                next.slot_generation = Some(slot.try_get("occupant_generation")?);
                next.observed_state = CampusLabObservedState::Provisioning;
                begins = Some(CampusLabOperationKind::Provision);
            }
            (State::Provisioning, State::Ready) => {
                next.workspace_id = Some(required(&t.workspace_id, "a workspace id")?);
                completed = Some(
                    complete_operation(
                        &mut tx,
                        &session,
                        Completion {
                            kind: CampusLabOperationKind::Provision,
                            lease: t.lease.as_ref(),
                            status: CampusLabOperationStatus::Succeeded,
                            result: &t.result,
                            error: None,
                            now,
                        },
                    )
                    .await?,
                );
                next.observed_state = CampusLabObservedState::Ready;
                next.observed_at = Some(now);
            }
            (State::Provisioning, State::Stopping) => {
                let error = t.failure.as_deref().or(t.reason.as_deref());
                completed = Some(if t.lease.is_some() {
                    complete_operation(
                        &mut tx,
                        &session,
                        Completion {
                            kind: CampusLabOperationKind::Provision,
                            lease: t.lease.as_ref(),
                            status: CampusLabOperationStatus::Failed,
                            result: &t.result,
                            error,
                            now,
                        },
                    )
                    .await?
                } else {
                    abandon_provision(&mut tx, &session, error, now).await?
                });
                revoke(&mut next, t.failure.as_ref(), now);
            }
            (State::Ready, State::Stopping) => revoke(&mut next, t.failure.as_ref(), now),
            (State::Stopping | State::CleanupBlocked, State::Cleaning) => {
                ensure_slot_occupant(&mut tx, &session).await?;
                next.blocked_reason = None;
                next.quarantined_at = None;
                next.observed_state = CampusLabObservedState::Deleting;
                begins = Some(CampusLabOperationKind::Delete);
            }
            (State::Cleaning, State::CleanupBlocked) => {
                let reason = required(&t.reason, "a blocked reason")?;
                completed = Some(
                    complete_operation(
                        &mut tx,
                        &session,
                        Completion {
                            kind: CampusLabOperationKind::Delete,
                            lease: t.lease.as_ref(),
                            status: CampusLabOperationStatus::Blocked,
                            result: &t.result,
                            error: Some(&reason),
                            now,
                        },
                    )
                    .await?,
                );
                next.blocked_reason = Some(reason);
                next.quarantined_at = Some(now);
                next.observed_state = CampusLabObservedState::Unreachable;
            }
            (State::Cleaning, State::Terminated | State::Failed) => {
                if (t.to == State::Failed) != session.failure_reason.is_some() {
                    return Err(CampusJournalError::Invalid(
                        "a session whose launch failed ends failed; any other ends terminated"
                            .into(),
                    ));
                }
                completed = Some(
                    complete_operation(
                        &mut tx,
                        &session,
                        Completion {
                            kind: CampusLabOperationKind::Delete,
                            lease: t.lease.as_ref(),
                            status: CampusLabOperationStatus::Succeeded,
                            result: &t.result,
                            error: None,
                            now,
                        },
                    )
                    .await?,
                );
                release_slot(&mut tx, &session, now).await?;
                next.observed_state = CampusLabObservedState::Deleted;
                next.terminated_at = Some(now);
            }
            (from, to) => {
                return Err(CampusJournalError::Invalid(format!(
                    "transition {from} -> {to} has no journal effect"
                )))
            }
        }
        if let Some(observed) = t.observed {
            next.observed_state = observed;
        }
        let changed = sqlx::query(
            "UPDATE campus_sessions_journal SET
               state = ?1, generation = ?2, desired_state = ?3, observed_state = ?4,
               slot_id = ?5, slot_generation = ?6, workspace_id = ?7, failure_reason = ?8,
               blocked_reason = ?9, observed_at = ?10, revoked_at = ?11, quarantined_at = ?12,
               terminated_at = ?13, updated_at = ?14
             WHERE session_id = ?15 AND generation = ?16 AND state = ?17",
        )
        .bind(t.to.as_str())
        .bind(generation)
        .bind(next.desired_state.as_str())
        .bind(next.observed_state.as_str())
        .bind(&next.slot_id)
        .bind(next.slot_generation)
        .bind(&next.workspace_id)
        .bind(&next.failure_reason)
        .bind(&next.blocked_reason)
        .bind(next.observed_at)
        .bind(next.revoked_at)
        .bind(next.quarantined_at)
        .bind(next.terminated_at)
        .bind(now)
        .bind(&session.session_id)
        .bind(session.generation)
        .bind(session.state.as_str())
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(fenced(&session, t.expected_generation, t.from));
        }
        let begun_key = match begins {
            Some(kind) => {
                if !(1..=MAX_CAMPUS_LEASE_SECONDS).contains(&t.lease_seconds) {
                    return Err(CampusJournalError::Invalid(format!(
                        "lease_seconds must be between 1 and {MAX_CAMPUS_LEASE_SECONDS}"
                    )));
                }
                let op_key = campus_operation_key(&session.session_id, kind, generation);
                sqlx::query(
                    "INSERT INTO campus_session_operations (
                       op_key, session_id, kind, session_generation, slot_id, slot_generation,
                       status, attempt_count, lease_owner, lease_expires_at, lease_fence,
                       created_at, updated_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', 1, ?7, ?8, 1, ?9, ?9)",
                )
                .bind(&op_key)
                .bind(&session.session_id)
                .bind(kind.as_str())
                .bind(generation)
                .bind(&next.slot_id)
                .bind(next.slot_generation)
                .bind(&t.actor)
                .bind(now.saturating_add(t.lease_seconds))
                .bind(now)
                .execute(&mut *tx)
                .await?;
                Some(op_key)
            }
            None => None,
        };
        let event_kind = format!("campus.session.{}", t.to.as_str());
        let event_sequence = insert_event(
            &mut tx,
            NewEvent {
                session_id: &session.session_id,
                kind: &event_kind,
                from: Some(t.from),
                to: Some(t.to),
                generation,
                op_key: begun_key.as_deref().or(completed.as_deref()),
                actor: &t.actor,
                payload: json!({
                    "reason": t.reason,
                    "failure": t.failure,
                    "workspace_id": next.workspace_id,
                    "slot_id": next.slot_id,
                    "slot_generation": next.slot_generation,
                    "observed_state": next.observed_state.as_str(),
                    "revoked": next.revoked_at.is_some(),
                }),
                now,
            },
        )
        .await?;
        let session = load_session(&mut tx, &session.session_id)
            .await?
            .ok_or_else(|| CampusJournalError::NotFound(t.session_id.clone()))?;
        let begun = match begun_key {
            Some(op_key) => load_operation(&mut tx, &op_key).await?,
            None => None,
        };
        tx.commit().await?;
        Ok(CampusLabTransitionOutcome {
            session,
            begun,
            event_sequence,
        })
    }

    /// Take the lease on a pending operation whose previous lease is absent or
    /// expired. Each lease advances the fence and the attempt count, so the
    /// previous holder can no longer complete the operation.
    pub async fn lease_campus_lab_operation(
        &self,
        op_key: &str,
        owner: &str,
        now: i64,
        lease_seconds: i64,
    ) -> CampusResult<StoredCampusLabOperation> {
        if owner.trim().is_empty() || !(1..=MAX_CAMPUS_LEASE_SECONDS).contains(&lease_seconds) {
            return Err(CampusJournalError::Invalid(
                "a lease needs an owner and a bounded duration".into(),
            ));
        }
        let mut tx = self.begin_campus_write().await?;
        let operation = load_operation(&mut tx, op_key)
            .await?
            .ok_or_else(|| CampusJournalError::NotFound(op_key.into()))?;
        if operation.status != CampusLabOperationStatus::Pending {
            return Err(CampusJournalError::Fenced(format!(
                "operation {op_key} is already {}",
                operation.status.as_str()
            )));
        }
        if let Some(lease) = operation.lease().filter(|lease| lease.expires_at > now) {
            return Err(CampusJournalError::LeaseHeld {
                op_key: lease.op_key,
                owner: lease.owner,
                expires_at: lease.expires_at,
            });
        }
        let session = load_session(&mut tx, &operation.session_id)
            .await?
            .ok_or_else(|| CampusJournalError::NotFound(operation.session_id.clone()))?;
        if session.generation != operation.session_generation {
            return Err(CampusJournalError::Fenced(format!(
                "operation {op_key} was superseded by generation {}",
                session.generation
            )));
        }
        ensure_slot_occupant(&mut tx, &session).await?;
        let row = sqlx::query(
            "UPDATE campus_session_operations SET
               lease_owner = ?1, lease_expires_at = ?2, lease_fence = lease_fence + 1,
               attempt_count = attempt_count + 1, updated_at = ?3
             WHERE op_key = ?4 AND status = 'pending' AND lease_fence = ?5
             RETURNING *",
        )
        .bind(owner)
        .bind(now.saturating_add(lease_seconds))
        .bind(now)
        .bind(op_key)
        .bind(operation.lease_fence)
        .fetch_one(&mut *tx)
        .await?;
        let leased = operation_from_row(&row)?;
        tx.commit().await?;
        Ok(leased)
    }

    /// Give up a lease after an inconclusive provider call, keeping the
    /// operation pending for the next attempt. Returns whether the lease was
    /// still held.
    pub async fn release_campus_lab_operation_lease(
        &self,
        lease: &CampusOperationLease,
        error: Option<&str>,
        now: i64,
    ) -> CampusResult<bool> {
        let changed = sqlx::query(
            "UPDATE campus_session_operations SET
               lease_owner = NULL, lease_expires_at = NULL,
               last_error = COALESCE(?1, last_error), updated_at = ?2
             WHERE op_key = ?3 AND status = 'pending' AND lease_owner = ?4 AND lease_fence = ?5",
        )
        .bind(error)
        .bind(now)
        .bind(&lease.op_key)
        .bind(&lease.owner)
        .bind(lease.fence)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(changed == 1)
    }

    pub async fn get_campus_lab_operation(
        &self,
        op_key: &str,
    ) -> CampusResult<Option<StoredCampusLabOperation>> {
        let mut connection = self.pool.acquire().await?;
        load_operation(&mut connection, op_key).await
    }

    /// The session's pending operation; at most one exists.
    pub async fn pending_campus_lab_operation(
        &self,
        session_id: &str,
    ) -> CampusResult<Option<StoredCampusLabOperation>> {
        sqlx::query(
            "SELECT * FROM campus_session_operations WHERE session_id = ?1 AND status = 'pending'",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?
        .map(|row| operation_from_row(&row))
        .transpose()
    }

    pub async fn list_campus_lab_operations(
        &self,
        session_id: &str,
    ) -> CampusResult<Vec<StoredCampusLabOperation>> {
        sqlx::query(
            "SELECT * FROM campus_session_operations WHERE session_id = ?1
             ORDER BY session_generation, op_key",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(operation_from_row)
        .collect()
    }

    pub async fn list_campus_lab_session_events(
        &self,
        session_id: &str,
    ) -> CampusResult<Vec<StoredCampusLabEvent>> {
        sqlx::query("SELECT * FROM campus_session_events WHERE session_id = ?1 ORDER BY sequence")
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(event_from_row)
            .collect()
    }

    /// Outbox read: unpublished events in commit order.
    pub async fn list_unpublished_campus_lab_events(
        &self,
        limit: i64,
    ) -> CampusResult<Vec<StoredCampusLabEvent>> {
        sqlx::query(
            "SELECT * FROM campus_session_events WHERE published_at IS NULL
             ORDER BY sequence LIMIT ?1",
        )
        .bind(limit.clamp(1, 1_000))
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(event_from_row)
        .collect()
    }

    /// Mark outbox events as published. Returns how many were newly marked;
    /// already published events keep their first publication time.
    pub async fn mark_campus_lab_events_published(
        &self,
        sequences: &[i64],
        now: i64,
    ) -> CampusResult<u64> {
        let mut tx = self.begin_campus_write().await?;
        let mut marked = 0;
        for sequence in sequences {
            marked += sqlx::query(
                "UPDATE campus_session_events SET published_at = ?1
                 WHERE sequence = ?2 AND published_at IS NULL",
            )
            .bind(now)
            .bind(sequence)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        }
        tx.commit().await?;
        Ok(marked)
    }

    /// Sessions whose cleanup could not be confirmed. They keep their slot and
    /// stay listed until a retried cleanup succeeds.
    pub async fn list_quarantined_campus_lab_sessions(
        &self,
        tenant_id: &str,
    ) -> CampusResult<Vec<StoredCampusLabSession>> {
        sqlx::query(
            "SELECT * FROM campus_sessions_journal
             WHERE tenant_id = ?1 AND state = 'cleanup_blocked'
             ORDER BY quarantined_at, session_id",
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(session_from_row)
        .collect()
    }

    /// Classify a provider's owned-resource inventory against the journal.
    /// Only resources that no live slot occupancy accounts for are returned.
    pub async fn find_campus_lab_orphans(
        &self,
        owned: &[OwnedCampusResource],
    ) -> CampusResult<Vec<CampusLabOrphan>> {
        let mut connection = self.pool.acquire().await?;
        let mut orphans = Vec::new();
        for resource in owned {
            let reason = match load_session(&mut connection, &resource.workload.session_id).await? {
                None => Some(CampusOrphanReason::UnknownSession),
                Some(session) if session.state.is_terminal() => {
                    Some(CampusOrphanReason::TerminalSession)
                }
                Some(session) if session.workload().as_ref() != Some(&resource.workload) => {
                    Some(CampusOrphanReason::StaleOccupancy)
                }
                Some(_) => None,
            };
            if let Some(reason) = reason {
                orphans.push(CampusLabOrphan {
                    resource: resource.clone(),
                    reason,
                });
            }
        }
        Ok(orphans)
    }
}

/// Insert a requested lab session and its first audit event inside the
/// caller's write transaction. A duplicate with identical content returns the
/// existing session and `false`.
pub(super) async fn insert_campus_lab_session(
    connection: &mut SqliteConnection,
    request: &CreateCampusLabSession,
) -> CampusResult<(StoredCampusLabSession, bool)> {
    if request.session_id.is_empty() || request.actor.trim().is_empty() {
        return Err(CampusJournalError::Invalid(
            "a lab session needs an id and an actor".into(),
        ));
    }
    if request.expires_at <= request.now {
        return Err(CampusJournalError::Invalid(
            "a lab session must expire in the future".into(),
        ));
    }
    let known_identity: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM campus_identities WHERE tenant_id = ?1 AND issuer = ?2 AND subject = ?3",
    )
    .bind(&request.tenant_id)
    .bind(&request.issuer)
    .bind(&request.subject)
    .fetch_optional(&mut *connection)
    .await?;
    if known_identity.is_none() {
        return Err(CampusJournalError::Invalid(
            "the student has no Campus identity in this tenant".into(),
        ));
    }
    let created = sqlx::query(
        "INSERT INTO campus_sessions_journal (
           session_id, tenant_id, course_id, issuer, subject,
           profile_id, profile_version, profile_hash,
           created_by, created_at, updated_at, expires_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10, ?11)
         ON CONFLICT (session_id) DO NOTHING",
    )
    .bind(&request.session_id)
    .bind(&request.tenant_id)
    .bind(&request.course_id)
    .bind(&request.issuer)
    .bind(&request.subject)
    .bind(&request.profile.id)
    .bind(&request.profile.version)
    .bind(&request.profile.profile_hash)
    .bind(&request.actor)
    .bind(request.now)
    .bind(request.expires_at)
    .execute(&mut *connection)
    .await?
    .rows_affected()
        == 1;
    let session = load_session(connection, &request.session_id)
        .await?
        .ok_or_else(|| CampusJournalError::NotFound(request.session_id.clone()))?;
    if created {
        insert_event(
            connection,
            NewEvent {
                session_id: &session.session_id,
                kind: "campus.session.requested",
                from: None,
                to: Some(CampusLabSessionState::Requested),
                generation: session.generation,
                op_key: None,
                actor: &request.actor,
                payload: json!({
                    "course_id": session.course_id,
                    "profile": session.profile(),
                    "expires_at": session.expires_at,
                }),
                now: request.now,
            },
        )
        .await?;
    } else if session.tenant_id != request.tenant_id
        || session.course_id != request.course_id
        || session.issuer != request.issuer
        || session.subject != request.subject
        || session.profile() != request.profile
        || session.expires_at != request.expires_at
    {
        return Err(CampusJournalError::Conflict(format!(
            "lab session {} already exists with different content",
            request.session_id
        )));
    }
    Ok((session, created))
}

/// Set a session's desired state to `Terminated` inside the caller's write
/// transaction. A terminal or already stopping session is returned unchanged.
pub(super) async fn request_campus_lab_stop_in(
    connection: &mut SqliteConnection,
    session_id: &str,
    actor: &str,
    reason: &str,
    now: i64,
) -> CampusResult<StoredCampusLabSession> {
    if actor.trim().is_empty() || reason.trim().is_empty() {
        return Err(CampusJournalError::Invalid(
            "a stop needs an actor and a reason".into(),
        ));
    }
    let session = load_session(connection, session_id)
        .await?
        .ok_or_else(|| CampusJournalError::NotFound(session_id.into()))?;
    if session.state.is_terminal() || session.desired_state == CampusLabDesiredState::Terminated {
        return Ok(session);
    }
    sqlx::query(
        "UPDATE campus_sessions_journal SET desired_state = 'terminated', updated_at = ?1
         WHERE session_id = ?2 AND generation = ?3",
    )
    .bind(now)
    .bind(session_id)
    .bind(session.generation)
    .execute(&mut *connection)
    .await?;
    insert_event(
        connection,
        NewEvent {
            session_id,
            kind: "campus.session.stop_requested",
            from: Some(session.state),
            to: None,
            generation: session.generation,
            op_key: None,
            actor,
            payload: json!({ "reason": reason }),
            now,
        },
    )
    .await?;
    load_session(connection, session_id)
        .await?
        .ok_or_else(|| CampusJournalError::NotFound(session_id.into()))
}
pub(super) struct NewEvent<'a> {
    pub(super) session_id: &'a str,
    pub(super) kind: &'a str,
    pub(super) from: Option<CampusLabSessionState>,
    pub(super) to: Option<CampusLabSessionState>,
    pub(super) generation: i64,
    pub(super) op_key: Option<&'a str>,
    pub(super) actor: &'a str,
    pub(super) payload: serde_json::Value,
    pub(super) now: i64,
}

pub(super) async fn insert_event(
    connection: &mut SqliteConnection,
    event: NewEvent<'_>,
) -> CampusResult<i64> {
    Ok(sqlx::query_scalar(
        "INSERT INTO campus_session_events (
           session_id, kind, from_state, to_state, generation, op_key, actor, payload_json,
           created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         RETURNING sequence",
    )
    .bind(event.session_id)
    .bind(event.kind)
    .bind(event.from.map(CampusLabSessionState::as_str))
    .bind(event.to.map(CampusLabSessionState::as_str))
    .bind(event.generation)
    .bind(event.op_key)
    .bind(event.actor)
    .bind(event.payload.to_string())
    .bind(event.now)
    .fetch_one(&mut *connection)
    .await?)
}

struct Completion<'a> {
    kind: CampusLabOperationKind,
    lease: Option<&'a CampusOperationLease>,
    status: CampusLabOperationStatus,
    result: &'a serde_json::Value,
    error: Option<&'a str>,
    now: i64,
}

/// Complete the session's current operation under a live, latest lease.
async fn complete_operation(
    connection: &mut SqliteConnection,
    session: &StoredCampusLabSession,
    completion: Completion<'_>,
) -> CampusResult<String> {
    let lease = completion.lease.ok_or_else(|| {
        CampusJournalError::Invalid("completing an operation requires its lease".into())
    })?;
    let operation = load_operation(connection, &lease.op_key)
        .await?
        .ok_or_else(|| CampusJournalError::Fenced(format!("unknown operation {}", lease.op_key)))?;
    if operation.session_id != session.session_id
        || operation.kind != completion.kind
        || operation.session_generation != session.generation
        || Some(&operation.slot_id) != session.slot_id.as_ref()
        || Some(operation.slot_generation) != session.slot_generation
    {
        return Err(CampusJournalError::Fenced(format!(
            "operation {} does not belong to generation {} of session {}",
            lease.op_key, session.generation, session.session_id
        )));
    }
    if operation.status != CampusLabOperationStatus::Pending {
        return Err(CampusJournalError::Fenced(format!(
            "operation {} is already {}",
            lease.op_key,
            operation.status.as_str()
        )));
    }
    if operation.lease_owner.as_deref() != Some(lease.owner.as_str())
        || operation.lease_fence != lease.fence
        || !matches!(operation.lease_expires_at, Some(expires_at) if expires_at > completion.now)
    {
        return Err(CampusJournalError::Fenced(format!(
            "lease {} of {} on {} expired or was superseded",
            lease.fence, lease.owner, lease.op_key
        )));
    }
    ensure_slot_occupant(connection, session).await?;
    sqlx::query(
        "UPDATE campus_session_operations SET
           status = ?1, result_json = ?2, last_error = COALESCE(?3, last_error),
           lease_owner = NULL, lease_expires_at = NULL, completed_at = ?4, updated_at = ?4
         WHERE op_key = ?5 AND status = 'pending' AND lease_fence = ?6",
    )
    .bind(completion.status.as_str())
    .bind(completion.result.to_string())
    .bind(completion.error)
    .bind(completion.now)
    .bind(&lease.op_key)
    .bind(lease.fence)
    .execute(&mut *connection)
    .await?;
    Ok(operation.op_key)
}

/// Supersede a pending provision that nobody holds a live lease on.
async fn abandon_provision(
    connection: &mut SqliteConnection,
    session: &StoredCampusLabSession,
    error: Option<&str>,
    now: i64,
) -> CampusResult<String> {
    let row = sqlx::query(
        "SELECT * FROM campus_session_operations WHERE session_id = ?1 AND status = 'pending'",
    )
    .bind(&session.session_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or_else(|| {
        CampusJournalError::Conflict("a provisioning session has no pending operation".into())
    })?;
    let operation = operation_from_row(&row)?;
    if operation.kind != CampusLabOperationKind::Provision {
        return Err(CampusJournalError::Conflict(
            "a provisioning session has a non-provision operation".into(),
        ));
    }
    if let Some(lease) = operation.lease().filter(|lease| lease.expires_at > now) {
        return Err(CampusJournalError::LeaseHeld {
            op_key: lease.op_key,
            owner: lease.owner,
            expires_at: lease.expires_at,
        });
    }
    sqlx::query(
        "UPDATE campus_session_operations SET
           status = 'abandoned', last_error = COALESCE(?1, last_error),
           lease_owner = NULL, lease_expires_at = NULL, completed_at = ?2, updated_at = ?2
         WHERE op_key = ?3 AND status = 'pending' AND lease_fence = ?4",
    )
    .bind(error)
    .bind(now)
    .bind(&operation.op_key)
    .bind(operation.lease_fence)
    .execute(&mut *connection)
    .await?;
    Ok(operation.op_key)
}

fn revoke(next: &mut StoredCampusLabSession, failure: Option<&String>, now: i64) {
    next.desired_state = CampusLabDesiredState::Terminated;
    next.revoked_at = Some(next.revoked_at.unwrap_or(now));
    if let Some(failure) = failure {
        next.failure_reason = Some(failure.clone());
    }
}

/// Require that the slot is still occupied by exactly this session's
/// occupancy generation.
async fn ensure_slot_occupant(
    connection: &mut SqliteConnection,
    session: &StoredCampusLabSession,
) -> CampusResult<()> {
    let (Some(slot_id), Some(slot_generation)) = (&session.slot_id, session.slot_generation) else {
        return Err(CampusJournalError::Fenced(format!(
            "session {} holds no lab slot",
            session.session_id
        )));
    };
    let slot = load_slot(connection, slot_id)
        .await?
        .ok_or_else(|| CampusJournalError::Fenced(format!("unknown slot {slot_id}")))?;
    if slot.occupant_session_id.as_deref() != Some(session.session_id.as_str())
        || slot.occupant_generation != slot_generation
    {
        return Err(CampusJournalError::Fenced(format!(
            "slot {slot_id} generation {slot_generation} is no longer held by {}",
            session.session_id
        )));
    }
    Ok(())
}

async fn release_slot(
    connection: &mut SqliteConnection,
    session: &StoredCampusLabSession,
    now: i64,
) -> CampusResult<()> {
    let released = sqlx::query(
        "UPDATE campus_lab_slots SET occupant_session_id = NULL, occupied_at = NULL, updated_at = ?1
         WHERE slot_id = ?2 AND occupant_session_id = ?3 AND occupant_generation = ?4",
    )
    .bind(now)
    .bind(&session.slot_id)
    .bind(&session.session_id)
    .bind(session.slot_generation)
    .execute(&mut *connection)
    .await?
    .rows_affected();
    if released == 1 {
        Ok(())
    } else {
        Err(CampusJournalError::Fenced(format!(
            "slot is no longer held by {}",
            session.session_id
        )))
    }
}

fn required(value: &Option<String>, what: &str) -> CampusResult<String> {
    value
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .ok_or_else(|| CampusJournalError::Invalid(format!("this transition needs {what}")))
}

fn fenced(
    session: &StoredCampusLabSession,
    expected_generation: i64,
    expected_state: CampusLabSessionState,
) -> CampusJournalError {
    CampusJournalError::Fenced(format!(
        "session {} is {} at generation {}, not {} at generation {}",
        session.session_id, session.state, session.generation, expected_state, expected_generation
    ))
}

async fn stale(
    connection: &mut SqliteConnection,
    session_id: &str,
    expected_generation: i64,
) -> CampusJournalError {
    match load_session(connection, session_id).await {
        Ok(Some(session)) => CampusJournalError::Fenced(format!(
            "session {session_id} is {} at generation {}, not generation {expected_generation}",
            session.state, session.generation
        )),
        Ok(None) => CampusJournalError::NotFound(session_id.into()),
        Err(error) => error,
    }
}

pub(super) async fn load_session(
    connection: &mut SqliteConnection,
    session_id: &str,
) -> CampusResult<Option<StoredCampusLabSession>> {
    sqlx::query("SELECT * FROM campus_sessions_journal WHERE session_id = ?1")
        .bind(session_id)
        .fetch_optional(&mut *connection)
        .await?
        .map(|row| session_from_row(&row))
        .transpose()
}

async fn load_operation(
    connection: &mut SqliteConnection,
    op_key: &str,
) -> CampusResult<Option<StoredCampusLabOperation>> {
    sqlx::query("SELECT * FROM campus_session_operations WHERE op_key = ?1")
        .bind(op_key)
        .fetch_optional(&mut *connection)
        .await?
        .map(|row| operation_from_row(&row))
        .transpose()
}

pub(super) async fn load_slot(
    connection: &mut SqliteConnection,
    slot_id: &str,
) -> CampusResult<Option<StoredCampusLabSlot>> {
    sqlx::query("SELECT * FROM campus_lab_slots WHERE slot_id = ?1")
        .bind(slot_id)
        .fetch_optional(&mut *connection)
        .await?
        .map(|row| {
            Ok(StoredCampusLabSlot {
                slot_id: row.try_get("slot_id")?,
                tenant_id: row.try_get("tenant_id")?,
                occupant_session_id: row.try_get("occupant_session_id")?,
                occupant_generation: row.try_get("occupant_generation")?,
                occupied_at: row.try_get("occupied_at")?,
                created_at: row.try_get("created_at")?,
                updated_at: row.try_get("updated_at")?,
            })
        })
        .transpose()
}

fn parse<T>(value: &str, parser: fn(&str) -> Option<T>, what: &str) -> CampusResult<T> {
    parser(value).ok_or_else(|| {
        CampusJournalError::Store(crate::StoreError::InvalidData(format!(
            "unknown campus {what} {value}"
        )))
    })
}

pub(super) fn session_from_row(row: &SqliteRow) -> CampusResult<StoredCampusLabSession> {
    Ok(StoredCampusLabSession {
        session_id: row.try_get("session_id")?,
        tenant_id: row.try_get("tenant_id")?,
        course_id: row.try_get("course_id")?,
        issuer: row.try_get("issuer")?,
        subject: row.try_get("subject")?,
        profile_id: row.try_get("profile_id")?,
        profile_version: row.try_get("profile_version")?,
        profile_hash: row.try_get("profile_hash")?,
        state: parse(
            &row.try_get::<String, _>("state")?,
            CampusLabSessionState::parse,
            "session state",
        )?,
        desired_state: parse(
            &row.try_get::<String, _>("desired_state")?,
            CampusLabDesiredState::parse,
            "desired state",
        )?,
        observed_state: parse(
            &row.try_get::<String, _>("observed_state")?,
            CampusLabObservedState::parse,
            "observed state",
        )?,
        generation: row.try_get("generation")?,
        slot_id: row.try_get("slot_id")?,
        slot_generation: row.try_get("slot_generation")?,
        workspace_id: row.try_get("workspace_id")?,
        failure_reason: row.try_get("failure_reason")?,
        blocked_reason: row.try_get("blocked_reason")?,
        created_by: row.try_get("created_by")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        observed_at: row.try_get("observed_at")?,
        revoked_at: row.try_get("revoked_at")?,
        quarantined_at: row.try_get("quarantined_at")?,
        terminated_at: row.try_get("terminated_at")?,
        expires_at: row.try_get("expires_at")?,
    })
}

fn operation_from_row(row: &SqliteRow) -> CampusResult<StoredCampusLabOperation> {
    Ok(StoredCampusLabOperation {
        op_key: row.try_get("op_key")?,
        session_id: row.try_get("session_id")?,
        kind: parse(
            &row.try_get::<String, _>("kind")?,
            CampusLabOperationKind::parse,
            "operation kind",
        )?,
        session_generation: row.try_get("session_generation")?,
        slot_id: row.try_get("slot_id")?,
        slot_generation: row.try_get("slot_generation")?,
        status: parse(
            &row.try_get::<String, _>("status")?,
            CampusLabOperationStatus::parse,
            "operation status",
        )?,
        attempt_count: row.try_get("attempt_count")?,
        lease_owner: row.try_get("lease_owner")?,
        lease_expires_at: row.try_get("lease_expires_at")?,
        lease_fence: row.try_get("lease_fence")?,
        last_error: row.try_get("last_error")?,
        result: serde_json::from_str(&row.try_get::<String, _>("result_json")?)?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        completed_at: row.try_get("completed_at")?,
    })
}

fn event_from_row(row: &SqliteRow) -> CampusResult<StoredCampusLabEvent> {
    let state = |column: &str| -> CampusResult<Option<CampusLabSessionState>> {
        row.try_get::<Option<String>, _>(column)?
            .map(|value| parse(&value, CampusLabSessionState::parse, "session state"))
            .transpose()
    };
    Ok(StoredCampusLabEvent {
        sequence: row.try_get("sequence")?,
        session_id: row.try_get("session_id")?,
        kind: row.try_get("kind")?,
        from_state: state("from_state")?,
        to_state: state("to_state")?,
        generation: row.try_get("generation")?,
        op_key: row.try_get("op_key")?,
        actor: row.try_get("actor")?,
        payload: serde_json::from_str(&row.try_get::<String, _>("payload_json")?)?,
        created_at: row.try_get("created_at")?,
        published_at: row.try_get("published_at")?,
    })
}

#[cfg(test)]
mod tests;

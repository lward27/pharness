use super::campus_journal::{
    insert_campus_lab_session, insert_event, load_session, request_campus_lab_stop_in, NewEvent,
};
use super::SqliteStore;
use crate::{
    AdmitCampusLabLaunch, CampusJournalError, CampusLabAdmission, CampusLabRejection,
    CampusLabSessionRecord, CreateCampusLabSession, StoredCampusLab,
};
use serde_json::json;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

type CampusResult<T> = Result<T, CampusJournalError>;

const MAX_IDEMPOTENCY_KEY_CHARS: usize = 128;
const MAX_LIST: i64 = 500;

/// Labs of tenant `?1` whose current session is torn down while a reset
/// waits to admit the next generation. Callers append further `AND` terms.
const AWAITING_RESET: &str = "campus_labs l
     JOIN campus_sessions_journal s ON s.session_id = l.current_session_id
     WHERE l.tenant_id = ?1 AND l.reset_requested_at IS NOT NULL
       AND s.state IN ('terminated', 'failed')";

impl SqliteStore {
    /// Admit one launch, or one reset, in a single `BEGIN IMMEDIATE`
    /// transaction:
    ///
    /// 1. a known idempotency key answers with the session it first produced;
    /// 2. the lab is found or created with a fresh, permanent budget id;
    /// 3. a live current session is returned instead of creating another;
    /// 4. the identity, course and tenant live-session limits are checked;
    /// 5. the journal session, its generation row, the lab's new current
    ///    session, the idempotency key and an audit event are written.
    ///
    /// A rejection writes nothing. Because SQLite admits one writer at a time
    /// and every check reads inside the same transaction that writes, no
    /// number of concurrent or retried requests can exceed a limit or create
    /// a second live session for a lab.
    pub async fn admit_campus_lab_launch(
        &self,
        request: AdmitCampusLabLaunch,
    ) -> CampusResult<CampusLabAdmission> {
        let r = request;
        let limits = r.limits;
        if limits.lab_slots < 1
            || limits.max_active_per_identity < 1
            || limits.max_active_per_course < 1
        {
            return Err(CampusJournalError::Invalid(
                "campus lab limits must be positive".into(),
            ));
        }
        if r.idempotency_key
            .as_ref()
            .is_some_and(|key| key.is_empty() || key.chars().count() > MAX_IDEMPOTENCY_KEY_CHARS)
        {
            return Err(CampusJournalError::Invalid(
                "an idempotency key must be 1-128 characters".into(),
            ));
        }
        if r.idempotency_key.is_some() && r.reset_from_session_id.is_some() {
            return Err(CampusJournalError::Invalid(
                "a reset is idempotent on the session it replaces and takes no key".into(),
            ));
        }
        let mut tx = self.begin_campus_write().await?;

        if let Some(key) = &r.idempotency_key {
            let previous = sqlx::query(
                "SELECT course_id, assignment_id, session_id FROM campus_launch_requests
                 WHERE tenant_id = ?1 AND issuer = ?2 AND subject = ?3 AND idempotency_key = ?4",
            )
            .bind(&r.tenant_id)
            .bind(&r.issuer)
            .bind(&r.subject)
            .bind(key)
            .fetch_optional(&mut *tx)
            .await?;
            if let Some(previous) = previous {
                let course: String = previous.try_get("course_id")?;
                let assignment: String = previous.try_get("assignment_id")?;
                if course != r.course_id || assignment != r.assignment_id {
                    return Ok(CampusLabAdmission::Rejected(
                        CampusLabRejection::IdempotencyKeyReused,
                    ));
                }
                let session_id: String = previous.try_get("session_id")?;
                let record = load_record(&mut tx, &session_id)
                    .await?
                    .ok_or(CampusJournalError::NotFound(session_id))?;
                return Ok(CampusLabAdmission::Replayed(record));
            }
        }

        let known_identity: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM campus_identities WHERE tenant_id = ?1 AND issuer = ?2 AND subject = ?3",
        )
        .bind(&r.tenant_id)
        .bind(&r.issuer)
        .bind(&r.subject)
        .fetch_optional(&mut *tx)
        .await?;
        if known_identity.is_none() {
            return Err(CampusJournalError::Invalid(
                "the student has no Campus identity in this tenant".into(),
            ));
        }
        sqlx::query(
            "INSERT INTO campus_labs (
               lab_id, tenant_id, course_id, assignment_id, issuer, subject, budget_id,
               created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
             ON CONFLICT (tenant_id, course_id, assignment_id, issuer, subject) DO NOTHING",
        )
        .bind(&r.candidate_lab_id)
        .bind(&r.tenant_id)
        .bind(&r.course_id)
        .bind(&r.assignment_id)
        .bind(&r.issuer)
        .bind(&r.subject)
        .bind(&r.candidate_budget_id)
        .bind(r.now)
        .execute(&mut *tx)
        .await?;
        let lab = sqlx::query(
            "SELECT * FROM campus_labs WHERE tenant_id = ?1 AND course_id = ?2
               AND assignment_id = ?3 AND issuer = ?4 AND subject = ?5",
        )
        .bind(&r.tenant_id)
        .bind(&r.course_id)
        .bind(&r.assignment_id)
        .bind(&r.issuer)
        .bind(&r.subject)
        .fetch_one(&mut *tx)
        .await
        .map_err(CampusJournalError::from)
        .and_then(|row| lab_from_row(&row))?;

        match (&lab.current_session_id, &r.reset_from_session_id) {
            (Some(current_id), Some(reset_from)) if current_id != reset_from => {
                let current = load_record(&mut tx, current_id)
                    .await?
                    .ok_or_else(|| CampusJournalError::NotFound(current_id.clone()))?;
                return Ok(
                    if current.reset_from_session_id.as_ref() == Some(reset_from) {
                        CampusLabAdmission::Replayed(current)
                    } else {
                        CampusLabAdmission::Rejected(CampusLabRejection::ResetSuperseded {
                            current_session_id: Some(current_id.clone()),
                        })
                    },
                );
            }
            (Some(current_id), reset_from) => {
                let current = load_record(&mut tx, current_id)
                    .await?
                    .ok_or_else(|| CampusJournalError::NotFound(current_id.clone()))?;
                if !current.session.state.is_terminal() {
                    if reset_from.is_some() {
                        return Ok(CampusLabAdmission::Rejected(
                            CampusLabRejection::TeardownPending,
                        ));
                    }
                    if let Some(key) = &r.idempotency_key {
                        insert_launch_request(&mut tx, &r, key, current_id).await?;
                    }
                    tx.commit().await?;
                    return Ok(CampusLabAdmission::Active(current));
                }
            }
            (None, Some(_)) => {
                return Ok(CampusLabAdmission::Rejected(
                    CampusLabRejection::ResetSuperseded {
                        current_session_id: None,
                    },
                ));
            }
            (None, None) => {}
        }

        // A lab whose reset waits for admission keeps its place: its torn-down
        // session counts as live everywhere except for its own lab, so a
        // concurrent launch cannot take the capacity a reset is about to use.
        let identity_live: Vec<String> = sqlx::query_scalar(&format!(
            "SELECT session_id FROM campus_sessions_journal
             WHERE tenant_id = ?1 AND issuer = ?2 AND subject = ?3
               AND state NOT IN ('terminated', 'failed')
             UNION ALL
             SELECT l.current_session_id FROM {AWAITING_RESET}
               AND l.issuer = ?2 AND l.subject = ?3 AND l.lab_id != ?4"
        ))
        .bind(&r.tenant_id)
        .bind(&r.issuer)
        .bind(&r.subject)
        .bind(&lab.lab_id)
        .fetch_all(&mut *tx)
        .await?;
        if count(&identity_live) >= limits.max_active_per_identity {
            return Ok(CampusLabAdmission::Rejected(
                CampusLabRejection::IdentitySessionLimit {
                    active_session_id: identity_live[0].clone(),
                },
            ));
        }
        let course_live: i64 = sqlx::query_scalar(&format!(
            "SELECT (SELECT COUNT(*) FROM campus_sessions_journal
                     WHERE tenant_id = ?1 AND course_id = ?2
                       AND state NOT IN ('terminated', 'failed'))
                  + (SELECT COUNT(*) FROM {AWAITING_RESET}
                       AND l.course_id = ?2 AND l.lab_id != ?3)"
        ))
        .bind(&r.tenant_id)
        .bind(&r.course_id)
        .bind(&lab.lab_id)
        .fetch_one(&mut *tx)
        .await?;
        let tenant_live: i64 = sqlx::query_scalar(&format!(
            "SELECT (SELECT COUNT(*) FROM campus_sessions_journal
                     WHERE tenant_id = ?1 AND state NOT IN ('terminated', 'failed'))
                  + (SELECT COUNT(*) FROM {AWAITING_RESET} AND l.lab_id != ?2)"
        ))
        .bind(&r.tenant_id)
        .bind(&lab.lab_id)
        .fetch_one(&mut *tx)
        .await?;
        // A session can only run in a registered slot, so the configured
        // count never admits more sessions than slots exist. A held
        // (quarantined or retired) slot counts only while its occupant,
        // already in `tenant_live`, still holds it.
        let usable_slots: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM campus_lab_slots s
             WHERE s.tenant_id = ?1 AND (
               s.occupant_session_id IS NOT NULL OR NOT EXISTS (
                 SELECT 1 FROM campus_lab_slot_holds h
                 WHERE h.slot_id = s.slot_id AND h.released_at IS NULL
               )
             )",
        )
        .bind(&r.tenant_id)
        .fetch_one(&mut *tx)
        .await?;
        if tenant_live >= limits.lab_slots.min(usable_slots) {
            return Ok(CampusLabAdmission::Rejected(
                CampusLabRejection::NoSlotsAvailable,
            ));
        }
        // Checked after the tenant so that a full tenant is always reported as
        // having no slots, whichever course asks.
        if course_live >= limits.max_active_per_course {
            return Ok(CampusLabAdmission::Rejected(
                CampusLabRejection::CourseCapacityReached,
            ));
        }

        let (session, created) = insert_campus_lab_session(
            &mut tx,
            &CreateCampusLabSession {
                session_id: r.candidate_session_id.clone(),
                tenant_id: r.tenant_id.clone(),
                course_id: r.course_id.clone(),
                issuer: r.issuer.clone(),
                subject: r.subject.clone(),
                profile: r.profile.clone(),
                expires_at: r.expires_at,
                actor: r.actor.clone(),
                now: r.now,
            },
        )
        .await?;
        if !created {
            return Err(CampusJournalError::Conflict(format!(
                "lab session {} already exists",
                session.session_id
            )));
        }
        let generation = lab.generation + 1;
        sqlx::query(
            "INSERT INTO campus_lab_generations (
               session_id, lab_id, generation, effective_policy_hash, reset_from_session_id,
               created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(&session.session_id)
        .bind(&lab.lab_id)
        .bind(generation)
        .bind(&r.effective_policy_hash)
        .bind(&r.reset_from_session_id)
        .bind(r.now)
        .execute(&mut *tx)
        .await?;
        let moved = sqlx::query(
            "UPDATE campus_labs SET current_session_id = ?1, generation = ?2,
               reset_requested_at = NULL, updated_at = ?3
             WHERE lab_id = ?4 AND generation = ?5",
        )
        .bind(&session.session_id)
        .bind(generation)
        .bind(r.now)
        .bind(&lab.lab_id)
        .bind(lab.generation)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if moved != 1 {
            return Err(CampusJournalError::Fenced(format!(
                "lab {} moved past generation {}",
                lab.lab_id, lab.generation
            )));
        }
        if let Some(key) = &r.idempotency_key {
            insert_launch_request(&mut tx, &r, key, &session.session_id).await?;
        }
        insert_event(
            &mut tx,
            NewEvent {
                session_id: &session.session_id,
                kind: "campus.lab.admitted",
                from: None,
                to: None,
                generation: session.generation,
                op_key: None,
                actor: &r.actor,
                payload: json!({
                    "lab_id": lab.lab_id,
                    "assignment_id": lab.assignment_id,
                    "lab_generation": generation,
                    "budget_id": lab.budget_id,
                    "reset_from_session_id": r.reset_from_session_id,
                    "effective_policy_hash": r.effective_policy_hash,
                    "tenant_live_before": tenant_live,
                    "course_live_before": course_live,
                }),
                now: r.now,
            },
        )
        .await?;
        let record = load_record(&mut tx, &session.session_id)
            .await?
            .ok_or_else(|| CampusJournalError::NotFound(session.session_id.clone()))?;
        tx.commit().await?;
        Ok(CampusLabAdmission::Created(record))
    }

    /// Ask a lab session to stop, in one transaction with its lab.
    ///
    /// With `reset`, the lab records that a reset is waiting for this
    /// session's teardown; without it, any pending reset is cancelled. Only
    /// the lab's current session changes the lab. Idempotent.
    pub async fn request_campus_lab_stop(
        &self,
        session_id: &str,
        actor: &str,
        reason: &str,
        reset: bool,
        now: i64,
    ) -> CampusResult<CampusLabSessionRecord> {
        let mut tx = self.begin_campus_write().await?;
        let record = load_record(&mut tx, session_id)
            .await?
            .ok_or_else(|| CampusJournalError::NotFound(session_id.into()))?;
        let session = request_campus_lab_stop_in(&mut tx, session_id, actor, reason, now).await?;
        if record.is_current() {
            let reset_requested_at = if reset && !session.state.is_terminal() {
                Some(record.lab.reset_requested_at.unwrap_or(now))
            } else {
                None
            };
            if reset_requested_at != record.lab.reset_requested_at {
                sqlx::query(
                    "UPDATE campus_labs SET reset_requested_at = ?1, updated_at = ?2
                     WHERE lab_id = ?3 AND generation = ?4",
                )
                .bind(reset_requested_at)
                .bind(now)
                .bind(&record.lab.lab_id)
                .bind(record.lab.generation)
                .execute(&mut *tx)
                .await?;
            }
        }
        let record = load_record(&mut tx, session_id)
            .await?
            .ok_or_else(|| CampusJournalError::NotFound(session_id.into()))?;
        tx.commit().await?;
        Ok(record)
    }

    /// The session with its lab and generation. Journal sessions that were
    /// not admitted through a lab are not returned.
    pub async fn get_campus_lab_session_record(
        &self,
        session_id: &str,
    ) -> CampusResult<Option<CampusLabSessionRecord>> {
        let mut connection = self.pool.acquire().await?;
        load_record(&mut connection, session_id).await
    }

    pub async fn get_campus_lab(
        &self,
        tenant_id: &str,
        course_id: &str,
        assignment_id: &str,
        issuer: &str,
        subject: &str,
    ) -> CampusResult<Option<StoredCampusLab>> {
        sqlx::query(
            "SELECT * FROM campus_labs WHERE tenant_id = ?1 AND course_id = ?2
               AND assignment_id = ?3 AND issuer = ?4 AND subject = ?5",
        )
        .bind(tenant_id)
        .bind(course_id)
        .bind(assignment_id)
        .bind(issuer)
        .bind(subject)
        .fetch_optional(&self.pool)
        .await?
        .map(|row| lab_from_row(&row))
        .transpose()
    }

    /// The identity's lab sessions across every course, newest first.
    pub async fn list_campus_lab_session_records_for_identity(
        &self,
        tenant_id: &str,
        issuer: &str,
        subject: &str,
        limit: i64,
    ) -> CampusResult<Vec<CampusLabSessionRecord>> {
        let mut connection = self.pool.acquire().await?;
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT g.session_id FROM campus_lab_generations g
             JOIN campus_labs l ON l.lab_id = g.lab_id
             WHERE l.tenant_id = ?1 AND l.issuer = ?2 AND l.subject = ?3
             ORDER BY g.created_at DESC, g.generation DESC, g.session_id DESC LIMIT ?4",
        )
        .bind(tenant_id)
        .bind(issuer)
        .bind(subject)
        .bind(limit.clamp(1, MAX_LIST))
        .fetch_all(&mut *connection)
        .await?;
        load_records(&mut connection, &ids).await
    }

    /// Non-terminal lab sessions of a course, oldest first.
    pub async fn list_live_campus_lab_session_records_for_course(
        &self,
        tenant_id: &str,
        course_id: &str,
        limit: i64,
    ) -> CampusResult<Vec<CampusLabSessionRecord>> {
        let mut connection = self.pool.acquire().await?;
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT s.session_id FROM campus_sessions_journal s
             JOIN campus_lab_generations g ON g.session_id = s.session_id
             WHERE s.tenant_id = ?1 AND s.course_id = ?2
               AND s.state NOT IN ('terminated', 'failed')
             ORDER BY s.created_at, s.session_id LIMIT ?3",
        )
        .bind(tenant_id)
        .bind(course_id)
        .bind(limit.clamp(1, MAX_LIST))
        .fetch_all(&mut *connection)
        .await?;
        load_records(&mut connection, &ids).await
    }

    /// Sessions past their absolute expiry that still hold, or are about to
    /// take, a slot. Quarantined sessions are excluded: only a cleanup retry
    /// moves them.
    pub async fn list_expired_campus_lab_session_ids(
        &self,
        tenant_id: &str,
        now: i64,
        limit: i64,
    ) -> CampusResult<Vec<String>> {
        Ok(sqlx::query_scalar(
            "SELECT session_id FROM campus_sessions_journal
             WHERE tenant_id = ?1 AND expires_at <= ?2
               AND state NOT IN ('terminated', 'failed', 'cleanup_blocked')
             ORDER BY expires_at, session_id LIMIT ?3",
        )
        .bind(tenant_id)
        .bind(now)
        .bind(limit.clamp(1, MAX_LIST))
        .fetch_all(&self.pool)
        .await?)
    }

    /// Lab sessions of a tenant that a reconciler can move forward: not
    /// terminal and not quarantined, least recently updated first.
    pub async fn list_steppable_campus_lab_session_ids(
        &self,
        tenant_id: &str,
        limit: i64,
    ) -> CampusResult<Vec<String>> {
        Ok(sqlx::query_scalar(
            "SELECT s.session_id FROM campus_sessions_journal s
             JOIN campus_lab_generations g ON g.session_id = s.session_id
             WHERE s.tenant_id = ?1
               AND s.state NOT IN ('terminated', 'failed', 'cleanup_blocked')
             ORDER BY s.updated_at, s.session_id LIMIT ?2",
        )
        .bind(tenant_id)
        .bind(limit.clamp(1, MAX_LIST))
        .fetch_all(&self.pool)
        .await?)
    }

    /// Labs whose current session finished tearing down while a reset waits
    /// to admit the next generation, oldest request first.
    pub async fn list_campus_labs_awaiting_reset(
        &self,
        tenant_id: &str,
        limit: i64,
    ) -> CampusResult<Vec<StoredCampusLab>> {
        sqlx::query(&format!(
            "SELECT l.* FROM {AWAITING_RESET}
             ORDER BY l.reset_requested_at, l.lab_id LIMIT ?2"
        ))
        .bind(tenant_id)
        .bind(limit.clamp(1, MAX_LIST))
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(lab_from_row)
        .collect()
    }

    /// Drop a pending reset that can no longer be admitted, e.g. because the
    /// assignment was withdrawn. Only applies while `session_id` is still the
    /// lab's current session; returns whether a reset was cleared.
    pub async fn clear_campus_lab_reset(
        &self,
        lab_id: &str,
        session_id: &str,
        now: i64,
    ) -> CampusResult<bool> {
        let cleared = sqlx::query(
            "UPDATE campus_labs SET reset_requested_at = NULL, updated_at = ?1
             WHERE lab_id = ?2 AND current_session_id = ?3 AND reset_requested_at IS NOT NULL",
        )
        .bind(now)
        .bind(lab_id)
        .bind(session_id)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(cleared == 1)
    }

    /// Non-terminal journal sessions of a tenant; each holds or will take a
    /// slot.
    pub async fn count_live_campus_lab_sessions(&self, tenant_id: &str) -> CampusResult<i64> {
        Ok(sqlx::query_scalar(
            "SELECT COUNT(*) FROM campus_sessions_journal
             WHERE tenant_id = ?1 AND state NOT IN ('terminated', 'failed')",
        )
        .bind(tenant_id)
        .fetch_one(&self.pool)
        .await?)
    }
}

fn count(values: &[String]) -> i64 {
    i64::try_from(values.len()).unwrap_or(i64::MAX)
}

async fn insert_launch_request(
    connection: &mut SqliteConnection,
    request: &AdmitCampusLabLaunch,
    key: &str,
    session_id: &str,
) -> CampusResult<()> {
    sqlx::query(
        "INSERT INTO campus_launch_requests (
           tenant_id, issuer, subject, idempotency_key, course_id, assignment_id, session_id,
           created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )
    .bind(&request.tenant_id)
    .bind(&request.issuer)
    .bind(&request.subject)
    .bind(key)
    .bind(&request.course_id)
    .bind(&request.assignment_id)
    .bind(session_id)
    .bind(request.now)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

async fn load_records(
    connection: &mut SqliteConnection,
    ids: &[String],
) -> CampusResult<Vec<CampusLabSessionRecord>> {
    let mut records = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(record) = load_record(connection, id).await? {
            records.push(record);
        }
    }
    Ok(records)
}

pub(super) async fn load_record(
    connection: &mut SqliteConnection,
    session_id: &str,
) -> CampusResult<Option<CampusLabSessionRecord>> {
    let Some(session) = load_session(connection, session_id).await? else {
        return Ok(None);
    };
    let Some(row) = sqlx::query(
        "SELECT g.generation AS lab_generation, g.effective_policy_hash, g.reset_from_session_id,
                l.*, i.id AS owner_identity_id
         FROM campus_lab_generations g
         JOIN campus_labs l ON l.lab_id = g.lab_id
         JOIN campus_identities i
           ON i.tenant_id = l.tenant_id AND i.issuer = l.issuer AND i.subject = l.subject
         WHERE g.session_id = ?1",
    )
    .bind(session_id)
    .fetch_optional(&mut *connection)
    .await?
    else {
        return Ok(None);
    };
    let lab = lab_from_row(&row)?;
    Ok(Some(CampusLabSessionRecord {
        generation: row.try_get("lab_generation")?,
        effective_policy_hash: row.try_get("effective_policy_hash")?,
        reset_from_session_id: row.try_get("reset_from_session_id")?,
        owner_identity_id: row.try_get("owner_identity_id")?,
        session,
        lab,
    }))
}

fn lab_from_row(row: &SqliteRow) -> CampusResult<StoredCampusLab> {
    Ok(StoredCampusLab {
        lab_id: row.try_get("lab_id")?,
        tenant_id: row.try_get("tenant_id")?,
        course_id: row.try_get("course_id")?,
        assignment_id: row.try_get("assignment_id")?,
        issuer: row.try_get("issuer")?,
        subject: row.try_get("subject")?,
        budget_id: row.try_get("budget_id")?,
        generation: row.try_get("generation")?,
        current_session_id: row.try_get("current_session_id")?,
        reset_requested_at: row.try_get("reset_requested_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[cfg(test)]
mod tests;

use super::campus_admission::load_record;
use super::SqliteStore;
use crate::{CampusJournalError, IssueCampusBrokerToken, StoredCampusBrokerToken};
use pharness_core::campus_broker_token::{
    CampusBrokerAudience, CampusBrokerDenial, CampusBrokerGrant, CampusBrokerTokenFacts,
    CAMPUS_BROKER_MAX_LIVE_TOKENS, CAMPUS_BROKER_TOKEN_MAX_TTL_SECONDS,
};
use pharness_core::{CampusLabDesiredState, CampusLabSessionState, CampusWorkloadRef};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

type CampusResult<T> = Result<T, CampusJournalError>;

const MAX_REASON_CHARS: usize = 64;

impl SqliteStore {
    /// Record a new broker token for a workload, in one `BEGIN IMMEDIATE`
    /// transaction with the checks that allow it:
    ///
    /// - the session is `Provisioning` or `Ready`, not asked to stop and not
    ///   revoked or expired;
    /// - `workload` is exactly the slot occupancy the session holds;
    /// - the session is its lab's current generation.
    ///
    /// The token expires after the requested TTL, capped at 15 minutes and at
    /// the session's expiry. Live tokens of the same session and audience
    /// beyond the newest [`CAMPUS_BROKER_MAX_LIVE_TOKENS`] are revoked as
    /// `superseded`. Because a stop request and this write take the same
    /// lock, a token can never be issued after its session was stopped.
    pub async fn issue_campus_broker_token(
        &self,
        request: IssueCampusBrokerToken,
    ) -> CampusResult<StoredCampusBrokerToken> {
        let r = request;
        if r.token_id.is_empty()
            || r.token_id.len() > 128
            || r.actor.trim().is_empty()
            || r.token_hash.len() != 64
            || !r.token_hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(CampusJournalError::Invalid(
                "a broker token needs an id, an actor and a SHA-256 hash".into(),
            ));
        }
        if !(1..=CAMPUS_BROKER_TOKEN_MAX_TTL_SECONDS).contains(&r.ttl_seconds) {
            return Err(CampusJournalError::Invalid(format!(
                "a broker token lives 1-{CAMPUS_BROKER_TOKEN_MAX_TTL_SECONDS} seconds"
            )));
        }
        let mut tx = self.begin_campus_write().await?;
        let record = load_record(&mut tx, &r.workload.session_id)
            .await?
            .ok_or_else(|| CampusJournalError::NotFound(r.workload.session_id.clone()))?;
        let session = &record.session;
        if !matches!(
            session.state,
            CampusLabSessionState::Provisioning | CampusLabSessionState::Ready
        ) || session.desired_state != CampusLabDesiredState::Ready
            || session.revoked_at.is_some()
        {
            return Err(CampusJournalError::Conflict(format!(
                "a {} session cannot be given a broker token",
                session.state
            )));
        }
        if r.now >= session.expires_at {
            return Err(CampusJournalError::Conflict(
                "the session has expired".into(),
            ));
        }
        if session.workload().as_ref() != Some(&r.workload) {
            return Err(CampusJournalError::Fenced(
                "the workload is not the session's current slot occupancy".into(),
            ));
        }
        if !record.is_current() {
            return Err(CampusJournalError::Fenced(
                "the session is no longer its lab's current generation".into(),
            ));
        }
        let expires_at = r.now.saturating_add(r.ttl_seconds).min(session.expires_at);
        sqlx::query(
            "INSERT INTO campus_broker_tokens (
               token_id, token_hash, audience, session_id, slot_id, slot_generation,
               lab_id, lab_generation, issued_by, issued_at, expires_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )
        .bind(&r.token_id)
        .bind(&r.token_hash)
        .bind(r.audience.as_str())
        .bind(&r.workload.session_id)
        .bind(&r.workload.slot_id)
        .bind(r.workload.slot_generation)
        .bind(&record.lab.lab_id)
        .bind(record.generation)
        .bind(&r.actor)
        .bind(r.now)
        .bind(expires_at)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE campus_broker_tokens SET revoked_at = ?1, revoked_reason = 'superseded'
             WHERE session_id = ?2 AND audience = ?3 AND revoked_at IS NULL
               AND sequence NOT IN (
                 SELECT sequence FROM campus_broker_tokens
                 WHERE session_id = ?2 AND audience = ?3 AND revoked_at IS NULL
                 ORDER BY sequence DESC LIMIT ?4
               )",
        )
        .bind(r.now)
        .bind(&r.workload.session_id)
        .bind(r.audience.as_str())
        .bind(CAMPUS_BROKER_MAX_LIVE_TOKENS)
        .execute(&mut *tx)
        .await?;
        let stored = load_token(&mut tx, &r.token_id)
            .await?
            .ok_or_else(|| CampusJournalError::NotFound(r.token_id.clone()))?;
        tx.commit().await?;
        Ok(stored)
    }

    /// The per-request check of a presented token: one read of the token, its
    /// session, its lab and the owner, decided by
    /// [`CampusBrokerTokenFacts::evaluate`]. Nothing is cached, so a stop,
    /// reset or expiry applies to the very next request.
    pub async fn authenticate_campus_broker_token(
        &self,
        token_hash: &str,
        audience: CampusBrokerAudience,
        now: i64,
    ) -> CampusResult<Result<CampusBrokerGrant, CampusBrokerDenial>> {
        let row = sqlx::query(
            "SELECT t.token_id, t.audience, t.session_id, t.slot_id, t.slot_generation,
                    t.lab_generation, t.expires_at, t.revoked_at,
                    s.tenant_id, s.course_id, s.state, s.desired_state,
                    s.revoked_at AS session_revoked_at, s.expires_at AS session_expires_at,
                    s.slot_id AS session_slot_id, s.slot_generation AS session_slot_generation,
                    l.current_session_id, l.generation AS current_lab_generation,
                    i.id AS owner_identity_id
             FROM campus_broker_tokens t
             JOIN campus_sessions_journal s ON s.session_id = t.session_id
             JOIN campus_labs l ON l.lab_id = t.lab_id
             JOIN campus_identities i
               ON i.tenant_id = l.tenant_id AND i.issuer = l.issuer AND i.subject = l.subject
             WHERE t.token_hash = ?1",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(Err(CampusBrokerDenial::UnknownToken));
        };
        let session_workload = match (
            row.try_get::<Option<String>, _>("session_slot_id")?,
            row.try_get::<Option<i64>, _>("session_slot_generation")?,
        ) {
            (Some(slot_id), Some(slot_generation)) => Some(CampusWorkloadRef {
                session_id: row.try_get("session_id")?,
                slot_id,
                slot_generation,
            }),
            _ => None,
        };
        let state: String = row.try_get("state")?;
        let desired: String = row.try_get("desired_state")?;
        let facts = CampusBrokerTokenFacts {
            token_id: row.try_get("token_id")?,
            audience: parse_audience(&row.try_get::<String, _>("audience")?)?,
            workload: workload_from_row(&row)?,
            lab_generation: row.try_get("lab_generation")?,
            expires_at: row.try_get("expires_at")?,
            revoked: row.try_get::<Option<i64>, _>("revoked_at")?.is_some(),
            tenant_id: row.try_get("tenant_id")?,
            course_id: row.try_get("course_id")?,
            owner_identity_id: row.try_get("owner_identity_id")?,
            session_state: CampusLabSessionState::parse(&state)
                .ok_or_else(|| invalid(format!("unknown campus session state {state}")))?,
            session_desired_state: CampusLabDesiredState::parse(&desired)
                .ok_or_else(|| invalid(format!("unknown campus desired state {desired}")))?,
            session_revoked: row
                .try_get::<Option<i64>, _>("session_revoked_at")?
                .is_some(),
            session_expires_at: row.try_get("session_expires_at")?,
            session_workload,
            current_session_id: row.try_get("current_session_id")?,
            current_lab_generation: row.try_get("current_lab_generation")?,
        };
        Ok(facts.evaluate(audience, now))
    }

    /// Every token issued for a session, oldest first. Hashes only.
    pub async fn list_campus_broker_tokens(
        &self,
        session_id: &str,
    ) -> CampusResult<Vec<StoredCampusBrokerToken>> {
        sqlx::query("SELECT * FROM campus_broker_tokens WHERE session_id = ?1 ORDER BY sequence")
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(token_from_row)
            .collect()
    }

    /// Revoke every live token of a session, e.g. when an operator suspects
    /// a leak. Stopping or resetting a session already does this. Returns the
    /// number of tokens revoked.
    pub async fn revoke_campus_broker_tokens(
        &self,
        session_id: &str,
        reason: &str,
        now: i64,
    ) -> CampusResult<u64> {
        if reason.trim().is_empty() || reason.chars().count() > MAX_REASON_CHARS {
            return Err(CampusJournalError::Invalid(format!(
                "a revocation reason must be 1-{MAX_REASON_CHARS} characters"
            )));
        }
        let mut tx = self.begin_campus_write().await?;
        let revoked = sqlx::query(
            "UPDATE campus_broker_tokens SET revoked_at = ?1, revoked_reason = ?2
             WHERE session_id = ?3 AND revoked_at IS NULL",
        )
        .bind(now)
        .bind(reason)
        .bind(session_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await?;
        Ok(revoked)
    }
}

fn invalid(message: String) -> CampusJournalError {
    CampusJournalError::Store(crate::StoreError::InvalidData(message))
}

fn parse_audience(value: &str) -> CampusResult<CampusBrokerAudience> {
    CampusBrokerAudience::parse(value)
        .ok_or_else(|| invalid(format!("unknown campus broker audience {value}")))
}

fn workload_from_row(row: &SqliteRow) -> CampusResult<CampusWorkloadRef> {
    Ok(CampusWorkloadRef {
        session_id: row.try_get("session_id")?,
        slot_id: row.try_get("slot_id")?,
        slot_generation: row.try_get("slot_generation")?,
    })
}

fn token_from_row(row: &SqliteRow) -> CampusResult<StoredCampusBrokerToken> {
    Ok(StoredCampusBrokerToken {
        sequence: row.try_get("sequence")?,
        token_id: row.try_get("token_id")?,
        token_hash: row.try_get("token_hash")?,
        audience: parse_audience(&row.try_get::<String, _>("audience")?)?,
        workload: workload_from_row(row)?,
        lab_id: row.try_get("lab_id")?,
        lab_generation: row.try_get("lab_generation")?,
        issued_by: row.try_get("issued_by")?,
        issued_at: row.try_get("issued_at")?,
        expires_at: row.try_get("expires_at")?,
        revoked_at: row.try_get("revoked_at")?,
        revoked_reason: row.try_get("revoked_reason")?,
    })
}

async fn load_token(
    connection: &mut SqliteConnection,
    token_id: &str,
) -> CampusResult<Option<StoredCampusBrokerToken>> {
    sqlx::query("SELECT * FROM campus_broker_tokens WHERE token_id = ?1")
        .bind(token_id)
        .fetch_optional(&mut *connection)
        .await?
        .map(|row| token_from_row(&row))
        .transpose()
}

#[cfg(test)]
mod tests;

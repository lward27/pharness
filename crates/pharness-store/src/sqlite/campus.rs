use super::{SqliteStore, StoreError};
use crate::{
    ActiveCampusSession, CampusCourseRole, CampusIdentityKey, ConsumedCampusLoginAttempt,
    CreateCampusLoginAttempt, CreateCampusSession, PutCampusMembership, StoredCampusIdentity,
    StoredCampusMembership, UpsertCampusIdentity,
};
use sqlx::sqlite::SqliteRow;
use sqlx::Row;

/// Expired sessions are kept briefly so that a late request still reads as
/// expired rather than unknown; afterwards they are deleted on new logins.
const EXPIRED_SESSION_RETENTION_SECONDS: i64 = 86_400;

impl SqliteStore {
    /// Record a one-time login attempt, first deleting expired attempts.
    ///
    /// Returns a conflict when `max_outstanding` unexpired attempts already
    /// exist, which bounds the table an unauthenticated caller can grow.
    pub async fn create_campus_login_attempt(
        &self,
        attempt: CreateCampusLoginAttempt,
        max_outstanding: i64,
    ) -> Result<(), StoreError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("DELETE FROM campus_login_attempts WHERE expires_at <= ?1")
            .bind(attempt.now)
            .execute(&mut *transaction)
            .await?;
        let outstanding: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM campus_login_attempts WHERE consumed_at IS NULL",
        )
        .fetch_one(&mut *transaction)
        .await?;
        if outstanding >= max_outstanding {
            return Err(StoreError::Conflict(
                "too many outstanding campus login attempts".into(),
            ));
        }
        sqlx::query(
            r#"
            INSERT INTO campus_login_attempts (
              state_hash, tenant_id, browser_binding_hash, nonce, pkce_verifier,
              created_at, expires_at, consumed_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)
            "#,
        )
        .bind(&attempt.state_hash)
        .bind(&attempt.tenant_id)
        .bind(&attempt.browser_binding_hash)
        .bind(&attempt.nonce)
        .bind(&attempt.pkce_verifier)
        .bind(attempt.now)
        .bind(attempt.expires_at)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Atomically mark a login attempt consumed and return it.
    ///
    /// Exactly one caller can consume a state; a replay, an unknown state or
    /// an already consumed state returns `None`. Expiry and browser binding
    /// are checked by the caller after consumption so that a failed check
    /// still burns the attempt.
    pub async fn consume_campus_login_attempt(
        &self,
        state_hash: &str,
        now: i64,
    ) -> Result<Option<ConsumedCampusLoginAttempt>, StoreError> {
        let row = sqlx::query(
            r#"
            UPDATE campus_login_attempts SET consumed_at = ?2
            WHERE state_hash = ?1 AND consumed_at IS NULL
            RETURNING tenant_id, browser_binding_hash, nonce, pkce_verifier, expires_at
            "#,
        )
        .bind(state_hash)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            Ok(ConsumedCampusLoginAttempt {
                tenant_id: row.try_get("tenant_id")?,
                browser_binding_hash: row.try_get("browser_binding_hash")?,
                nonce: row.try_get("nonce")?,
                pkce_verifier: row.try_get("pkce_verifier")?,
                expires_at: row.try_get("expires_at")?,
            })
        })
        .transpose()
    }

    /// Insert or refresh the identity for `(tenant_id, issuer, subject)`.
    ///
    /// Display claims are overwritten on every login; they never select the
    /// row, so an email change cannot move an identity.
    pub async fn upsert_campus_identity(
        &self,
        identity: UpsertCampusIdentity,
    ) -> Result<StoredCampusIdentity, StoreError> {
        let row = sqlx::query(
            r#"
            INSERT INTO campus_identities (
              id, tenant_id, issuer, subject, email, display_name,
              created_at, updated_at, last_login_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?7)
            ON CONFLICT (tenant_id, issuer, subject) DO UPDATE SET
              email = excluded.email,
              display_name = excluded.display_name,
              updated_at = excluded.updated_at,
              last_login_at = excluded.last_login_at
            RETURNING *
            "#,
        )
        .bind(&identity.candidate_id)
        .bind(&identity.key.tenant_id)
        .bind(&identity.key.issuer)
        .bind(&identity.key.subject)
        .bind(&identity.email)
        .bind(&identity.display_name)
        .bind(identity.now)
        .fetch_one(&self.pool)
        .await?;
        row_to_identity(&row)
    }

    pub async fn create_campus_session(
        &self,
        session: CreateCampusSession,
    ) -> Result<(), StoreError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("DELETE FROM campus_sessions WHERE absolute_expires_at <= ?1")
            .bind(session.now - EXPIRED_SESSION_RETENTION_SECONDS)
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            r#"
            INSERT INTO campus_sessions (
              id_hash, tenant_id, identity_id, csrf_hash, created_at, last_seen_at,
              idle_expires_at, absolute_expires_at, revoked_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7, NULL)
            "#,
        )
        .bind(&session.id_hash)
        .bind(&session.tenant_id)
        .bind(&session.identity_id)
        .bind(&session.csrf_hash)
        .bind(session.now)
        .bind(session.idle_expires_at)
        .bind(session.absolute_expires_at)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Validate a session at `now` and extend its idle window.
    ///
    /// Returns `None` for an unknown, revoked, idle-expired or absolutely
    /// expired session, or one belonging to another tenant. The idle window
    /// never extends past the absolute expiry.
    pub async fn touch_campus_session(
        &self,
        id_hash: &str,
        tenant_id: &str,
        now: i64,
        idle_ttl_seconds: i64,
    ) -> Result<Option<ActiveCampusSession>, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query(
            r#"
            UPDATE campus_sessions SET
              last_seen_at = ?3,
              idle_expires_at = MIN(?3 + ?4, absolute_expires_at)
            WHERE id_hash = ?1
              AND tenant_id = ?2
              AND revoked_at IS NULL
              AND idle_expires_at > ?3
              AND absolute_expires_at > ?3
            RETURNING tenant_id, identity_id, csrf_hash, idle_expires_at, absolute_expires_at
            "#,
        )
        .bind(id_hash)
        .bind(tenant_id)
        .bind(now)
        .bind(idle_ttl_seconds)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let identity_id: String = row.try_get("identity_id")?;
        let identity_row = sqlx::query("SELECT * FROM campus_identities WHERE id = ?1")
            .bind(&identity_id)
            .fetch_one(&mut *transaction)
            .await?;
        let identity = row_to_identity(&identity_row)?;
        transaction.commit().await?;
        if identity.tenant_id != tenant_id {
            return Ok(None);
        }
        Ok(Some(ActiveCampusSession {
            tenant_id: row.try_get("tenant_id")?,
            csrf_hash: row.try_get("csrf_hash")?,
            idle_expires_at: row.try_get("idle_expires_at")?,
            absolute_expires_at: row.try_get("absolute_expires_at")?,
            identity,
        }))
    }

    /// Revoke a session. Returns whether an unrevoked session was revoked.
    pub async fn revoke_campus_session(&self, id_hash: &str, now: i64) -> Result<bool, StoreError> {
        let result = sqlx::query(
            "UPDATE campus_sessions SET revoked_at = ?2 WHERE id_hash = ?1 AND revoked_at IS NULL",
        )
        .bind(id_hash)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Make the bootstrap administrators of `tenant_id` exactly `admins`.
    ///
    /// Removing a pair from configuration therefore revokes it at the next
    /// startup.
    pub async fn sync_campus_bootstrap_admins(
        &self,
        tenant_id: &str,
        admins: &[(String, String)],
        now: i64,
    ) -> Result<(), StoreError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "DELETE FROM campus_tenant_admins WHERE tenant_id = ?1 AND source = 'bootstrap'",
        )
        .bind(tenant_id)
        .execute(&mut *transaction)
        .await?;
        for (issuer, subject) in admins {
            sqlx::query(
                r#"
                INSERT INTO campus_tenant_admins (tenant_id, issuer, subject, source, created_at)
                VALUES (?1, ?2, ?3, 'bootstrap', ?4)
                ON CONFLICT (tenant_id, issuer, subject) DO NOTHING
                "#,
            )
            .bind(tenant_id)
            .bind(issuer)
            .bind(subject)
            .bind(now)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn is_campus_tenant_admin(
        &self,
        key: &CampusIdentityKey,
    ) -> Result<bool, StoreError> {
        let found: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM campus_tenant_admins WHERE tenant_id = ?1 AND issuer = ?2 AND subject = ?3",
        )
        .bind(&key.tenant_id)
        .bind(&key.issuer)
        .bind(&key.subject)
        .fetch_optional(&self.pool)
        .await?;
        Ok(found.is_some())
    }

    pub async fn get_campus_course_role(
        &self,
        key: &CampusIdentityKey,
        course_id: &str,
    ) -> Result<Option<CampusCourseRole>, StoreError> {
        let role: Option<String> = sqlx::query_scalar(
            r#"
            SELECT role FROM campus_course_memberships
            WHERE tenant_id = ?1 AND course_id = ?2 AND issuer = ?3 AND subject = ?4
            "#,
        )
        .bind(&key.tenant_id)
        .bind(course_id)
        .bind(&key.issuer)
        .bind(&key.subject)
        .fetch_optional(&self.pool)
        .await?;
        role.map(|role| parse_role(&role)).transpose()
    }

    /// Courses and roles held by one identity within its tenant.
    pub async fn list_campus_memberships_for_identity(
        &self,
        key: &CampusIdentityKey,
    ) -> Result<Vec<(String, CampusCourseRole)>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT course_id, role FROM campus_course_memberships
            WHERE tenant_id = ?1 AND issuer = ?2 AND subject = ?3
            ORDER BY course_id
            "#,
        )
        .bind(&key.tenant_id)
        .bind(&key.issuer)
        .bind(&key.subject)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let role: String = row.try_get("role")?;
                Ok((row.try_get("course_id")?, parse_role(&role)?))
            })
            .collect()
    }

    pub async fn campus_course_exists(
        &self,
        tenant_id: &str,
        course_id: &str,
    ) -> Result<bool, StoreError> {
        let found: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM campus_course_memberships WHERE tenant_id = ?1 AND course_id = ?2 LIMIT 1",
        )
        .bind(tenant_id)
        .bind(course_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(found.is_some())
    }

    pub async fn list_campus_course_members(
        &self,
        tenant_id: &str,
        course_id: &str,
    ) -> Result<Vec<StoredCampusMembership>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT m.tenant_id, m.course_id, m.issuer, m.subject, m.role,
                   m.granted_by_identity_id, m.created_at, m.updated_at,
                   i.email, i.display_name
            FROM campus_course_memberships m
            LEFT JOIN campus_identities i
              ON i.tenant_id = m.tenant_id AND i.issuer = m.issuer AND i.subject = m.subject
            WHERE m.tenant_id = ?1 AND m.course_id = ?2
            ORDER BY m.role, m.issuer, m.subject
            "#,
        )
        .bind(tenant_id)
        .bind(course_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_membership).collect()
    }

    /// Create or change one membership. Returns whether a row was created.
    pub async fn put_campus_course_membership(
        &self,
        membership: PutCampusMembership,
    ) -> Result<bool, StoreError> {
        let existed = self
            .get_campus_course_role(&membership.key, &membership.course_id)
            .await?
            .is_some();
        sqlx::query(
            r#"
            INSERT INTO campus_course_memberships (
              tenant_id, course_id, issuer, subject, role, granted_by_identity_id,
              created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
            ON CONFLICT (tenant_id, course_id, issuer, subject) DO UPDATE SET
              role = excluded.role,
              granted_by_identity_id = excluded.granted_by_identity_id,
              updated_at = excluded.updated_at
            "#,
        )
        .bind(&membership.key.tenant_id)
        .bind(&membership.course_id)
        .bind(&membership.key.issuer)
        .bind(&membership.key.subject)
        .bind(membership.role.as_str())
        .bind(&membership.granted_by_identity_id)
        .bind(membership.now)
        .execute(&self.pool)
        .await?;
        Ok(!existed)
    }

    pub async fn delete_campus_course_membership(
        &self,
        key: &CampusIdentityKey,
        course_id: &str,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            DELETE FROM campus_course_memberships
            WHERE tenant_id = ?1 AND course_id = ?2 AND issuer = ?3 AND subject = ?4
            "#,
        )
        .bind(&key.tenant_id)
        .bind(course_id)
        .bind(&key.issuer)
        .bind(&key.subject)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}

fn parse_role(value: &str) -> Result<CampusCourseRole, StoreError> {
    CampusCourseRole::parse(value)
        .ok_or_else(|| StoreError::InvalidData(format!("unknown campus course role {value}")))
}

fn row_to_identity(row: &SqliteRow) -> Result<StoredCampusIdentity, StoreError> {
    Ok(StoredCampusIdentity {
        id: row.try_get("id")?,
        tenant_id: row.try_get("tenant_id")?,
        issuer: row.try_get("issuer")?,
        subject: row.try_get("subject")?,
        email: row.try_get("email")?,
        display_name: row.try_get("display_name")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        last_login_at: row.try_get("last_login_at")?,
    })
}

fn row_to_membership(row: &SqliteRow) -> Result<StoredCampusMembership, StoreError> {
    let role: String = row.try_get("role")?;
    Ok(StoredCampusMembership {
        tenant_id: row.try_get("tenant_id")?,
        course_id: row.try_get("course_id")?,
        issuer: row.try_get("issuer")?,
        subject: row.try_get("subject")?,
        role: parse_role(&role)?,
        email: row.try_get("email")?,
        display_name: row.try_get("display_name")?,
        granted_by_identity_id: row.try_get("granted_by_identity_id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[cfg(test)]
mod tests;

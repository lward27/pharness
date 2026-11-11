//! Campus broker tokens (migration 0060) and the journal-backed issuer a
//! trusted execution provider uses to mint them.
//!
//! See [`pharness_workspaces::campus_broker_token`] for the token model. This
//! module adds the stored record and [`JournalCampusBrokerTokenIssuer`],
//! which draws each secret from the system CSPRNG and stores only its hash.

use crate::{CampusJournalError, SqliteStore};
use async_trait::async_trait;
use pharness_workspaces::campus_broker_token::{
    CampusBrokerAudience, CampusBrokerSecret, CampusBrokerTokenError, CampusBrokerTokenIssuer,
    CampusIssuedBrokerToken, CAMPUS_BROKER_TOKEN_MAX_TTL_SECONDS,
};
use pharness_workspaces::campus_capability::CampusBrokerClock;
use pharness_workspaces::CampusWorkloadRef;
use ring::rand::{SecureRandom, SystemRandom};
use serde::Serialize;
use std::fmt::Write;
use std::sync::Arc;

/// A stored token. It holds the hash only; the raw token is never stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StoredCampusBrokerToken {
    pub sequence: i64,
    pub token_id: String,
    pub token_hash: String,
    pub audience: CampusBrokerAudience,
    pub workload: CampusWorkloadRef,
    pub lab_id: String,
    pub lab_generation: i64,
    pub issued_by: String,
    pub issued_at: i64,
    pub expires_at: i64,
    pub revoked_at: Option<i64>,
    pub revoked_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueCampusBrokerToken {
    pub token_id: String,
    /// [`pharness_workspaces::campus_broker_token::campus_broker_token_hash`] of the
    /// raw token.
    pub token_hash: String,
    pub workload: CampusWorkloadRef,
    pub audience: CampusBrokerAudience,
    pub actor: String,
    pub now: i64,
    /// Capped at [`CAMPUS_BROKER_TOKEN_MAX_TTL_SECONDS`] and at the session's
    /// expiry.
    pub ttl_seconds: i64,
}

/// The [`CampusBrokerTokenIssuer`] backed by the lab journal.
pub struct JournalCampusBrokerTokenIssuer {
    store: SqliteStore,
    clock: Arc<dyn CampusBrokerClock>,
    actor: String,
    rng: SystemRandom,
}

impl JournalCampusBrokerTokenIssuer {
    /// `actor` names the trusted process issuing tokens, for the record.
    pub fn new(
        store: SqliteStore,
        clock: Arc<dyn CampusBrokerClock>,
        actor: impl Into<String>,
    ) -> Self {
        Self {
            store,
            clock,
            actor: actor.into(),
            rng: SystemRandom::new(),
        }
    }

    fn random<const N: usize>(&self) -> Result<[u8; N], CampusBrokerTokenError> {
        let mut bytes = [0_u8; N];
        self.rng.fill(&mut bytes).map_err(|_| {
            CampusBrokerTokenError::Unavailable("the system random generator failed".into())
        })?;
        Ok(bytes)
    }
}

#[async_trait]
impl CampusBrokerTokenIssuer for JournalCampusBrokerTokenIssuer {
    async fn issue(
        &self,
        workload: &CampusWorkloadRef,
        audience: CampusBrokerAudience,
    ) -> Result<CampusIssuedBrokerToken, CampusBrokerTokenError> {
        let secret = CampusBrokerSecret::from_random_bytes(&self.random::<32>()?);
        let mut token_id = String::from("cbt_");
        for byte in self.random::<16>()? {
            let _ = write!(token_id, "{byte:02x}");
        }
        let stored = self
            .store
            .issue_campus_broker_token(IssueCampusBrokerToken {
                token_id,
                token_hash: secret.hash(),
                workload: workload.clone(),
                audience,
                actor: self.actor.clone(),
                now: self.clock.now(),
                ttl_seconds: CAMPUS_BROKER_TOKEN_MAX_TTL_SECONDS,
            })
            .await
            .map_err(|error| match error {
                CampusJournalError::NotFound(_)
                | CampusJournalError::Conflict(_)
                | CampusJournalError::Fenced(_)
                | CampusJournalError::Invalid(_) => {
                    CampusBrokerTokenError::NotIssuable(error.to_string())
                }
                other => CampusBrokerTokenError::Unavailable(other.to_string()),
            })?;
        Ok(CampusIssuedBrokerToken {
            token_id: stored.token_id,
            audience: stored.audience,
            workload: stored.workload,
            lab_generation: stored.lab_generation,
            issued_at: stored.issued_at,
            expires_at: stored.expires_at,
            secret,
        })
    }
}

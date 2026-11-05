//! Scoped credentials for the trusted Campus broker endpoints.
//!
//! Student code runs in a sandbox that holds no worker, operator, provider
//! or cluster credential. When it needs a trusted service (today only the
//! inference broker) it presents a broker token instead. A broker token:
//!
//! - is 256 random bits behind a fixed prefix, stored only as a
//!   domain-separated SHA-256 hash;
//! - names exactly one [`CampusBrokerAudience`], one lab session, the slot
//!   occupancy ([`CampusWorkloadRef`]) and the lab generation it was issued
//!   for;
//! - lives at most [`CAMPUS_BROKER_TOKEN_MAX_TTL_SECONDS`] and never past the
//!   session's own expiry;
//! - is checked against the journal on every request: the session must be
//!   `Ready`, not asked to stop, unexpired, still the lab's current
//!   generation and still on the same slot occupancy.
//!
//! Because the active-state check runs per request, revocation (stop, reset,
//! expiry, failure) takes effect on the next request whatever the token's
//! remaining lifetime; the TTL only bounds how long a copied token would stay
//! useful if every other check were somehow bypassed. The token is never
//! accepted by worker, operator or Campus browser routes, and their
//! credentials are never accepted by broker routes.

use crate::{CampusLabDesiredState, CampusLabSessionState, CampusWorkloadRef};
use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

/// Every broker token starts with this. Worker and operator middleware
/// refuse any bearer value with it, and broker middleware refuses anything
/// without it.
pub const CAMPUS_BROKER_TOKEN_PREFIX: &str = "pcbt1_";
/// Base64url characters of a 256-bit secret without padding.
const SECRET_CHARS: usize = 43;
pub const CAMPUS_BROKER_TOKEN_CHARS: usize = CAMPUS_BROKER_TOKEN_PREFIX.len() + SECRET_CHARS;
const HASH_DOMAIN: &[u8] = b"pharness-campus-broker-token-v1\0";

/// Upper bound on one token's lifetime. The store enforces it as well.
pub const CAMPUS_BROKER_TOKEN_MAX_TTL_SECONDS: i64 = 900;
/// A provider replaces a token once it is this old, leaving half its
/// lifetime for the new Secret to reach the Pod.
pub const CAMPUS_BROKER_TOKEN_ROTATE_AFTER_SECONDS: i64 = 450;
/// Live tokens per session and audience: the current one and the one it is
/// replacing. Issuing another revokes the oldest.
pub const CAMPUS_BROKER_MAX_LIVE_TOKENS: i64 = 2;

/// The trusted service a token may be presented to. A token for one audience
/// is refused by every other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusBrokerAudience {
    /// The inference broker. The only audience a sandbox is given.
    Inference,
    /// The capability (tool) broker. Held by the trusted agent runtime,
    /// outside the sandbox.
    Tools,
}

impl CampusBrokerAudience {
    pub const ALL: [Self; 2] = [Self::Inference, Self::Tools];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inference => "inference",
            Self::Tools => "tools",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|audience| audience.as_str() == value)
    }

    /// Whether a token of this audience may be placed in a student sandbox.
    pub fn sandbox_may_hold(self) -> bool {
        self == Self::Inference
    }
}

impl fmt::Display for CampusBrokerAudience {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A raw broker token. It is shown once, to the provider that writes it into
/// the session's Secret, and is never logged or stored.
#[derive(Clone, PartialEq, Eq)]
pub struct CampusBrokerSecret(String);

impl CampusBrokerSecret {
    /// Encode 32 bytes from a cryptographically secure generator.
    pub fn from_random_bytes(bytes: &[u8; 32]) -> Self {
        Self(format!(
            "{CAMPUS_BROKER_TOKEN_PREFIX}{}",
            URL_SAFE_NO_PAD.encode(bytes)
        ))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    /// The form stored at rest.
    pub fn hash(&self) -> String {
        campus_broker_token_hash(&self.0)
    }
}

impl fmt::Debug for CampusBrokerSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CampusBrokerSecret(<redacted>)")
    }
}

/// Whether a bearer value claims to be a broker token, well-formed or not.
/// Routes outside the broker refuse such values outright.
pub fn is_campus_broker_token(value: &str) -> bool {
    value.starts_with(CAMPUS_BROKER_TOKEN_PREFIX)
}

/// Whether a bearer value is a syntactically valid broker token.
pub fn is_well_formed_campus_broker_token(value: &str) -> bool {
    value.len() == CAMPUS_BROKER_TOKEN_CHARS
        && value
            .strip_prefix(CAMPUS_BROKER_TOKEN_PREFIX)
            .is_some_and(|secret| {
                secret
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
            })
}

/// Domain-separated SHA-256 of a raw token, lower-case hex. Only this is
/// stored, so a database copy cannot be replayed as tokens.
pub fn campus_broker_token_hash(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(HASH_DOMAIN);
    hasher.update(token.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// A freshly issued token together with what it is bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampusIssuedBrokerToken {
    pub token_id: String,
    pub audience: CampusBrokerAudience,
    pub workload: CampusWorkloadRef,
    pub lab_generation: i64,
    pub issued_at: i64,
    pub expires_at: i64,
    pub secret: CampusBrokerSecret,
}

impl CampusIssuedBrokerToken {
    /// When the provider should issue and deliver a replacement.
    pub fn rotate_at(&self) -> i64 {
        self.issued_at
            .saturating_add(CAMPUS_BROKER_TOKEN_ROTATE_AFTER_SECONDS)
            .min(self.expires_at)
    }

    pub fn needs_rotation(&self, now: i64) -> bool {
        now >= self.rotate_at()
    }
}

/// What a valid token authorizes: one audience of one session's current
/// generation. Everything here comes from trusted state, none of it from the
/// request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CampusBrokerGrant {
    pub token_id: String,
    pub audience: CampusBrokerAudience,
    pub tenant_id: String,
    pub course_id: String,
    /// `campus_identities.id` of the session owner.
    pub owner_identity_id: String,
    pub workload: CampusWorkloadRef,
    pub lab_generation: i64,
    pub expires_at: i64,
}

impl CampusBrokerGrant {
    pub fn session_id(&self) -> &str {
        &self.workload.session_id
    }
}

/// Why a presented token was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusBrokerDenial {
    /// Not a broker token at all, e.g. a worker or operator token.
    Malformed,
    UnknownToken,
    /// A valid token presented to a service it was not issued for.
    WrongAudience,
    TokenRevoked,
    TokenExpired,
    /// The session is not `Ready`, was asked to stop, or was revoked.
    SessionInactive,
    SessionExpired,
    /// The session no longer holds the slot occupancy, or is no longer its
    /// lab's current generation, that the token was issued for.
    GenerationMismatch,
}

impl CampusBrokerDenial {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::UnknownToken => "unknown_token",
            Self::WrongAudience => "wrong_audience",
            Self::TokenRevoked => "token_revoked",
            Self::TokenExpired => "token_expired",
            Self::SessionInactive => "session_inactive",
            Self::SessionExpired => "session_expired",
            Self::GenerationMismatch => "generation_mismatch",
        }
    }
}

/// Trusted facts about one stored token and its session, read in one query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampusBrokerTokenFacts {
    pub token_id: String,
    pub audience: CampusBrokerAudience,
    /// The occupancy and lab generation the token was issued for.
    pub workload: CampusWorkloadRef,
    pub lab_generation: i64,
    pub expires_at: i64,
    pub revoked: bool,
    pub tenant_id: String,
    pub course_id: String,
    pub owner_identity_id: String,
    pub session_state: CampusLabSessionState,
    pub session_desired_state: CampusLabDesiredState,
    pub session_revoked: bool,
    pub session_expires_at: i64,
    /// The occupancy the session holds now.
    pub session_workload: Option<CampusWorkloadRef>,
    /// The lab's current session and generation now.
    pub current_session_id: Option<String>,
    pub current_lab_generation: i64,
}

impl CampusBrokerTokenFacts {
    /// The per-request decision. The audience is checked first so that a
    /// token replayed against another service is reported as such.
    pub fn evaluate(
        &self,
        audience: CampusBrokerAudience,
        now: i64,
    ) -> Result<CampusBrokerGrant, CampusBrokerDenial> {
        use CampusBrokerDenial as D;
        if self.audience != audience {
            return Err(D::WrongAudience);
        }
        if self.revoked {
            return Err(D::TokenRevoked);
        }
        if now >= self.expires_at {
            return Err(D::TokenExpired);
        }
        if !self.session_state.grants_access()
            || self.session_desired_state != CampusLabDesiredState::Ready
            || self.session_revoked
        {
            return Err(D::SessionInactive);
        }
        if now >= self.session_expires_at {
            return Err(D::SessionExpired);
        }
        if self.session_workload.as_ref() != Some(&self.workload)
            || self.current_session_id.as_deref() != Some(self.workload.session_id.as_str())
            || self.current_lab_generation != self.lab_generation
        {
            return Err(D::GenerationMismatch);
        }
        Ok(CampusBrokerGrant {
            token_id: self.token_id.clone(),
            audience: self.audience,
            tenant_id: self.tenant_id.clone(),
            course_id: self.course_id.clone(),
            owner_identity_id: self.owner_identity_id.clone(),
            workload: self.workload.clone(),
            lab_generation: self.lab_generation,
            expires_at: self.expires_at,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CampusBrokerTokenError {
    /// The workload is not one a token may be issued for now: it is not
    /// provisioning or ready, was asked to stop, or is a stale occupancy.
    #[error("no campus broker token can be issued: {0}")]
    NotIssuable(String),
    #[error("campus broker token issuance is unavailable: {0}")]
    Unavailable(String),
}

/// Issues broker tokens to a trusted execution provider.
///
/// A provider calls this while provisioning a workload, writes the secret
/// into the session's token Secret, and calls it again once
/// [`CampusIssuedBrokerToken::needs_rotation`] says so. Each call returns a
/// new token; the oldest live token beyond [`CAMPUS_BROKER_MAX_LIVE_TOKENS`]
/// is revoked.
#[async_trait]
pub trait CampusBrokerTokenIssuer: Send + Sync {
    async fn issue(
        &self,
        workload: &CampusWorkloadRef,
        audience: CampusBrokerAudience,
    ) -> Result<CampusIssuedBrokerToken, CampusBrokerTokenError>;
}

#[cfg(test)]
mod tests;

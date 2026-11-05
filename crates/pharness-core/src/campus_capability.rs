//! Campus capability broker.
//!
//! Every Campus tool effect passes through one authorization function,
//! [`CampusCapabilityBroker::authorize`], whether the call comes from the
//! agent runtime's tool executor ([`CampusSessionTools`]) or directly from an
//! authenticated Campus principal ([`CampusCaller::Principal`]), which is how
//! an API handler must call it. The broker runs outside
//! the student sandbox and decides from trusted state only:
//!
//! - the [`CampusSessionAuthority`] it resolves from a
//!   [`CampusAuthoritySource`] on every call, including whether it has been
//!   revoked, never from the request;
//! - the [`CampusCaller`] established by the boundary that built the request:
//!   the runtime bound to one session at launch, or an authenticated principal;
//! - the [`CampusResourceSelector`] it derives itself from the typed action.
//!
//! A request's [`CampusSessionClaims`] (session id, effective policy hash and
//! profile hash) only bind the call to the authority the caller believes it
//! holds. They are not credentials: knowing them grants nothing. An unknown,
//! expired or revoked session, a caller that does not own the session, a
//! stale or forged hash, an undeclared capability or resource selector, or an
//! approval not bound to the exact action is denied before any effect.
//!
//! Policy is then the session's [`EffectiveCampusPolicy`] evaluated against
//! [`campus_baseline_policy`]. Work inside the student's own session is
//! [`CampusEffectClass::SelfWork`]; anything else is
//! [`CampusEffectClass::Privileged`] and needs an instructor approval or is
//! refused outright.
//!
//! Every decision is recorded through a [`CampusCapabilityAuditSink`] before
//! the effect runs; if it cannot be recorded, nothing runs. Events carry the
//! trusted actor, the deciding authority, the profile and effective policy
//! hashes and a redacted argument shape with a digest, never argument
//! values. Tool results come from the student's sandbox and are recorded as
//! untrusted, including a reported success.
//!
//! The broker is not the isolation boundary. The sandbox must hold no
//! platform or broker credentials ([`check_sandbox_environment`]) and its
//! network must be exactly [`CampusSessionAuthority::sandbox_network`].

mod audit;
mod broker;
mod memory;
mod registry;
mod sandbox;
mod selector;
mod session_tools;

#[cfg(test)]
mod tests;

pub use audit::{
    arguments_sha256, CampusAuditError, CampusCapabilityAuditSink, CampusCapabilityEvent,
    CampusCapabilityEventKind, CampusCapabilityRef, CampusRedactedArguments, CampusUntrustedResult,
    CAMPUS_CAPABILITY_EVENT_SCHEMA,
};
pub use broker::{
    campus_baseline_policy, CampusBrokerError, CampusCapabilityBroker, CampusCapabilityDecision,
    CampusCapabilityOutcome, CampusCapabilityReason,
};
pub use memory::{InMemoryCampusAuditSink, InMemoryCampusAuthoritySource};
pub use registry::{
    campus_tool_binding, CampusSelectorKind, CampusToolBinding, CAMPUS_CAPABILITY_REGISTRY,
};
pub use sandbox::{check_sandbox_environment, CampusSandboxError, CampusSandboxNetwork};
pub use selector::{CampusResourceSelector, CampusSelectorError};
pub use session_tools::CampusSessionTools;

use crate::policy::{CampusApproval, CampusApproverRole};
use crate::{ActionId, AgentAction, ApprovalKind, EffectiveCampusPolicy, ToolError, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// A Campus identity as the authenticating boundary established it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampusActorRef {
    pub tenant_id: String,
    pub identity_id: String,
}

/// What a caller presents to bind a call to one session's authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampusSessionClaims {
    pub session_id: String,
    pub effective_policy_hash: String,
    pub profile_hash: String,
}

/// Who is calling, as established by the boundary that built the request,
/// never by the request body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CampusCaller {
    /// The trusted agent runtime, bound to exactly one session at launch.
    SessionRuntime { session_id: String },
    /// An authenticated Campus principal calling the broker directly.
    Principal(CampusActorRef),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusCallPath {
    Tool,
    Direct,
}

impl CampusCaller {
    pub fn call_path(&self) -> CampusCallPath {
        match self {
            Self::SessionRuntime { .. } => CampusCallPath::Tool,
            Self::Principal(_) => CampusCallPath::Direct,
        }
    }
}

/// Whether an effect stays inside the student's own session or reaches
/// beyond it. Only an instructor approval can authorize a privileged effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusEffectClass {
    SelfWork,
    Privileged,
}

impl CampusEffectClass {
    /// File writes and shell commands inside the session are self-work; every
    /// other approval kind (destructive, network, privileged, secret,
    /// delivery) is a privileged effect.
    pub fn for_approval_kind(kind: Option<ApprovalKind>) -> Self {
        match kind {
            None | Some(ApprovalKind::FileWrite | ApprovalKind::ShellCommand) => Self::SelfWork,
            Some(_) => Self::Privileged,
        }
    }
}

/// The trusted record of one Campus lab session's authority, written by the
/// launcher and only ever read by the broker from its authority source.
#[derive(Debug, Clone)]
pub struct CampusSessionAuthority {
    pub session_id: String,
    pub course_id: String,
    /// The student the session belongs to. The tenant is the owner's tenant.
    pub owner: CampusActorRef,
    pub policy: EffectiveCampusPolicy,
    /// Unix seconds after which no call is authorized.
    pub expires_at: i64,
}

impl CampusSessionAuthority {
    /// The claims the launcher hands the trusted runtime for this session.
    pub fn claims(&self) -> CampusSessionClaims {
        CampusSessionClaims {
            session_id: self.session_id.clone(),
            effective_policy_hash: self.policy.effective_policy_hash().into(),
            profile_hash: self.policy.profile_hash().into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusAuthorityStatus {
    /// The session is ready: its capabilities may be used.
    Active,
    /// The session is admitted and its workload is being launched. The
    /// execution provider may start it, but no capability is granted yet.
    Provisioning,
    Revoked,
}

/// A session authority together with its status at the moment it was read.
#[derive(Debug, Clone)]
pub struct CampusAuthoritySnapshot {
    pub authority: Arc<CampusSessionAuthority>,
    pub status: CampusAuthorityStatus,
}

/// An approval as the trusted approval flow recorded it: who approved, in
/// which role, and the digest of the exact action they saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampusTrustedApproval {
    pub approval: CampusApproval,
    pub approver: CampusActorRef,
    /// [`arguments_sha256`] of the approved action.
    pub arguments_sha256: String,
}

impl CampusTrustedApproval {
    pub fn role(&self) -> CampusApproverRole {
        self.approval.role
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("campus authority source is unavailable: {0}")]
pub struct CampusAuthorityError(pub String);

/// Trusted, current session authority. The broker reads it on every call so
/// a revocation takes effect on the next call.
#[async_trait]
pub trait CampusAuthoritySource: Send + Sync {
    async fn session(
        &self,
        session_id: &str,
    ) -> Result<Option<CampusAuthoritySnapshot>, CampusAuthorityError>;

    async fn approval(
        &self,
        session_id: &str,
        action_id: &ActionId,
    ) -> Result<Option<CampusTrustedApproval>, CampusAuthorityError>;
}

/// What the broker hands the effect executor after authorizing a call: the
/// trusted session identity and the derived selector. It carries no
/// credentials, and the executor receives the caller's action unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CampusAuthorizedEffect {
    pub session_id: String,
    pub course_id: String,
    pub owner: CampusActorRef,
    pub resource: CampusResourceSelector,
}

/// Runs an authorized effect inside the session's own sandbox.
#[async_trait]
pub trait CampusEffectExecutor: Send + Sync {
    async fn execute(
        &self,
        effect: &CampusAuthorizedEffect,
        action: &AgentAction,
    ) -> Result<ToolResult, ToolError>;
}

/// Seconds since the Unix epoch.
pub trait CampusBrokerClock: Send + Sync {
    fn now(&self) -> i64;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemCampusBrokerClock;

impl CampusBrokerClock for SystemCampusBrokerClock {
    fn now(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX))
            .unwrap_or_default()
    }
}

/// One call to the broker. `caller` must be set by the trusted boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct CampusCapabilityRequest {
    pub caller: CampusCaller,
    pub claims: CampusSessionClaims,
    /// The capability the caller declares; it must name the action's tool.
    pub capability: String,
    /// The selector the caller declares. A direct caller must declare one;
    /// the session runtime may leave it to the broker to derive.
    pub resource: Option<CampusResourceSelector>,
    pub action: AgentAction,
}

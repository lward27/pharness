//! Campus lab session lifecycle and the execution-provider contract.
//!
//! A Campus lab session is a student workspace that runs under a
//! [`crate::CampusProfile`] in one admission-controlled lab slot. Its
//! lifecycle is intentionally separate from SDLC runs and WorkItems: nothing
//! here depends on run, stage or work-item types, and nothing there depends on
//! these.
//!
//! The local journal (in `pharness-store`) is the source of truth. Every
//! transition, the operation it begins or completes, and its audit event are
//! committed together before any provider side effect, so a crash at any
//! point leaves a resumable record. Providers are fenced by
//! [`CampusWorkloadRef`]: a session, a slot and the slot occupancy generation.

use crate::CampusProfileRef;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;

mod fake;
pub mod kubernetes;

pub use fake::{FakeCampusProvider, FakeFault, FakeFaultPoint, FakeTeardownHook};

/// Lifecycle state of a Campus lab session.
///
/// | State | Holds a slot or resources | Terminal |
/// | --- | --- | --- |
/// | `Requested` | no | no |
/// | `Authorizing` | no | no |
/// | `Provisioning` | slot; resources may exist | no |
/// | `Ready` | slot and resources | no |
/// | `Stopping` | slot and resources; access revoked | no |
/// | `Cleaning` | slot; resources may remain | no |
/// | `CleanupBlocked` | slot; resources may remain; quarantined | no |
/// | `Terminated` | no | yes |
/// | `Failed` | no | yes |
///
/// Only `Ready` grants student access. Every path out of a state that may hold
/// resources passes through `Stopping` (revocation) and `Cleaning`
/// (reconciliation of every owned resource) before it can become terminal, so
/// a terminal state always means the provider confirmed that nothing remains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusLabSessionState {
    Requested,
    Authorizing,
    Provisioning,
    Ready,
    Stopping,
    Cleaning,
    CleanupBlocked,
    Terminated,
    Failed,
}

impl CampusLabSessionState {
    pub const ALL: [Self; 9] = [
        Self::Requested,
        Self::Authorizing,
        Self::Provisioning,
        Self::Ready,
        Self::Stopping,
        Self::Cleaning,
        Self::CleanupBlocked,
        Self::Terminated,
        Self::Failed,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::Authorizing => "authorizing",
            Self::Provisioning => "provisioning",
            Self::Ready => "ready",
            Self::Stopping => "stopping",
            Self::Cleaning => "cleaning",
            Self::CleanupBlocked => "cleanup_blocked",
            Self::Terminated => "terminated",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|state| state.as_str() == value)
    }

    /// The legal next states. This is the whole transition table; the store
    /// mirrors it in `campus_session_transitions` and a trigger enforces it.
    ///
    /// - `Requested`: `Authorizing`, or `Terminated` when stopped first.
    /// - `Authorizing`: `Provisioning` once a slot is admitted, `Failed` when
    ///   the launch is denied, or `Terminated` when stopped first.
    /// - `Provisioning`: `Ready`, or `Stopping` on failure or stop.
    /// - `Ready`: `Stopping`.
    /// - `Stopping`: `Cleaning`.
    /// - `Cleaning`: `Terminated` or `Failed` once nothing remains, or
    ///   `CleanupBlocked` when deletion cannot be confirmed.
    /// - `CleanupBlocked`: `Cleaning` when cleanup is retried.
    /// - `Terminated`, `Failed`: none.
    pub fn successors(self) -> &'static [Self] {
        match self {
            Self::Requested => &[Self::Authorizing, Self::Terminated],
            Self::Authorizing => &[Self::Provisioning, Self::Failed, Self::Terminated],
            Self::Provisioning => &[Self::Ready, Self::Stopping],
            Self::Ready => &[Self::Stopping],
            Self::Stopping => &[Self::Cleaning],
            Self::Cleaning => &[Self::Terminated, Self::Failed, Self::CleanupBlocked],
            Self::CleanupBlocked => &[Self::Cleaning],
            Self::Terminated | Self::Failed => &[],
        }
    }

    /// Validate one transition.
    pub fn transition(self, to: Self) -> Result<Self, CampusLabTransitionError> {
        if self.is_terminal() {
            return Err(CampusLabTransitionError::Terminal { from: self, to });
        }
        if self.successors().contains(&to) {
            Ok(to)
        } else {
            Err(CampusLabTransitionError::Illegal { from: self, to })
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Terminated | Self::Failed)
    }

    /// Whether provider resources may exist for the session. Such a session
    /// also holds its lab slot, which is released only when it terminates.
    pub fn may_hold_resources(self) -> bool {
        matches!(
            self,
            Self::Provisioning
                | Self::Ready
                | Self::Stopping
                | Self::Cleaning
                | Self::CleanupBlocked
        )
    }

    /// Whether the session is quarantined: cleanup could not be confirmed and
    /// the session must stay visible until an operator or a retry resolves it.
    pub fn is_quarantined(self) -> bool {
        self == Self::CleanupBlocked
    }

    /// Whether a student may use the workspace.
    pub fn grants_access(self) -> bool {
        self == Self::Ready
    }
}

impl fmt::Display for CampusLabSessionState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CampusLabTransitionError {
    #[error("campus lab session is {from}, a terminal state, and cannot become {to}")]
    Terminal {
        from: CampusLabSessionState,
        to: CampusLabSessionState,
    },
    #[error("illegal campus lab session transition from {from} to {to}")]
    Illegal {
        from: CampusLabSessionState,
        to: CampusLabSessionState,
    },
}

/// What the session should converge to. It only ever moves from `Ready` to
/// `Terminated`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusLabDesiredState {
    Ready,
    Terminated,
}

impl CampusLabDesiredState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Terminated => "terminated",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "ready" => Some(Self::Ready),
            "terminated" => Some(Self::Terminated),
            _ => None,
        }
    }
}

/// The last condition the provider reported for the session's workload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusLabObservedState {
    Unknown,
    Absent,
    Provisioning,
    Ready,
    Deleting,
    Deleted,
    Failed,
    Unreachable,
}

impl CampusLabObservedState {
    pub const ALL: [Self; 8] = [
        Self::Unknown,
        Self::Absent,
        Self::Provisioning,
        Self::Ready,
        Self::Deleting,
        Self::Deleted,
        Self::Failed,
        Self::Unreachable,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Absent => "absent",
            Self::Provisioning => "provisioning",
            Self::Ready => "ready",
            Self::Deleting => "deleting",
            Self::Deleted => "deleted",
            Self::Failed => "failed",
            Self::Unreachable => "unreachable",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|state| state.as_str() == value)
    }
}

/// A provider side effect recorded in the journal before it is attempted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusLabOperationKind {
    Provision,
    Delete,
}

impl CampusLabOperationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Provision => "provision",
            Self::Delete => "delete",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "provision" => Some(Self::Provision),
            "delete" => Some(Self::Delete),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusLabOperationStatus {
    /// Recorded and possibly in flight. Any holder of a live lease may call
    /// the provider with the operation key; the call is idempotent.
    Pending,
    Succeeded,
    Failed,
    /// Cleanup could not be confirmed. The session is quarantined.
    Blocked,
    /// A provision that was superseded by a stop before it completed. Any
    /// resource it created is removed by the following delete operation.
    Abandoned,
}

impl CampusLabOperationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
            Self::Abandoned => "abandoned",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "succeeded" => Some(Self::Succeeded),
            "failed" => Some(Self::Failed),
            "blocked" => Some(Self::Blocked),
            "abandoned" => Some(Self::Abandoned),
            _ => None,
        }
    }
}

/// The fenced identity of one slot occupancy. A provider labels every
/// resource it creates with all three fields and acts only on exact matches,
/// so an operation for an earlier occupancy can never touch a later one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CampusWorkloadRef {
    pub session_id: String,
    pub slot_id: String,
    /// Incremented each time the slot gets a new occupant.
    pub slot_generation: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampusProvisionSpec {
    pub workload: CampusWorkloadRef,
    pub tenant_id: String,
    pub course_id: String,
    pub profile: CampusProfileRef,
    /// Absolute expiry in Unix seconds. The provider must not keep the
    /// workload running past it, even without further instructions.
    pub expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum CampusProvisionOutcome {
    Ready {
        workspace_id: String,
    },
    /// Accepted but not yet ready. Calling again with the same key is safe.
    InProgress {
        workspace_id: String,
    },
    /// Refused permanently, e.g. a qualified runtime class is unavailable.
    Rejected {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "observation")]
pub enum CampusObservation {
    Absent,
    Provisioning,
    Ready { workspace_id: String },
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum CampusDeleteOutcome {
    /// No resource labelled with the workload remains, including when none
    /// existed.
    Deleted,
    /// Deletion was requested but has not been confirmed yet.
    InProgress,
    /// Deletion cannot proceed, e.g. a finalizer or a stuck volume.
    Blocked { reason: String },
    /// The node or API holding the resources cannot be reached. Never
    /// reported as deleted.
    Unreachable { reason: String },
}

/// One resource in the provider's owned-resource inventory.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OwnedCampusResource {
    pub resource_id: String,
    pub kind: String,
    pub workload: CampusWorkloadRef,
    /// Key of the provision operation that created it.
    pub op_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CampusProviderError {
    /// The outcome is unknown; the caller retries with the same key.
    #[error("campus execution provider is unreachable: {0}")]
    Unreachable(String),
    #[error("campus execution provider failed: {0}")]
    Internal(String),
}

/// The narrow contract between the Campus journal and an execution backend
/// (an in-memory fake today, admission-controlled Kubernetes Jobs later).
///
/// Implementations must:
///
/// - make `provision` idempotent on `op_key`: a repeated call with the same
///   key and spec returns the same workspace and creates nothing new, and a
///   reused key with a different spec is rejected;
/// - label every resource with the full [`CampusWorkloadRef`] and `op_key`;
/// - restrict `observe` and `delete` to resources whose labels equal the
///   given workload exactly, so a stale slot generation never matches a newer
///   occupant;
/// - never create anything for a workload once `delete` was called for it,
///   so a stale worker's late `provision` cannot resurrect a torn-down
///   occupancy;
/// - make `delete` idempotent, and return `Deleted` only once nothing
///   labelled with the workload remains; an unreachable node is reported as
///   `Unreachable`, never as deleted;
/// - return every resource they own from `list_owned`, for orphan detection.
#[async_trait]
pub trait CampusExecutionProvider: Send + Sync {
    async fn provision(
        &self,
        op_key: &str,
        spec: &CampusProvisionSpec,
    ) -> Result<CampusProvisionOutcome, CampusProviderError>;

    async fn observe(
        &self,
        workload: &CampusWorkloadRef,
    ) -> Result<CampusObservation, CampusProviderError>;

    async fn delete(
        &self,
        op_key: &str,
        workload: &CampusWorkloadRef,
    ) -> Result<CampusDeleteOutcome, CampusProviderError>;

    async fn list_owned(&self) -> Result<Vec<OwnedCampusResource>, CampusProviderError>;
}

/// Cleanup of something a session used outside its execution provider, such
/// as an interactive attachment or an external volume.
///
/// Hooks run after the provider confirmed that the workload is gone and
/// before the session becomes terminal; a hook that has not succeeded keeps
/// the session (and its slot) out of `Terminated`. The journal records each
/// hook that succeeded, so a replay skips it. A crash between a hook's effect
/// and that record repeats the call, so `release` must be idempotent per
/// workload.
#[async_trait]
pub trait CampusTeardownHook: Send + Sync {
    /// Stable name recorded in the journal: 1 to 64 characters.
    fn name(&self) -> &str;

    /// Release what the workload held. `Unreachable` quarantines the
    /// session's cleanup like an unreachable provider; any other error is
    /// retried on the next step.
    async fn release(
        &self,
        op_key: &str,
        workload: &CampusWorkloadRef,
    ) -> Result<(), CampusProviderError>;
}

#[cfg(test)]
mod tests;

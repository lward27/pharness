//! Records for the Campus lab session journal (migration 0058).
//!
//! These are separate from both the SDLC records and the Campus browser
//! sessions in [`crate::campus`]: a lab session is a provisioned student
//! workspace, not a login.

use crate::StoreError;
use pharness_core::{
    CampusLabDesiredState, CampusLabObservedState, CampusLabOperationKind,
    CampusLabOperationStatus, CampusLabSessionState, CampusLabTransitionError, CampusProfileRef,
    CampusWorkloadRef, OwnedCampusResource,
};
use serde::{Deserialize, Serialize};

/// The deterministic idempotency key of the operation a transition begins.
/// A restarted or duplicate worker derives the same key from the journal, so
/// the provider sees one operation however often it is retried.
pub fn campus_operation_key(
    session_id: &str,
    kind: CampusLabOperationKind,
    session_generation: i64,
) -> String {
    format!(
        "campus-lab/{session_id}/{}/g{session_generation}",
        kind.as_str()
    )
}

#[derive(Debug, thiserror::Error)]
pub enum CampusJournalError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Transition(#[from] CampusLabTransitionError),
    #[error("campus lab session not found: {0}")]
    NotFound(String),
    /// The write was based on a superseded generation, slot occupancy or
    /// lease. Nothing was changed; the caller reloads the journal.
    #[error("stale campus journal write rejected: {0}")]
    Fenced(String),
    #[error("campus operation {op_key} is leased by {owner} until {expires_at}")]
    LeaseHeld {
        op_key: String,
        owner: String,
        expires_at: i64,
    },
    #[error("no free campus lab slot for tenant {0}")]
    NoFreeSlot(String),
    #[error("invalid campus journal request: {0}")]
    Invalid(String),
    #[error("campus lab session conflict: {0}")]
    Conflict(String),
}

impl From<sqlx::Error> for CampusJournalError {
    fn from(error: sqlx::Error) -> Self {
        Self::Store(StoreError::Sqlx(error))
    }
}

impl From<serde_json::Error> for CampusJournalError {
    fn from(error: serde_json::Error) -> Self {
        Self::Store(StoreError::Json(error))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateCampusLabSession {
    /// Chosen by the caller and used as the request idempotency key: a
    /// duplicate delivery with identical content returns the existing row.
    pub session_id: String,
    pub tenant_id: String,
    pub course_id: String,
    pub issuer: String,
    pub subject: String,
    pub profile: CampusProfileRef,
    pub expires_at: i64,
    pub actor: String,
    pub now: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredCampusLabSession {
    pub session_id: String,
    pub tenant_id: String,
    pub course_id: String,
    pub issuer: String,
    pub subject: String,
    pub profile_id: String,
    pub profile_version: String,
    pub profile_hash: String,
    pub state: CampusLabSessionState,
    pub desired_state: CampusLabDesiredState,
    pub observed_state: CampusLabObservedState,
    pub generation: i64,
    pub slot_id: Option<String>,
    pub slot_generation: Option<i64>,
    pub workspace_id: Option<String>,
    pub failure_reason: Option<String>,
    pub blocked_reason: Option<String>,
    pub created_by: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub observed_at: Option<i64>,
    pub revoked_at: Option<i64>,
    pub quarantined_at: Option<i64>,
    pub terminated_at: Option<i64>,
    pub expires_at: i64,
}

impl StoredCampusLabSession {
    /// The fenced provider identity, once a slot has been admitted.
    pub fn workload(&self) -> Option<CampusWorkloadRef> {
        Some(CampusWorkloadRef {
            session_id: self.session_id.clone(),
            slot_id: self.slot_id.clone()?,
            slot_generation: self.slot_generation?,
        })
    }

    pub fn profile(&self) -> CampusProfileRef {
        CampusProfileRef {
            id: self.profile_id.clone(),
            version: self.profile_version.clone(),
            profile_hash: self.profile_hash.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredCampusLabOperation {
    pub op_key: String,
    pub session_id: String,
    pub kind: CampusLabOperationKind,
    /// The session generation that the beginning transition produced.
    pub session_generation: i64,
    pub slot_id: String,
    pub slot_generation: i64,
    pub status: CampusLabOperationStatus,
    pub attempt_count: i64,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<i64>,
    pub lease_fence: i64,
    pub last_error: Option<String>,
    pub result: serde_json::Value,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
}

impl StoredCampusLabOperation {
    pub fn lease(&self) -> Option<CampusOperationLease> {
        Some(CampusOperationLease {
            op_key: self.op_key.clone(),
            owner: self.lease_owner.clone()?,
            fence: self.lease_fence,
            expires_at: self.lease_expires_at?,
        })
    }

    pub fn workload(&self) -> CampusWorkloadRef {
        CampusWorkloadRef {
            session_id: self.session_id.clone(),
            slot_id: self.slot_id.clone(),
            slot_generation: self.slot_generation,
        }
    }
}

/// Proof of holding an operation lease. Completing an operation requires the
/// latest fence and an unexpired lease.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampusOperationLease {
    pub op_key: String,
    pub owner: String,
    pub fence: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredCampusLabEvent {
    pub sequence: i64,
    pub session_id: String,
    pub kind: String,
    pub from_state: Option<CampusLabSessionState>,
    pub to_state: Option<CampusLabSessionState>,
    pub generation: i64,
    pub op_key: Option<String>,
    pub actor: String,
    pub payload: serde_json::Value,
    pub created_at: i64,
    pub published_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredCampusLabSlot {
    pub slot_id: String,
    pub tenant_id: String,
    pub occupant_session_id: Option<String>,
    pub occupant_generation: i64,
    pub occupied_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// One state change. The store derives its effects from `(from, to)`:
///
/// | Transition | Effects in the same transaction |
/// | --- | --- |
/// | `Authorizing -> Provisioning` | take a free slot of the tenant (new slot generation); begin a leased `provision` operation |
/// | `Provisioning -> Ready` | complete the provision with `lease`; record `workspace_id` |
/// | `Provisioning -> Stopping` | revoke; fail the provision with `lease`, or abandon it if no live lease exists |
/// | `Ready -> Stopping` | revoke |
/// | `Stopping/CleanupBlocked -> Cleaning` | begin a leased `delete` operation |
/// | `Cleaning -> CleanupBlocked` | block the delete with `lease`; quarantine |
/// | `Cleaning -> Terminated/Failed` | complete the delete with `lease`; free the slot (fenced) |
/// | `Requested/Authorizing -> Terminated/Failed` | none |
///
/// Every transition appends one audit event and advances the generation.
#[derive(Debug, Clone, PartialEq)]
pub struct CampusLabTransition {
    pub session_id: String,
    pub expected_generation: i64,
    pub from: CampusLabSessionState,
    pub to: CampusLabSessionState,
    pub actor: String,
    pub now: i64,
    /// The lease on the pending operation this transition completes.
    pub lease: Option<CampusOperationLease>,
    /// Lease length for an operation this transition begins.
    pub lease_seconds: i64,
    /// Recorded as the session's failure reason; required to reach `Failed`
    /// from `Authorizing`, and decides `Failed` over `Terminated` after
    /// cleanup.
    pub failure: Option<String>,
    /// Why the transition happened; required for `CleanupBlocked`.
    pub reason: Option<String>,
    /// Required for `Provisioning -> Ready`.
    pub workspace_id: Option<String>,
    pub observed: Option<CampusLabObservedState>,
    /// Provider result stored on the completed operation.
    pub result: serde_json::Value,
}

impl CampusLabTransition {
    pub fn new(
        session: &StoredCampusLabSession,
        to: CampusLabSessionState,
        actor: impl Into<String>,
        now: i64,
    ) -> Self {
        Self {
            session_id: session.session_id.clone(),
            expected_generation: session.generation,
            from: session.state,
            to,
            actor: actor.into(),
            now,
            lease: None,
            lease_seconds: 0,
            failure: None,
            reason: None,
            workspace_id: None,
            observed: None,
            result: serde_json::json!({}),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampusLabTransitionOutcome {
    pub session: StoredCampusLabSession,
    /// The operation this transition began, leased to the actor.
    pub begun: Option<StoredCampusLabOperation>,
    pub event_sequence: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusOrphanReason {
    /// No journal row names the resource's session.
    UnknownSession,
    /// The session is terminal, so nothing should remain.
    TerminalSession,
    /// The resource belongs to a slot occupancy the session does not hold.
    StaleOccupancy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampusLabOrphan {
    pub resource: OwnedCampusResource,
    pub reason: CampusOrphanReason,
}

#[cfg(test)]
pub(crate) mod test_support;

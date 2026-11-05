//! Records for Campus lab admission (migration 0059): assignment labs, their
//! reset generations and launch idempotency keys.
//!
//! A lab is one identity's workspace for one assignment. Its `lab_id` and
//! `budget_id` never change, so resets keep charging the same budget. Each
//! session the lab runs is one journal row ([`StoredCampusLabSession`]) and
//! one generation.

use crate::StoredCampusLabSession;
use pharness_core::CampusProfileRef;
use serde::{Deserialize, Serialize};

/// Admission limits, enforced in the same transaction that creates a
/// session. Every non-terminal journal session counts, whether or not it
/// already holds a slot, because each one either holds a slot or is about to
/// take one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CampusLabLimits {
    /// Live sessions allowed in the tenant; the number of lab slots.
    pub lab_slots: i64,
    /// Live sessions one identity may have across the whole tenant.
    pub max_active_per_identity: i64,
    /// Live sessions allowed in the course.
    pub max_active_per_course: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmitCampusLabLaunch {
    pub tenant_id: String,
    pub course_id: String,
    pub assignment_id: String,
    pub issuer: String,
    pub subject: String,
    /// Client-supplied, scoped to the identity. Replaying it returns the
    /// session it first produced.
    pub idempotency_key: Option<String>,
    /// Set for a reset: the session the new generation replaces. Admission
    /// refuses until that session is terminal.
    pub reset_from_session_id: Option<String>,
    /// Used only if the lab does not exist yet.
    pub candidate_lab_id: String,
    /// Used only if the lab does not exist yet.
    pub candidate_budget_id: String,
    /// Used only if a new session is created.
    pub candidate_session_id: String,
    pub profile: CampusProfileRef,
    pub effective_policy_hash: String,
    pub expires_at: i64,
    pub actor: String,
    pub now: i64,
    pub limits: CampusLabLimits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredCampusLab {
    pub lab_id: String,
    pub tenant_id: String,
    pub course_id: String,
    pub assignment_id: String,
    pub issuer: String,
    pub subject: String,
    pub budget_id: String,
    /// Generation of the current session; 0 before the first launch.
    pub generation: i64,
    pub current_session_id: Option<String>,
    /// Set while a reset waits for the current session's teardown.
    pub reset_requested_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A journal session together with the lab and generation it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampusLabSessionRecord {
    pub session: StoredCampusLabSession,
    pub lab: StoredCampusLab,
    pub generation: i64,
    pub effective_policy_hash: String,
    pub reset_from_session_id: Option<String>,
    /// `campus_identities.id` of the owner.
    pub owner_identity_id: String,
}

impl CampusLabSessionRecord {
    pub fn is_current(&self) -> bool {
        self.lab.current_session_id.as_deref() == Some(self.session.session_id.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CampusLabAdmission {
    /// A new session was recorded in `requested`.
    Created(CampusLabSessionRecord),
    /// The lab already has a live session; it is returned unchanged.
    Active(CampusLabSessionRecord),
    /// The idempotency key, or the reset, was already answered by this
    /// session.
    Replayed(CampusLabSessionRecord),
    /// Nothing was written.
    Rejected(CampusLabRejection),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CampusLabRejection {
    /// Every lab slot of the tenant is taken. There is no queue.
    NoSlotsAvailable,
    /// The course reached its live-session limit.
    CourseCapacityReached,
    /// The identity already has its maximum number of live sessions.
    IdentitySessionLimit { active_session_id: String },
    /// The key was first used for another assignment.
    IdempotencyKeyReused,
    /// A reset named a session that is no longer the lab's current one.
    ResetSuperseded { current_session_id: Option<String> },
    /// A reset named a session that is not terminal yet.
    TeardownPending,
}

//! A [`CampusAuthoritySource`] backed by the lab journal.
//!
//! The capability broker reads session authority on every call. This source
//! answers from the journal and lab admission rows, so stopping, resetting,
//! expiring or failing a session revokes its authority on the very next
//! call, and a session whose assignment policy changed since launch has no
//! authority at all.

use crate::SqliteStore;
use async_trait::async_trait;
use pharness_core::campus_capability::{
    CampusActorRef, CampusAuthorityError, CampusAuthoritySnapshot, CampusAuthoritySource,
    CampusAuthorityStatus, CampusBrokerClock, CampusSessionAuthority, CampusTrustedApproval,
};
use pharness_core::{ActionId, CampusLabDesiredState, EffectiveCampusPolicy};
use std::sync::Arc;

/// The effective policy each assignment is configured with now.
pub trait CampusAssignmentPolicies: Send + Sync {
    fn effective_policy(
        &self,
        tenant_id: &str,
        course_id: &str,
        assignment_id: &str,
    ) -> Option<EffectiveCampusPolicy>;
}

pub struct JournalCampusAuthoritySource {
    store: SqliteStore,
    policies: Arc<dyn CampusAssignmentPolicies>,
    clock: Arc<dyn CampusBrokerClock>,
}

impl JournalCampusAuthoritySource {
    pub fn new(
        store: SqliteStore,
        policies: Arc<dyn CampusAssignmentPolicies>,
        clock: Arc<dyn CampusBrokerClock>,
    ) -> Self {
        Self {
            store,
            policies,
            clock,
        }
    }
}

#[async_trait]
impl CampusAuthoritySource for JournalCampusAuthoritySource {
    async fn session(
        &self,
        session_id: &str,
    ) -> Result<Option<CampusAuthoritySnapshot>, CampusAuthorityError> {
        let record = self
            .store
            .get_campus_lab_session_record(session_id)
            .await
            .map_err(|_| CampusAuthorityError("the campus lab journal is unavailable".into()))?;
        let Some(record) = record else {
            return Ok(None);
        };
        let session = &record.session;
        let Some(policy) = self.policies.effective_policy(
            &session.tenant_id,
            &session.course_id,
            &record.lab.assignment_id,
        ) else {
            return Ok(None);
        };
        if policy.effective_policy_hash() != record.effective_policy_hash
            || policy.profile() != &session.profile()
        {
            return Ok(None);
        }
        let active = session.state.grants_access()
            && session.desired_state == CampusLabDesiredState::Ready
            && session.revoked_at.is_none()
            && self.clock.now() < session.expires_at;
        Ok(Some(CampusAuthoritySnapshot {
            authority: Arc::new(CampusSessionAuthority {
                session_id: session.session_id.clone(),
                course_id: session.course_id.clone(),
                owner: CampusActorRef {
                    tenant_id: session.tenant_id.clone(),
                    identity_id: record.owner_identity_id.clone(),
                },
                policy,
                expires_at: session.expires_at,
            }),
            status: if active {
                CampusAuthorityStatus::Active
            } else {
                CampusAuthorityStatus::Revoked
            },
        }))
    }

    /// No trusted approval flow records Campus approvals yet, so none exist.
    async fn approval(
        &self,
        _session_id: &str,
        _action_id: &ActionId,
    ) -> Result<Option<CampusTrustedApproval>, CampusAuthorityError> {
        Ok(None)
    }
}

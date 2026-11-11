//! In-memory authority source and audit sink for tests and local fixtures.

use super::audit::{CampusAuditError, CampusCapabilityAuditSink, CampusCapabilityEvent};
use super::{
    CampusAuthorityError, CampusAuthoritySnapshot, CampusAuthoritySource, CampusAuthorityStatus,
    CampusSessionAuthority, CampusTrustedApproval,
};
use async_trait::async_trait;
use pharness_core::ActionId;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct InMemoryCampusAuthoritySource {
    sessions: Mutex<BTreeMap<String, CampusAuthoritySnapshot>>,
    approvals: Mutex<BTreeMap<(String, String), CampusTrustedApproval>>,
    unavailable: AtomicBool,
}

impl InMemoryCampusAuthoritySource {
    pub fn insert(&self, authority: CampusSessionAuthority) -> Arc<CampusSessionAuthority> {
        let authority = Arc::new(authority);
        self.sessions.lock().expect("authority lock").insert(
            authority.session_id.clone(),
            CampusAuthoritySnapshot {
                authority: authority.clone(),
                status: CampusAuthorityStatus::Active,
            },
        );
        authority
    }

    pub fn revoke(&self, session_id: &str) {
        self.set_status(session_id, CampusAuthorityStatus::Revoked);
    }

    pub fn set_status(&self, session_id: &str, status: CampusAuthorityStatus) {
        if let Some(snapshot) = self
            .sessions
            .lock()
            .expect("authority lock")
            .get_mut(session_id)
        {
            snapshot.status = status;
        }
    }

    pub fn record_approval(&self, session_id: &str, approval: CampusTrustedApproval) {
        self.approvals.lock().expect("approval lock").insert(
            (
                session_id.to_string(),
                approval.approval.action_id.as_str().to_string(),
            ),
            approval,
        );
    }

    pub fn set_unavailable(&self, unavailable: bool) {
        self.unavailable.store(unavailable, Ordering::SeqCst);
    }

    fn check_available(&self) -> Result<(), CampusAuthorityError> {
        if self.unavailable.load(Ordering::SeqCst) {
            Err(CampusAuthorityError(
                "in-memory source set unavailable".into(),
            ))
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl CampusAuthoritySource for InMemoryCampusAuthoritySource {
    async fn session(
        &self,
        session_id: &str,
    ) -> Result<Option<CampusAuthoritySnapshot>, CampusAuthorityError> {
        self.check_available()?;
        Ok(self
            .sessions
            .lock()
            .expect("authority lock")
            .get(session_id)
            .cloned())
    }

    async fn approval(
        &self,
        session_id: &str,
        action_id: &ActionId,
    ) -> Result<Option<CampusTrustedApproval>, CampusAuthorityError> {
        self.check_available()?;
        Ok(self
            .approvals
            .lock()
            .expect("approval lock")
            .get(&(session_id.to_string(), action_id.as_str().to_string()))
            .cloned())
    }
}

#[derive(Default)]
pub struct InMemoryCampusAuditSink {
    events: Mutex<Vec<CampusCapabilityEvent>>,
    failing: AtomicBool,
}

impl InMemoryCampusAuditSink {
    pub fn events(&self) -> Vec<CampusCapabilityEvent> {
        self.events.lock().expect("audit lock").clone()
    }

    /// Make every later write fail, as an unavailable audit store would.
    pub fn set_failing(&self, failing: bool) {
        self.failing.store(failing, Ordering::SeqCst);
    }
}

#[async_trait]
impl CampusCapabilityAuditSink for InMemoryCampusAuditSink {
    async fn record(&self, event: &CampusCapabilityEvent) -> Result<(), CampusAuditError> {
        if self.failing.load(Ordering::SeqCst) {
            return Err(CampusAuditError("in-memory sink set failing".into()));
        }
        self.events.lock().expect("audit lock").push(event.clone());
        Ok(())
    }
}

//! The in-memory side of Campus browser terminals: limits, single-use
//! connection tickets, the per-session connection count and the
//! revalidation signal. It depends on nothing that holds Campus state, so
//! the shared state can own it.
//!
//! The registry keeps only a ticket's SHA-256, bound to the Campus session,
//! identity, lab session, lab generation and fenced workload it was issued
//! for, and consuming it removes it, so a ticket works once, for one
//! session, from one browser session. Tickets live in memory: a restart of
//! the API invalidates every outstanding ticket, and every open terminal
//! ends with the process.

use super::tokens::sha256_hex;
use pharness_core::campus_terminal::CampusTerminalAttacher;
use pharness_core::CampusWorkloadRef;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;

/// Bounds of the terminal boundary. Production uses [`Self::default`];
/// tests shorten the timers.
#[derive(Debug, Clone)]
pub(crate) struct CampusTerminalLimits {
    pub(crate) ticket_ttl_seconds: i64,
    /// Outstanding tickets per Campus browser session.
    pub(crate) max_tickets_per_browser_session: usize,
    pub(crate) max_outstanding_tickets: usize,
    /// Open terminals per lab session.
    pub(crate) max_terminals_per_session: usize,
    /// How often an open terminal re-checks its authorization even without
    /// a revalidation signal. Bounds how long a terminal can outlive an
    /// expiry or a change made by another replica.
    pub(crate) recheck_interval: Duration,
    pub(crate) auth_timeout: Duration,
    pub(crate) idle_timeout: Duration,
    pub(crate) ping_interval: Duration,
    pub(crate) send_timeout: Duration,
    pub(crate) max_message_bytes: usize,
    pub(crate) output_bytes_per_second: u64,
}

impl Default for CampusTerminalLimits {
    fn default() -> Self {
        Self {
            ticket_ttl_seconds: 30,
            max_tickets_per_browser_session: 4,
            max_outstanding_tickets: 1024,
            max_terminals_per_session: 2,
            recheck_interval: Duration::from_secs(5),
            auth_timeout: Duration::from_secs(10),
            idle_timeout: Duration::from_secs(30 * 60),
            ping_interval: Duration::from_secs(30),
            send_timeout: Duration::from_secs(10),
            max_message_bytes: 64 * 1024,
            output_bytes_per_second: 1024 * 1024,
        }
    }
}

/// What a ticket was issued for. Everything here comes from the server-side
/// session and the journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TerminalGrant {
    pub(crate) identity_id: String,
    pub(crate) browser_session_hash: String,
    pub(crate) lab_session_id: String,
    pub(crate) course_id: String,
    /// The lab generation the session belongs to.
    pub(crate) lab_generation: i64,
    pub(crate) workload: CampusWorkloadRef,
    pub(crate) effective_policy_hash: String,
    pub(crate) pids: u32,
}

struct IssuedTicket {
    grant: TerminalGrant,
    expires_at: i64,
}

/// Why a presented ticket was refused. All of them look the same to the
/// browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TicketRefusal {
    Unknown,
    Expired,
    WrongSession,
}

pub(crate) struct CampusTerminals {
    attacher: Arc<dyn CampusTerminalAttacher>,
    pub(crate) limits: CampusTerminalLimits,
    /// SHA-256 of the ticket to what it grants.
    tickets: Mutex<HashMap<String, IssuedTicket>>,
    /// Lab session id to its number of open terminals.
    open: Arc<Mutex<HashMap<String, usize>>>,
    revalidate: watch::Sender<u64>,
}

/// Holds one of a lab session's terminal places until dropped.
pub(crate) struct OpenTerminal {
    open: Arc<Mutex<HashMap<String, usize>>>,
    session_id: String,
}

impl Drop for OpenTerminal {
    fn drop(&mut self) {
        let mut open = self.open.lock().expect("terminal registry lock");
        if let Some(count) = open.get_mut(&self.session_id) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                open.remove(&self.session_id);
            }
        }
    }
}

impl CampusTerminals {
    pub(crate) fn new(
        attacher: Arc<dyn CampusTerminalAttacher>,
        limits: CampusTerminalLimits,
    ) -> Self {
        Self {
            attacher,
            limits,
            tickets: Mutex::new(HashMap::new()),
            open: Arc::new(Mutex::new(HashMap::new())),
            revalidate: watch::Sender::new(0),
        }
    }

    pub(crate) fn attacher(&self) -> &dyn CampusTerminalAttacher {
        self.attacher.as_ref()
    }

    /// Make every open terminal re-check its authorization now. Called after
    /// anything that can end access: logout, stop, reset, membership removal.
    pub(crate) fn revalidate(&self) {
        self.revalidate
            .send_modify(|epoch| *epoch = epoch.wrapping_add(1));
    }

    pub(crate) fn revalidation(&self) -> watch::Receiver<u64> {
        self.revalidate.subscribe()
    }

    /// Store a new ticket; `None` when too many are outstanding.
    pub(crate) fn issue(&self, ticket: &str, grant: TerminalGrant, now: i64) -> Option<i64> {
        let mut tickets = self.tickets.lock().expect("terminal ticket lock");
        tickets.retain(|_, issued| issued.expires_at > now);
        let for_browser_session = tickets
            .values()
            .filter(|issued| issued.grant.browser_session_hash == grant.browser_session_hash)
            .count();
        if for_browser_session >= self.limits.max_tickets_per_browser_session
            || tickets.len() >= self.limits.max_outstanding_tickets
        {
            return None;
        }
        let expires_at = now.saturating_add(self.limits.ticket_ttl_seconds);
        tickets.insert(sha256_hex(ticket), IssuedTicket { grant, expires_at });
        Some(expires_at)
    }

    /// Consume a ticket. It is removed whatever the outcome, so a ticket is
    /// never accepted twice, not even after a refusal.
    pub(crate) fn redeem(
        &self,
        ticket: &str,
        identity_id: &str,
        browser_session_hash: &str,
        lab_session_id: &str,
        now: i64,
    ) -> Result<TerminalGrant, TicketRefusal> {
        let issued = self
            .tickets
            .lock()
            .expect("terminal ticket lock")
            .remove(&sha256_hex(ticket))
            .ok_or(TicketRefusal::Unknown)?;
        if issued.expires_at <= now {
            return Err(TicketRefusal::Expired);
        }
        let grant = issued.grant;
        if grant.identity_id != identity_id
            || grant.browser_session_hash != browser_session_hash
            || grant.lab_session_id != lab_session_id
        {
            return Err(TicketRefusal::WrongSession);
        }
        Ok(grant)
    }

    /// Take one of the session's terminal places.
    pub(crate) fn open(&self, session_id: &str) -> Option<OpenTerminal> {
        let mut open = self.open.lock().expect("terminal registry lock");
        let count = open.entry(session_id.to_string()).or_default();
        if *count >= self.limits.max_terminals_per_session {
            return None;
        }
        *count += 1;
        Some(OpenTerminal {
            open: self.open.clone(),
            session_id: session_id.to_string(),
        })
    }

    #[cfg(test)]
    pub(crate) fn open_count(&self, session_id: &str) -> usize {
        self.open
            .lock()
            .expect("terminal registry lock")
            .get(session_id)
            .copied()
            .unwrap_or_default()
    }
}

//! Campus teardown beyond one session step (migration 0061): cleanup retry
//! backoff, slot holds, and the janitor that reconciles the provider's
//! owned-resource inventory against the journal.
//!
//! Nothing here force-deletes anything or strips a finalizer. A resource
//! that cannot be confirmed gone keeps its session in `CleanupBlocked` (and
//! its slot occupied), or keeps an unaccounted slot under a `quarantined`
//! hold, until a later inventory confirms it is gone.

use crate::{CampusJournalError, CampusOrphanReason, SqliteStore};
use pharness_workspaces::{
    CampusDeleteOutcome, CampusExecutionProvider, CampusLabSessionState, CampusProviderError,
    CampusWorkloadRef,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// First retry of a blocked cleanup; each further block doubles the wait.
pub const CAMPUS_CLEANUP_RETRY_BASE_SECONDS: i64 = 30;
pub const CAMPUS_CLEANUP_RETRY_MAX_SECONDS: i64 = 600;
/// How long an unaccounted workload must stay in the inventory before the
/// janitor deletes it. It is quarantined as soon as it is seen.
pub const CAMPUS_ORPHAN_GRACE_SECONDS: i64 = 60;
/// Actor of janitor holds; the janitor only ever releases its own.
pub const CAMPUS_JANITOR_ACTOR: &str = "campus-lab-janitor";

/// When a session blocked `blocked_attempts` times (at least once, most
/// recently at `quarantined_at`) is due for its next cleanup attempt:
/// 30 s, 60 s, 120 s, ... capped at 10 minutes.
pub fn campus_cleanup_retry_at(quarantined_at: i64, blocked_attempts: i64) -> i64 {
    let doublings = u32::try_from(blocked_attempts.saturating_sub(1).clamp(0, 16)).unwrap_or(16);
    let wait = CAMPUS_CLEANUP_RETRY_BASE_SECONDS
        .saturating_mul(1_i64 << doublings)
        .min(CAMPUS_CLEANUP_RETRY_MAX_SECONDS);
    quarantined_at.saturating_add(wait)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CampusLabSlotHoldKind {
    /// Unaccounted provider resources were found in the slot.
    Quarantined,
    /// The slot is no longer configured.
    Retired,
}

impl CampusLabSlotHoldKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Quarantined => "quarantined",
            Self::Retired => "retired",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "quarantined" => Some(Self::Quarantined),
            "retired" => Some(Self::Retired),
            _ => None,
        }
    }
}

/// A slot hold. While `released_at` is unset the slot is never allocated
/// and does not count toward the tenant's capacity unless it is occupied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredCampusLabSlotHold {
    pub sequence: i64,
    pub slot_id: String,
    pub kind: CampusLabSlotHoldKind,
    pub reason: String,
    pub held_by: String,
    pub held_at: i64,
    pub released_by: Option<String>,
    pub released_at: Option<i64>,
    pub release_reason: Option<String>,
}

/// What [`SqliteStore::sync_campus_lab_slots`] changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CampusLabSlotSync {
    /// Configured slots that were not registered before.
    pub registered: Vec<String>,
    /// Registered slots that are no longer configured.
    pub retired: Vec<String>,
    /// Retired slots that are configured again.
    pub restored: Vec<String>,
}

/// A quarantined session and how often its cleanup has been blocked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampusCleanupBlockedSession {
    pub session_id: String,
    pub slot_id: Option<String>,
    pub blocked_reason: Option<String>,
    pub quarantined_at: i64,
    pub blocked_attempts: i64,
}

impl CampusCleanupBlockedSession {
    pub fn retry_at(&self) -> i64 {
        campus_cleanup_retry_at(self.quarantined_at, self.blocked_attempts)
    }
}

/// The condition of one lab slot, for metrics. Each slot has exactly one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CampusLabSlotCondition {
    Free,
    Occupied,
    /// Occupied by a session whose cleanup is blocked.
    CleanupBlocked,
    /// Under a `quarantined` hold.
    Quarantined,
    /// Under a `retired` hold and empty.
    Retired,
}

impl CampusLabSlotCondition {
    pub const ALL: [Self; 5] = [
        Self::Free,
        Self::Occupied,
        Self::CleanupBlocked,
        Self::Quarantined,
        Self::Retired,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Occupied => "occupied",
            Self::CleanupBlocked => "cleanup_blocked",
            Self::Quarantined => "quarantined",
            Self::Retired => "retired",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|condition| condition.as_str() == value)
    }
}

/// Journal-derived Campus lab gauges of one tenant.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CampusTenantLabMetrics {
    pub tenant_id: String,
    /// Non-terminal sessions by state.
    pub sessions: BTreeMap<CampusLabSessionState, i64>,
    pub slots: BTreeMap<CampusLabSlotCondition, i64>,
    /// Earliest start of a quarantine still in force: a blocked cleanup or a
    /// `quarantined` slot hold.
    pub oldest_quarantined_at: Option<i64>,
    /// Sessions past their absolute expiry that are neither terminal nor
    /// quarantined: teardown that is not making progress.
    pub expired_live: i64,
}

/// First sighting of each unaccounted workload, kept by one janitor between
/// passes. A restarted process starts again from nothing, which only delays
/// a deletion.
#[derive(Debug, Default)]
pub struct CampusOrphanTracker {
    first_seen: BTreeMap<CampusWorkloadRef, i64>,
}

/// What one janitor pass found and did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CampusJanitorReport {
    /// Resources in the provider inventory.
    pub owned: usize,
    /// Workloads no live occupancy accounts for.
    pub orphaned_workloads: usize,
    /// Orphaned workloads the provider confirmed deleted.
    pub deleted: usize,
    /// Orphaned workloads whose deletion is still in progress or waits for
    /// the grace period.
    pub pending: usize,
    /// Orphaned workloads in a slot this tenant does not own; never touched.
    pub foreign: usize,
    pub slots_quarantined: Vec<String>,
    pub slots_released: Vec<String>,
    pub errors: Vec<String>,
}

/// Janitor delete key: deterministic per workload, so repeated passes and
/// restarted janitors repeat one idempotent delete.
pub fn campus_janitor_op_key(workload: &CampusWorkloadRef) -> String {
    format!(
        "campus-janitor/{}/{}/{}",
        workload.session_id, workload.slot_id, workload.slot_generation
    )
}

/// One janitor pass over a tenant's slots:
///
/// 1. list every resource the provider owns and classify it against the
///    journal ([`SqliteStore::find_campus_lab_orphans`]);
/// 2. hold each tenant slot holding an unaccounted workload as
///    `quarantined`, so nothing new is placed next to it;
/// 3. once a workload has been unaccounted for [`CAMPUS_ORPHAN_GRACE_SECONDS`],
///    ask the provider to delete exactly that workload (the delete is fenced
///    on its session, slot and slot generation, so it cannot touch a live
///    occupant);
/// 4. release the janitor's holds on slots whose inventory came back clean.
///
/// A provider that cannot list its inventory releases nothing.
pub async fn reconcile_campus_lab_orphans(
    store: &SqliteStore,
    provider: &dyn CampusExecutionProvider,
    tenant_id: &str,
    now: i64,
    tracker: &mut CampusOrphanTracker,
) -> Result<CampusJanitorReport, CampusJournalError> {
    let mut report = CampusJanitorReport::default();
    let owned = match provider.list_owned().await {
        Ok(owned) => owned,
        Err(error) => {
            report
                .errors
                .push(format!("inventory unavailable: {error}"));
            return Ok(report);
        }
    };
    report.owned = owned.len();
    let tenant_slots = store
        .list_campus_lab_slots(tenant_id)
        .await?
        .into_iter()
        .map(|slot| slot.slot_id)
        .collect::<BTreeSet<_>>();
    let mut orphaned: BTreeMap<CampusWorkloadRef, (CampusOrphanReason, usize)> = BTreeMap::new();
    for orphan in store.find_campus_lab_orphans(&owned).await? {
        orphaned
            .entry(orphan.resource.workload.clone())
            .or_insert((orphan.reason, 0))
            .1 += 1;
    }
    report.orphaned_workloads = orphaned.len();
    let mut dirty_slots = BTreeSet::new();
    for (workload, (reason, resources)) in &orphaned {
        if !tenant_slots.contains(&workload.slot_id) {
            report.foreign += 1;
            continue;
        }
        dirty_slots.insert(workload.slot_id.clone());
        let held = store
            .hold_campus_lab_slot(
                &workload.slot_id,
                tenant_id,
                CampusLabSlotHoldKind::Quarantined,
                &format!(
                    "{resources} unaccounted resources of session {} generation {} ({reason:?})",
                    workload.session_id, workload.slot_generation
                ),
                CAMPUS_JANITOR_ACTOR,
                now,
            )
            .await?;
        if held.is_some() {
            report.slots_quarantined.push(workload.slot_id.clone());
        }
        let first_seen = *tracker.first_seen.entry(workload.clone()).or_insert(now);
        if now - first_seen < CAMPUS_ORPHAN_GRACE_SECONDS {
            report.pending += 1;
            continue;
        }
        match provider
            .delete(&campus_janitor_op_key(workload), workload)
            .await
        {
            Ok(CampusDeleteOutcome::Deleted) => report.deleted += 1,
            Ok(CampusDeleteOutcome::InProgress) => report.pending += 1,
            Ok(
                CampusDeleteOutcome::Blocked { reason }
                | CampusDeleteOutcome::Unreachable { reason },
            )
            | Err(CampusProviderError::Unreachable(reason)) => {
                report.errors.push(format!(
                    "orphan {} in {} not removed: {reason}",
                    workload.session_id, workload.slot_id
                ));
            }
            Err(error) => report.errors.push(format!(
                "orphan {} in {}: {error}",
                workload.session_id, workload.slot_id
            )),
        }
    }
    tracker
        .first_seen
        .retain(|workload, _| orphaned.contains_key(workload));
    for hold in store.list_campus_lab_slot_holds(tenant_id).await? {
        if hold.kind == CampusLabSlotHoldKind::Quarantined
            && hold.held_by == CAMPUS_JANITOR_ACTOR
            && !dirty_slots.contains(&hold.slot_id)
            && store
                .release_campus_lab_slot_hold(
                    &hold.slot_id,
                    CampusLabSlotHoldKind::Quarantined,
                    Some(CAMPUS_JANITOR_ACTOR),
                    CAMPUS_JANITOR_ACTOR,
                    "the provider inventory has no unaccounted resources in the slot",
                    now,
                )
                .await?
        {
            report.slots_released.push(hold.slot_id);
        }
    }
    report.slots_quarantined.sort();
    report.slots_released.sort();
    Ok(report)
}

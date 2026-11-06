//! The Campus lab runtime: admission requests built from the catalog, and the
//! reconciler that drives admitted sessions through the execution provider.
//!
//! Handlers only write the journal and wake the reconciler, so no request
//! waits on a provider. Each sweep, in order:
//!
//! 1. steps sessions past their absolute expiry first, so a stop (which
//!    revokes the session's broker tokens in the journal) never waits behind
//!    other work;
//! 2. steps every other live session with [`advance_campus_lab_session`];
//! 3. retries each blocked cleanup whose backoff
//!    ([`pharness_store::campus_cleanup_retry_at`]) is due;
//! 4. admits the next generation of a lab once its reset's teardown is
//!    confirmed;
//! 5. every [`JANITOR_INTERVAL_SECONDS`], reconciles the provider's
//!    owned-resource inventory against the journal
//!    ([`reconcile_campus_lab_orphans`]) and the registered slots against
//!    the catalog.
//!
//! Steps are leased and fenced in the journal, so concurrent or restarted
//! reconcilers never duplicate a side effect. Nothing is force-deleted: a
//! cleanup the provider cannot confirm stays `cleanup_blocked`, holding its
//! slot, and leftovers no session accounts for keep their slot quarantined.

use super::lab_catalog::{CampusLabAssignment, CampusLabCatalog};
use pharness_core::{CampusExecutionProvider, CampusTeardownHook};
use pharness_store::{
    advance_campus_lab_session, reconcile_campus_lab_orphans, retry_campus_lab_cleanup,
    AdmitCampusLabLaunch, CampusDriverContext, CampusIdentityKey, CampusJournalError,
    CampusLabAdmission, CampusLabSlotSync, CampusOrphanTracker, CampusStepOutcome, SqliteStore,
    StoredCampusLab,
};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, Notify};

const LEASE_SECONDS: i64 = 120;
/// A session moves through at most six transitions; a few more absorb
/// observations without letting one session starve the sweep.
const MAX_STEPS_PER_SESSION: usize = 8;
const SWEEP_BATCH: i64 = 200;
const RECONCILE_INTERVAL: Duration = Duration::from_secs(5);
/// How often the janitor lists the provider's inventory. Orphans wait
/// [`pharness_store::CAMPUS_ORPHAN_GRACE_SECONDS`] after their first
/// sighting before they are deleted.
pub(crate) const JANITOR_INTERVAL_SECONDS: i64 = 30;
pub(crate) const RECONCILER_ACTOR: &str = "campus-lab-reconciler";

/// The most recent janitor pass of this process, for metrics.
pub(crate) struct CampusJanitorTelemetry {
    pub(crate) last_pass: AtomicI64,
    pub(crate) orphaned_workloads: AtomicI64,
    pub(crate) errors: AtomicU64,
}

pub(crate) static CAMPUS_JANITOR: CampusJanitorTelemetry = CampusJanitorTelemetry {
    last_pass: AtomicI64::new(0),
    orphaned_workloads: AtomicI64::new(0),
    errors: AtomicU64::new(0),
};

#[derive(Default)]
struct Janitor {
    tracker: CampusOrphanTracker,
    last_pass: Option<i64>,
}

pub(crate) struct CampusLabs {
    pub(crate) catalog: Arc<CampusLabCatalog>,
    provider: Option<Arc<dyn CampusExecutionProvider>>,
    teardown_hooks: Vec<Arc<dyn CampusTeardownHook>>,
    worker_id: String,
    wake: Notify,
    janitor: Mutex<Janitor>,
}

/// What one reconciler sweep did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CampusLabSweep {
    pub(crate) steps: usize,
    /// Expired sessions stepped ahead of the rest.
    pub(crate) expired: usize,
    /// Blocked cleanups retried after their backoff.
    pub(crate) cleanup_retries: usize,
    pub(crate) resets_admitted: usize,
    pub(crate) resets_cleared: usize,
    pub(crate) janitor_passes: usize,
    /// Unaccounted workloads the janitor saw in this sweep.
    pub(crate) orphaned_workloads: usize,
    pub(crate) orphans_deleted: usize,
    pub(crate) errors: usize,
}

/// Who a launch is for and what it asks for. Everything else comes from the
/// catalog.
pub(crate) struct LabLaunch<'a> {
    pub(crate) owner: &'a CampusIdentityKey,
    pub(crate) assignment: &'a CampusLabAssignment,
    pub(crate) idempotency_key: Option<String>,
    pub(crate) reset_from_session_id: Option<String>,
    pub(crate) actor: String,
    pub(crate) now: i64,
}

fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::now_v7().simple())
}

impl CampusLabs {
    #[cfg(test)]
    pub(crate) fn new(
        catalog: CampusLabCatalog,
        provider: Option<Arc<dyn CampusExecutionProvider>>,
    ) -> Self {
        Self::with_shared_catalog(Arc::new(catalog), provider)
    }

    /// For a provider that itself reads the catalog, such as the Kubernetes
    /// provider's authority source. Each runtime gets a fresh worker
    /// identity, which is what a restarted process (or a second replica) of
    /// the same deployment runs.
    pub(crate) fn with_shared_catalog(
        catalog: Arc<CampusLabCatalog>,
        provider: Option<Arc<dyn CampusExecutionProvider>>,
    ) -> Self {
        Self {
            catalog,
            provider,
            teardown_hooks: Vec::new(),
            worker_id: new_id("campus-api"),
            wake: Notify::new(),
            janitor: Mutex::new(Janitor::default()),
        }
    }

    /// Hooks that release what a session held outside the execution
    /// provider. They run, in order, after the provider confirmed a
    /// workload gone and before the cleanup is recorded; each completes at
    /// most once per session.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn with_teardown_hooks(mut self, hooks: Vec<Arc<dyn CampusTeardownHook>>) -> Self {
        self.teardown_hooks = hooks;
        self
    }

    /// Whether a provider is configured, without which nothing is launched.
    pub(crate) fn can_launch(&self) -> bool {
        self.provider.is_some()
    }

    pub(crate) fn wake(&self) {
        self.wake.notify_one();
    }

    /// The catalog's slot ids (`lab_slot_ids`, by default
    /// `<tenant>-lab-slot-NN`). With the Kubernetes provider they are the M1
    /// slot namespaces `campus-lab-slot-N`, which the provider's slot map and
    /// the slots' `pharness.dev/campus-slot` label expect.
    pub(crate) fn slot_ids(&self) -> Vec<String> {
        self.catalog.slot_ids.clone()
    }

    /// Make the registered slots match the catalog: register missing ones,
    /// retire ones no longer configured (an occupant keeps running until it
    /// ends; the slot is never reused) and restore retired ones that are
    /// configured again.
    pub(crate) async fn register_slots(
        &self,
        store: &SqliteStore,
        now: i64,
    ) -> Result<CampusLabSlotSync, CampusJournalError> {
        store
            .sync_campus_lab_slots(
                &self.catalog.tenant_id,
                &self.slot_ids(),
                RECONCILER_ACTOR,
                now,
            )
            .await
    }
    /// The admission request for a launch or a reset. Identity, profile,
    /// policy, lifetime and limits are all derived here.
    pub(crate) fn admission(&self, launch: LabLaunch<'_>) -> AdmitCampusLabLaunch {
        let assignment = launch.assignment;
        AdmitCampusLabLaunch {
            tenant_id: launch.owner.tenant_id.clone(),
            course_id: assignment.course_id.clone(),
            assignment_id: assignment.assignment_id.clone(),
            issuer: launch.owner.issuer.clone(),
            subject: launch.owner.subject.clone(),
            idempotency_key: launch.idempotency_key,
            reset_from_session_id: launch.reset_from_session_id,
            candidate_lab_id: new_id("clab"),
            candidate_budget_id: new_id("cbud"),
            candidate_session_id: new_id("cls"),
            profile: assignment.policy.profile().clone(),
            effective_policy_hash: assignment.policy.effective_policy_hash().to_string(),
            expires_at: launch.now.saturating_add(assignment.session_seconds),
            actor: launch.actor,
            now: launch.now,
            limits: self.catalog.limits,
        }
    }

    /// One reconciler pass over the tenant. A failure on one session is
    /// logged and counted; the sweep moves on to the next one.
    pub(crate) async fn reconcile(&self, store: &SqliteStore, now: i64) -> CampusLabSweep {
        let mut sweep = CampusLabSweep::default();
        let Some(provider) = self.provider.as_deref() else {
            return sweep;
        };
        let context = CampusDriverContext {
            worker_id: &self.worker_id,
            now,
            lease_seconds: LEASE_SECONDS,
            central_authority: true,
            teardown_hooks: &self.teardown_hooks,
        };
        let tenant_id = &self.catalog.tenant_id;
        let mut settled = BTreeSet::new();
        match store
            .list_expired_campus_lab_session_ids(tenant_id, now, SWEEP_BATCH)
            .await
        {
            Ok(ids) => {
                for id in ids {
                    sweep.expired += 1;
                    self.settle(store, provider, &context, &id, &mut sweep)
                        .await;
                    settled.insert(id);
                }
            }
            Err(error) => {
                tracing::error!(%error, "expired campus lab sessions could not be listed");
                sweep.errors += 1;
            }
        }
        match store
            .list_steppable_campus_lab_session_ids(tenant_id, SWEEP_BATCH)
            .await
        {
            Ok(ids) => {
                for id in ids.into_iter().filter(|id| !settled.contains(id)) {
                    self.settle(store, provider, &context, &id, &mut sweep)
                        .await;
                }
            }
            Err(error) => {
                tracing::error!(%error, "campus lab sessions could not be listed");
                sweep.errors += 1;
            }
        }
        self.retry_blocked_cleanups(store, provider, &context, &mut sweep)
            .await;
        match store
            .list_campus_labs_awaiting_reset(tenant_id, SWEEP_BATCH)
            .await
        {
            Ok(labs) => {
                for lab in labs {
                    if let Err(error) = self
                        .admit_reset(store, provider, &context, &lab, &mut sweep)
                        .await
                    {
                        tracing::error!(%error, lab_id = %lab.lab_id, "campus lab reset failed");
                        sweep.errors += 1;
                    }
                }
            }
            Err(error) => {
                tracing::error!(%error, "campus lab resets could not be listed");
                sweep.errors += 1;
            }
        }
        self.janitor_pass(store, provider, now, &mut sweep).await;
        sweep
    }

    /// Retry every blocked cleanup whose backoff has run out. A retry that
    /// blocks again stays quarantined with a longer wait.
    async fn retry_blocked_cleanups(
        &self,
        store: &SqliteStore,
        provider: &dyn CampusExecutionProvider,
        context: &CampusDriverContext<'_>,
        sweep: &mut CampusLabSweep,
    ) {
        let blocked = match store
            .list_campus_cleanup_blocked_sessions(&self.catalog.tenant_id)
            .await
        {
            Ok(blocked) => blocked,
            Err(error) => {
                tracing::error!(%error, "blocked campus lab cleanups could not be listed");
                sweep.errors += 1;
                return;
            }
        };
        for session in blocked
            .iter()
            .filter(|session| context.now >= session.retry_at())
        {
            let session_id = session.session_id.as_str();
            match retry_campus_lab_cleanup(store, provider, context, session_id).await {
                Ok(CampusStepOutcome::Quarantined { reason }) => {
                    sweep.cleanup_retries += 1;
                    tracing::warn!(
                        %session_id,
                        slot_id = session.slot_id.as_deref().unwrap_or(""),
                        attempts = session.blocked_attempts + 1,
                        %reason,
                        "campus lab cleanup is still blocked"
                    );
                }
                Ok(_) => {
                    sweep.cleanup_retries += 1;
                    tracing::info!(%session_id, "blocked campus lab cleanup retried");
                    self.settle(store, provider, context, session_id, sweep)
                        .await;
                }
                Err(CampusJournalError::Fenced(_)) => {}
                Err(error) => {
                    tracing::warn!(%error, %session_id, "campus lab cleanup retry failed");
                    sweep.errors += 1;
                }
            }
        }
    }

    /// Reconcile the provider inventory and the slot registry, at most once
    /// per [`JANITOR_INTERVAL_SECONDS`].
    async fn janitor_pass(
        &self,
        store: &SqliteStore,
        provider: &dyn CampusExecutionProvider,
        now: i64,
        sweep: &mut CampusLabSweep,
    ) {
        let mut janitor = self.janitor.lock().await;
        if janitor
            .last_pass
            .is_some_and(|last| now - last < JANITOR_INTERVAL_SECONDS)
        {
            return;
        }
        janitor.last_pass = Some(now);
        sweep.janitor_passes += 1;
        let tenant_id = &self.catalog.tenant_id;
        match reconcile_campus_lab_orphans(store, provider, tenant_id, now, &mut janitor.tracker)
            .await
        {
            Ok(report) => {
                sweep.orphaned_workloads += report.orphaned_workloads;
                sweep.orphans_deleted += report.deleted;
                sweep.errors += report.errors.len();
                CAMPUS_JANITOR.last_pass.store(now, Ordering::Relaxed);
                CAMPUS_JANITOR.orphaned_workloads.store(
                    i64::try_from(report.orphaned_workloads).unwrap_or(i64::MAX),
                    Ordering::Relaxed,
                );
                CAMPUS_JANITOR
                    .errors
                    .fetch_add(report.errors.len() as u64, Ordering::Relaxed);
                if report.orphaned_workloads > 0 || !report.errors.is_empty() {
                    tracing::warn!(
                        owned = report.owned,
                        orphaned = report.orphaned_workloads,
                        deleted = report.deleted,
                        pending = report.pending,
                        foreign = report.foreign,
                        quarantined = ?report.slots_quarantined,
                        errors = ?report.errors,
                        "campus lab janitor found unaccounted resources"
                    );
                }
                if !report.slots_released.is_empty() {
                    tracing::info!(
                        released = ?report.slots_released,
                        "campus lab janitor released quarantined slots"
                    );
                }
            }
            Err(error) => {
                tracing::error!(%error, "campus lab janitor pass failed");
                CAMPUS_JANITOR.errors.fetch_add(1, Ordering::Relaxed);
                sweep.errors += 1;
            }
        }
        match self.register_slots(store, now).await {
            Ok(sync) if sync != CampusLabSlotSync::default() => {
                tracing::info!(?sync, "campus lab slots changed");
            }
            Ok(_) => {}
            Err(error) => {
                tracing::error!(%error, "campus lab slots could not be synchronised");
                sweep.errors += 1;
            }
        }
    }
    async fn settle(
        &self,
        store: &SqliteStore,
        provider: &dyn CampusExecutionProvider,
        context: &CampusDriverContext<'_>,
        session_id: &str,
        sweep: &mut CampusLabSweep,
    ) {
        for _ in 0..MAX_STEPS_PER_SESSION {
            match advance_campus_lab_session(store, provider, context, session_id).await {
                Ok(CampusStepOutcome::Advanced { .. }) => sweep.steps += 1,
                Ok(_) => return,
                Err(CampusJournalError::Fenced(_)) => return,
                Err(error) => {
                    tracing::warn!(%error, %session_id, "campus lab session step failed");
                    sweep.errors += 1;
                    return;
                }
            }
        }
    }

    /// Admit the next generation of a lab whose reset finished tearing down.
    /// A reset that can no longer be admitted (assignment withdrawn,
    /// membership revoked, capacity reduced) is cleared rather than left
    /// holding capacity.
    async fn admit_reset(
        &self,
        store: &SqliteStore,
        provider: &dyn CampusExecutionProvider,
        context: &CampusDriverContext<'_>,
        lab: &StoredCampusLab,
        sweep: &mut CampusLabSweep,
    ) -> Result<(), CampusJournalError> {
        let Some(previous) = lab.current_session_id.clone() else {
            return Ok(());
        };
        let owner = CampusIdentityKey {
            tenant_id: lab.tenant_id.clone(),
            issuer: lab.issuer.clone(),
            subject: lab.subject.clone(),
        };
        let still_member = store
            .get_campus_course_role(&owner, &lab.course_id)
            .await?
            .is_some();
        let assignment = self.catalog.assignment(&lab.course_id, &lab.assignment_id);
        let outcome = match assignment.filter(|_| still_member) {
            Some(assignment) => Some(
                store
                    .admit_campus_lab_launch(self.admission(LabLaunch {
                        owner: &owner,
                        assignment,
                        idempotency_key: None,
                        reset_from_session_id: Some(previous.clone()),
                        actor: RECONCILER_ACTOR.to_string(),
                        now: context.now,
                    }))
                    .await?,
            ),
            None => None,
        };
        match outcome {
            Some(CampusLabAdmission::Created(record)) => {
                sweep.resets_admitted += 1;
                self.settle(store, provider, context, &record.session.session_id, sweep)
                    .await;
            }
            Some(CampusLabAdmission::Active(_) | CampusLabAdmission::Replayed(_)) => {}
            Some(CampusLabAdmission::Rejected(rejection)) => {
                tracing::warn!(?rejection, lab_id = %lab.lab_id, "campus lab reset refused");
                if store
                    .clear_campus_lab_reset(&lab.lab_id, &previous, context.now)
                    .await?
                {
                    sweep.resets_cleared += 1;
                }
            }
            None => {
                tracing::warn!(lab_id = %lab.lab_id, "campus lab reset is no longer permitted");
                if store
                    .clear_campus_lab_reset(&lab.lab_id, &previous, context.now)
                    .await?
                {
                    sweep.resets_cleared += 1;
                }
            }
        }
        Ok(())
    }

    /// Run the reconciler until the process exits: a sweep every few
    /// seconds, and immediately after a handler wakes it. `on_change` runs
    /// after a sweep that moved a session, so open terminals re-check their
    /// session at once instead of at their next interval.
    pub(crate) fn spawn_reconciler(
        self: Arc<Self>,
        store: Arc<SqliteStore>,
        now: impl Fn() -> i64 + Send + Sync + 'static,
        on_change: impl Fn() + Send + Sync + 'static,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                let sweep = self.reconcile(&store, now()).await;
                if sweep.steps > 0 || sweep.cleanup_retries > 0 {
                    on_change();
                }
                if sweep.errors > 0 {
                    tracing::warn!(?sweep, "campus lab sweep finished with errors");
                }
                let _ = tokio::time::timeout(RECONCILE_INTERVAL, self.wake.notified()).await;
            }
        })
    }
}

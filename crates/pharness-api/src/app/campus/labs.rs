//! The Campus lab runtime: admission requests built from the catalog, and the
//! reconciler that drives admitted sessions through the execution provider.
//!
//! Handlers only write the journal and wake the reconciler, so no request
//! waits on a provider. The reconciler steps every live session with
//! [`advance_campus_lab_session`] and admits the next generation of a lab
//! once its reset's teardown is confirmed. Steps are leased and fenced in the
//! journal, so concurrent or restarted reconcilers never duplicate a side
//! effect.

use super::lab_catalog::{CampusLabAssignment, CampusLabCatalog};
use pharness_core::CampusExecutionProvider;
use pharness_store::{
    advance_campus_lab_session, AdmitCampusLabLaunch, CampusDriverContext, CampusIdentityKey,
    CampusJournalError, CampusLabAdmission, CampusStepOutcome, SqliteStore, StoredCampusLab,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

const LEASE_SECONDS: i64 = 120;
/// A session moves through at most six transitions; a few more absorb
/// observations without letting one session starve the sweep.
const MAX_STEPS_PER_SESSION: usize = 8;
const SWEEP_BATCH: i64 = 200;
const RECONCILE_INTERVAL: Duration = Duration::from_secs(5);
pub(crate) const RECONCILER_ACTOR: &str = "campus-lab-reconciler";

pub(crate) struct CampusLabs {
    pub(crate) catalog: Arc<CampusLabCatalog>,
    provider: Option<Arc<dyn CampusExecutionProvider>>,
    worker_id: String,
    wake: Notify,
}

/// What one reconciler sweep did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CampusLabSweep {
    pub(crate) steps: usize,
    pub(crate) resets_admitted: usize,
    pub(crate) resets_cleared: usize,
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
    /// provider's authority source.
    pub(crate) fn with_shared_catalog(
        catalog: Arc<CampusLabCatalog>,
        provider: Option<Arc<dyn CampusExecutionProvider>>,
    ) -> Self {
        Self {
            catalog,
            provider,
            worker_id: new_id("campus-api"),
            wake: Notify::new(),
        }
    }

    /// Whether a provider is configured, without which nothing is launched.
    pub(crate) fn can_launch(&self) -> bool {
        self.provider.is_some()
    }

    pub(crate) fn wake(&self) {
        self.wake.notify_one();
    }

    pub(crate) fn slot_ids(&self) -> Vec<String> {
        self.catalog.slot_ids.clone()
    }

    /// Register the configured lab slots. Re-registering is a no-op.
    pub(crate) async fn register_slots(
        &self,
        store: &SqliteStore,
        now: i64,
    ) -> Result<(), CampusJournalError> {
        for slot_id in self.slot_ids() {
            store
                .register_campus_lab_slot(&slot_id, &self.catalog.tenant_id, now)
                .await?;
        }
        Ok(())
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
            teardown_hooks: &[],
        };
        let tenant_id = &self.catalog.tenant_id;
        match store
            .list_steppable_campus_lab_session_ids(tenant_id, SWEEP_BATCH)
            .await
        {
            Ok(ids) => {
                for id in ids {
                    self.settle(store, provider, &context, &id, &mut sweep)
                        .await;
                }
            }
            Err(error) => {
                tracing::error!(%error, "campus lab sessions could not be listed");
                sweep.errors += 1;
            }
        }
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
        sweep
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
    /// seconds, and immediately after a handler wakes it.
    pub(crate) fn spawn_reconciler(
        self: Arc<Self>,
        store: Arc<SqliteStore>,
        now: impl Fn() -> i64 + Send + Sync + 'static,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                let sweep = self.reconcile(&store, now()).await;
                if sweep.errors > 0 {
                    tracing::warn!(?sweep, "campus lab sweep finished with errors");
                }
                let _ = tokio::time::timeout(RECONCILE_INTERVAL, self.wake.notified()).await;
            }
        })
    }
}

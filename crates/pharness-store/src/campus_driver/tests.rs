use super::*;
use crate::campus_journal::test_support::{seed, DiskStore, TENANT, TTL};
use async_trait::async_trait;
use pharness_core::{
    CampusLabOperationStatus, CampusWorkloadRef, FakeCampusProvider, FakeFault, FakeFaultPoint,
    OwnedCampusResource,
};
use std::sync::{Arc, Mutex};

use CampusLabSessionState as State;

const LEASE: i64 = 30;

fn context(worker: &str, now: i64) -> CampusDriverContext<'_> {
    CampusDriverContext {
        worker_id: worker,
        now,
        lease_seconds: LEASE,
        central_authority: true,
        teardown_hooks: &[],
    }
}

async fn advance(
    store: &SqliteStore,
    provider: &dyn CampusExecutionProvider,
    worker: &str,
    now: i64,
    session_id: &str,
) -> CampusStepOutcome {
    advance_campus_lab_session(store, provider, &context(worker, now), session_id)
        .await
        .unwrap()
}

/// Step until the session stops making progress, and return its state.
async fn settle(
    store: &SqliteStore,
    provider: &dyn CampusExecutionProvider,
    worker: &str,
    now: i64,
    session_id: &str,
) -> State {
    for _ in 0..12 {
        if !matches!(
            advance(store, provider, worker, now, session_id).await,
            CampusStepOutcome::Advanced { .. }
        ) {
            break;
        }
    }
    session(store, session_id).await.state
}

async fn session(store: &SqliteStore, session_id: &str) -> StoredCampusLabSession {
    store
        .get_campus_lab_session(session_id)
        .await
        .unwrap()
        .unwrap()
}

async fn operations(store: &SqliteStore, session_id: &str) -> Vec<StoredCampusLabOperation> {
    store.list_campus_lab_operations(session_id).await.unwrap()
}

async fn event_kinds(store: &SqliteStore, session_id: &str) -> Vec<String> {
    store
        .list_campus_lab_session_events(session_id)
        .await
        .unwrap()
        .into_iter()
        .map(|event| event.kind)
        .collect()
}

/// Step on a separate task so the test can abort it at a fault point, which
/// is exactly what a process crash does to the in-flight future.
fn spawn_step(
    store: &SqliteStore,
    fake: &Arc<FakeCampusProvider>,
    worker: &'static str,
    now: i64,
    session_id: &'static str,
) -> tokio::task::JoinHandle<Result<CampusStepOutcome, CampusJournalError>> {
    let store = store.clone();
    let fake = fake.clone();
    tokio::spawn(async move {
        advance_campus_lab_session(&store, &*fake, &context(worker, now), session_id).await
    })
}

async fn crash(task: tokio::task::JoinHandle<Result<CampusStepOutcome, CampusJournalError>>) {
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}

async fn stop(store: &SqliteStore, session_id: &str, now: i64) {
    store
        .request_campus_lab_session_stop(session_id, "student", "finished", now)
        .await
        .unwrap();
}

#[tokio::test]
async fn a_session_runs_its_whole_lifecycle_with_one_workspace() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1"], &["s1"], 100).await;
    let fake = FakeCampusProvider::new();
    assert_eq!(
        advance(&store, &fake, "w", 101, "s1").await,
        CampusStepOutcome::Advanced {
            state: State::Authorizing,
            generation: 2
        }
    );
    assert_eq!(
        advance(&store, &fake, "w", 102, "s1").await,
        CampusStepOutcome::Advanced {
            state: State::Ready,
            generation: 4
        }
    );
    assert_eq!(
        advance(&store, &fake, "w", 103, "s1").await,
        CampusStepOutcome::Observed {
            observed: CampusLabObservedState::Ready
        }
    );
    stop(&store, "s1", 104).await;
    assert_eq!(
        advance(&store, &fake, "w", 105, "s1").await,
        CampusStepOutcome::Advanced {
            state: State::Stopping,
            generation: 5
        }
    );
    assert_eq!(
        advance(&store, &fake, "w", 106, "s1").await,
        CampusStepOutcome::Advanced {
            state: State::Terminated,
            generation: 7
        }
    );
    assert_eq!(
        advance(&store, &fake, "w", 107, "s1").await,
        CampusStepOutcome::Terminal {
            state: State::Terminated
        }
    );

    let done = session(&store, "s1").await;
    assert_eq!(done.revoked_at, Some(105));
    assert_eq!(done.terminated_at, Some(106));
    assert_eq!(done.observed_state, CampusLabObservedState::Deleted);
    assert!(done.workspace_id.is_some());
    assert_eq!(
        event_kinds(&store, "s1").await,
        [
            "campus.session.requested",
            "campus.session.authorizing",
            "campus.session.provisioning",
            "campus.session.ready",
            "campus.session.stop_requested",
            "campus.session.stopping",
            "campus.session.cleaning",
            "campus.session.terminated",
        ]
    );
    let operations = operations(&store, "s1").await;
    assert_eq!(operations.len(), 2);
    assert!(operations.iter().all(|operation| {
        operation.status == CampusLabOperationStatus::Succeeded
            && operation.attempt_count == 1
            && operation.lease_owner.is_none()
    }));
    assert_eq!(fake.workspaces_created(), 1);
    assert!(fake.resources().is_empty());
    assert_eq!(
        fake.calls(),
        ["provision:s1:1", "observe:s1:1", "delete:s1:1"]
    );
    let slot = store.get_campus_lab_slot("slot-1").await.unwrap().unwrap();
    assert_eq!(
        (slot.occupant_session_id, slot.occupant_generation),
        (None, 1)
    );
}

#[tokio::test]
async fn a_crash_before_the_provision_side_effect_resumes_after_lease_expiry() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1"], &["s1"], 100).await;
    let fake = Arc::new(FakeCampusProvider::new());
    advance(&store, &*fake, "w", 101, "s1").await;
    fake.inject(FakeFaultPoint::BeforeProvision, FakeFault::Pause);
    let task = spawn_step(&store, &fake, "worker-a", 102, "s1");
    fake.wait_for_pause().await;
    crash(task).await;

    // The intent survived the crash even though nothing was created.
    assert_eq!(session(&store, "s1").await.state, State::Provisioning);
    let pending = store
        .pending_campus_lab_operation("s1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending.lease_owner.as_deref(), Some("worker-a"));
    assert_eq!(fake.workspaces_created(), 0);

    assert_eq!(
        advance(&store, &*fake, "worker-b", 110, "s1").await,
        CampusStepOutcome::LeaseHeld {
            owner: "worker-a".into(),
            expires_at: 102 + LEASE
        }
    );
    assert!(matches!(
        advance(&store, &*fake, "worker-b", 102 + LEASE, "s1").await,
        CampusStepOutcome::Advanced {
            state: State::Ready,
            ..
        }
    ));
    assert_eq!(fake.workspaces_created(), 1);
    assert_eq!(fake.workspaces_for_session("s1").len(), 1);
    let operation = store
        .get_campus_lab_operation(&pending.op_key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(operation.status, CampusLabOperationStatus::Succeeded);
    assert_eq!(operation.attempt_count, 2);
}

#[tokio::test]
async fn a_crash_after_the_side_effect_and_duplicate_delivery_reuse_one_workspace() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1"], &["s1"], 100).await;
    let fake = Arc::new(FakeCampusProvider::new());
    advance(&store, &*fake, "w", 101, "s1").await;
    fake.inject(FakeFaultPoint::AfterProvision, FakeFault::Pause);
    let task = spawn_step(&store, &fake, "worker-a", 102, "s1");
    fake.wait_for_pause().await;
    crash(task).await;

    // The workspace exists but the journal never heard about it.
    assert_eq!(fake.workspaces_created(), 1);
    let crashed = session(&store, "s1").await;
    assert_eq!(crashed.state, State::Provisioning);
    assert_eq!(crashed.workspace_id, None);

    // A duplicate delivery while the dead worker's lease is live is a no-op.
    assert!(matches!(
        advance(&store, &*fake, "worker-b", 120, "s1").await,
        CampusStepOutcome::LeaseHeld { .. }
    ));
    // The restarted worker uses a new id and resumes the same operation.
    assert!(matches!(
        advance(&store, &*fake, "worker-a-restarted", 102 + LEASE, "s1").await,
        CampusStepOutcome::Advanced {
            state: State::Ready,
            ..
        }
    ));
    // Further duplicate deliveries only observe.
    assert!(matches!(
        advance(&store, &*fake, "worker-b", 140, "s1").await,
        CampusStepOutcome::Observed { .. }
    ));
    assert_eq!(fake.workspaces_created(), 1);
    let workspaces = fake.workspaces_for_session("s1");
    assert_eq!(workspaces.len(), 1);
    assert_eq!(
        session(&store, "s1").await.workspace_id,
        Some(workspaces[0].clone())
    );
    let operations = operations(&store, "s1").await;
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0].attempt_count, 2);
    assert_eq!(
        fake.calls(),
        ["provision:s1:1", "provision:s1:1", "observe:s1:1"]
    );
}

#[tokio::test]
async fn concurrent_duplicate_deliveries_create_one_workspace() {
    let disk = DiskStore::new("duplicate").await;
    let store = &disk.store;
    seed(store, &["slot-1"], &["s1"], 100).await;
    let fake = FakeCampusProvider::new();
    advance(store, &fake, "w", 101, "s1").await;
    let (w1, w2) = (context("w1", 102), context("w2", 102));
    let (first, second) = tokio::join!(
        advance_campus_lab_session(store, &fake, &w1, "s1"),
        advance_campus_lab_session(store, &fake, &w2, "s1"),
    );
    for outcome in [first, second] {
        assert!(
            matches!(
                outcome,
                Ok(CampusStepOutcome::Advanced { .. }
                    | CampusStepOutcome::LeaseHeld { .. }
                    | CampusStepOutcome::Observed { .. })
                    | Err(CampusJournalError::Fenced(_))
            ),
            "{outcome:?}"
        );
    }
    assert_eq!(settle(store, &fake, "w1", 103, "s1").await, State::Ready);
    assert_eq!(fake.workspaces_created(), 1);
    assert_eq!(operations(store, "s1").await.len(), 1);
}

#[tokio::test]
async fn a_stale_worker_cannot_mutate_the_slot_after_it_is_reoccupied() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1"], &["s1", "s2"], 100).await;
    let fake = Arc::new(FakeCampusProvider::new());
    advance(&store, &*fake, "w", 101, "s1").await;
    fake.inject(FakeFaultPoint::AfterProvision, FakeFault::Pause);
    let stale = spawn_step(&store, &fake, "worker-a", 102, "s1");
    fake.wait_for_pause().await;

    // worker-a stalls. worker-b finishes s1, and s2 takes the slot.
    assert!(matches!(
        advance(&store, &*fake, "worker-b", 133, "s1").await,
        CampusStepOutcome::Advanced {
            state: State::Ready,
            ..
        }
    ));
    stop(&store, "s1", 134).await;
    assert_eq!(
        settle(&store, &*fake, "worker-b", 135, "s1").await,
        State::Terminated
    );
    assert_eq!(
        settle(&store, &*fake, "worker-b", 136, "s2").await,
        State::Ready
    );
    let s2 = session(&store, "s2").await;
    assert_eq!(s2.slot_generation, Some(2));

    fake.release_pause();
    let late = stale.await.unwrap();
    assert!(
        matches!(late, Err(CampusJournalError::Fenced(_))),
        "{late:?}"
    );

    let slot = store.get_campus_lab_slot("slot-1").await.unwrap().unwrap();
    assert_eq!(slot.occupant_session_id.as_deref(), Some("s2"));
    assert_eq!(slot.occupant_generation, 2);
    assert_eq!(session(&store, "s2").await, s2);
    assert_eq!(fake.resources_for(&s2.workload().unwrap()).len(), 2);
    assert_eq!(fake.workspaces_created(), 2);
}

#[tokio::test]
async fn a_stale_provision_cannot_resurrect_a_torn_down_workspace() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1"], &["s1"], 100).await;
    let fake = Arc::new(FakeCampusProvider::new());
    advance(&store, &*fake, "w", 101, "s1").await;
    fake.inject(FakeFaultPoint::BeforeProvision, FakeFault::Pause);
    let stale = spawn_step(&store, &fake, "worker-a", 102, "s1");
    fake.wait_for_pause().await;

    advance(&store, &*fake, "worker-b", 133, "s1").await;
    stop(&store, "s1", 134).await;
    assert_eq!(
        settle(&store, &*fake, "worker-b", 135, "s1").await,
        State::Terminated
    );

    fake.release_pause();
    let late = stale.await.unwrap();
    assert!(
        matches!(late, Err(CampusJournalError::Fenced(_))),
        "{late:?}"
    );
    assert!(fake.resources().is_empty());
    assert_eq!(fake.workspaces_created(), 1);
}

#[tokio::test]
async fn a_second_worker_loses_the_lease_race() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1"], &["s1"], 100).await;
    let fake = Arc::new(FakeCampusProvider::new());
    advance(&store, &*fake, "w", 101, "s1").await;
    fake.inject(FakeFaultPoint::BeforeProvision, FakeFault::Pause);
    let winner = spawn_step(&store, &fake, "worker-a", 102, "s1");
    fake.wait_for_pause().await;
    assert_eq!(
        advance(&store, &*fake, "worker-b", 103, "s1").await,
        CampusStepOutcome::LeaseHeld {
            owner: "worker-a".into(),
            expires_at: 102 + LEASE
        }
    );
    fake.release_pause();
    assert!(matches!(
        winner.await.unwrap().unwrap(),
        CampusStepOutcome::Advanced {
            state: State::Ready,
            ..
        }
    ));
    assert_eq!(fake.workspaces_created(), 1);
    assert_eq!(
        fake.calls(),
        ["provision:s1:1"],
        "the losing worker never called the provider"
    );
}

/// Asserts, from inside every side effect, that its operation, transition and
/// audit event are already committed.
struct AuditingProvider {
    store: SqliteStore,
    inner: FakeCampusProvider,
    checked: Mutex<Vec<String>>,
}

impl AuditingProvider {
    async fn assert_intent_is_durable(&self, op_key: &str, session_id: &str, state: State) {
        let operation = self
            .store
            .get_campus_lab_operation(op_key)
            .await
            .unwrap()
            .expect("the operation is recorded before its side effect");
        assert_eq!(operation.status, CampusLabOperationStatus::Pending);
        assert!(operation.lease_owner.is_some());
        let current = session(&self.store, session_id).await;
        assert_eq!(current.state, state);
        assert_eq!(current.generation, operation.session_generation);
        let events = self
            .store
            .list_campus_lab_session_events(session_id)
            .await
            .unwrap();
        assert!(
            events
                .iter()
                .any(|event| event.op_key.as_deref() == Some(op_key)
                    && event.to_state == Some(state)
                    && event.generation == operation.session_generation),
            "the audit event for {op_key} is committed before the side effect"
        );
        self.checked
            .lock()
            .unwrap()
            .push(format!("{state}:{op_key}"));
    }
}

#[async_trait]
impl CampusExecutionProvider for AuditingProvider {
    async fn provision(
        &self,
        op_key: &str,
        spec: &CampusProvisionSpec,
    ) -> Result<CampusProvisionOutcome, CampusProviderError> {
        self.assert_intent_is_durable(op_key, &spec.workload.session_id, State::Provisioning)
            .await;
        self.inner.provision(op_key, spec).await
    }

    async fn observe(
        &self,
        workload: &CampusWorkloadRef,
    ) -> Result<CampusObservation, CampusProviderError> {
        self.inner.observe(workload).await
    }

    async fn delete(
        &self,
        op_key: &str,
        workload: &CampusWorkloadRef,
    ) -> Result<CampusDeleteOutcome, CampusProviderError> {
        self.assert_intent_is_durable(op_key, &workload.session_id, State::Cleaning)
            .await;
        self.inner.delete(op_key, workload).await
    }

    async fn list_owned(&self) -> Result<Vec<OwnedCampusResource>, CampusProviderError> {
        self.inner.list_owned().await
    }
}

#[tokio::test]
async fn audit_events_and_intent_are_committed_before_each_side_effect() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1"], &["s1"], 100).await;
    let provider = AuditingProvider {
        store: store.clone(),
        inner: FakeCampusProvider::new(),
        checked: Mutex::default(),
    };
    assert_eq!(
        settle(&store, &provider, "w", 101, "s1").await,
        State::Ready
    );
    stop(&store, "s1", 102).await;
    assert_eq!(
        settle(&store, &provider, "w", 103, "s1").await,
        State::Terminated
    );
    let checked = provider.checked.lock().unwrap().clone();
    assert_eq!(checked.len(), 2);
    assert!(checked[0].starts_with("provisioning:campus-lab/s1/provision/"));
    assert!(checked[1].starts_with("cleaning:campus-lab/s1/delete/"));
}

#[tokio::test]
async fn a_crash_mid_delete_resumes_and_removes_every_owned_resource() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1"], &["s1"], 100).await;
    let fake = Arc::new(FakeCampusProvider::new());
    assert_eq!(settle(&store, &*fake, "w", 101, "s1").await, State::Ready);
    let workload = session(&store, "s1").await.workload().unwrap();
    stop(&store, "s1", 102).await;
    advance(&store, &*fake, "w", 103, "s1").await;
    fake.inject(FakeFaultPoint::MidDelete, FakeFault::Pause);
    let task = spawn_step(&store, &fake, "worker-a", 104, "s1");
    fake.wait_for_pause().await;
    crash(task).await;

    assert_eq!(session(&store, "s1").await.state, State::Cleaning);
    assert_eq!(fake.resources_for(&workload).len(), 1);
    assert!(matches!(
        advance(&store, &*fake, "worker-b", 120, "s1").await,
        CampusStepOutcome::LeaseHeld { .. }
    ));
    assert!(matches!(
        advance(&store, &*fake, "worker-b", 104 + LEASE, "s1").await,
        CampusStepOutcome::Advanced {
            state: State::Terminated,
            ..
        }
    ));
    assert!(fake.resources().is_empty());
    let delete = operations(&store, "s1").await.pop().unwrap();
    assert_eq!(delete.kind, CampusLabOperationKind::Delete);
    assert_eq!(delete.attempt_count, 2);
    assert_eq!(
        store
            .get_campus_lab_slot("slot-1")
            .await
            .unwrap()
            .unwrap()
            .occupant_session_id,
        None
    );
}

#[tokio::test]
async fn unreachable_cleanup_is_quarantined_and_never_reported_terminated() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1"], &["s1"], 100).await;
    let fake = FakeCampusProvider::new();
    assert_eq!(settle(&store, &fake, "w", 101, "s1").await, State::Ready);
    stop(&store, "s1", 102).await;
    advance(&store, &fake, "w", 103, "s1").await;
    fake.inject(FakeFaultPoint::BeforeDelete, FakeFault::Unreachable);
    assert!(matches!(
        advance(&store, &fake, "w", 104, "s1").await,
        CampusStepOutcome::Advanced {
            state: State::CleanupBlocked,
            ..
        }
    ));
    for now in [105, 10_000] {
        assert!(matches!(
            advance(&store, &fake, "w", now, "s1").await,
            CampusStepOutcome::Quarantined { .. }
        ));
    }
    let blocked = session(&store, "s1").await;
    assert_eq!(blocked.state, State::CleanupBlocked);
    assert_eq!(blocked.terminated_at, None);
    assert_eq!(blocked.quarantined_at, Some(104));
    assert_eq!(blocked.observed_state, CampusLabObservedState::Unreachable);
    assert!(blocked.blocked_reason.unwrap().contains("unreachable"));
    let quarantined = store
        .list_quarantined_campus_lab_sessions(TENANT)
        .await
        .unwrap();
    assert_eq!(quarantined.len(), 1);
    assert_eq!(quarantined[0].session_id, "s1");
    let slot = store.get_campus_lab_slot("slot-1").await.unwrap().unwrap();
    assert_eq!(slot.occupant_session_id.as_deref(), Some("s1"));
    assert_eq!(fake.resources().len(), 2);
    assert_eq!(
        event_kinds(&store, "s1").await.last().map(String::as_str),
        Some("campus.session.cleanup_blocked")
    );
    assert_eq!(
        operations(&store, "s1").await.last().unwrap().status,
        CampusLabOperationStatus::Blocked
    );

    // Once the node is reachable again an explicit retry completes cleanup.
    assert!(matches!(
        retry_campus_lab_cleanup(&store, &fake, &context("operator", 200), "s1")
            .await
            .unwrap(),
        CampusStepOutcome::Advanced {
            state: State::Terminated,
            ..
        }
    ));
    assert!(fake.resources().is_empty());
    assert!(store
        .list_quarantined_campus_lab_sessions(TENANT)
        .await
        .unwrap()
        .is_empty());
    let statuses: Vec<_> = operations(&store, "s1")
        .await
        .into_iter()
        .map(|operation| operation.status)
        .collect();
    assert_eq!(
        statuses,
        [
            CampusLabOperationStatus::Succeeded,
            CampusLabOperationStatus::Blocked,
            CampusLabOperationStatus::Succeeded,
        ]
    );
}

#[tokio::test]
async fn partial_or_unconfirmed_deletion_is_quarantined() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1", "slot-2"], &["s1", "s2"], 100).await;
    let fake = FakeCampusProvider::new();
    for id in ["s1", "s2"] {
        assert_eq!(settle(&store, &fake, "w", 101, id).await, State::Ready);
        stop(&store, id, 102).await;
        advance(&store, &fake, "w", 103, id).await;
    }
    fake.inject(FakeFaultPoint::MidDelete, FakeFault::Unreachable);
    advance(&store, &fake, "w", 104, "s1").await;
    let s1 = session(&store, "s1").await;
    assert_eq!(s1.state, State::CleanupBlocked);
    assert_eq!(fake.resources_for(&s1.workload().unwrap()).len(), 1);

    // The provider says deleted, but the inventory cannot confirm it.
    fake.inject(FakeFaultPoint::ListOwned, FakeFault::Unreachable);
    advance(&store, &fake, "w", 104, "s2").await;
    let s2 = session(&store, "s2").await;
    assert_eq!(s2.state, State::CleanupBlocked);
    assert!(s2
        .blocked_reason
        .unwrap()
        .contains("deletion could not be confirmed"));
    assert_eq!(
        store
            .list_quarantined_campus_lab_sessions(TENANT)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn a_rejected_launch_is_cleaned_up_and_ends_failed() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1"], &["s1"], 100).await;
    let fake = FakeCampusProvider::new();
    fake.inject(
        FakeFaultPoint::BeforeProvision,
        FakeFault::Reject("no qualified runtime class".into()),
    );
    advance(&store, &fake, "w", 101, "s1").await;
    assert!(matches!(
        advance(&store, &fake, "w", 102, "s1").await,
        CampusStepOutcome::Advanced {
            state: State::Stopping,
            ..
        }
    ));
    assert_eq!(
        operations(&store, "s1").await[0].status,
        CampusLabOperationStatus::Failed
    );
    assert_eq!(settle(&store, &fake, "w", 103, "s1").await, State::Failed);
    let failed = session(&store, "s1").await;
    assert!(failed
        .failure_reason
        .unwrap()
        .contains("no qualified runtime class"));
    assert!(failed.revoked_at.is_some());
    assert_eq!(
        store
            .get_campus_lab_slot("slot-1")
            .await
            .unwrap()
            .unwrap()
            .occupant_session_id,
        None
    );
}

#[tokio::test]
async fn without_central_authority_launches_are_denied_but_admitted_work_runs_until_expiry() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1", "slot-2"], &["s1", "s2"], 100).await;
    let fake = FakeCampusProvider::new();
    assert_eq!(settle(&store, &fake, "w", 101, "s1").await, State::Ready);
    let offline = |now| CampusDriverContext {
        central_authority: false,
        ..context("w", now)
    };

    advance(&store, &fake, "w", 102, "s2").await;
    assert!(matches!(
        advance_campus_lab_session(&store, &fake, &offline(103), "s2")
            .await
            .unwrap(),
        CampusStepOutcome::Advanced {
            state: State::Failed,
            ..
        }
    ));
    let denied = session(&store, "s2").await;
    assert_eq!(
        denied.failure_reason.as_deref(),
        Some(CENTRAL_AUTHORITY_UNAVAILABLE)
    );
    assert_eq!(denied.slot_id, None);
    assert_eq!(fake.workspaces_created(), 1);

    assert_eq!(
        advance_campus_lab_session(&store, &fake, &offline(200), "s1")
            .await
            .unwrap(),
        CampusStepOutcome::Observed {
            observed: CampusLabObservedState::Ready
        }
    );
    fake.inject(FakeFaultPoint::Observe, FakeFault::Unreachable);
    assert_eq!(
        advance_campus_lab_session(&store, &fake, &offline(201), "s1")
            .await
            .unwrap(),
        CampusStepOutcome::Observed {
            observed: CampusLabObservedState::Unreachable
        }
    );
    assert_eq!(session(&store, "s1").await.state, State::Ready);

    let expiry = 100 + TTL;
    assert!(matches!(
        advance_campus_lab_session(&store, &fake, &offline(expiry), "s1")
            .await
            .unwrap(),
        CampusStepOutcome::Advanced {
            state: State::Stopping,
            ..
        }
    ));
    let events = store.list_campus_lab_session_events("s1").await.unwrap();
    let stop = events
        .iter()
        .find(|event| event.kind == "campus.session.stop_requested")
        .unwrap();
    assert_eq!(stop.payload["reason"], "absolute expiry reached");
    for _ in 0..3 {
        advance_campus_lab_session(&store, &fake, &offline(expiry + 1), "s1")
            .await
            .unwrap();
    }
    assert_eq!(session(&store, "s1").await.state, State::Terminated);
    assert!(fake.resources().is_empty());
}

#[tokio::test]
async fn a_session_without_a_free_slot_waits_in_authorizing() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1"], &["s1", "s2"], 100).await;
    let fake = FakeCampusProvider::new();
    assert_eq!(settle(&store, &fake, "w", 101, "s1").await, State::Ready);
    advance(&store, &fake, "w", 102, "s2").await;
    let waiting = advance(&store, &fake, "w", 103, "s2").await;
    assert!(
        matches!(&waiting, CampusStepOutcome::Waiting { state: State::Authorizing, reason }
            if reason.contains("no free lab slot")),
        "{waiting:?}"
    );
    assert!(operations(&store, "s2").await.is_empty());
    stop(&store, "s1", 104).await;
    assert_eq!(
        settle(&store, &fake, "w", 105, "s1").await,
        State::Terminated
    );
    assert_eq!(settle(&store, &fake, "w", 106, "s2").await, State::Ready);
    assert_eq!(session(&store, "s2").await.slot_generation, Some(2));
}

#[tokio::test]
async fn a_lost_workspace_is_torn_down_as_failed() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1"], &["s1"], 100).await;
    let fake = FakeCampusProvider::new();
    assert_eq!(settle(&store, &fake, "w", 101, "s1").await, State::Ready);
    let workspace = session(&store, "s1").await.workspace_id.unwrap();
    assert!(fake.lose_resource(&workspace));
    assert!(matches!(
        advance(&store, &fake, "w", 102, "s1").await,
        CampusStepOutcome::Advanced {
            state: State::Stopping,
            ..
        }
    ));
    let stopping = session(&store, "s1").await;
    assert_eq!(stopping.observed_state, CampusLabObservedState::Absent);
    assert!(stopping.failure_reason.unwrap().contains("workspace lost"));
    assert_eq!(settle(&store, &fake, "w", 103, "s1").await, State::Failed);
    assert!(fake.resources().is_empty());
}

#[tokio::test]
async fn a_stop_during_provisioning_waits_for_the_live_lease_then_cleans_up() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1"], &["s1"], 100).await;
    let fake = Arc::new(FakeCampusProvider::new());
    advance(&store, &*fake, "w", 101, "s1").await;
    fake.inject(FakeFaultPoint::AfterProvision, FakeFault::Pause);
    let task = spawn_step(&store, &fake, "worker-a", 102, "s1");
    fake.wait_for_pause().await;
    crash(task).await;
    stop(&store, "s1", 103).await;

    assert!(matches!(
        advance(&store, &*fake, "worker-b", 110, "s1").await,
        CampusStepOutcome::LeaseHeld { .. }
    ));
    assert!(matches!(
        advance(&store, &*fake, "worker-b", 102 + LEASE, "s1").await,
        CampusStepOutcome::Advanced {
            state: State::Stopping,
            ..
        }
    ));
    assert_eq!(
        operations(&store, "s1").await[0].status,
        CampusLabOperationStatus::Abandoned
    );
    assert_eq!(
        settle(&store, &*fake, "worker-b", 140, "s1").await,
        State::Terminated
    );
    assert!(fake.resources().is_empty());
    assert_eq!(fake.workspaces_created(), 1);
}

#[tokio::test]
async fn an_in_progress_provision_releases_its_lease_and_retries_with_the_same_key() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1"], &["s1"], 100).await;
    let fake = FakeCampusProvider::new();
    advance(&store, &fake, "w", 101, "s1").await;
    fake.inject(FakeFaultPoint::AfterProvision, FakeFault::InProgress);
    assert!(matches!(
        advance(&store, &fake, "w", 102, "s1").await,
        CampusStepOutcome::Waiting {
            state: State::Provisioning,
            ..
        }
    ));
    let pending = store
        .pending_campus_lab_operation("s1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending.lease_owner, None);
    assert!(pending.last_error.unwrap().contains("still provisioning"));
    assert!(matches!(
        advance(&store, &fake, "other", 103, "s1").await,
        CampusStepOutcome::Advanced {
            state: State::Ready,
            ..
        }
    ));
    assert_eq!(fake.workspaces_created(), 1);
    assert_eq!(operations(&store, "s1").await[0].attempt_count, 2);
}

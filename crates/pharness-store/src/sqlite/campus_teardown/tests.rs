use crate::campus_journal::test_support::{profile, seed, student, ISSUER, TENANT};
use crate::{
    advance_campus_lab_session, campus_cleanup_retry_at, reconcile_campus_lab_orphans,
    retry_campus_lab_cleanup, AdmitCampusLabLaunch, CampusDriverContext, CampusJournalError,
    CampusLabAdmission, CampusLabLimits, CampusLabRejection, CampusLabSlotCondition,
    CampusLabSlotHoldKind, CampusOrphanTracker, CampusStepOutcome, SqliteStore,
    CAMPUS_JANITOR_ACTOR, CAMPUS_ORPHAN_GRACE_SECONDS,
};
use pharness_core::{
    CampusExecutionProvider, CampusLabSessionState as State, CampusProviderError,
    CampusProvisionSpec, CampusTeardownHook, CampusWorkloadRef, FakeCampusProvider, FakeFault,
    FakeFaultPoint, FakeTeardownHook,
};
use std::sync::Arc;

mod migration;

const T0: i64 = 1_000;

fn context<'a>(
    worker: &'a str,
    now: i64,
    hooks: &'a [Arc<dyn CampusTeardownHook>],
) -> CampusDriverContext<'a> {
    CampusDriverContext {
        worker_id: worker,
        now,
        lease_seconds: 30,
        central_authority: true,
        teardown_hooks: hooks,
    }
}

/// Step until the session stops advancing; return its state.
async fn settle(
    store: &SqliteStore,
    provider: &dyn CampusExecutionProvider,
    hooks: &[Arc<dyn CampusTeardownHook>],
    now: i64,
    session_id: &str,
) -> State {
    for _ in 0..12 {
        match advance_campus_lab_session(store, provider, &context("w", now, hooks), session_id)
            .await
            .unwrap()
        {
            CampusStepOutcome::Advanced { .. } => {}
            _ => break,
        }
    }
    state(store, session_id).await
}

async fn state(store: &SqliteStore, session_id: &str) -> State {
    store
        .get_campus_lab_session(session_id)
        .await
        .unwrap()
        .unwrap()
        .state
}

async fn stop(store: &SqliteStore, session_id: &str, now: i64) {
    store
        .request_campus_lab_session_stop(session_id, "student", "finished", now)
        .await
        .unwrap();
}

fn workload(session_id: &str, slot_id: &str, generation: i64) -> CampusWorkloadRef {
    CampusWorkloadRef {
        session_id: session_id.into(),
        slot_id: slot_id.into(),
        slot_generation: generation,
    }
}

/// Create provider resources for a workload the journal may not know.
async fn plant(provider: &FakeCampusProvider, workload: CampusWorkloadRef) {
    let spec = CampusProvisionSpec {
        workload: workload.clone(),
        tenant_id: TENANT.into(),
        course_id: "course-101".into(),
        profile: profile(),
        expires_at: T0 + 3_600,
    };
    let op_key = format!(
        "planted/{}/{}",
        workload.session_id, workload.slot_generation
    );
    provider.provision(&op_key, &spec).await.unwrap();
}

fn admission(session: &str, subject: &str, slots: i64, now: i64) -> AdmitCampusLabLaunch {
    AdmitCampusLabLaunch {
        tenant_id: TENANT.into(),
        course_id: "course-101".into(),
        assignment_id: "lab-1".into(),
        issuer: ISSUER.into(),
        subject: subject.into(),
        idempotency_key: None,
        reset_from_session_id: None,
        candidate_lab_id: format!("lab-{session}"),
        candidate_budget_id: format!("budget-{session}"),
        candidate_session_id: session.into(),
        profile: profile(),
        effective_policy_hash: "sha256:policy".into(),
        expires_at: now + 3_600,
        actor: format!("student:{subject}"),
        now,
        limits: CampusLabLimits {
            lab_slots: slots,
            max_active_per_identity: 1,
            max_active_per_course: 10,
        },
    }
}

#[test]
fn blocked_cleanup_retries_back_off_and_are_capped() {
    assert_eq!(campus_cleanup_retry_at(100, 0), 130);
    assert_eq!(campus_cleanup_retry_at(100, 1), 130);
    assert_eq!(campus_cleanup_retry_at(100, 2), 160);
    assert_eq!(campus_cleanup_retry_at(100, 3), 220);
    assert_eq!(campus_cleanup_retry_at(100, 5), 580);
    assert_eq!(campus_cleanup_retry_at(100, 6), 700);
    assert_eq!(campus_cleanup_retry_at(100, 60), 700);
    assert_eq!(campus_cleanup_retry_at(i64::MAX - 1, 3), i64::MAX);
}

#[tokio::test]
async fn held_slots_are_never_allocated_and_do_not_count_as_capacity() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1", "slot-2"], &["s1", "s2"], T0).await;
    let fake = FakeCampusProvider::new();
    let held = store
        .hold_campus_lab_slot(
            "slot-1",
            TENANT,
            CampusLabSlotHoldKind::Quarantined,
            "probe",
            "operator",
            T0,
        )
        .await
        .unwrap()
        .expect("new hold");
    assert_eq!(held.kind, CampusLabSlotHoldKind::Quarantined);
    // A second hold on the same slot is not stacked.
    assert!(store
        .hold_campus_lab_slot(
            "slot-1",
            TENANT,
            CampusLabSlotHoldKind::Retired,
            "again",
            "operator",
            T0,
        )
        .await
        .unwrap()
        .is_none());

    assert_eq!(settle(&store, &fake, &[], T0 + 1, "s1").await, State::Ready);
    let s1 = store.get_campus_lab_session("s1").await.unwrap().unwrap();
    assert_eq!(s1.slot_id.as_deref(), Some("slot-2"));
    assert!(matches!(
        advance_campus_lab_session(&store, &fake, &context("w", T0 + 2, &[]), "s2")
            .await
            .unwrap(),
        CampusStepOutcome::Advanced {
            state: State::Authorizing,
            ..
        }
    ));
    assert!(matches!(
        advance_campus_lab_session(&store, &fake, &context("w", T0 + 2, &[]), "s2")
            .await
            .unwrap(),
        CampusStepOutcome::Waiting { reason, .. } if reason.contains("no free lab slot")
    ));

    // Admission counts the held, empty slot out of the tenant's capacity.
    student(&store, "carol").await;
    assert_eq!(
        store
            .admit_campus_lab_launch(admission("s3", "carol", 2, T0 + 3))
            .await
            .unwrap(),
        CampusLabAdmission::Rejected(CampusLabRejection::NoSlotsAvailable)
    );
    assert!(store
        .release_campus_lab_slot_hold(
            "slot-1",
            CampusLabSlotHoldKind::Quarantined,
            None,
            "operator",
            "inspected",
            T0 + 4,
        )
        .await
        .unwrap());
    assert_eq!(settle(&store, &fake, &[], T0 + 5, "s2").await, State::Ready);
    let history = store
        .list_campus_lab_slot_hold_history("slot-1")
        .await
        .unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].released_at, Some(T0 + 4));
    assert_eq!(history[0].release_reason.as_deref(), Some("inspected"));
}

#[tokio::test]
async fn holds_change_only_by_one_release_and_are_never_deleted() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1"], &[], T0).await;
    store
        .hold_campus_lab_slot(
            "slot-1",
            TENANT,
            CampusLabSlotHoldKind::Quarantined,
            "probe",
            "operator",
            T0,
        )
        .await
        .unwrap();
    for statement in [
        "UPDATE campus_lab_slot_holds SET reason = 'other'",
        "UPDATE campus_lab_slot_holds SET kind = 'retired'",
        "DELETE FROM campus_lab_slot_holds",
    ] {
        assert!(
            sqlx::query(statement).execute(&store.pool).await.is_err(),
            "{statement}"
        );
    }
    let release = |at| {
        store.release_campus_lab_slot_hold(
            "slot-1",
            CampusLabSlotHoldKind::Quarantined,
            None,
            "operator",
            "done",
            at,
        )
    };
    assert!(release(T0 + 1).await.unwrap());
    assert!(!release(T0 + 2).await.unwrap());
    assert!(
        sqlx::query("UPDATE campus_lab_slot_holds SET released_at = 5")
            .execute(&store.pool)
            .await
            .is_err()
    );
    // Another tenant's slot cannot be held.
    assert!(matches!(
        store
            .hold_campus_lab_slot(
                "slot-1",
                "tenant-b",
                CampusLabSlotHoldKind::Quarantined,
                "x",
                "operator",
                T0
            )
            .await,
        Err(CampusJournalError::Conflict(_))
    ));
}

#[tokio::test]
async fn slot_sync_retires_unconfigured_slots_after_their_occupant_leaves() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["tenant-a-lab-slot-01"], &["s1"], T0).await;
    let fake = FakeCampusProvider::new();
    assert_eq!(settle(&store, &fake, &[], T0, "s1").await, State::Ready);

    let configured = ["campus-lab-slot-1".to_string()];
    let sync = store
        .sync_campus_lab_slots(TENANT, &configured, "api", T0 + 1)
        .await
        .unwrap();
    assert_eq!(sync.registered, ["campus-lab-slot-1"]);
    assert_eq!(sync.retired, ["tenant-a-lab-slot-01"]);
    assert!(sync.restored.is_empty());
    // Idempotent.
    let again = store
        .sync_campus_lab_slots(TENANT, &configured, "api", T0 + 2)
        .await
        .unwrap();
    assert_eq!(again, Default::default());

    // The old slot's occupant keeps running and tears down normally.
    assert_eq!(
        store
            .get_campus_lab_session("s1")
            .await
            .unwrap()
            .unwrap()
            .state,
        State::Ready
    );
    stop(&store, "s1", T0 + 3).await;
    assert_eq!(
        settle(&store, &fake, &[], T0 + 3, "s1").await,
        State::Terminated
    );
    // New sessions only ever land in configured slots.
    seed(&store, &[], &["s2"], T0 + 4).await;
    assert_eq!(settle(&store, &fake, &[], T0 + 4, "s2").await, State::Ready);
    let s2 = store.get_campus_lab_session("s2").await.unwrap().unwrap();
    assert_eq!(s2.slot_id.as_deref(), Some("campus-lab-slot-1"));
    let metrics = store.campus_lab_metrics(T0 + 5).await.unwrap();
    assert_eq!(metrics[0].slots[&CampusLabSlotCondition::Retired], 1);
    assert_eq!(metrics[0].slots[&CampusLabSlotCondition::Occupied], 1);

    let restored = store
        .sync_campus_lab_slots(
            TENANT,
            &[
                "campus-lab-slot-1".to_string(),
                "tenant-a-lab-slot-01".to_string(),
            ],
            "api",
            T0 + 6,
        )
        .await
        .unwrap();
    assert_eq!(restored.restored, ["tenant-a-lab-slot-01"]);
}

#[tokio::test]
async fn the_janitor_quarantines_then_deletes_unknown_workloads_and_releases_the_slot() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1", "slot-2"], &["live"], T0).await;
    let fake = FakeCampusProvider::new();
    assert_eq!(settle(&store, &fake, &[], T0, "live").await, State::Ready);
    let live = store
        .get_campus_lab_session("live")
        .await
        .unwrap()
        .unwrap()
        .workload()
        .unwrap();
    // Leftovers of a session the journal never recorded, in the free slot,
    // and of an older occupancy of the live session's own slot.
    plant(&fake, workload("ghost", "slot-2", 3)).await;
    plant(&fake, workload("live", &live.slot_id, 9)).await;
    let mut tracker = CampusOrphanTracker::default();

    let first = reconcile_campus_lab_orphans(&store, &fake, TENANT, T0 + 10, &mut tracker)
        .await
        .unwrap();
    assert_eq!(first.orphaned_workloads, 2);
    assert_eq!(first.deleted, 0);
    assert_eq!(first.pending, 2);
    assert_eq!(
        first.slots_quarantined,
        [live.slot_id.clone(), "slot-2".into()]
    );
    assert!(!fake.calls().iter().any(|call| call.starts_with("delete")));
    // Quarantined at once: no new session lands next to the leftovers.
    seed(&store, &[], &["next"], T0 + 11).await;
    assert_eq!(
        settle(&store, &fake, &[], T0 + 11, "next").await,
        State::Authorizing
    );

    let later = T0 + 10 + CAMPUS_ORPHAN_GRACE_SECONDS;
    let second = reconcile_campus_lab_orphans(&store, &fake, TENANT, later, &mut tracker)
        .await
        .unwrap();
    assert_eq!(second.deleted, 2);
    assert!(second.slots_released.is_empty(), "released on a clean pass");
    // Only the orphans went; the live workspace is untouched.
    assert_eq!(fake.resources_for(&live).len(), 2);
    assert_eq!(fake.resources().len(), 2);

    let third = reconcile_campus_lab_orphans(&store, &fake, TENANT, later + 30, &mut tracker)
        .await
        .unwrap();
    assert_eq!(third.orphaned_workloads, 0);
    assert_eq!(
        third.slots_released,
        [live.slot_id.clone(), "slot-2".into()]
    );
    assert!(store
        .list_campus_lab_slot_holds(TENANT)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        settle(&store, &fake, &[], later + 31, "next").await,
        State::Ready
    );
}

#[tokio::test]
async fn an_orphan_that_cannot_be_removed_keeps_its_slot_quarantined() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1"], &[], T0).await;
    let fake = FakeCampusProvider::new();
    plant(&fake, workload("ghost", "slot-1", 1)).await;
    let mut tracker = CampusOrphanTracker::default();
    reconcile_campus_lab_orphans(&store, &fake, TENANT, T0, &mut tracker)
        .await
        .unwrap();

    let due = T0 + CAMPUS_ORPHAN_GRACE_SECONDS;
    fake.inject(
        FakeFaultPoint::BeforeDelete,
        FakeFault::Blocked("finalizer example.com/hold".into()),
    );
    let blocked = reconcile_campus_lab_orphans(&store, &fake, TENANT, due, &mut tracker)
        .await
        .unwrap();
    assert_eq!(blocked.deleted, 0);
    assert!(blocked.errors[0].contains("finalizer"), "{blocked:?}");
    fake.inject(FakeFaultPoint::BeforeDelete, FakeFault::Unreachable);
    let unreachable = reconcile_campus_lab_orphans(&store, &fake, TENANT, due + 30, &mut tracker)
        .await
        .unwrap();
    assert!(
        unreachable.errors[0].contains("unreachable") || unreachable.errors[0].contains("injected")
    );
    // An inventory that cannot be read releases nothing.
    fake.inject(FakeFaultPoint::ListOwned, FakeFault::Unreachable);
    let blind = reconcile_campus_lab_orphans(&store, &fake, TENANT, due + 60, &mut tracker)
        .await
        .unwrap();
    assert!(blind.errors[0].contains("inventory unavailable"));
    assert_eq!(
        store
            .list_campus_lab_slot_holds(TENANT)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(fake.resources().len(), 2, "nothing was forced");
    let metrics = store.campus_lab_metrics(due + 60).await.unwrap();
    assert_eq!(metrics[0].slots[&CampusLabSlotCondition::Quarantined], 1);
    assert_eq!(metrics[0].oldest_quarantined_at, Some(T0));

    // The blocker clears: the next passes delete, then release.
    reconcile_campus_lab_orphans(&store, &fake, TENANT, due + 90, &mut tracker)
        .await
        .unwrap();
    let clean = reconcile_campus_lab_orphans(&store, &fake, TENANT, due + 120, &mut tracker)
        .await
        .unwrap();
    assert_eq!(clean.slots_released, ["slot-1"]);
    assert!(fake.resources().is_empty());
}

#[tokio::test]
async fn the_janitor_never_touches_foreign_slots_or_operator_holds() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1", "slot-2"], &[], T0).await;
    store
        .register_campus_lab_slot("other-1", "tenant-b", T0)
        .await
        .unwrap();
    store
        .hold_campus_lab_slot(
            "slot-2",
            TENANT,
            CampusLabSlotHoldKind::Quarantined,
            "disk check",
            "operator",
            T0,
        )
        .await
        .unwrap();
    let fake = FakeCampusProvider::new();
    plant(&fake, workload("theirs", "other-1", 1)).await;
    plant(&fake, workload("unregistered", "nowhere", 1)).await;
    let mut tracker = CampusOrphanTracker::default();
    for now in [T0, T0 + 100, T0 + 200] {
        let report = reconcile_campus_lab_orphans(&store, &fake, TENANT, now, &mut tracker)
            .await
            .unwrap();
        assert_eq!(report.foreign, 2);
        assert!(report.slots_released.is_empty());
    }
    assert!(!fake.calls().iter().any(|call| call.starts_with("delete")));
    let holds = store.list_campus_lab_slot_holds(TENANT).await.unwrap();
    assert_eq!(holds.len(), 1);
    assert_eq!(holds[0].held_by, "operator");
    assert_ne!(holds[0].held_by, CAMPUS_JANITOR_ACTOR);
}

#[tokio::test]
async fn teardown_hooks_run_once_after_the_workload_is_gone() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1"], &["s1"], T0).await;
    let fake = FakeCampusProvider::new();
    let terminal = Arc::new(FakeTeardownHook::new("terminal"));
    let volume = Arc::new(FakeTeardownHook::new("volume"));
    let hooks: Vec<Arc<dyn CampusTeardownHook>> = vec![terminal.clone(), volume.clone()];
    assert_eq!(settle(&store, &fake, &hooks, T0, "s1").await, State::Ready);
    assert!(terminal.calls().is_empty());

    // The second hook fails once; the first is not repeated on the retry.
    volume.fail_next(CampusProviderError::Internal("busy".into()));
    stop(&store, "s1", T0 + 1).await;
    assert_eq!(
        settle(&store, &fake, &hooks, T0 + 1, "s1").await,
        State::Cleaning
    );
    assert!(fake.resources().is_empty(), "hooks run after the provider");
    assert_eq!(terminal.calls().len(), 1);
    assert_eq!(volume.calls().len(), 1);
    assert_eq!(
        store.list_campus_teardown_hook_runs("s1").await.unwrap(),
        ["terminal"]
    );
    assert_eq!(
        settle(&store, &fake, &hooks, T0 + 40, "s1").await,
        State::Terminated
    );
    assert_eq!(terminal.calls().len(), 1);
    assert_eq!(volume.calls().len(), 2);
    let mut runs = store.list_campus_teardown_hook_runs("s1").await.unwrap();
    runs.sort();
    assert_eq!(runs, ["terminal", "volume"]);
    // Replays of a terminal session call nothing.
    settle(&store, &fake, &hooks, T0 + 50, "s1").await;
    assert_eq!(volume.calls().len(), 2);
    assert!(sqlx::query("DELETE FROM campus_teardown_hook_runs")
        .execute(&store.pool)
        .await
        .is_err());
}

#[tokio::test]
async fn an_unreachable_hook_quarantines_the_cleanup_until_a_retry() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1"], &["s1"], T0).await;
    let fake = FakeCampusProvider::new();
    let hook = Arc::new(FakeTeardownHook::new("volume"));
    let hooks: Vec<Arc<dyn CampusTeardownHook>> = vec![hook.clone()];
    settle(&store, &fake, &hooks, T0, "s1").await;
    hook.fail_next(CampusProviderError::Unreachable("storage node down".into()));
    stop(&store, "s1", T0 + 1).await;
    assert_eq!(
        settle(&store, &fake, &hooks, T0 + 1, "s1").await,
        State::CleanupBlocked
    );
    let blocked = store
        .list_campus_cleanup_blocked_sessions(TENANT)
        .await
        .unwrap();
    assert_eq!(blocked.len(), 1);
    assert_eq!(blocked[0].blocked_attempts, 1);
    assert!(blocked[0]
        .blocked_reason
        .as_deref()
        .unwrap()
        .contains("storage node down"));
    assert_eq!(blocked[0].retry_at(), T0 + 1 + 30);
    // The slot is still occupied while quarantined.
    let metrics = store.campus_lab_metrics(T0 + 2).await.unwrap();
    assert_eq!(metrics[0].slots[&CampusLabSlotCondition::CleanupBlocked], 1);
    assert_eq!(metrics[0].sessions[&State::CleanupBlocked], 1);

    retry_campus_lab_cleanup(&store, &fake, &context("w", T0 + 31, &hooks), "s1")
        .await
        .unwrap();
    assert_eq!(state(&store, "s1").await, State::Terminated);
    assert_eq!(hook.calls().len(), 2);
    let metrics = store.campus_lab_metrics(T0 + 32).await.unwrap();
    assert_eq!(metrics[0].slots[&CampusLabSlotCondition::Free], 1);
    assert!(metrics[0].sessions.is_empty());
    assert_eq!(metrics[0].oldest_quarantined_at, None);
}

#[tokio::test]
async fn metrics_count_expired_sessions_that_are_still_live() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1"], &["s1"], T0).await;
    let fake = FakeCampusProvider::new();
    settle(&store, &fake, &[], T0, "s1").await;
    let expires_at = store
        .get_campus_lab_session("s1")
        .await
        .unwrap()
        .unwrap()
        .expires_at;
    assert_eq!(
        store.campus_lab_metrics(expires_at - 1).await.unwrap()[0].expired_live,
        0
    );
    assert_eq!(
        store.campus_lab_metrics(expires_at).await.unwrap()[0].expired_live,
        1
    );
    assert_eq!(
        settle(&store, &fake, &[], expires_at, "s1").await,
        State::Terminated
    );
    assert_eq!(
        store.campus_lab_metrics(expires_at).await.unwrap()[0].expired_live,
        0
    );
}

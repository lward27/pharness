//! Campus teardown through the reconciler (LUC-133): expiry first, blocked
//! cleanup backoff, the orphan janitor, slot registration, restarts, and a
//! long run of create, expire and stop cycles, on the fake provider.

use super::super::labs::{CampusLabs, JANITOR_INTERVAL_SECONDS};
use super::broker::{denial, issue, ready_session, workload};
use super::labs::{act, catalog_value, enroll_students, id, labs_harness, launch, launched, parse};
use super::{body_json, Harness, Session, TENANT};
use axum::http::StatusCode;
use pharness_core::campus_broker_token::CampusBrokerAudience as Audience;
use pharness_core::{
    CampusExecutionProvider, CampusLabSessionState as State, CampusProviderError,
    CampusProvisionSpec, CampusTeardownHook, CampusWorkloadRef, FakeFault, FakeFaultPoint,
    FakeTeardownHook,
};
use pharness_store::{
    CampusLabSlotCondition, CampusLabSlotHoldKind, StoredCampusLabSession,
    CAMPUS_ORPHAN_GRACE_SECONDS,
};
use serde_json::json;
use std::sync::Arc;

/// A harness whose catalog lists the M1 slot namespaces as its slot ids, as
/// the Campus PoC catalog does for the Kubernetes provider.
async fn m1_harness(slots: usize) -> Harness {
    let mut catalog = catalog_value(slots as i64);
    catalog["lab_slot_ids"] = json!((1..=slots)
        .map(|index| format!("campus-lab-slot-{index}"))
        .collect::<Vec<_>>());
    Harness::with_labs(parse(&catalog).unwrap(), true, false).await
}

async fn stored(harness: &Harness, session_id: &str) -> StoredCampusLabSession {
    harness
        .store
        .get_campus_lab_session(session_id)
        .await
        .unwrap()
        .unwrap()
}

async fn slots_in(harness: &Harness, condition: CampusLabSlotCondition) -> i64 {
    harness
        .store
        .campus_lab_metrics(harness.now())
        .await
        .unwrap()
        .iter()
        .find(|tenant| tenant.tenant_id == TENANT)
        .and_then(|tenant| tenant.slots.get(&condition).copied())
        .unwrap_or(0)
}

#[tokio::test]
async fn an_expired_session_is_stopped_first_and_loses_its_tokens_before_teardown() {
    let harness = labs_harness(2).await;
    enroll_students(&harness, "cs101", &["alice"]).await;
    let alice = harness.login("alice").await;
    let session_id = ready_session(&harness, &alice, "lab-2").await;
    let token = issue(&harness, &session_id, Audience::Inference).await;
    assert_eq!(denial(&harness, &token).await, None);
    let expires_at = stored(&harness, &session_id).await.expires_at;

    harness
        .provider
        .inject(FakeFaultPoint::BeforeDelete, FakeFault::InProgress);
    harness.clock.advance(expires_at - harness.now());
    let sweep = harness.reconcile().await;
    assert_eq!(sweep.expired, 1);
    // Revoked in the same sweep, while the workload is still being removed.
    assert!(denial(&harness, &token).await.is_some());
    let issued = harness
        .store
        .list_campus_broker_tokens(&session_id)
        .await
        .unwrap();
    assert_eq!(issued.len(), 1);
    assert_eq!(issued[0].revoked_at, Some(expires_at));
    let cleaning = stored(&harness, &session_id).await;
    assert_eq!(cleaning.state, State::Cleaning);
    assert_eq!(cleaning.expires_at, expires_at, "expiry is never extended");
    assert!(!harness.provider.resources().is_empty());

    harness.reconcile().await;
    assert_eq!(stored(&harness, &session_id).await.state, State::Terminated);
    assert!(harness.provider.resources().is_empty());
}

#[tokio::test]
async fn a_blocked_cleanup_holds_its_slot_and_is_retried_with_backoff() {
    let harness = labs_harness(1).await;
    enroll_students(&harness, "cs101", &["alice", "bob"]).await;
    let alice = harness.login("alice").await;
    let bob = harness.login("bob").await;
    let session_id = ready_session(&harness, &alice, "lab-1").await;
    // A node that stopped answering, then a finalizer nobody removes.
    harness
        .provider
        .inject(FakeFaultPoint::BeforeDelete, FakeFault::Unreachable);
    harness.provider.inject(
        FakeFaultPoint::BeforeDelete,
        FakeFault::Blocked("Job held by finalizer example.com/hold".into()),
    );
    assert!(act(&harness, &alice, &session_id, "stop")
        .await
        .status()
        .is_success());
    harness.reconcile().await;
    assert_eq!(
        stored(&harness, &session_id).await.state,
        State::CleanupBlocked
    );
    assert_eq!(
        slots_in(&harness, CampusLabSlotCondition::CleanupBlocked).await,
        1
    );
    // The slot is not reused while its cleanup is unconfirmed.
    let refused = launch(&harness, &bob, "lab-1", json!({})).await;
    assert_eq!(refused.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body_json(refused).await["code"], "no_slots_available");

    harness.clock.advance(29);
    assert_eq!(harness.reconcile().await.cleanup_retries, 0);
    harness.clock.advance(1);
    assert_eq!(harness.reconcile().await.cleanup_retries, 1);
    let blocked = harness
        .store
        .list_campus_cleanup_blocked_sessions(TENANT)
        .await
        .unwrap();
    assert_eq!(blocked.len(), 1);
    assert_eq!(blocked[0].blocked_attempts, 2);
    assert!(blocked[0]
        .blocked_reason
        .as_deref()
        .unwrap()
        .contains("finalizer"));
    assert_eq!(blocked[0].retry_at(), harness.now() + 60);

    harness.clock.advance(59);
    assert_eq!(harness.reconcile().await.cleanup_retries, 0);
    harness.clock.advance(1);
    assert_eq!(harness.reconcile().await.cleanup_retries, 1);
    assert_eq!(stored(&harness, &session_id).await.state, State::Terminated);
    assert!(harness.provider.resources().is_empty());
    assert_eq!(
        slots_in(&harness, CampusLabSlotCondition::CleanupBlocked).await,
        0
    );
    assert_eq!(
        launch(&harness, &bob, "lab-1", json!({})).await.status(),
        StatusCode::CREATED
    );
}

/// Provider resources for a session this journal never recorded.
async fn plant(harness: &Harness, workload: CampusWorkloadRef) {
    let labs = harness.labs.clone().unwrap();
    let assignment = labs.catalog.assignment("cs101", "lab-1").unwrap();
    let spec = CampusProvisionSpec {
        workload,
        tenant_id: TENANT.into(),
        course_id: "cs101".into(),
        profile: assignment.policy.profile().clone(),
        expires_at: harness.now() + 3_600,
    };
    harness.provider.provision("planted", &spec).await.unwrap();
}

#[tokio::test]
async fn the_janitor_quarantines_leftovers_deletes_them_after_grace_and_releases_the_slot() {
    let harness = m1_harness(2).await;
    enroll_students(&harness, "cs101", &["alice", "bob"]).await;
    let alice = harness.login("alice").await;
    let bob = harness.login("bob").await;
    let live = ready_session(&harness, &alice, "lab-1").await;
    let live = workload(&harness, &live).await;
    assert_eq!(live.slot_id, "campus-lab-slot-1");
    plant(
        &harness,
        CampusWorkloadRef {
            session_id: "cls_lost_journal".into(),
            slot_id: "campus-lab-slot-2".into(),
            slot_generation: 1,
        },
    )
    .await;

    harness.clock.advance(JANITOR_INTERVAL_SECONDS);
    let seen = harness.reconcile().await;
    assert_eq!(seen.janitor_passes, 1);
    assert_eq!(seen.orphaned_workloads, 1);
    assert_eq!(seen.orphans_deleted, 0);
    let holds = harness
        .store
        .list_campus_lab_slot_holds(TENANT)
        .await
        .unwrap();
    assert_eq!(holds.len(), 1);
    assert_eq!(holds[0].slot_id, "campus-lab-slot-2");
    assert_eq!(holds[0].kind, CampusLabSlotHoldKind::Quarantined);
    assert_eq!(
        slots_in(&harness, CampusLabSlotCondition::Quarantined).await,
        1
    );
    let refused = launch(&harness, &bob, "lab-1", json!({})).await;
    assert_eq!(refused.status(), StatusCode::SERVICE_UNAVAILABLE);

    // Between passes nothing runs; at the grace period the orphan goes.
    harness.clock.advance(JANITOR_INTERVAL_SECONDS - 1);
    assert_eq!(harness.reconcile().await.janitor_passes, 0);
    harness
        .clock
        .advance(CAMPUS_ORPHAN_GRACE_SECONDS - JANITOR_INTERVAL_SECONDS + 1);
    let removed = harness.reconcile().await;
    assert_eq!(removed.orphans_deleted, 1);
    assert_eq!(harness.provider.resources_for(&live).len(), 2);
    assert_eq!(harness.provider.resources().len(), 2);
    assert_eq!(
        harness
            .store
            .list_campus_lab_slot_holds(TENANT)
            .await
            .unwrap()
            .len(),
        1
    );

    harness.clock.advance(JANITOR_INTERVAL_SECONDS);
    let clean = harness.reconcile().await;
    assert_eq!(clean.orphaned_workloads, 0);
    assert!(harness
        .store
        .list_campus_lab_slot_holds(TENANT)
        .await
        .unwrap()
        .is_empty());
    let next = launched(&harness, &bob, "lab-1").await;
    harness.reconcile().await;
    assert_eq!(
        workload(&harness, &id(&next)).await.slot_id,
        "campus-lab-slot-2"
    );
}

#[tokio::test]
async fn slots_are_the_m1_namespaces_and_unconfigured_ones_are_retired() {
    let harness = m1_harness(2).await;
    let labs = harness.labs.clone().unwrap();
    let slots = |harness: &Harness| {
        let store = harness.store.clone();
        async move {
            store
                .list_campus_lab_slots(TENANT)
                .await
                .unwrap()
                .into_iter()
                .map(|slot| slot.slot_id)
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(
        slots(&harness).await,
        ["campus-lab-slot-1", "campus-lab-slot-2"]
    );
    // A slot registered by an earlier release under the old naming.
    harness
        .store
        .register_campus_lab_slot("tenant-a-lab-slot-01", TENANT, harness.now())
        .await
        .unwrap();
    let sync = labs
        .register_slots(&harness.store, harness.now())
        .await
        .unwrap();
    assert_eq!(sync.retired, ["tenant-a-lab-slot-01"]);
    assert!(sync.registered.is_empty());
    assert_eq!(
        labs.register_slots(&harness.store, harness.now())
            .await
            .unwrap(),
        Default::default(),
        "registration is idempotent"
    );
    assert_eq!(slots_in(&harness, CampusLabSlotCondition::Retired).await, 1);

    enroll_students(&harness, "cs101", &["alice", "bob", "carol"]).await;
    for subject in ["alice", "bob"] {
        let student = harness.login(subject).await;
        let session_id = ready_session(&harness, &student, "lab-1").await;
        assert!(workload(&harness, &session_id)
            .await
            .slot_id
            .starts_with("campus-lab-slot-"));
    }
    let carol = harness.login("carol").await;
    assert_eq!(
        launch(&harness, &carol, "lab-1", json!({})).await.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "a retired slot adds no capacity"
    );
}

#[tokio::test]
async fn a_restarted_reconciler_finishes_teardown_without_leaks_or_a_new_expiry() {
    let harness = labs_harness(2).await;
    enroll_students(&harness, "cs101", &["alice", "bob"]).await;
    let alice = harness.login("alice").await;
    let bob = harness.login("bob").await;
    let labs = harness.labs.clone().unwrap();

    // Alice: a delete still in progress when the process dies, stopped twice.
    let ready = ready_session(&harness, &alice, "lab-1").await;
    let ready_expiry = stored(&harness, &ready).await.expires_at;
    harness
        .provider
        .inject(FakeFaultPoint::BeforeDelete, FakeFault::InProgress);
    for _ in 0..2 {
        assert!(act(&harness, &alice, &ready, "stop")
            .await
            .status()
            .is_success());
    }
    harness.reconcile().await;
    assert_eq!(stored(&harness, &ready).await.state, State::Cleaning);

    // Bob: a create that had made its Job, not yet its Pod.
    harness
        .provider
        .inject(FakeFaultPoint::AfterProvision, FakeFault::InProgress);
    let partial = id(&launched(&harness, &bob, "lab-2").await);
    let partial_expiry = stored(&harness, &partial).await.expires_at;
    harness.reconcile().await;
    assert_eq!(stored(&harness, &partial).await.state, State::Provisioning);
    assert!(act(&harness, &bob, &partial, "stop")
        .await
        .status()
        .is_success());

    // The old process is gone; a new one, with a new worker id, takes over
    // once the old leases run out.
    drop(labs.clone());
    let restarted = CampusLabs::with_shared_catalog(
        labs.catalog.clone(),
        Some(harness.provider.clone() as Arc<dyn CampusExecutionProvider>),
    );
    harness.clock.advance(121);
    for _ in 0..3 {
        restarted.reconcile(&harness.store, harness.now()).await;
    }
    for (session_id, expiry) in [(&ready, ready_expiry), (&partial, partial_expiry)] {
        let session = stored(&harness, session_id).await;
        assert_eq!(session.state, State::Terminated, "{session_id}");
        assert_eq!(session.expires_at, expiry, "{session_id}");
    }
    assert!(harness.provider.resources().is_empty());
    assert_eq!(harness.provider.workspaces_created(), 2);
    assert!(harness
        .store
        .list_campus_lab_slot_holds(TENANT)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn a_teardown_hook_runs_once_after_the_workload_is_gone_even_across_retries() {
    let harness = labs_harness(1).await;
    enroll_students(&harness, "cs101", &["alice"]).await;
    let alice = harness.login("alice").await;
    let catalog = harness.labs.clone().unwrap().catalog.clone();
    let hook = Arc::new(FakeTeardownHook::new("scratch-volume"));
    hook.fail_next(CampusProviderError::Internal(
        "volume API returned 500".into(),
    ));
    let labs = CampusLabs::with_shared_catalog(
        catalog,
        Some(harness.provider.clone() as Arc<dyn CampusExecutionProvider>),
    )
    .with_teardown_hooks(vec![hook.clone() as Arc<dyn CampusTeardownHook>]);
    let session_id = id(&launched(&harness, &alice, "lab-1").await);
    labs.reconcile(&harness.store, harness.now()).await;
    assert!(act(&harness, &alice, &session_id, "stop")
        .await
        .status()
        .is_success());

    labs.reconcile(&harness.store, harness.now()).await;
    assert_eq!(stored(&harness, &session_id).await.state, State::Cleaning);
    assert!(
        harness.provider.resources().is_empty(),
        "the hook runs after the delete"
    );
    harness.clock.advance(1);
    labs.reconcile(&harness.store, harness.now()).await;
    assert_eq!(stored(&harness, &session_id).await.state, State::Terminated);
    let calls = hook.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0], calls[1], "a retry replays the same key");
    harness.clock.advance(JANITOR_INTERVAL_SECONDS);
    labs.reconcile(&harness.store, harness.now()).await;
    assert_eq!(hook.calls().len(), 2);
    assert_eq!(
        harness
            .store
            .list_campus_teardown_hook_runs(&session_id)
            .await
            .unwrap(),
        ["scratch-volume"]
    );
}

async fn finish(harness: &Harness, session_id: &str) {
    for _ in 0..10 {
        if stored(harness, session_id).await.state.is_terminal() {
            return;
        }
        harness.clock.advance(31);
        harness.reconcile().await;
    }
    panic!("{session_id} did not finish");
}

/// Launch, then end the session by stop, by expiry or by a stop during
/// provisioning, with a fault at a different boundary each time.
#[tokio::test]
async fn a_hundred_create_expire_and_stop_cycles_leave_nothing_behind() {
    let harness = labs_harness(5).await;
    let students = ["s0", "s1", "s2", "s3", "s4"];
    enroll_students(&harness, "cs101", &students).await;
    let faults = [
        None,
        Some((FakeFaultPoint::BeforeDelete, FakeFault::InProgress)),
        Some((FakeFaultPoint::MidDelete, FakeFault::Unreachable)),
        Some((
            FakeFaultPoint::BeforeDelete,
            FakeFault::Blocked("held".into()),
        )),
        Some((FakeFaultPoint::ListOwned, FakeFault::Unreachable)),
        Some((FakeFaultPoint::AfterProvision, FakeFault::InProgress)),
    ];
    let mut tokens = Vec::new();
    for cycle in 0..100_usize {
        let student: Session = harness.login(students[cycle % students.len()]).await;
        let view = launched(&harness, &student, "lab-1").await;
        let session_id = id(&view);
        if let Some((point, fault)) = faults[cycle % faults.len()].clone() {
            harness.provider.inject(point, fault);
        }
        harness.reconcile().await;
        if stored(&harness, &session_id).await.state == State::Ready {
            tokens.push(issue(&harness, &session_id, Audience::Inference).await);
        }
        match cycle % 3 {
            0 => {
                assert!(act(&harness, &student, &session_id, "stop")
                    .await
                    .status()
                    .is_success());
            }
            1 => {
                let expires_at = stored(&harness, &session_id).await.expires_at;
                harness.clock.advance(expires_at - harness.now());
            }
            _ => {
                assert!(act(&harness, &student, &session_id, "stop")
                    .await
                    .status()
                    .is_success());
                assert!(act(&harness, &student, &session_id, "stop")
                    .await
                    .status()
                    .is_success());
            }
        }
        harness.reconcile().await;
        finish(&harness, &session_id).await;
    }
    harness.clock.advance(JANITOR_INTERVAL_SECONDS);
    let last = harness.reconcile().await;
    assert_eq!(last.orphaned_workloads, 0);
    assert!(harness.provider.resources().is_empty());
    assert_eq!(harness.provider.workspaces_created(), 100);
    assert!(harness
        .store
        .list_campus_lab_slot_holds(TENANT)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        harness
            .store
            .count_live_campus_lab_sessions(TENANT)
            .await
            .unwrap(),
        0
    );
    assert!(!tokens.is_empty());
    for token in &tokens {
        assert!(denial(&harness, token).await.is_some());
    }
    assert_eq!(slots_in(&harness, CampusLabSlotCondition::Free).await, 5);
}

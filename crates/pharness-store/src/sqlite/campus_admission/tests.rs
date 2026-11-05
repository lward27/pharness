use crate::campus_journal::test_support::{profile, student, DiskStore, ISSUER, TENANT};
use crate::{
    advance_campus_lab_session, AdmitCampusLabLaunch, CampusAssignmentPolicies,
    CampusDriverContext, CampusLabAdmission, CampusLabLimits, CampusLabRejection,
    CampusLabSessionRecord, JournalCampusAuthoritySource, SqliteStore,
};
use pharness_core::campus_capability::{
    CampusAuthoritySource, CampusAuthorityStatus, CampusBrokerClock,
};
use pharness_core::{
    CampusLabSessionState as State, CampusPolicyLayer, CampusProfile, EffectiveCampusPolicy,
    FakeCampusProvider, CAMPUS_POLICY_LAYER_SCHEMA,
};
use serde_json::json;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;

mod migration;

const NOW: i64 = 10_000;
const LIMITS: CampusLabLimits = CampusLabLimits {
    lab_slots: 3,
    max_active_per_identity: 1,
    max_active_per_course: 3,
};

fn next_id(prefix: &str) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!("{prefix}-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

fn launch(subject: &str, assignment: &str) -> AdmitCampusLabLaunch {
    AdmitCampusLabLaunch {
        tenant_id: TENANT.into(),
        course_id: "course-101".into(),
        assignment_id: assignment.into(),
        issuer: ISSUER.into(),
        subject: subject.into(),
        idempotency_key: None,
        reset_from_session_id: None,
        candidate_lab_id: next_id("lab"),
        candidate_budget_id: next_id("budget"),
        candidate_session_id: next_id("session"),
        profile: profile(),
        effective_policy_hash: "sha256:policy".into(),
        expires_at: NOW + 7_200,
        actor: format!("student:{subject}"),
        now: NOW,
        limits: LIMITS,
    }
}

fn keyed(subject: &str, assignment: &str, key: &str) -> AdmitCampusLabLaunch {
    AdmitCampusLabLaunch {
        idempotency_key: Some(key.into()),
        ..launch(subject, assignment)
    }
}

async fn slots(store: &SqliteStore, count: usize) {
    for index in 0..count {
        store
            .register_campus_lab_slot(&format!("slot-{index}"), TENANT, NOW)
            .await
            .unwrap();
    }
}

fn created(admission: CampusLabAdmission) -> CampusLabSessionRecord {
    match admission {
        CampusLabAdmission::Created(record) => record,
        other => panic!("expected a created session, got {other:?}"),
    }
}

fn rejected(admission: CampusLabAdmission) -> CampusLabRejection {
    match admission {
        CampusLabAdmission::Rejected(rejection) => rejection,
        other => panic!("expected a rejection, got {other:?}"),
    }
}

async fn journal_rows(store: &SqliteStore) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM campus_sessions_journal")
        .fetch_one(&store.pool)
        .await
        .unwrap()
}

/// Step a session until it stops moving.
async fn settle(store: &SqliteStore, provider: &FakeCampusProvider, session_id: &str, now: i64) {
    let context = CampusDriverContext {
        worker_id: "test-worker",
        now,
        lease_seconds: 60,
        central_authority: true,
    };
    for _ in 0..8 {
        advance_campus_lab_session(store, provider, &context, session_id)
            .await
            .unwrap();
        let session = store
            .get_campus_lab_session(session_id)
            .await
            .unwrap()
            .unwrap();
        if session.state.is_terminal()
            || session.state == State::CleanupBlocked
            || (session.state == State::Ready
                && session.desired_state == pharness_core::CampusLabDesiredState::Ready)
        {
            return;
        }
    }
}

async fn stop(store: &SqliteStore, provider: &FakeCampusProvider, session_id: &str, now: i64) {
    store
        .request_campus_lab_stop(session_id, "student", "stop requested", false, now)
        .await
        .unwrap();
    settle(store, provider, session_id, now).await;
}

#[tokio::test]
async fn admission_creates_one_live_session_per_lab_and_answers_retries_with_it() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    slots(&store, 3).await;
    student(&store, "alice").await;

    let first = created(
        store
            .admit_campus_lab_launch(launch("alice", "lab-1"))
            .await
            .unwrap(),
    );
    assert_eq!(first.generation, 1);
    assert_eq!(first.session.state, State::Requested);
    assert!(first.is_current());
    assert_eq!(first.owner_identity_id, "cid_alice");

    // A retry without a key, with a new key and with new candidate ids all
    // return the live session.
    for request in [launch("alice", "lab-1"), keyed("alice", "lab-1", "k-1")] {
        match store.admit_campus_lab_launch(request).await.unwrap() {
            CampusLabAdmission::Active(record) => {
                assert_eq!(record.session.session_id, first.session.session_id);
                assert_eq!(record.lab.budget_id, first.lab.budget_id);
            }
            other => panic!("expected the live session, got {other:?}"),
        }
    }
    // The key now answers with that session, and only for that assignment.
    match store
        .admit_campus_lab_launch(keyed("alice", "lab-1", "k-1"))
        .await
        .unwrap()
    {
        CampusLabAdmission::Replayed(record) => {
            assert_eq!(record.session.session_id, first.session.session_id);
        }
        other => panic!("expected a replay, got {other:?}"),
    }
    assert_eq!(
        rejected(
            store
                .admit_campus_lab_launch(keyed("alice", "lab-2", "k-1"))
                .await
                .unwrap()
        ),
        CampusLabRejection::IdempotencyKeyReused
    );
    // A second assignment is refused while the first is live, and the
    // refusal writes nothing, not even a lab row.
    let labs_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM campus_labs")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(
        rejected(
            store
                .admit_campus_lab_launch(launch("alice", "lab-2"))
                .await
                .unwrap()
        ),
        CampusLabRejection::IdentitySessionLimit {
            active_session_id: first.session.session_id.clone()
        }
    );
    let labs_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM campus_labs")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(labs_before, labs_after);
    assert_eq!(journal_rows(&store).await, 1);

    let events = store
        .list_campus_lab_session_events(&first.session.session_id)
        .await
        .unwrap();
    let kinds = events
        .iter()
        .map(|event| event.kind.as_str())
        .collect::<Vec<_>>();
    assert_eq!(kinds, ["campus.session.requested", "campus.lab.admitted"]);
    assert_eq!(events[1].payload["budget_id"], first.lab.budget_id.as_str());
}

#[tokio::test]
async fn course_and_tenant_limits_reject_without_queueing() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    slots(&store, 3).await;
    for subject in ["a", "b", "c", "d"] {
        student(&store, subject).await;
    }
    let course_limited = CampusLabLimits {
        max_active_per_course: 2,
        ..LIMITS
    };
    for subject in ["a", "b"] {
        created(
            store
                .admit_campus_lab_launch(AdmitCampusLabLaunch {
                    limits: course_limited,
                    ..launch(subject, "lab-1")
                })
                .await
                .unwrap(),
        );
    }
    assert_eq!(
        rejected(
            store
                .admit_campus_lab_launch(AdmitCampusLabLaunch {
                    limits: course_limited,
                    ..launch("c", "lab-1")
                })
                .await
                .unwrap()
        ),
        CampusLabRejection::CourseCapacityReached
    );
    // Another course still has room up to the tenant's three slots.
    let other_course = |subject: &str| AdmitCampusLabLaunch {
        course_id: "course-202".into(),
        ..launch(subject, "lab-1")
    };
    created(
        store
            .admit_campus_lab_launch(other_course("c"))
            .await
            .unwrap(),
    );
    assert_eq!(
        rejected(
            store
                .admit_campus_lab_launch(other_course("d"))
                .await
                .unwrap()
        ),
        CampusLabRejection::NoSlotsAvailable
    );
    assert_eq!(
        store.count_live_campus_lab_sessions(TENANT).await.unwrap(),
        3
    );

    // Configuring more slots than are registered admits no more sessions
    // than registered slots exist.
    let generous = CampusLabLimits {
        lab_slots: 50,
        max_active_per_course: 50,
        ..LIMITS
    };
    assert_eq!(
        rejected(
            store
                .admit_campus_lab_launch(AdmitCampusLabLaunch {
                    limits: generous,
                    ..other_course("d")
                })
                .await
                .unwrap()
        ),
        CampusLabRejection::NoSlotsAvailable
    );
    assert_eq!(journal_rows(&store).await, 3);
}

#[tokio::test]
async fn a_freed_slot_is_admitted_again_only_after_teardown() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    let provider = FakeCampusProvider::new();
    slots(&store, 1).await;
    student(&store, "a").await;
    student(&store, "b").await;
    let a = created(
        store
            .admit_campus_lab_launch(launch("a", "lab-1"))
            .await
            .unwrap(),
    );
    settle(&store, &provider, &a.session.session_id, NOW).await;
    assert_eq!(
        rejected(
            store
                .admit_campus_lab_launch(launch("b", "lab-1"))
                .await
                .unwrap()
        ),
        CampusLabRejection::NoSlotsAvailable
    );
    // Asking to stop is not enough: the slot is held until cleanup confirms.
    store
        .request_campus_lab_stop(&a.session.session_id, "a", "stop", false, NOW)
        .await
        .unwrap();
    assert_eq!(
        rejected(
            store
                .admit_campus_lab_launch(launch("b", "lab-1"))
                .await
                .unwrap()
        ),
        CampusLabRejection::NoSlotsAvailable
    );
    settle(&store, &provider, &a.session.session_id, NOW).await;
    created(
        store
            .admit_campus_lab_launch(launch("b", "lab-1"))
            .await
            .unwrap(),
    );
}

#[tokio::test]
async fn reset_creates_the_next_generation_only_after_teardown_and_keeps_the_budget() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    let provider = FakeCampusProvider::new();
    slots(&store, 2).await;
    student(&store, "alice").await;
    let first = created(
        store
            .admit_campus_lab_launch(launch("alice", "lab-1"))
            .await
            .unwrap(),
    );
    let first_id = first.session.session_id.clone();
    settle(&store, &provider, &first_id, NOW).await;

    let reset = |from: &str| AdmitCampusLabLaunch {
        reset_from_session_id: Some(from.into()),
        now: NOW + 30,
        ..launch("alice", "lab-1")
    };
    assert_eq!(
        rejected(
            store
                .admit_campus_lab_launch(reset(&first_id))
                .await
                .unwrap()
        ),
        CampusLabRejection::TeardownPending
    );
    let pending = store
        .request_campus_lab_stop(&first_id, "alice", "reset requested", true, NOW)
        .await
        .unwrap();
    assert_eq!(pending.lab.reset_requested_at, Some(NOW));
    assert_eq!(
        rejected(
            store
                .admit_campus_lab_launch(reset(&first_id))
                .await
                .unwrap()
        ),
        CampusLabRejection::TeardownPending
    );
    settle(&store, &provider, &first_id, NOW + 5).await;
    let terminated = store
        .get_campus_lab_session_record(&first_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(terminated.session.state, State::Terminated);

    let second = created(
        store
            .admit_campus_lab_launch(reset(&first_id))
            .await
            .unwrap(),
    );
    assert_eq!(second.generation, 2);
    assert_eq!(second.lab.lab_id, first.lab.lab_id);
    assert_eq!(second.lab.budget_id, first.lab.budget_id);
    assert_eq!(
        second.reset_from_session_id.as_deref(),
        Some(first_id.as_str())
    );
    assert_eq!(second.lab.reset_requested_at, None);
    assert!(terminated.session.terminated_at.unwrap() <= second.session.created_at);

    // A retried reset answers with the generation it created.
    match store
        .admit_campus_lab_launch(reset(&first_id))
        .await
        .unwrap()
    {
        CampusLabAdmission::Replayed(record) => {
            assert_eq!(record.session.session_id, second.session.session_id);
        }
        other => panic!("expected a replay, got {other:?}"),
    }
    // Resets are unlimited; each one keeps the budget.
    let mut current = second;
    for expected in 3..=6 {
        settle(&store, &provider, &current.session.session_id, NOW + 10).await;
        stop(&store, &provider, &current.session.session_id, NOW + 10).await;
        let next = created(
            store
                .admit_campus_lab_launch(reset(&current.session.session_id))
                .await
                .unwrap(),
        );
        assert_eq!(next.generation, expected);
        assert_eq!(next.lab.budget_id, first.lab.budget_id);
        current = next;
    }
    // A reset of an old generation is superseded.
    assert_eq!(
        rejected(
            store
                .admit_campus_lab_launch(reset(&first_id))
                .await
                .unwrap()
        ),
        CampusLabRejection::ResetSuperseded {
            current_session_id: Some(current.session.session_id.clone())
        }
    );
    // A plain stop cancels a pending reset.
    store
        .request_campus_lab_stop(
            &current.session.session_id,
            "alice",
            "reset",
            true,
            NOW + 20,
        )
        .await
        .unwrap();
    let stopped = store
        .request_campus_lab_stop(
            &current.session.session_id,
            "alice",
            "stop",
            false,
            NOW + 20,
        )
        .await
        .unwrap();
    assert_eq!(stopped.lab.reset_requested_at, None);
}

#[tokio::test]
async fn a_pending_reset_keeps_its_capacity_until_it_is_admitted_or_cleared() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    let provider = FakeCampusProvider::new();
    slots(&store, 1).await;
    student(&store, "alice").await;
    student(&store, "bob").await;
    let first = created(
        store
            .admit_campus_lab_launch(launch("alice", "lab-1"))
            .await
            .unwrap(),
    );
    let first_id = first.session.session_id.clone();
    settle(&store, &provider, &first_id, NOW).await;
    assert_eq!(
        store
            .list_steppable_campus_lab_session_ids(TENANT, 10)
            .await
            .unwrap(),
        vec![first_id.clone()]
    );
    store
        .request_campus_lab_stop(&first_id, "alice", "reset requested", true, NOW)
        .await
        .unwrap();
    assert!(store
        .list_campus_labs_awaiting_reset(TENANT, 10)
        .await
        .unwrap()
        .is_empty());
    settle(&store, &provider, &first_id, NOW).await;
    assert!(store
        .list_steppable_campus_lab_session_ids(TENANT, 10)
        .await
        .unwrap()
        .is_empty());

    // The torn-down session no longer holds a slot, but the waiting reset
    // keeps its place: neither another student nor another assignment of the
    // same student can take it.
    let awaiting = store
        .list_campus_labs_awaiting_reset(TENANT, 10)
        .await
        .unwrap();
    assert_eq!(awaiting.len(), 1);
    assert_eq!(awaiting[0].lab_id, first.lab.lab_id);
    assert_eq!(
        rejected(
            store
                .admit_campus_lab_launch(launch("bob", "lab-1"))
                .await
                .unwrap()
        ),
        CampusLabRejection::NoSlotsAvailable
    );
    assert_eq!(
        rejected(
            store
                .admit_campus_lab_launch(launch("alice", "lab-2"))
                .await
                .unwrap()
        ),
        CampusLabRejection::IdentitySessionLimit {
            active_session_id: first_id.clone()
        }
    );
    let second = created(
        store
            .admit_campus_lab_launch(AdmitCampusLabLaunch {
                reset_from_session_id: Some(first_id.clone()),
                ..launch("alice", "lab-1")
            })
            .await
            .unwrap(),
    );
    assert_eq!(second.generation, 2);
    assert!(store
        .list_campus_labs_awaiting_reset(TENANT, 10)
        .await
        .unwrap()
        .is_empty());

    // A cleared reset releases the capacity it held.
    let second_id = second.session.session_id.clone();
    settle(&store, &provider, &second_id, NOW).await;
    store
        .request_campus_lab_stop(&second_id, "alice", "reset requested", true, NOW)
        .await
        .unwrap();
    settle(&store, &provider, &second_id, NOW).await;
    assert!(!store
        .clear_campus_lab_reset(&second.lab.lab_id, &first_id, NOW)
        .await
        .unwrap());
    assert!(store
        .clear_campus_lab_reset(&second.lab.lab_id, &second_id, NOW)
        .await
        .unwrap());
    assert!(!store
        .clear_campus_lab_reset(&second.lab.lab_id, &second_id, NOW)
        .await
        .unwrap());
    created(
        store
            .admit_campus_lab_launch(launch("bob", "lab-1"))
            .await
            .unwrap(),
    );
}

#[tokio::test]
async fn lab_identity_budget_and_generations_are_fenced_in_sql() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    slots(&store, 1).await;
    student(&store, "alice").await;
    let record = created(
        store
            .admit_campus_lab_launch(launch("alice", "lab-1"))
            .await
            .unwrap(),
    );
    for statement in [
        "UPDATE campus_labs SET budget_id = 'other'",
        "UPDATE campus_labs SET subject = 'mallory'",
        "UPDATE campus_labs SET assignment_id = 'lab-9'",
        "UPDATE campus_labs SET generation = generation + 1",
        "UPDATE campus_labs SET current_session_id = NULL",
        "DELETE FROM campus_labs",
        "UPDATE campus_lab_generations SET generation = 7",
        "DELETE FROM campus_lab_generations",
    ] {
        assert!(
            sqlx::query(statement).execute(&store.pool).await.is_err(),
            "{statement} was accepted"
        );
    }
    let reread = store
        .get_campus_lab_session_record(&record.session.session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reread, record);
}

#[tokio::test]
async fn expired_sessions_are_listed_for_reclaiming() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    slots(&store, 2).await;
    student(&store, "a").await;
    student(&store, "b").await;
    let a = created(
        store
            .admit_campus_lab_launch(launch("a", "lab-1"))
            .await
            .unwrap(),
    );
    created(
        store
            .admit_campus_lab_launch(AdmitCampusLabLaunch {
                expires_at: NOW + 9_000,
                ..launch("b", "lab-1")
            })
            .await
            .unwrap(),
    );
    assert!(store
        .list_expired_campus_lab_session_ids(TENANT, NOW + 7_199, 10)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        store
            .list_expired_campus_lab_session_ids(TENANT, NOW + 7_200, 10)
            .await
            .unwrap(),
        vec![a.session.session_id]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_admissions_never_exceed_any_limit() {
    let disk = DiskStore::new("admission-race").await;
    let store = Arc::new(disk.store.clone());
    slots(&store, 5).await;
    let subjects = (0..10).map(|index| format!("s{index}")).collect::<Vec<_>>();
    for subject in &subjects {
        student(&store, subject).await;
    }
    let mut tasks = Vec::new();
    for round in 0..4 {
        for subject in &subjects {
            let store = store.clone();
            let mut request = launch(subject, if round % 2 == 0 { "lab-1" } else { "lab-2" });
            request.limits = CampusLabLimits {
                lab_slots: 5,
                max_active_per_identity: 1,
                max_active_per_course: 5,
            };
            if round == 3 {
                request.idempotency_key = Some(format!("{subject}-retry"));
            }
            tasks.push(tokio::spawn(async move {
                store.admit_campus_lab_launch(request).await.unwrap()
            }));
        }
    }
    let mut created_count = 0;
    let mut per_subject = std::collections::BTreeMap::<String, BTreeSet<String>>::new();
    for task in tasks {
        match task.await.unwrap() {
            CampusLabAdmission::Created(record) => {
                created_count += 1;
                per_subject
                    .entry(record.session.subject.clone())
                    .or_default()
                    .insert(record.session.session_id);
            }
            CampusLabAdmission::Active(record) | CampusLabAdmission::Replayed(record) => {
                per_subject
                    .entry(record.session.subject.clone())
                    .or_default()
                    .insert(record.session.session_id);
            }
            CampusLabAdmission::Rejected(
                CampusLabRejection::NoSlotsAvailable
                | CampusLabRejection::CourseCapacityReached
                | CampusLabRejection::IdentitySessionLimit { .. },
            ) => {}
            CampusLabAdmission::Rejected(other) => panic!("unexpected rejection {other:?}"),
        }
    }
    assert_eq!(created_count, 5);
    assert_eq!(journal_rows(&store).await, 5);
    assert!(per_subject.values().all(|sessions| sessions.len() == 1));
    assert_eq!(
        store.count_live_campus_lab_sessions(TENANT).await.unwrap(),
        5
    );
}

struct FixedPolicies(EffectiveCampusPolicy);

impl CampusAssignmentPolicies for FixedPolicies {
    fn effective_policy(
        &self,
        _tenant_id: &str,
        _course_id: &str,
        assignment_id: &str,
    ) -> Option<EffectiveCampusPolicy> {
        (assignment_id == "lab-1").then(|| self.0.clone())
    }
}

struct Clock(AtomicI64);

impl CampusBrokerClock for Clock {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

fn coding_policy() -> EffectiveCampusPolicy {
    let profile = CampusProfile::parse(include_str!(
        "../../../../pharness-core/src/campus_profile/fixtures/coding.json"
    ))
    .unwrap();
    let layer = |authority: &str| {
        CampusPolicyLayer::parse(
            &json!({
                "schema_version": CAMPUS_POLICY_LAYER_SCHEMA,
                "authority": authority,
                "id": format!("{authority}-policy"),
                "version": "v1",
                "constraints": {},
                "mandatory_denies": {
                    "tools": [], "egress_hosts": [], "model_aliases": [], "approval_kinds": []
                },
            })
            .to_string(),
        )
        .unwrap()
    };
    EffectiveCampusPolicy::compose(
        &profile,
        &[layer("platform"), layer("institution")],
        &BTreeSet::new(),
    )
    .unwrap()
}

#[tokio::test]
async fn journal_authority_is_active_only_while_the_session_is_ready_and_unexpired() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    let provider = FakeCampusProvider::new();
    slots(&store, 2).await;
    student(&store, "alice").await;
    let policy = coding_policy();
    let clock = Arc::new(Clock(AtomicI64::new(NOW)));
    let source = JournalCampusAuthoritySource::new(
        store.clone(),
        Arc::new(FixedPolicies(policy.clone())),
        clock.clone(),
    );
    let mut request = launch("alice", "lab-1");
    request.profile = policy.profile().clone();
    request.effective_policy_hash = policy.effective_policy_hash().into();
    let record = created(store.admit_campus_lab_launch(request).await.unwrap());
    let id = record.session.session_id.clone();

    let status = |snapshot: Option<pharness_core::campus_capability::CampusAuthoritySnapshot>| {
        snapshot.map(|snapshot| snapshot.status)
    };
    assert_eq!(
        status(source.session(&id).await.unwrap()),
        Some(CampusAuthorityStatus::Revoked)
    );
    settle(&store, &provider, &id, NOW).await;
    let snapshot = source.session(&id).await.unwrap().unwrap();
    assert_eq!(snapshot.status, CampusAuthorityStatus::Active);
    assert_eq!(snapshot.authority.owner.identity_id, "cid_alice");
    assert_eq!(snapshot.authority.owner.tenant_id, TENANT);
    assert_eq!(snapshot.authority.course_id, "course-101");
    assert_eq!(snapshot.authority.expires_at, record.session.expires_at);
    assert_eq!(
        snapshot.authority.claims().effective_policy_hash,
        policy.effective_policy_hash()
    );

    clock.0.store(record.session.expires_at, Ordering::SeqCst);
    assert_eq!(
        status(source.session(&id).await.unwrap()),
        Some(CampusAuthorityStatus::Revoked)
    );
    clock.0.store(NOW, Ordering::SeqCst);
    store
        .request_campus_lab_stop(&id, "alice", "stop", false, NOW)
        .await
        .unwrap();
    assert_eq!(
        status(source.session(&id).await.unwrap()),
        Some(CampusAuthorityStatus::Revoked)
    );
    assert!(source.session("unknown").await.unwrap().is_none());

    // A session launched under another policy hash has no authority.
    student(&store, "bob").await;
    let mut stale = launch("bob", "lab-1");
    stale.profile = policy.profile().clone();
    let stale = created(store.admit_campus_lab_launch(stale).await.unwrap());
    assert!(source
        .session(&stale.session.session_id)
        .await
        .unwrap()
        .is_none());
}

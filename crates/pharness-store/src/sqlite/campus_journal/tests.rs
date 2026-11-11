use crate::campus_journal::test_support::{request, seed, student, DiskStore, TENANT};
use crate::{
    CampusJournalError, CampusLabTransition, CampusOperationLease, CampusOrphanReason, SqliteStore,
    StoredCampusLabSession,
};
use pharness_workspaces::{
    CampusLabDesiredState, CampusLabOperationStatus, CampusLabSessionState as State,
    CampusLabTransitionError, CampusWorkloadRef, OwnedCampusResource,
};
use std::collections::BTreeSet;

mod migration;

const LEASE: i64 = 30;

async fn step(
    store: &SqliteStore,
    session: &StoredCampusLabSession,
    to: State,
    actor: &str,
    now: i64,
    lease: Option<CampusOperationLease>,
) -> Result<crate::CampusLabTransitionOutcome, CampusJournalError> {
    let mut transition = CampusLabTransition::new(session, to, actor, now);
    transition.lease = lease;
    transition.lease_seconds = LEASE;
    if to == State::Ready {
        transition.workspace_id = Some(format!("ws-{}", session.session_id));
    }
    store.transition_campus_lab_session(transition).await
}

async fn reload(store: &SqliteStore, session_id: &str) -> StoredCampusLabSession {
    store
        .get_campus_lab_session(session_id)
        .await
        .unwrap()
        .unwrap()
}

/// Drive a session to `Provisioning` and return its leased provision.
async fn provisioning(
    store: &SqliteStore,
    session_id: &str,
    actor: &str,
    now: i64,
) -> (StoredCampusLabSession, CampusOperationLease) {
    let session = reload(store, session_id).await;
    let authorizing = step(store, &session, State::Authorizing, actor, now, None)
        .await
        .unwrap()
        .session;
    let outcome = step(store, &authorizing, State::Provisioning, actor, now, None)
        .await
        .unwrap();
    (outcome.session, outcome.begun.unwrap().lease().unwrap())
}

/// Tear a ready session down to `Terminated` under `actor`'s leases.
async fn terminate(store: &SqliteStore, session_id: &str, actor: &str, now: i64) {
    store
        .request_campus_lab_session_stop(session_id, actor, "done", now)
        .await
        .unwrap();
    let ready = reload(store, session_id).await;
    let stopping = step(store, &ready, State::Stopping, actor, now, None)
        .await
        .unwrap()
        .session;
    let cleaning = step(store, &stopping, State::Cleaning, actor, now, None)
        .await
        .unwrap();
    let lease = cleaning.begun.unwrap().lease();
    step(
        store,
        &cleaning.session,
        State::Terminated,
        actor,
        now,
        lease,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn database_transition_table_matches_the_core_state_machine() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT from_state, to_state FROM campus_session_transitions")
            .fetch_all(&store.pool)
            .await
            .unwrap();
    let database: BTreeSet<_> = rows
        .iter()
        .map(|(from, to)| (State::parse(from).unwrap(), State::parse(to).unwrap()))
        .collect();
    let core: BTreeSet<_> = State::ALL
        .into_iter()
        .flat_map(|from| from.successors().iter().map(move |to| (from, *to)))
        .collect();
    assert_eq!(database, core);
    assert!(
        sqlx::query("INSERT INTO campus_session_transitions VALUES ('ready', 'terminated')")
            .execute(&store.pool)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn database_rejects_writes_that_bypass_the_state_machine() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1"], &["s1", "s2"], 100).await;
    let raw = |sql: &'static str| {
        let pool = store.pool.clone();
        async move {
            sqlx::query(sql)
                .execute(&pool)
                .await
                .unwrap_err()
                .to_string()
        }
    };
    let illegal = raw(
        "UPDATE campus_sessions_journal SET state = 'failed', generation = 2,
         failure_reason = 'x', terminated_at = 1 WHERE session_id = 's1'",
    )
    .await;
    assert!(
        illegal.contains("illegal campus session transition"),
        "{illegal}"
    );
    let unfenced =
        raw("UPDATE campus_sessions_journal SET state = 'authorizing' WHERE session_id = 's1'")
            .await;
    assert!(unfenced.contains("not fenced"), "{unfenced}");
    let identity =
        raw("UPDATE campus_sessions_journal SET subject = 'someone-else' WHERE session_id = 's1'")
            .await;
    assert!(identity.contains("not fenced"), "{identity}");

    let s1 = reload(&store, "s1").await;
    step(&store, &s1, State::Terminated, "op", 101, None)
        .await
        .unwrap();
    let terminal =
        raw("UPDATE campus_sessions_journal SET observed_state = 'ready' WHERE session_id = 's1'")
            .await;
    assert!(terminal.contains("not fenced"), "{terminal}");

    provisioning(&store, "s2", "worker", 101).await;
    let takeover =
        raw("UPDATE campus_lab_slots SET occupant_session_id = 's1' WHERE slot_id = 'slot-1'")
            .await;
    assert!(takeover.contains("fenced"), "{takeover}");
    let rewind =
        raw("UPDATE campus_lab_slots SET occupant_generation = 0 WHERE slot_id = 'slot-1'").await;
    assert!(rewind.contains("fenced"), "{rewind}");
    let reopen =
        raw("UPDATE campus_session_operations SET kind = 'delete' WHERE session_id = 's2'").await;
    assert!(reopen.contains("immutable"), "{reopen}");
}

#[tokio::test]
async fn creation_is_idempotent_for_duplicate_delivery() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    student(&store, "alice").await;
    let (first, created) = store
        .create_campus_lab_session(request("s1", "alice", 100))
        .await
        .unwrap();
    assert!(created);
    assert_eq!(first.state, State::Requested);
    assert_eq!(first.generation, 1);
    let (again, created) = store
        .create_campus_lab_session(request("s1", "alice", 100))
        .await
        .unwrap();
    assert!(!created);
    assert_eq!(again, first);
    let mut changed = request("s1", "alice", 100);
    changed.course_id = "course-202".into();
    assert!(matches!(
        store.create_campus_lab_session(changed).await,
        Err(CampusJournalError::Conflict(_))
    ));
    assert!(matches!(
        store
            .create_campus_lab_session(request("s2", "nobody", 100))
            .await,
        Err(CampusJournalError::Invalid(_))
    ));
    let events = store.list_campus_lab_session_events("s1").await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, "campus.session.requested");
    assert_eq!(events[0].to_state, Some(State::Requested));
}

#[tokio::test]
async fn illegal_and_stale_transitions_write_nothing() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    let session = seed(&store, &["slot-1"], &["s1"], 100).await.remove(0);
    assert!(matches!(
        step(&store, &session, State::Ready, "op", 101, None).await,
        Err(CampusJournalError::Transition(
            CampusLabTransitionError::Illegal { .. }
        ))
    ));
    step(&store, &session, State::Authorizing, "op", 101, None)
        .await
        .unwrap();
    // `session` still names generation 1.
    let stale = step(&store, &session, State::Authorizing, "op", 102, None).await;
    assert!(
        matches!(stale, Err(CampusJournalError::Fenced(_))),
        "{stale:?}"
    );
    let stale = step(&store, &session, State::Terminated, "op", 102, None).await;
    assert!(
        matches!(stale, Err(CampusJournalError::Fenced(_))),
        "{stale:?}"
    );
    let current = reload(&store, "s1").await;
    assert_eq!((current.state, current.generation), (State::Authorizing, 2));
    assert_eq!(
        store
            .list_campus_lab_session_events("s1")
            .await
            .unwrap()
            .len(),
        2
    );
    let mut denied = CampusLabTransition::new(&current, State::Failed, "op", 103);
    assert!(matches!(
        store.transition_campus_lab_session(denied.clone()).await,
        Err(CampusJournalError::Invalid(_))
    ));
    denied.failure = Some("denied".into());
    let failed = store.transition_campus_lab_session(denied).await.unwrap();
    assert_eq!(failed.session.state, State::Failed);
    assert!(matches!(
        step(&store, &failed.session, State::Authorizing, "op", 104, None).await,
        Err(CampusJournalError::Transition(
            CampusLabTransitionError::Terminal { .. }
        ))
    ));
}

#[tokio::test]
async fn provisioning_takes_a_fenced_slot_and_records_intent_with_its_event() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1"], &["s1", "s2"], 100).await;
    let (session, lease) = provisioning(&store, "s1", "worker-a", 101).await;
    assert_eq!(session.slot_id.as_deref(), Some("slot-1"));
    assert_eq!(session.slot_generation, Some(1));
    let operation = store
        .get_campus_lab_operation(&lease.op_key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(operation.status, CampusLabOperationStatus::Pending);
    assert_eq!(operation.session_generation, session.generation);
    assert_eq!((operation.attempt_count, operation.lease_fence), (1, 1));
    let event = store
        .list_campus_lab_session_events("s1")
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(event.to_state, Some(State::Provisioning));
    assert_eq!(event.op_key.as_deref(), Some(lease.op_key.as_str()));
    assert_eq!(event.generation, session.generation);

    let s2 = reload(&store, "s2").await;
    let s2 = step(&store, &s2, State::Authorizing, "worker-a", 102, None)
        .await
        .unwrap()
        .session;
    assert!(matches!(
        step(&store, &s2, State::Provisioning, "worker-a", 102, None).await,
        Err(CampusJournalError::NoFreeSlot(_))
    ));
    let s2 = reload(&store, "s2").await;
    assert_eq!((s2.state, s2.slot_id), (State::Authorizing, None));
}

#[tokio::test]
async fn a_stale_lease_or_generation_cannot_mutate_a_reoccupied_slot() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1"], &["s1", "s2"], 100).await;
    let (s1, stale_lease) = provisioning(&store, "s1", "worker-a", 100).await;

    // worker-a stalls past its lease; worker-b takes over and finishes.
    assert!(matches!(
        store
            .lease_campus_lab_operation(&stale_lease.op_key, "worker-b", 110, LEASE)
            .await,
        Err(CampusJournalError::LeaseHeld { .. })
    ));
    let fresh = store
        .lease_campus_lab_operation(&stale_lease.op_key, "worker-b", 131, LEASE)
        .await
        .unwrap();
    assert_eq!((fresh.lease_fence, fresh.attempt_count), (2, 2));
    assert!(matches!(
        step(
            &store,
            &s1,
            State::Ready,
            "worker-a",
            132,
            Some(stale_lease.clone())
        )
        .await,
        Err(CampusJournalError::Fenced(_))
    ));
    step(&store, &s1, State::Ready, "worker-b", 132, fresh.lease())
        .await
        .unwrap();
    terminate(&store, "s1", "worker-b", 140).await;
    assert_eq!(
        store
            .get_campus_lab_slot("slot-1")
            .await
            .unwrap()
            .unwrap()
            .occupant_session_id,
        None
    );

    let (s2, _) = provisioning(&store, "s2", "worker-b", 150).await;
    assert_eq!(s2.slot_generation, Some(2));

    // Every write worker-a could still attempt for its old occupancy fails.
    assert!(matches!(
        step(
            &store,
            &s1,
            State::Ready,
            "worker-a",
            151,
            Some(stale_lease.clone())
        )
        .await,
        Err(CampusJournalError::Fenced(_))
    ));
    assert!(matches!(
        store
            .lease_campus_lab_operation(&stale_lease.op_key, "worker-a", 151, LEASE)
            .await,
        Err(CampusJournalError::Fenced(_))
    ));
    assert!(!store
        .release_campus_lab_operation_lease(&stale_lease, Some("late"), 151)
        .await
        .unwrap());
    let mut forged = s1.clone();
    forged.state = State::Cleaning;
    forged.generation = reload(&store, "s1").await.generation;
    assert!(
        step(&store, &forged, State::Terminated, "worker-a", 151, None)
            .await
            .is_err()
    );

    let slot = store.get_campus_lab_slot("slot-1").await.unwrap().unwrap();
    assert_eq!(slot.occupant_session_id.as_deref(), Some("s2"));
    assert_eq!(slot.occupant_generation, 2);
    let s2_now = reload(&store, "s2").await;
    assert_eq!((s2_now.state, s2_now.generation), (s2.state, s2.generation));
}

#[tokio::test]
async fn stopping_a_provisioning_session_waits_for_its_live_lease() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1"], &["s1"], 100).await;
    let (provisioned, lease) = provisioning(&store, "s1", "worker-a", 100).await;
    let session = store
        .request_campus_lab_session_stop("s1", "student", "cancel", 101)
        .await
        .unwrap();
    assert_eq!(session.desired_state, CampusLabDesiredState::Terminated);
    assert_eq!(
        session.generation, provisioned.generation,
        "a stop request does not fence workers"
    );
    assert!(matches!(
        step(&store, &session, State::Stopping, "worker-b", 105, None).await,
        Err(CampusJournalError::LeaseHeld { .. })
    ));
    let stopped = step(&store, &session, State::Stopping, "worker-b", 131, None)
        .await
        .unwrap()
        .session;
    assert_eq!(stopped.revoked_at, Some(131));
    let operation = store
        .get_campus_lab_operation(&lease.op_key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(operation.status, CampusLabOperationStatus::Abandoned);
    assert!(matches!(
        step(&store, &session, State::Ready, "worker-a", 132, Some(lease)).await,
        Err(CampusJournalError::Fenced(_))
    ));
}

#[tokio::test]
async fn cleanup_ends_failed_exactly_when_the_launch_failed() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1"], &["s1"], 100).await;
    let (session, lease) = provisioning(&store, "s1", "worker", 100).await;
    let mut rejected = CampusLabTransition::new(&session, State::Stopping, "worker", 101);
    rejected.lease = Some(lease);
    rejected.failure = Some("no qualified runtime".into());
    let stopping = store
        .transition_campus_lab_session(rejected)
        .await
        .unwrap()
        .session;
    let cleaning = step(&store, &stopping, State::Cleaning, "worker", 102, None)
        .await
        .unwrap();
    let lease = cleaning.begun.unwrap().lease();
    assert!(matches!(
        step(
            &store,
            &cleaning.session,
            State::Terminated,
            "worker",
            103,
            lease.clone()
        )
        .await,
        Err(CampusJournalError::Invalid(_))
    ));
    let failed = step(
        &store,
        &cleaning.session,
        State::Failed,
        "worker",
        103,
        lease,
    )
    .await
    .unwrap()
    .session;
    assert_eq!(
        failed.failure_reason.as_deref(),
        Some("no qualified runtime")
    );
    assert_eq!(failed.terminated_at, Some(103));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_workers_race_and_exactly_one_wins() {
    let disk = DiskStore::new("race").await;
    let store = disk.store.clone();
    seed(&store, &["slot-1"], &["s1"], 100).await;
    let session = reload(&store, "s1").await;
    let transitions = (0..8).map(|worker| {
        let store = store.clone();
        let session = session.clone();
        tokio::spawn(async move {
            step(
                &store,
                &session,
                State::Authorizing,
                &format!("w{worker}"),
                101,
                None,
            )
            .await
        })
    });
    let mut won = 0;
    for task in transitions.collect::<Vec<_>>() {
        match task.await.unwrap() {
            Ok(_) => won += 1,
            Err(CampusJournalError::Fenced(_)) => {}
            Err(error) => panic!("unexpected {error:?}"),
        }
    }
    assert_eq!(won, 1);

    let authorizing = reload(&store, "s1").await;
    let begun = step(&store, &authorizing, State::Provisioning, "w0", 102, None)
        .await
        .unwrap()
        .begun
        .unwrap();
    // The creator's lease has expired; eight workers try to resume at once.
    let leases = (0..8).map(|worker| {
        let store = store.clone();
        let op_key = begun.op_key.clone();
        tokio::spawn(async move {
            store
                .lease_campus_lab_operation(&op_key, &format!("w{worker}"), 200, LEASE)
                .await
        })
    });
    let mut winners = Vec::new();
    for task in leases.collect::<Vec<_>>() {
        match task.await.unwrap() {
            Ok(operation) => winners.push(operation),
            Err(CampusJournalError::LeaseHeld { .. }) => {}
            Err(error) => panic!("unexpected {error:?}"),
        }
    }
    assert_eq!(winners.len(), 1);
    assert_eq!(winners[0].lease_fence, 2);
    assert_eq!(winners[0].attempt_count, 2);
}

#[tokio::test]
async fn outbox_events_are_append_only_and_published_once() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    let session = seed(&store, &["slot-1"], &["s1"], 100).await.remove(0);
    step(&store, &session, State::Authorizing, "worker", 101, None)
        .await
        .unwrap();
    let unpublished = store.list_unpublished_campus_lab_events(10).await.unwrap();
    assert_eq!(unpublished.len(), 2);
    assert!(unpublished[0].sequence < unpublished[1].sequence);
    assert_eq!(
        store
            .mark_campus_lab_events_published(&[unpublished[0].sequence], 200)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .mark_campus_lab_events_published(&[unpublished[0].sequence], 300)
            .await
            .unwrap(),
        0
    );
    let remaining = store.list_unpublished_campus_lab_events(10).await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].sequence, unpublished[1].sequence);
    let events = store.list_campus_lab_session_events("s1").await.unwrap();
    assert_eq!(events[0].published_at, Some(200));
    for sql in [
        "UPDATE campus_session_events SET payload_json = '{}'",
        "UPDATE campus_session_events SET published_at = NULL",
        "DELETE FROM campus_session_events",
    ] {
        let error = sqlx::query(sql).execute(&store.pool).await.unwrap_err();
        assert!(error.to_string().contains("append-only"), "{sql}: {error}");
    }
}

#[tokio::test]
async fn owned_resources_are_classified_against_the_journal() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    seed(&store, &["slot-1", "slot-2"], &["live", "done"], 100).await;
    let (live, _) = provisioning(&store, "live", "worker", 100).await;
    let done = reload(&store, "done").await;
    step(&store, &done, State::Terminated, "worker", 100, None)
        .await
        .unwrap();
    let resource = |session: &str, slot: &str, generation: i64| OwnedCampusResource {
        resource_id: format!("{session}-{slot}-{generation}"),
        kind: "workspace".into(),
        workload: CampusWorkloadRef {
            session_id: session.into(),
            slot_id: slot.into(),
            slot_generation: generation,
        },
        op_key: "op".into(),
    };
    let owned = vec![
        resource("live", "slot-1", 1),
        resource("live", "slot-1", 7),
        resource("done", "slot-2", 1),
        resource("ghost", "slot-2", 1),
    ];
    assert_eq!(live.workload(), Some(owned[0].workload.clone()));
    let reasons: Vec<_> = store
        .find_campus_lab_orphans(&owned)
        .await
        .unwrap()
        .into_iter()
        .map(|orphan| (orphan.resource.resource_id, orphan.reason))
        .collect();
    assert_eq!(
        reasons,
        vec![
            ("live-slot-1-7".into(), CampusOrphanReason::StaleOccupancy),
            ("done-slot-2-1".into(), CampusOrphanReason::TerminalSession),
            ("ghost-slot-2-1".into(), CampusOrphanReason::UnknownSession),
        ]
    );
    assert!(store
        .list_quarantined_campus_lab_sessions(TENANT)
        .await
        .unwrap()
        .is_empty());
}

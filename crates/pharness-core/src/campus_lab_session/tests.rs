use super::*;
use crate::CampusProfileRef;
use std::collections::BTreeSet;

use CampusLabSessionState::{
    Authorizing, Cleaning, CleanupBlocked, Failed, Provisioning, Ready, Requested, Stopping,
    Terminated,
};

const LEGAL: &[(CampusLabSessionState, CampusLabSessionState)] = &[
    (Requested, Authorizing),
    (Requested, Terminated),
    (Authorizing, Provisioning),
    (Authorizing, Failed),
    (Authorizing, Terminated),
    (Provisioning, Ready),
    (Provisioning, Stopping),
    (Ready, Stopping),
    (Stopping, Cleaning),
    (Cleaning, Terminated),
    (Cleaning, Failed),
    (Cleaning, CleanupBlocked),
    (CleanupBlocked, Cleaning),
];

#[test]
fn every_legal_transition_is_accepted() {
    for (from, to) in LEGAL {
        assert_eq!(from.transition(*to), Ok(*to), "{from} -> {to}");
    }
}

#[test]
fn every_other_transition_is_rejected_with_a_typed_error() {
    let legal: BTreeSet<_> = LEGAL.iter().copied().collect();
    let mut rejected = 0;
    for from in CampusLabSessionState::ALL {
        for to in CampusLabSessionState::ALL {
            if legal.contains(&(from, to)) {
                continue;
            }
            rejected += 1;
            let expected = if from.is_terminal() {
                CampusLabTransitionError::Terminal { from, to }
            } else {
                CampusLabTransitionError::Illegal { from, to }
            };
            assert_eq!(from.transition(to), Err(expected), "{from} -> {to}");
        }
    }
    assert_eq!(rejected, 81 - LEGAL.len());
}

#[test]
fn successors_match_the_documented_table() {
    let from_table: BTreeSet<_> = CampusLabSessionState::ALL
        .into_iter()
        .flat_map(|from| from.successors().iter().map(move |to| (from, *to)))
        .collect();
    assert_eq!(from_table, LEGAL.iter().copied().collect());
}

#[test]
fn resource_and_terminal_classification_is_exclusive() {
    for state in CampusLabSessionState::ALL {
        assert!(
            !(state.is_terminal() && state.may_hold_resources()),
            "{state} cannot be terminal while holding resources"
        );
        assert_eq!(state.is_terminal(), state.successors().is_empty());
        if state.grants_access() {
            assert!(state.may_hold_resources());
        }
    }
    assert_eq!(
        CampusLabSessionState::ALL
            .into_iter()
            .filter(|state| state.is_terminal())
            .collect::<Vec<_>>(),
        vec![Terminated, Failed]
    );
    assert!(CleanupBlocked.is_quarantined() && CleanupBlocked.may_hold_resources());
    assert!(!Requested.may_hold_resources() && !Authorizing.may_hold_resources());
}

#[test]
fn a_resource_holding_state_reaches_a_terminal_state_only_through_cleaning() {
    for from in CampusLabSessionState::ALL {
        if from.may_hold_resources() {
            for to in from.successors() {
                if to.is_terminal() {
                    assert_eq!(from, Cleaning, "{from} -> {to} skips cleanup");
                }
            }
        }
    }
}

#[test]
fn names_round_trip() {
    for state in CampusLabSessionState::ALL {
        assert_eq!(CampusLabSessionState::parse(state.as_str()), Some(state));
        assert_eq!(
            serde_json::to_value(state).unwrap(),
            serde_json::Value::String(state.as_str().into())
        );
    }
    for state in CampusLabObservedState::ALL {
        assert_eq!(CampusLabObservedState::parse(state.as_str()), Some(state));
    }
    for kind in [
        CampusLabOperationKind::Provision,
        CampusLabOperationKind::Delete,
    ] {
        assert_eq!(CampusLabOperationKind::parse(kind.as_str()), Some(kind));
    }
    for status in [
        CampusLabOperationStatus::Pending,
        CampusLabOperationStatus::Succeeded,
        CampusLabOperationStatus::Failed,
        CampusLabOperationStatus::Blocked,
        CampusLabOperationStatus::Abandoned,
    ] {
        assert_eq!(
            CampusLabOperationStatus::parse(status.as_str()),
            Some(status)
        );
    }
    assert_eq!(CampusLabSessionState::parse("running"), None);
}

fn workload(session: &str, generation: i64) -> CampusWorkloadRef {
    CampusWorkloadRef {
        session_id: session.into(),
        slot_id: "slot-1".into(),
        slot_generation: generation,
    }
}

fn spec(session: &str, generation: i64) -> CampusProvisionSpec {
    CampusProvisionSpec {
        workload: workload(session, generation),
        tenant_id: "tenant-a".into(),
        course_id: "course-1".into(),
        profile: CampusProfileRef {
            id: "campus-coding".into(),
            version: "1".into(),
            profile_hash: "hash".into(),
        },
        expires_at: 10_000,
    }
}

#[tokio::test]
async fn fake_provision_is_idempotent_on_the_operation_key() {
    let fake = FakeCampusProvider::new();
    let first = fake.provision("op-1", &spec("s1", 1)).await.unwrap();
    let again = fake.provision("op-1", &spec("s1", 1)).await.unwrap();
    assert_eq!(first, again);
    assert_eq!(fake.workspaces_created(), 1);
    assert_eq!(fake.resources().len(), 2);
    assert!(matches!(
        fake.provision("op-1", &spec("s1", 2)).await.unwrap(),
        CampusProvisionOutcome::Rejected { .. }
    ));
}

#[tokio::test]
async fn fake_delete_only_matches_the_exact_slot_generation() {
    let fake = FakeCampusProvider::new();
    fake.provision("op-new", &spec("s2", 2)).await.unwrap();
    assert_eq!(
        fake.delete("stale", &workload("s1", 1)).await.unwrap(),
        CampusDeleteOutcome::Deleted
    );
    assert_eq!(
        fake.delete("stale", &workload("s2", 1)).await.unwrap(),
        CampusDeleteOutcome::Deleted
    );
    assert_eq!(fake.resources_for(&workload("s2", 2)).len(), 2);
    assert!(matches!(
        fake.observe(&workload("s2", 2)).await.unwrap(),
        CampusObservation::Ready { .. }
    ));
    assert_eq!(
        fake.observe(&workload("s2", 1)).await.unwrap(),
        CampusObservation::Absent
    );
}

#[tokio::test]
async fn fake_never_provisions_a_deleted_workload() {
    let fake = FakeCampusProvider::new();
    fake.provision("op", &spec("s", 1)).await.unwrap();
    fake.delete("del", &workload("s", 1)).await.unwrap();
    assert!(matches!(
        fake.provision("op", &spec("s", 1)).await.unwrap(),
        CampusProvisionOutcome::Rejected { .. }
    ));
    assert!(fake.resources().is_empty());
    assert_eq!(fake.workspaces_created(), 1);
}

#[tokio::test]
async fn fake_faults_fire_once_in_order() {
    let fake = FakeCampusProvider::new();
    fake.inject(FakeFaultPoint::BeforeProvision, FakeFault::Unreachable);
    fake.inject(
        FakeFaultPoint::BeforeProvision,
        FakeFault::Reject("no runtime".into()),
    );
    assert!(matches!(
        fake.provision("op", &spec("s", 1)).await,
        Err(CampusProviderError::Unreachable(_))
    ));
    assert_eq!(
        fake.provision("op", &spec("s", 1)).await.unwrap(),
        CampusProvisionOutcome::Rejected {
            reason: "no runtime".into()
        }
    );
    assert_eq!(fake.workspaces_created(), 0);
    fake.provision("op", &spec("s", 1)).await.unwrap();
    fake.inject(FakeFaultPoint::MidDelete, FakeFault::Unreachable);
    assert!(matches!(
        fake.delete("del", &workload("s", 1)).await.unwrap(),
        CampusDeleteOutcome::Unreachable { .. }
    ));
    assert_eq!(fake.resources_for(&workload("s", 1)).len(), 1);
    assert_eq!(
        fake.delete("del", &workload("s", 1)).await.unwrap(),
        CampusDeleteOutcome::Deleted
    );
    assert!(fake.resources().is_empty());
}

#[tokio::test]
async fn a_paused_call_continues_after_release() {
    let fake = std::sync::Arc::new(FakeCampusProvider::new());
    fake.inject(FakeFaultPoint::AfterProvision, FakeFault::Pause);
    let task = tokio::spawn({
        let fake = fake.clone();
        async move { fake.provision("op", &spec("s", 1)).await }
    });
    fake.wait_for_pause().await;
    assert_eq!(fake.workspaces_created(), 1);
    fake.release_pause();
    assert!(matches!(
        task.await.unwrap().unwrap(),
        CampusProvisionOutcome::Ready { .. }
    ));
}

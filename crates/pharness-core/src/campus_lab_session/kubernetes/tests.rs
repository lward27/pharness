use super::fake_kube::{matches_selector, FakeKube};
use super::*;
use crate::campus_capability::{
    CampusActorRef, CampusSandboxNetwork, CampusSessionAuthority, InMemoryCampusAuthoritySource,
};
use crate::policy::{CampusPolicyLayer, CAMPUS_POLICY_LAYER_SCHEMA};
use crate::{EffectiveCampusPolicy, SECURITY_PROFILE_QUALIFICATION};
use std::sync::atomic::{AtomicI64, Ordering};

const RESEARCH: &str = include_str!("../../campus_profile/fixtures/research.json");
const CODING: &str = include_str!("../../campus_profile/fixtures/coding.json");
const SECURITY: &str = include_str!("../../campus_profile/fixtures/security.json");
const START: i64 = 1_900_000_000;
const POOL: (&str, &str) = ("pharness.dev/campus-lab-runtime", "gvisor");
const OP: &str = "op-provision-1";

struct TestClock(AtomicI64);

impl CampusBrokerClock for TestClock {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

fn layer(authority: &str, constraints: Value) -> CampusPolicyLayer {
    CampusPolicyLayer::parse(
        &json!({
            "schema_version": CAMPUS_POLICY_LAYER_SCHEMA,
            "authority": authority,
            "id": format!("{authority}-policy"),
            "version": "v1",
            "constraints": constraints,
            "mandatory_denies": {
                "tools": [], "egress_hosts": [], "model_aliases": [], "approval_kinds": []
            },
        })
        .to_string(),
    )
    .expect("layer validates")
}

fn profile(document: &str) -> CampusProfile {
    CampusProfile::parse(document).expect("fixture validates")
}

fn compose(document: &str, institution: Value) -> EffectiveCampusPolicy {
    EffectiveCampusPolicy::compose(
        &profile(document),
        &[
            layer("platform", json!({})),
            layer("institution", institution),
        ],
        &BTreeSet::from([SECURITY_PROFILE_QUALIFICATION.to_string()]),
    )
    .expect("policy composes")
}

/// The coding profile narrowed to what a lab slot can hold: no egress and a
/// smaller envelope.
fn lab_coding() -> Value {
    json!({
        "egress": [],
        "cpu_millicores": 1500,
        "memory_mib": 1024,
        "ephemeral_storage_mib": 1024,
    })
}

struct Harness {
    kube: Arc<FakeKube>,
    authority: Arc<InMemoryCampusAuthoritySource>,
    clock: Arc<TestClock>,
    provider: KubernetesCampusProvider,
}

impl Harness {
    fn new() -> Self {
        Self::with_kube(FakeKube::with_runtime_class("runsc", POOL))
    }

    fn with_kube(kube: FakeKube) -> Self {
        let kube = Arc::new(kube);
        let authority = Arc::new(InMemoryCampusAuthoritySource::default());
        let clock = Arc::new(TestClock(AtomicI64::new(START)));
        let provider = KubernetesCampusProvider::new(
            KubernetesCampusProviderConfig::m1_lab(),
            [profile(RESEARCH), profile(CODING), profile(SECURITY)],
            authority.clone(),
            kube.clone(),
            clock.clone(),
        )
        .expect("provider config validates");
        Self {
            kube,
            authority,
            clock,
            provider,
        }
    }

    fn admit(&self, session_id: &str, policy: EffectiveCampusPolicy) {
        self.authority.insert(CampusSessionAuthority {
            session_id: session_id.into(),
            course_id: "course-101".into(),
            owner: CampusActorRef {
                tenant_id: "tenant-a".into(),
                identity_id: "idn-student-a".into(),
            },
            policy,
            expires_at: START + 3_600,
        });
    }

    fn spec(&self, session_id: &str, slot: &str, generation: i64) -> CampusProvisionSpec {
        let snapshot = futures_lite_block(self.authority.session(session_id))
            .expect("authority source")
            .expect("session admitted");
        CampusProvisionSpec {
            workload: workload(session_id, slot, generation),
            tenant_id: "tenant-a".into(),
            course_id: "course-101".into(),
            profile: snapshot.authority.policy.profile().clone(),
            expires_at: START + 3_600,
        }
    }

    fn job(&self) -> Value {
        let jobs = self.kube.state().of_kind(CampusKubeKind::Job);
        assert_eq!(jobs.len(), 1, "exactly one Job");
        jobs[0].clone()
    }
}

/// The in-memory authority source never awaits, so polling once completes it.
fn futures_lite_block<F: std::future::Future>(future: F) -> F::Output {
    let waker = std::task::Waker::noop();
    let mut context = std::task::Context::from_waker(waker);
    let mut future = std::pin::pin!(future);
    match future.as_mut().poll(&mut context) {
        std::task::Poll::Ready(output) => output,
        std::task::Poll::Pending => panic!("in-memory future did not complete"),
    }
}

fn workload(session_id: &str, slot: &str, generation: i64) -> CampusWorkloadRef {
    CampusWorkloadRef {
        session_id: session_id.into(),
        slot_id: slot.into(),
        slot_generation: generation,
    }
}

fn rejected(outcome: CampusProvisionOutcome) -> String {
    match outcome {
        CampusProvisionOutcome::Rejected { reason } => reason,
        other => panic!("expected a rejection, got {other:?}"),
    }
}

fn assert_ownership(object: &Value, record: &str, generation: i64) {
    let labels = &object["metadata"]["labels"];
    assert_eq!(labels[LABEL_CONTROLLER], "pharness-campus-controller");
    assert_eq!(labels[LABEL_TENANT], "tenant-a");
    assert_eq!(labels[LABEL_SESSION], "session-a");
    assert_eq!(labels[LABEL_SLOT], "campus-lab-slot-1");
    assert_eq!(labels[LABEL_GENERATION], generation.to_string());
    assert_eq!(labels[LABEL_RECORD], record);
}

#[tokio::test]
async fn approved_research_template_runs_on_the_qualified_runtime_and_pool() {
    let harness = Harness::new();
    harness.admit("session-a", compose(RESEARCH, json!({})));
    let spec = harness.spec("session-a", "campus-lab-slot-1", 1);

    let outcome = harness
        .provider
        .provision(OP, &spec)
        .await
        .expect("provision");
    let name = session_resource_name(&spec.workload);
    assert_eq!(
        outcome,
        CampusProvisionOutcome::InProgress {
            workspace_id: name.clone()
        }
    );

    let job = harness.job();
    assert_eq!(job["metadata"]["namespace"], "campus-lab-slot-1");
    assert_ownership(&job, RECORD_WORKLOAD, 1);
    assert_ownership(&job["spec"]["template"], RECORD_WORKLOAD, 1);
    assert_eq!(job["metadata"]["annotations"][ANNOTATION_OP_KEY], OP);
    let pod = &job["spec"]["template"]["spec"];
    assert_eq!(pod["runtimeClassName"], "gvisor");
    assert_eq!(pod["nodeSelector"], json!({ POOL.0: POOL.1 }));
    assert_eq!(pod["automountServiceAccountToken"], false);
    assert_eq!(pod["enableServiceLinks"], false);
    assert_eq!(pod["hostNetwork"], false);
    assert_eq!(pod["restartPolicy"], "Never");
    assert!(pod.get("serviceAccountName").is_none());
    assert!(pod.get("nodeName").is_none());
    let container = &pod["containers"][0];
    assert_eq!(container["image"], profile(RESEARCH).image);
    assert_eq!(container["securityContext"]["privileged"], false);
    assert_eq!(
        container["securityContext"]["allowPrivilegeEscalation"],
        false
    );
    assert_eq!(container["securityContext"]["readOnlyRootFilesystem"], true);
    assert_eq!(
        container["securityContext"]["capabilities"]["drop"],
        json!(["ALL"])
    );
    // 1024 MiB of process memory plus the 1024 MiB memory-backed workspace.
    assert_eq!(container["resources"]["limits"]["memory"], "2048Mi");
    assert_eq!(container["resources"]["limits"]["cpu"], "500m");
    assert_eq!(
        container["resources"]["limits"]["ephemeral-storage"],
        "128Mi"
    );
    assert_eq!(
        container["resources"]["requests"],
        container["resources"]["limits"]
    );
    let script = container["command"][2].as_str().expect("entrypoint");
    assert!(
        script
            .starts_with("{ ulimit -u 128 || ulimit -p 128; } 2>/dev/null && ulimit -c 0 && exec"),
        "{script}"
    );
    assert_eq!(container["command"][4], "sleep");
    let volumes = pod["volumes"].as_array().expect("volumes");
    assert!(volumes
        .iter()
        .all(|volume| volume["emptyDir"]["medium"] == "Memory"));
    assert_eq!(volumes[0]["emptyDir"]["sizeLimit"], "768Mi");
    assert_eq!(volumes[1]["emptyDir"]["sizeLimit"], "256Mi");
    let env = container["env"].as_array().expect("env");
    assert!(env.iter().all(|entry| entry.get("valueFrom").is_none()));
    // The deadline is the shorter of the remaining authority (3600 s) and the
    // profile TTL (1800 s).
    assert_eq!(job["spec"]["activeDeadlineSeconds"], 1_800);
    assert_eq!(pod["activeDeadlineSeconds"], 1_800);
    assert_eq!(job["spec"]["backoffLimit"], 0);

    let state = harness.kube.state();
    let anchor = &state.of_kind(CampusKubeKind::ConfigMap)[0];
    assert_ownership(anchor, RECORD_ANCHOR, 1);
    assert_eq!(anchor["immutable"], true);
    assert_eq!(
        job["metadata"]["ownerReferences"][0]["uid"],
        anchor["metadata"]["uid"]
    );
}

#[tokio::test]
async fn repeated_create_is_idempotent_and_reports_ready_once_running() {
    let harness = Harness::new();
    harness.admit("session-a", compose(RESEARCH, json!({})));
    let spec = harness.spec("session-a", "campus-lab-slot-1", 1);
    harness.provider.provision(OP, &spec).await.expect("first");
    let objects_after_first = harness.kube.state().objects.len();

    let again = harness.provider.provision(OP, &spec).await.expect("again");
    assert!(matches!(again, CampusProvisionOutcome::InProgress { .. }));
    assert_eq!(harness.kube.state().objects.len(), objects_after_first);
    assert_eq!(harness.kube.state().count(CampusKubeKind::Job), 1);

    harness.kube.state().start_pods(true);
    let ready = harness.provider.provision(OP, &spec).await.expect("ready");
    assert_eq!(
        ready,
        CampusProvisionOutcome::Ready {
            workspace_id: session_resource_name(&spec.workload)
        }
    );
    assert_eq!(
        harness
            .provider
            .observe(&spec.workload)
            .await
            .expect("observe"),
        CampusObservation::Ready {
            workspace_id: session_resource_name(&spec.workload)
        }
    );
    // Pods inherit every ownership label from the template.
    let pods = harness.kube.state().of_kind(CampusKubeKind::Pod);
    assert_ownership(&pods[0], RECORD_WORKLOAD, 1);
}

#[tokio::test]
async fn missing_or_unqualified_runtime_class_refuses_and_creates_nothing() {
    let missing = Harness::with_kube(FakeKube::default());
    missing.admit("session-a", compose(RESEARCH, json!({})));
    let spec = missing.spec("session-a", "campus-lab-slot-1", 1);
    let reason = rejected(missing.provider.provision(OP, &spec).await.expect("call"));
    assert!(
        reason.contains("RuntimeClass gvisor is not installed"),
        "{reason}"
    );
    assert!(missing.kube.state().creates.is_empty());

    let wrong_handler = Harness::with_kube(FakeKube::with_runtime_class("runc", POOL));
    wrong_handler.admit("session-a", compose(RESEARCH, json!({})));
    let reason = rejected(
        wrong_handler
            .provider
            .provision(OP, &spec)
            .await
            .expect("call"),
    );
    assert!(reason.contains("does not use handler runsc"), "{reason}");

    let wrong_pool = Harness::with_kube(FakeKube::with_runtime_class(
        "runsc",
        ("kubernetes.io/os", "linux"),
    ));
    wrong_pool.admit("session-a", compose(RESEARCH, json!({})));
    let reason = rejected(
        wrong_pool
            .provider
            .provision(OP, &spec)
            .await
            .expect("call"),
    );
    assert!(reason.contains("qualified pool"), "{reason}");
    assert!(wrong_pool.kube.state().creates.is_empty());
}

#[tokio::test]
async fn security_profile_and_unenforceable_egress_are_refused() {
    let harness = Harness::new();
    harness.admit("session-sec", compose(SECURITY, json!({})));
    let spec = harness.spec("session-sec", "campus-lab-slot-1", 1);
    let reason = rejected(harness.provider.provision(OP, &spec).await.expect("call"));
    assert!(reason.contains("security profile"), "{reason}");

    harness.admit("session-egress", compose(CODING, json!({})));
    let spec = harness.spec("session-egress", "campus-lab-slot-2", 1);
    let reason = rejected(harness.provider.provision(OP, &spec).await.expect("call"));
    assert!(reason.contains("egress"), "{reason}");
    assert!(harness.kube.state().creates.is_empty());
}

#[tokio::test]
async fn coding_profile_narrowed_to_the_slot_launches_and_oversized_is_refused() {
    let harness = Harness::new();
    harness.admit("session-code", compose(CODING, lab_coding()));
    let spec = harness.spec("session-code", "campus-lab-slot-1", 1);
    assert!(matches!(
        harness.provider.provision(OP, &spec).await.expect("call"),
        CampusProvisionOutcome::InProgress { .. }
    ));
    let job = harness.job();
    let container = &job["spec"]["template"]["spec"]["containers"][0];
    assert_eq!(container["image"], profile(CODING).image);
    assert!(container["command"][2]
        .as_str()
        .expect("entrypoint")
        .contains("ulimit -u 256 || ulimit -p 256"));

    harness.admit("session-big", compose(CODING, json!({ "egress": [] })));
    let spec = harness.spec("session-big", "campus-lab-slot-2", 1);
    let reason = rejected(
        harness
            .provider
            .provision("op-big", &spec)
            .await
            .expect("call"),
    );
    assert!(reason.contains("does not fit the lab slot"), "{reason}");
    harness.admit(
        "session-mem",
        compose(CODING, json!({ "egress": [], "cpu_millicores": 1500 })),
    );
    let spec = harness.spec("session-mem", "campus-lab-slot-3", 1);
    let reason = rejected(
        harness
            .provider
            .provision("op-mem", &spec)
            .await
            .expect("call"),
    );
    assert!(reason.contains("memory 8192 MiB"), "{reason}");
    assert_eq!(harness.kube.state().count(CampusKubeKind::Job), 1);
}

#[tokio::test]
async fn authority_mismatches_are_refused_before_any_effect() {
    let harness = Harness::new();
    harness.admit("session-a", compose(RESEARCH, json!({})));
    let good = harness.spec("session-a", "campus-lab-slot-1", 1);

    let mut other_tenant = good.clone();
    other_tenant.tenant_id = "tenant-b".into();
    let mut other_profile = good.clone();
    other_profile.profile = profile(CODING).profile_ref();
    let mut forged_hash = good.clone();
    forged_hash.profile.profile_hash = "0".repeat(64);
    let mut unknown_slot = good.clone();
    unknown_slot.workload.slot_id = "campus-lab-slot-9".into();
    let mut unknown_session = good.clone();
    unknown_session.workload.session_id = "session-z".into();
    let mut expired = good.clone();
    expired.expires_at = START;
    for (spec, expected) in [
        (other_tenant, "tenant or course"),
        (other_profile, "differs from the effective policy"),
        (forged_hash, "differs from the effective policy"),
        (unknown_slot, "not a configured lab slot"),
        (unknown_session, "no trusted authority"),
        (expired, "expired"),
    ] {
        let reason = rejected(harness.provider.provision(OP, &spec).await.expect("call"));
        assert!(reason.contains(expected), "{expected}: {reason}");
    }

    harness.authority.revoke("session-a");
    let reason = rejected(harness.provider.provision(OP, &good).await.expect("call"));
    assert!(reason.contains("revoked"), "{reason}");
    assert!(harness.kube.state().creates.is_empty());
}

#[tokio::test]
async fn an_unlisted_profile_version_is_not_launchable() {
    let kube = Arc::new(FakeKube::with_runtime_class("runsc", POOL));
    let authority = Arc::new(InMemoryCampusAuthoritySource::default());
    let provider = KubernetesCampusProvider::new(
        KubernetesCampusProviderConfig::m1_lab(),
        [profile(CODING)],
        authority.clone(),
        kube.clone(),
        Arc::new(TestClock(AtomicI64::new(START))),
    )
    .expect("provider");
    let policy = compose(RESEARCH, json!({}));
    let spec = CampusProvisionSpec {
        workload: workload("session-a", "campus-lab-slot-1", 1),
        tenant_id: "tenant-a".into(),
        course_id: "course-101".into(),
        profile: policy.profile().clone(),
        expires_at: START + 3_600,
    };
    authority.insert(CampusSessionAuthority {
        session_id: "session-a".into(),
        course_id: "course-101".into(),
        owner: CampusActorRef {
            tenant_id: "tenant-a".into(),
            identity_id: "idn".into(),
        },
        policy,
        expires_at: START + 3_600,
    });
    let reason = rejected(provider.provision(OP, &spec).await.expect("call"));
    assert!(
        reason.contains("not approved for the lab slots"),
        "{reason}"
    );
}

#[tokio::test]
async fn an_operation_key_reused_for_another_workload_is_refused() {
    let harness = Harness::new();
    harness.admit("session-a", compose(RESEARCH, json!({})));
    harness.admit("session-b", compose(RESEARCH, json!({})));
    let first = harness.spec("session-a", "campus-lab-slot-1", 1);
    harness.provider.provision(OP, &first).await.expect("first");

    let elsewhere = harness.spec("session-b", "campus-lab-slot-2", 1);
    let reason = rejected(
        harness
            .provider
            .provision(OP, &elsewhere)
            .await
            .expect("call"),
    );
    assert!(reason.contains("reused"), "{reason}");

    let mut changed = first.clone();
    changed.expires_at = START + 1_800;
    let reason = rejected(
        harness
            .provider
            .provision(OP, &changed)
            .await
            .expect("call"),
    );
    assert!(reason.contains("reused with a different spec"), "{reason}");

    let reason = rejected(
        harness
            .provider
            .provision("op-other", &first)
            .await
            .expect("call"),
    );
    assert!(reason.contains("another provision operation"), "{reason}");
    assert_eq!(harness.kube.state().count(CampusKubeKind::Job), 1);
}

#[tokio::test]
async fn a_stale_generation_cannot_provision_after_a_newer_occupant() {
    let harness = Harness::new();
    harness.admit("session-old", compose(RESEARCH, json!({})));
    harness.admit("session-new", compose(RESEARCH, json!({})));
    let old = harness.spec("session-old", "campus-lab-slot-1", 1);
    let new = harness.spec("session-new", "campus-lab-slot-1", 2);
    harness
        .provider
        .provision("op-new", &new)
        .await
        .expect("new");

    let reason = rejected(
        harness
            .provider
            .provision("op-old", &old)
            .await
            .expect("call"),
    );
    assert!(reason.contains("newer occupant"), "{reason}");
    // Observing or deleting the stale occupancy never touches the newer one.
    assert_eq!(
        harness
            .provider
            .observe(&old.workload)
            .await
            .expect("observe"),
        CampusObservation::Absent
    );
    assert_eq!(
        harness
            .provider
            .delete("op-old-delete", &old.workload)
            .await
            .expect("delete"),
        CampusDeleteOutcome::Deleted
    );
    assert_eq!(harness.kube.state().count(CampusKubeKind::Job), 1);
    let job = harness.job();
    assert_eq!(job["metadata"]["labels"][LABEL_GENERATION], "2");
}

#[tokio::test]
async fn delete_fences_the_occupancy_and_is_idempotent() {
    let harness = Harness::new();
    harness.admit("session-a", compose(RESEARCH, json!({})));
    let spec = harness.spec("session-a", "campus-lab-slot-1", 1);
    harness
        .provider
        .provision(OP, &spec)
        .await
        .expect("provision");
    harness.kube.state().start_pods(true);

    let outcome = harness
        .provider
        .delete("op-delete", &spec.workload)
        .await
        .expect("delete");
    assert_eq!(outcome, CampusDeleteOutcome::Deleted);
    {
        let state = harness.kube.state();
        assert_eq!(state.count(CampusKubeKind::Job), 0);
        assert_eq!(state.count(CampusKubeKind::Pod), 0);
        let records = state.of_kind(CampusKubeKind::ConfigMap);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["data"]["state"], "tombstone");
        assert_ownership(&records[0], RECORD_TOMBSTONE, 1);
    }
    assert_eq!(
        harness
            .provider
            .delete("op-delete", &spec.workload)
            .await
            .expect("again"),
        CampusDeleteOutcome::Deleted
    );

    // A stale worker's late provision cannot resurrect the occupancy.
    let reason = rejected(harness.provider.provision(OP, &spec).await.expect("late"));
    assert!(reason.contains("already deleted"), "{reason}");
    assert_eq!(harness.kube.state().count(CampusKubeKind::Job), 0);
    assert!(harness
        .provider
        .list_owned()
        .await
        .expect("list")
        .is_empty());
}

#[tokio::test]
async fn delete_during_provision_removes_the_late_job() {
    let harness = Harness::new();
    harness.admit("session-a", compose(RESEARCH, json!({})));
    let spec = harness.spec("session-a", "campus-lab-slot-1", 1);
    let name = session_resource_name(&spec.workload);
    harness
        .kube
        .after_next_job_create(move |state| state.tombstone_now("campus-lab-slot-1", &name));
    let reason = rejected(harness.provider.provision(OP, &spec).await.expect("call"));
    assert!(
        reason.contains("deleted while it was being provisioned"),
        "{reason}"
    );
    assert_eq!(harness.kube.state().count(CampusKubeKind::Job), 0);
}

#[tokio::test]
async fn a_newer_occupancy_prunes_older_tombstones() {
    let harness = Harness::new();
    harness.admit("session-old", compose(RESEARCH, json!({})));
    harness.admit("session-new", compose(RESEARCH, json!({})));
    let old = harness.spec("session-old", "campus-lab-slot-1", 1);
    harness
        .provider
        .provision("op-old", &old)
        .await
        .expect("old");
    harness
        .provider
        .delete("del-old", &old.workload)
        .await
        .expect("delete old");
    let new = harness.spec("session-new", "campus-lab-slot-1", 2);
    harness
        .provider
        .provision("op-new", &new)
        .await
        .expect("new");
    harness
        .provider
        .delete("del-new", &new.workload)
        .await
        .expect("delete new");
    let records = harness.kube.state().of_kind(CampusKubeKind::ConfigMap);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["metadata"]["labels"][LABEL_GENERATION], "2");
}

#[tokio::test]
async fn unconfirmed_deletion_is_never_reported_as_deleted() {
    let harness = Harness::new();
    harness.admit("session-a", compose(RESEARCH, json!({})));
    let spec = harness.spec("session-a", "campus-lab-slot-1", 1);
    harness
        .provider
        .provision(OP, &spec)
        .await
        .expect("provision");
    harness.kube.state().start_pods(true);
    harness.kube.state().pods_hang_on_delete = true;

    // The fake marks deletions at START; within the grace window it waits.
    assert_eq!(
        harness
            .provider
            .delete("del", &spec.workload)
            .await
            .expect("delete"),
        CampusDeleteOutcome::InProgress
    );
    harness.clock.0.store(START + 600, Ordering::SeqCst);
    let outcome = harness
        .provider
        .delete("del", &spec.workload)
        .await
        .expect("delete");
    assert!(
        matches!(&outcome, CampusDeleteOutcome::Unreachable { reason } if reason.contains("campus-node")),
        "{outcome:?}"
    );

    harness.kube.state().unreachable = true;
    assert!(matches!(
        harness
            .provider
            .delete("del", &spec.workload)
            .await
            .expect("delete"),
        CampusDeleteOutcome::Unreachable { .. }
    ));
    assert!(matches!(
        harness.provider.provision(OP, &spec).await,
        Err(CampusProviderError::Unreachable(_))
    ));
}

#[tokio::test]
async fn a_job_held_by_a_finalizer_blocks_cleanup() {
    let harness = Harness::new();
    harness.admit("session-a", compose(RESEARCH, json!({})));
    let spec = harness.spec("session-a", "campus-lab-slot-1", 1);
    harness
        .provider
        .provision(OP, &spec)
        .await
        .expect("provision");
    harness.kube.state().jobs_hang_on_delete = true;
    harness.clock.0.store(START + 600, Ordering::SeqCst);
    let outcome = harness
        .provider
        .delete("del", &spec.workload)
        .await
        .expect("delete");
    assert!(
        matches!(&outcome, CampusDeleteOutcome::Blocked { reason } if reason.contains("finalizers")),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn admission_refusal_is_a_rejection_not_a_retry() {
    let harness = Harness::new();
    harness.kube.state().reject_kind = Some((
        CampusKubeKind::Job,
        "ValidatingAdmissionPolicy 'pharness-campus-lab-session-guard' denied request".into(),
    ));
    harness.admit("session-a", compose(RESEARCH, json!({})));
    let spec = harness.spec("session-a", "campus-lab-slot-1", 1);
    let reason = rejected(harness.provider.provision(OP, &spec).await.expect("call"));
    assert!(
        reason.contains("lab slot refused the session Job"),
        "{reason}"
    );
    // The anchor remains for the journal's cleanup to fence and remove.
    harness.kube.state().reject_kind = None;
    assert_eq!(
        harness
            .provider
            .delete("del", &spec.workload)
            .await
            .expect("delete"),
        CampusDeleteOutcome::Deleted
    );
}

#[tokio::test]
async fn failed_workloads_are_observed_as_failed() {
    let harness = Harness::new();
    harness.admit("session-a", compose(RESEARCH, json!({})));
    let spec = harness.spec("session-a", "campus-lab-slot-1", 1);
    harness
        .provider
        .provision(OP, &spec)
        .await
        .expect("provision");
    harness.kube.state().start_pods(false);
    assert_eq!(
        harness
            .provider
            .observe(&spec.workload)
            .await
            .expect("observe"),
        CampusObservation::Provisioning
    );
    harness.kube.state().set_pod_status(json!({
        "phase": "Pending",
        "containerStatuses": [{ "state": { "waiting": { "reason": "ImagePullBackOff" } } }],
    }));
    assert!(matches!(
        harness.provider.observe(&spec.workload).await.expect("observe"),
        CampusObservation::Failed { reason } if reason.contains("ImagePullBackOff")
    ));
    harness.kube.state().set_job_status(json!({
        "conditions": [{ "type": "Failed", "status": "True", "reason": "DeadlineExceeded" }],
    }));
    let reason = rejected(harness.provider.provision(OP, &spec).await.expect("call"));
    assert!(reason.contains("DeadlineExceeded"), "{reason}");
}

#[tokio::test]
async fn list_owned_reports_every_live_resource_with_its_operation() {
    let harness = Harness::new();
    harness.admit("session-a", compose(RESEARCH, json!({})));
    let spec = harness.spec("session-a", "campus-lab-slot-1", 1);
    harness
        .provider
        .provision(OP, &spec)
        .await
        .expect("provision");
    harness.kube.state().start_pods(true);
    let owned = harness.provider.list_owned().await.expect("list");
    let kinds = owned
        .iter()
        .map(|resource| resource.kind.as_str())
        .collect::<Vec<_>>();
    assert_eq!(kinds, ["anchor", "job", "pod"]);
    assert!(owned
        .iter()
        .all(|resource| resource.workload == spec.workload && resource.op_key == OP));
}

#[test]
fn template_refuses_what_the_slots_cannot_provide() {
    let research = profile(RESEARCH);
    let denied = CampusSandboxNetwork {
        mode: crate::CampusNetworkMode::Denied,
        egress: BTreeSet::new(),
    };
    assert_eq!(check_launchable_in_slot(&research, &denied), Ok(()));
    assert!(matches!(
        check_launchable_in_slot(&profile(SECURITY), &denied),
        Err(CampusTemplateError::SecurityProfile(_))
    ));
    let mut runc = research.clone();
    runc.isolation = crate::CampusIsolation::HardwareIsolated;
    assert!(matches!(
        check_launchable_in_slot(&runc, &denied),
        Err(CampusTemplateError::Isolation(..))
    ));
    let egress = CampusSandboxNetwork {
        mode: crate::CampusNetworkMode::ApprovedEgress,
        egress: profile(CODING).network.egress.iter().cloned().collect(),
    };
    assert!(matches!(
        check_launchable_in_slot(&research, &egress),
        Err(CampusTemplateError::ApprovedEgress(5))
    ));
}

#[test]
fn provider_config_refuses_credentials_and_inconsistent_capacity() {
    assert_eq!(KubernetesCampusProviderConfig::m1_lab().validate(), Ok(()));
    for (name, value) in [
        ("PHARNESS_WORKER_TOKEN", "x"),
        ("OPENAI_API_KEY", "x"),
        ("AWS_REGION", "us-east-1"),
        ("HOME", "/root"),
        ("lower", "x"),
    ] {
        let mut config = KubernetesCampusProviderConfig::m1_lab();
        config.environment.insert(name.into(), value.into());
        assert!(config.validate().is_err(), "{name}");
    }
    let mut config = KubernetesCampusProviderConfig::m1_lab();
    config.log_storage_mib = 512;
    assert!(config.validate().is_err());
    let mut config = KubernetesCampusProviderConfig::m1_lab();
    config
        .slots
        .insert("campus-lab-slot-6".into(), "campus-lab-slot-1".into());
    assert!(config.validate().is_err());
    let mut config = KubernetesCampusProviderConfig::m1_lab();
    config.runtime.node_selector.clear();
    assert!(config.validate().is_err());
}

#[test]
fn sizing_counts_memory_backed_volumes_and_runtime_overhead() {
    let config = KubernetesCampusProviderConfig::m1_lab();
    let research = compose(RESEARCH, json!({}));
    let sizing = CampusSessionSizing::derive(research.constraints(), &config).expect("fits");
    assert_eq!(sizing.memory_limit_mib, 2048);
    assert_eq!((sizing.workspace_mib, sizing.tmp_mib), (768, 256));
    let tight = compose(RESEARCH, json!({ "ephemeral_storage_mib": 128 }));
    let sizing = CampusSessionSizing::derive(tight.constraints(), &config).expect("fits");
    assert_eq!((sizing.workspace_mib, sizing.tmp_mib), (96, 32));
    // 2944 MiB of processes and volumes plus 128 MiB overhead is the edge.
    let edge = compose(RESEARCH, json!({}));
    let mut constraints = edge.constraints().clone();
    constraints.memory_mib = 1920;
    assert!(CampusSessionSizing::derive(&constraints, &config).is_ok());
    constraints.memory_mib = 1921;
    assert!(matches!(
        CampusSessionSizing::derive(&constraints, &config),
        Err(CampusTemplateError::Capacity(_))
    ));
}

#[test]
fn rendered_environment_is_checked_for_credentials() {
    let research = compose(RESEARCH, json!({}));
    let config = KubernetesCampusProviderConfig::m1_lab();
    // Bypasses validate() to prove the render-time check is independent.
    let mut leaky = config.clone();
    leaky.environment.insert("GITHUB_TOKEN".into(), "x".into());
    let workload = workload("session-a", "campus-lab-slot-1", 1);
    let profile = profile(RESEARCH);
    let network = CampusSandboxNetwork {
        mode: crate::CampusNetworkMode::Denied,
        egress: BTreeSet::new(),
    };
    let input = CampusJobTemplateInput {
        config: &leaky,
        namespace: "campus-lab-slot-1",
        workload: &workload,
        tenant_id: "tenant-a",
        course_id: "course-101",
        op_key: OP,
        spec_sha256: "abc",
        profile: &profile,
        constraints: research.constraints(),
        network: &network,
        effective_policy_hash: research.effective_policy_hash(),
        expires_at: START + 60,
        active_deadline_seconds: 60,
    };
    assert!(matches!(
        render_session_job(&input),
        Err(CampusTemplateError::Environment(_))
    ));
    let mut bad_tenant = input;
    bad_tenant.config = &config;
    bad_tenant.tenant_id = "Tenant A";
    assert!(matches!(
        render_session_job(&bad_tenant),
        Err(CampusTemplateError::Label { .. })
    ));
    let mut long = input;
    long.config = &config;
    long.active_deadline_seconds = 14_401;
    assert!(matches!(
        render_session_job(&long),
        Err(CampusTemplateError::Deadline { .. })
    ));
}

#[test]
fn resource_names_and_selectors_are_deterministic_and_exact() {
    let first = workload("session-a", "campus-lab-slot-1", 1);
    let second = workload("session-a", "campus-lab-slot-1", 2);
    assert_eq!(
        session_resource_name(&first),
        session_resource_name(&first.clone())
    );
    assert_ne!(
        session_resource_name(&first),
        session_resource_name(&second)
    );
    assert_eq!(session_resource_name(&first).len(), "campus-".len() + 24);
    let job = json!({ "metadata": { "labels": {
        LABEL_CONTROLLER: "pharness-campus-controller",
        LABEL_SESSION: "session-a",
        LABEL_SLOT: "campus-lab-slot-1",
        LABEL_GENERATION: "2",
        LABEL_RECORD: RECORD_WORKLOAD,
    } } });
    let controller = "pharness-campus-controller";
    assert!(matches_selector(
        &job,
        &record_selector(controller, &second, RECORD_WORKLOAD)
    ));
    assert!(!matches_selector(
        &job,
        &record_selector(controller, &first, RECORD_WORKLOAD)
    ));
    assert!(!matches_selector(
        &job,
        &record_selector("other", &second, RECORD_WORKLOAD)
    ));
    assert_eq!(op_label(OP).len(), 40);
    assert!(is_label_value(&op_label(OP)));
}

#[test]
fn kubectl_errors_are_classified_conservatively() {
    let cases = [
        (
            "Error from server (AlreadyExists): configmaps \"campus-x\" already exists",
            "already",
        ),
        (
            "The jobs \"campus-x\" is invalid: : ValidatingAdmissionPolicy \
             'pharness-campus-lab-session-guard' with binding denied request",
            "rejected",
        ),
        (
            "Error from server (Forbidden): jobs.batch is forbidden: exceeded quota",
            "rejected",
        ),
        (
            "Unable to connect to the server: dial tcp 10.0.0.1:6443: i/o timeout",
            "unreachable",
        ),
        ("", "unreachable"),
        ("error: unexpected EOF", "failed"),
    ];
    for (stderr, expected) in cases {
        let got = match classify_kubectl_error(stderr) {
            CampusKubeError::AlreadyExists(_) => "already",
            CampusKubeError::Rejected(_) => "rejected",
            CampusKubeError::Unreachable(_) => "unreachable",
            CampusKubeError::Failed(_) => "failed",
        };
        assert_eq!(got, expected, "{stderr}");
    }
    assert_eq!(
        get_args(CampusKubeKind::RuntimeClass, "ignored", "gvisor"),
        [
            "get",
            "runtimeclasses.node.k8s.io",
            "gvisor",
            "-o",
            "json",
            "--ignore-not-found"
        ]
    );
    assert!(
        delete_args(CampusKubeKind::Job, "campus-lab-slot-1", "campus-x")
            .contains(&"--cascade=background".to_string())
    );
    assert_eq!(list_args(CampusKubeKind::Pod, "ns", "a=b")[5], "a=b");
}

#[test]
fn api_server_timestamps_parse() {
    assert_eq!(
        parse_rfc3339_utc("2030-03-17T17:46:40Z"),
        Some(1_900_000_000)
    );
    assert_eq!(parse_rfc3339_utc("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(parse_rfc3339_utc("2030-03-17T17:46:40.5Z"), None);
    assert_eq!(parse_rfc3339_utc("2030-13-17T17:46:40Z"), None);
}

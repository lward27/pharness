use super::fake_kube::{matches_selector, FakeKube};
use super::*;
use crate::campus_broker_token::{
    CampusBrokerAudience, CampusBrokerSecret, CampusIssuedBrokerToken,
};
use crate::campus_capability::{
    CampusActorRef, CampusSandboxNetwork, CampusSessionAuthority, InMemoryCampusAuthoritySource,
};
use crate::campus_policy::{CampusPolicyLayer, CAMPUS_POLICY_LAYER_SCHEMA};
use crate::campus_sandbox_pod::{
    campus_broker_token_secret_name, CAMPUS_BROKER_TOKEN_DIR, CAMPUS_BROKER_TOKEN_VOLUME,
    CAMPUS_EGRESS_CA_SECRET_KEY,
};
use crate::{EffectiveCampusPolicy, SECURITY_PROFILE_QUALIFICATION};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Mutex;

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

/// Issues tokens like the journal does, bound to the workload, and records
/// them so tests can find the raw value.
struct FakeIssuer {
    clock: Arc<TestClock>,
    issued: Mutex<Vec<CampusIssuedBrokerToken>>,
    refuse: Mutex<Option<CampusBrokerTokenError>>,
}

impl FakeIssuer {
    fn tokens(&self) -> Vec<CampusIssuedBrokerToken> {
        self.issued.lock().expect("issuer lock").clone()
    }
}

#[async_trait]
impl CampusBrokerTokenIssuer for FakeIssuer {
    async fn issue(
        &self,
        workload: &CampusWorkloadRef,
        audience: CampusBrokerAudience,
    ) -> Result<CampusIssuedBrokerToken, CampusBrokerTokenError> {
        if let Some(error) = self.refuse.lock().expect("issuer lock").clone() {
            return Err(error);
        }
        let mut issued = self.issued.lock().expect("issuer lock");
        let index = u8::try_from(issued.len() + 1).expect("few tokens");
        let now = self.clock.now();
        let token = CampusIssuedBrokerToken {
            token_id: format!("cbt-{index}"),
            audience,
            workload: workload.clone(),
            lab_generation: 1,
            issued_at: now,
            expires_at: now + 900,
            secret: CampusBrokerSecret::from_random_bytes(&[index; 32]),
        };
        issued.push(token.clone());
        Ok(token)
    }
}

const TEST_CA: &str =
    "-----BEGIN CERTIFICATE-----\nMIIBszCCAVmgAwIBAgIUY2FtcHVz\n-----END CERTIFICATE-----\n";

fn egress_config() -> KubernetesCampusProviderConfig {
    let mut config = KubernetesCampusProviderConfig::m1_lab();
    config.egress = Some(CampusEgressProxyConfig {
        proxy_url: Url::parse("http://10.43.130.30:3128").expect("url"),
        ca_certificate_pem: TEST_CA.into(),
        network_policy: "campus-lab-session-egress".into(),
    });
    config
}

struct Harness {
    kube: Arc<FakeKube>,
    authority: Arc<InMemoryCampusAuthoritySource>,
    clock: Arc<TestClock>,
    issuer: Arc<FakeIssuer>,
    provider: KubernetesCampusProvider,
}

impl Harness {
    fn new() -> Self {
        Self::with_kube(FakeKube::with_runtime_class("runsc", POOL))
    }

    fn with_kube(kube: FakeKube) -> Self {
        Self::with(kube, KubernetesCampusProviderConfig::m1_lab())
    }

    fn with(kube: FakeKube, config: KubernetesCampusProviderConfig) -> Self {
        let kube = Arc::new(kube);
        let authority = Arc::new(InMemoryCampusAuthoritySource::default());
        let clock = Arc::new(TestClock(AtomicI64::new(START)));
        let issuer = Arc::new(FakeIssuer {
            clock: clock.clone(),
            issued: Mutex::default(),
            refuse: Mutex::default(),
        });
        let provider = KubernetesCampusProvider::new(
            config,
            [profile(RESEARCH), profile(CODING), profile(SECURITY)],
            authority.clone(),
            issuer.clone(),
            kube.clone(),
            clock.clone(),
        )
        .expect("provider config validates");
        Self {
            kube,
            authority,
            clock,
            issuer,
            provider,
        }
    }

    fn secret(&self) -> Value {
        let secrets = self.kube.state().of_kind(CampusKubeKind::Secret);
        assert_eq!(secrets.len(), 1, "exactly one token Secret");
        secrets[0].clone()
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
        .filter(|volume| volume["name"] != CAMPUS_BROKER_TOKEN_VOLUME)
        .all(|volume| volume["emptyDir"]["medium"] == "Memory"));
    assert_eq!(volumes.len(), 3);
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
    assert!(
        reason.contains("no Campus egress proxy is configured"),
        "{reason}"
    );
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
async fn a_session_still_provisioning_is_launched() {
    let harness = Harness::new();
    harness.admit("session-a", compose(RESEARCH, json!({})));
    harness
        .authority
        .set_status("session-a", CampusAuthorityStatus::Provisioning);
    let spec = harness.spec("session-a", "campus-lab-slot-1", 1);
    let outcome = harness.provider.provision(OP, &spec).await.expect("call");
    assert!(
        matches!(outcome, CampusProvisionOutcome::InProgress { .. }),
        "{outcome:?}"
    );
    harness.job();
}

#[tokio::test]
async fn an_unlisted_profile_version_is_not_launchable() {
    let kube = Arc::new(FakeKube::with_runtime_class("runsc", POOL));
    let authority = Arc::new(InMemoryCampusAuthoritySource::default());
    let clock = Arc::new(TestClock(AtomicI64::new(START)));
    let provider = KubernetesCampusProvider::new(
        KubernetesCampusProviderConfig::m1_lab(),
        [profile(CODING)],
        authority.clone(),
        Arc::new(FakeIssuer {
            clock: clock.clone(),
            issued: Mutex::default(),
            refuse: Mutex::default(),
        }),
        kube.clone(),
        clock,
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
async fn a_retried_partial_create_gets_only_the_remaining_lifetime() {
    let harness = Harness::new();
    harness.admit("session-a", compose(RESEARCH, json!({})));
    let spec = harness.spec("session-a", "campus-lab-slot-1", 1);
    // The anchor and token exist, then the API server goes away before the
    // Job is created: a partial create.
    harness.kube.state().fail_next_create = Some(CampusKubeKind::Job);
    assert!(matches!(
        harness.provider.provision(OP, &spec).await,
        Err(CampusProviderError::Unreachable(_))
    ));
    assert_eq!(harness.kube.state().count(CampusKubeKind::Job), 0);
    assert_eq!(harness.kube.state().count(CampusKubeKind::ConfigMap), 1);

    // A restarted controller retries 2000 s later. The Job only gets what is
    // left of the absolute expiry fixed at launch (START + 3600).
    harness.clock.0.store(START + 2_000, Ordering::SeqCst);
    harness.provider.provision(OP, &spec).await.expect("retry");
    let job = harness.job();
    assert_eq!(job["spec"]["activeDeadlineSeconds"], 1_600);
    assert_eq!(
        job["spec"]["template"]["spec"]["activeDeadlineSeconds"],
        1_600
    );

    // Later replays never re-create or extend the Job.
    harness.clock.0.store(START + 2_500, Ordering::SeqCst);
    harness.provider.provision(OP, &spec).await.expect("replay");
    let replayed = harness.job();
    assert_eq!(replayed["metadata"]["uid"], job["metadata"]["uid"]);
    assert_eq!(replayed["spec"]["activeDeadlineSeconds"], 1_600);
    let job_creates = harness
        .kube
        .state()
        .creates
        .iter()
        .filter(|create| create.starts_with("Job/"))
        .count();
    assert_eq!(job_creates, 2, "the failed attempt and one create");

    // At the absolute expiry nothing is launched any more.
    harness.clock.0.store(START + 3_600, Ordering::SeqCst);
    let refused = rejected(
        harness
            .provider
            .provision(OP, &spec)
            .await
            .expect("expired"),
    );
    assert!(refused.contains("expired"), "{refused}");
    assert_eq!(harness.job()["metadata"]["uid"], job["metadata"]["uid"]);
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
    assert_eq!(kinds, ["anchor", "broker-token", "job", "pod"]);
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
    assert_eq!(check_launchable_in_slot(&research, &denied, false), Ok(()));
    assert!(matches!(
        check_launchable_in_slot(&profile(SECURITY), &denied, true),
        Err(CampusTemplateError::SecurityProfile(_))
    ));
    let mut runc = research.clone();
    runc.isolation = crate::CampusIsolation::HardwareIsolated;
    assert!(matches!(
        check_launchable_in_slot(&runc, &denied, true),
        Err(CampusTemplateError::Isolation(..))
    ));
    let egress = CampusSandboxNetwork {
        mode: crate::CampusNetworkMode::ApprovedEgress,
        egress: profile(CODING).network.egress.iter().cloned().collect(),
    };
    assert!(matches!(
        check_launchable_in_slot(&research, &egress, false),
        Err(CampusTemplateError::ApprovedEgress(5))
    ));
    assert_eq!(check_launchable_in_slot(&research, &egress, true), Ok(()));
    assert_eq!(
        approved_egress_hosts(&egress).expect("approved"),
        BTreeSet::from([
            "codeload.github.com",
            "files.pythonhosted.org",
            "github.com",
            "pypi.org",
            "registry.npmjs.org",
        ])
    );
    // Anything outside the approved routes refuses the whole session, even
    // with the proxy configured.
    for (host, port) in [
        ("example.com", 443),
        ("pypi.org", 80),
        ("PYPI.ORG", 443),
        ("pypi.org.", 443),
        ("140.82.112.3", 443),
        ("169.254.169.254", 443),
        ("kubernetes.default.svc", 443),
    ] {
        let mut network = egress.clone();
        network.egress.insert(crate::CampusEgressDestination {
            host: host.into(),
            port,
        });
        assert!(
            matches!(
                check_launchable_in_slot(&research, &network, true),
                Err(CampusTemplateError::UnapprovedEgress(_))
            ),
            "{host}:{port}"
        );
    }
    let mut inconsistent = egress.clone();
    inconsistent.mode = crate::CampusNetworkMode::Denied;
    assert!(check_launchable_in_slot(&research, &inconsistent, true).is_err());
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
    let binding = CampusSandboxBrokerBinding::new(&config.broker_url, &workload).expect("binding");
    let input = CampusJobTemplateInput {
        binding: &binding,
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

fn env_of(container: &Value) -> BTreeMap<String, String> {
    container["env"]
        .as_array()
        .expect("env")
        .iter()
        .map(|entry| {
            (
                entry["name"].as_str().expect("name").to_string(),
                entry["value"].as_str().expect("literal value").to_string(),
            )
        })
        .collect()
}

#[tokio::test]
async fn every_session_gets_its_scoped_token_and_the_network_gate() {
    let harness = Harness::new();
    harness.admit("session-a", compose(RESEARCH, json!({})));
    let spec = harness.spec("session-a", "campus-lab-slot-1", 1);
    harness
        .provider
        .provision(OP, &spec)
        .await
        .expect("provision");

    let job = harness.job();
    let pod = &job["spec"]["template"]["spec"];
    let secret_name = campus_broker_token_secret_name(&spec.workload);
    let volumes = pod["volumes"].as_array().expect("volumes");
    let token_volume = volumes
        .iter()
        .find(|volume| volume["name"] == CAMPUS_BROKER_TOKEN_VOLUME)
        .expect("token volume");
    assert_eq!(token_volume["secret"]["secretName"], secret_name);
    assert_eq!(
        token_volume["secret"]["items"],
        json!([{ "key": "token", "path": "token" }])
    );
    let container = &pod["containers"][0];
    let mount = container["volumeMounts"]
        .as_array()
        .expect("mounts")
        .iter()
        .find(|mount| mount["name"] == CAMPUS_BROKER_TOKEN_VOLUME)
        .expect("token mount")
        .clone();
    assert_eq!(mount["mountPath"], CAMPUS_BROKER_TOKEN_DIR);
    assert_eq!(mount["readOnly"], true);
    let env = env_of(container);
    assert_eq!(env["CAMPUS_BROKER_URL"], "http://10.43.130.31:4790/");
    // No egress: no proxy, no CA variables, no egress label or annotation.
    assert!(!env.contains_key("HTTPS_PROXY") && !env.contains_key("SSL_CERT_FILE"));
    assert!(job["spec"]["template"]["metadata"]["labels"]
        .get(crate::campus_egress::CAMPUS_EGRESS_LABEL)
        .is_none());
    assert!(job["spec"]["template"]["metadata"]["annotations"]
        .get(crate::campus_egress::CAMPUS_EGRESS_HOSTS_ANNOTATION)
        .is_none());

    // The gate runs first, with the same image and confinement but neither
    // the token nor any platform variable.
    let gates = pod["initContainers"].as_array().expect("init containers");
    assert_eq!(gates.len(), 1);
    let gate = &gates[0];
    assert_eq!(gate["name"], NETWORK_GATE_CONTAINER);
    assert_eq!(gate["image"], container["image"]);
    assert_eq!(gate["securityContext"], container["securityContext"]);
    assert_eq!(gate["resources"], container["resources"]);
    assert!(gate.get("volumeMounts").is_none());
    assert_eq!(env_of(gate).len(), 1);
    assert_eq!(gate["command"][4], "60");
    assert_eq!(gate["command"][5], "10.43.0.1:443");

    // The Secret holds the raw token of an inference token for this exact
    // occupancy and is owned by the anchor, so it goes with the occupancy.
    let tokens = harness.issuer.tokens();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].audience, CampusBrokerAudience::Inference);
    assert_eq!(tokens[0].workload, spec.workload);
    let secret = harness.secret();
    assert_eq!(secret["metadata"]["name"], secret_name);
    assert_eq!(secret["stringData"]["token"], tokens[0].secret.expose());
    assert!(secret["stringData"]
        .get(CAMPUS_EGRESS_CA_SECRET_KEY)
        .is_none());
    let anchor = &harness.kube.state().of_kind(CampusKubeKind::ConfigMap)[0];
    assert_eq!(
        secret["metadata"]["ownerReferences"][0]["uid"],
        anchor["metadata"]["uid"]
    );
    assert_ownership(&secret, RECORD_BROKER_TOKEN, 1);
    // The raw token appears in the Secret only, never in the Job.
    assert!(!job.to_string().contains(tokens[0].secret.expose()));

    // A repeated provision keeps the token until it is due for rotation.
    harness.provider.provision(OP, &spec).await.expect("again");
    assert_eq!(harness.issuer.tokens().len(), 1);
    assert!(harness.kube.state().replaces.is_empty());
}

#[tokio::test]
async fn approved_egress_goes_only_through_the_configured_proxy() {
    let harness = Harness::with(FakeKube::with_runtime_class("runsc", POOL), egress_config());
    let narrowed = json!({
        "cpu_millicores": 1500,
        "memory_mib": 1024,
        "ephemeral_storage_mib": 1024,
    });
    harness.admit("session-code", compose(CODING, narrowed));
    let spec = harness.spec("session-code", "campus-lab-slot-1", 1);
    assert!(matches!(
        harness.provider.provision(OP, &spec).await.expect("call"),
        CampusProvisionOutcome::InProgress { .. }
    ));
    let job = harness.job();
    let template = &job["spec"]["template"];
    assert_eq!(
        template["metadata"]["labels"][crate::campus_egress::CAMPUS_EGRESS_LABEL],
        "proxy"
    );
    assert_eq!(
        template["metadata"]["annotations"][crate::campus_egress::CAMPUS_EGRESS_HOSTS_ANNOTATION],
        "codeload.github.com,files.pythonhosted.org,github.com,pypi.org,registry.npmjs.org"
    );
    let env = env_of(&template["spec"]["containers"][0]);
    assert_eq!(env["HTTPS_PROXY"], "http://10.43.130.30:3128");
    assert_eq!(env["NO_PROXY"], "10.43.130.31");
    for name in crate::campus_sandbox_pod::CAMPUS_EGRESS_CA_ENV {
        assert_eq!(env[*name], "/var/run/campus-broker/egress-ca.pem", "{name}");
    }
    assert!(!env.contains_key("HTTP_PROXY"));
    let secret = harness.secret();
    assert_eq!(secret["stringData"][CAMPUS_EGRESS_CA_SECRET_KEY], TEST_CA);

    // Without the slot's egress policy the session is refused before
    // anything is created.
    let missing = Harness::with(FakeKube::with_runtime_class("runsc", POOL), egress_config());
    missing.kube.state().objects.remove(&(
        CampusKubeKind::NetworkPolicy,
        "campus-lab-slot-2".into(),
        "campus-lab-session-egress".into(),
    ));
    missing.admit(
        "session-code",
        compose(
            CODING,
            json!({
                "cpu_millicores": 1500, "memory_mib": 1024, "ephemeral_storage_mib": 1024,
            }),
        ),
    );
    let spec = missing.spec("session-code", "campus-lab-slot-2", 1);
    let reason = rejected(missing.provider.provision(OP, &spec).await.expect("call"));
    assert!(reason.contains("campus-lab-session-egress"), "{reason}");
    assert!(missing.kube.state().creates.is_empty());
    assert!(missing.issuer.tokens().is_empty());
}

#[tokio::test]
async fn missing_or_weakened_slot_isolation_prevents_launch() {
    for (removed, weaken) in [
        (Some("campus-lab-default-deny"), false),
        (Some("campus-lab-session-broker"), false),
        (None, true),
    ] {
        let harness = Harness::new();
        {
            let mut state = harness.kube.state();
            let key = |name: &str| {
                (
                    CampusKubeKind::NetworkPolicy,
                    "campus-lab-slot-1".to_string(),
                    name.to_string(),
                )
            };
            if let Some(name) = removed {
                state.objects.remove(&key(name));
            }
            if weaken {
                let policy = state
                    .objects
                    .get_mut(&key("campus-lab-default-deny"))
                    .expect("policy");
                policy["spec"]["egress"] =
                    json!([{ "to": [{ "ipBlock": { "cidr": "0.0.0.0/0" } }] }]);
            }
        }
        harness.admit("session-a", compose(RESEARCH, json!({})));
        let spec = harness.spec("session-a", "campus-lab-slot-1", 1);
        let reason = rejected(harness.provider.provision(OP, &spec).await.expect("call"));
        assert!(reason.contains("campus-lab-"), "{reason}");
        assert!(harness.kube.state().creates.is_empty(), "{reason}");
        assert!(harness.issuer.tokens().is_empty());
    }
    let unreadable = Harness::new();
    unreadable.kube.state().unreachable = true;
    unreadable.admit("session-a", compose(RESEARCH, json!({})));
    let spec = unreadable.spec("session-a", "campus-lab-slot-1", 1);
    assert!(matches!(
        unreadable.provider.provision(OP, &spec).await,
        Err(CampusProviderError::Unreachable(_))
    ));
}

#[tokio::test]
async fn tokens_rotate_in_place_and_go_with_the_occupancy() {
    let harness = Harness::new();
    harness.admit("session-a", compose(RESEARCH, json!({})));
    let spec = harness.spec("session-a", "campus-lab-slot-1", 1);
    harness
        .provider
        .provision(OP, &spec)
        .await
        .expect("provision");
    harness.kube.state().start_pods(true);
    assert!(!harness
        .provider
        .rotate_broker_token(&spec.workload)
        .await
        .expect("not due"));

    harness.clock.0.store(START + 450, Ordering::SeqCst);
    assert!(harness
        .provider
        .rotate_broker_token(&spec.workload)
        .await
        .expect("rotate"));
    let tokens = harness.issuer.tokens();
    assert_eq!(tokens.len(), 2);
    let secret = harness.secret();
    assert_eq!(secret["stringData"]["token"], tokens[1].secret.expose());
    assert_eq!(
        secret["metadata"]["annotations"]
            [crate::campus_sandbox_pod::CAMPUS_BROKER_TOKEN_ID_ANNOTATION],
        "cbt-2"
    );
    assert_eq!(harness.kube.state().replaces.len(), 1);
    // A repeated provision after the next rotation point rotates too.
    harness.clock.0.store(START + 900, Ordering::SeqCst);
    let spec_later = CampusProvisionSpec {
        expires_at: START + 3_600,
        ..spec.clone()
    };
    harness
        .provider
        .provision(OP, &spec_later)
        .await
        .expect("again");
    assert_eq!(harness.issuer.tokens().len(), 3);

    assert_eq!(
        harness
            .provider
            .delete("del", &spec.workload)
            .await
            .expect("delete"),
        CampusDeleteOutcome::Deleted
    );
    assert_eq!(harness.kube.state().count(CampusKubeKind::Secret), 0);
    assert!(harness
        .provider
        .rotate_broker_token(&spec.workload)
        .await
        .is_err());
}

#[tokio::test]
async fn a_refused_token_means_no_workload() {
    let harness = Harness::new();
    harness.admit("session-a", compose(RESEARCH, json!({})));
    let spec = harness.spec("session-a", "campus-lab-slot-1", 1);
    *harness.issuer.refuse.lock().expect("lock") = Some(CampusBrokerTokenError::NotIssuable(
        "the session was asked to stop".into(),
    ));
    let reason = rejected(harness.provider.provision(OP, &spec).await.expect("call"));
    assert!(reason.contains("no broker token can be issued"), "{reason}");
    assert_eq!(harness.kube.state().count(CampusKubeKind::Job), 0);
    assert_eq!(harness.kube.state().count(CampusKubeKind::Secret), 0);

    *harness.issuer.refuse.lock().expect("lock") = Some(CampusBrokerTokenError::Unavailable(
        "database is down".into(),
    ));
    assert!(matches!(
        harness.provider.provision(OP, &spec).await,
        Err(CampusProviderError::Unreachable(_))
    ));
    assert_eq!(harness.kube.state().count(CampusKubeKind::Job), 0);
}

#[test]
fn provider_config_refuses_unsafe_broker_proxy_and_gate_settings() {
    assert_eq!(egress_config().validate(), Ok(()));
    let mut config = KubernetesCampusProviderConfig::m1_lab();
    config.broker_url = Url::parse("http://127.0.0.1:4790/").expect("url");
    assert!(config.validate().is_err());
    let mut config = KubernetesCampusProviderConfig::m1_lab();
    config.broker_url = Url::parse("http://169.254.169.254/").expect("url");
    assert!(config.validate().is_err());
    let mut config = KubernetesCampusProviderConfig::m1_lab();
    config.required_network_policies = vec!["campus-lab-session-broker".into()];
    assert!(config.validate().is_err());
    let mut config = KubernetesCampusProviderConfig::m1_lab();
    config.network_gate.canaries = vec!["kubernetes.default:443".into()];
    assert!(config.validate().is_err());
    let mut config = KubernetesCampusProviderConfig::m1_lab();
    config.network_gate.timeout_seconds = 0;
    assert!(config.validate().is_err());
    for (proxy, ca) in [
        ("https://10.43.130.30:3128", TEST_CA),
        ("http://10.43.130.30", TEST_CA),
        ("http://user:pw@10.43.130.30:3128", TEST_CA),
        ("http://localhost:3128", TEST_CA),
        (
            "http://10.43.130.30:3128",
            "-----BEGIN PRIVATE KEY-----\nMII\n-----END PRIVATE KEY-----",
        ),
        ("http://10.43.130.30:3128", ""),
    ] {
        let mut config = egress_config();
        let egress = config.egress.as_mut().expect("egress");
        egress.proxy_url = Url::parse(proxy).expect("url");
        egress.ca_certificate_pem = ca.into();
        assert!(config.validate().is_err(), "{proxy} {ca:?}");
    }
}

#[test]
fn the_network_gate_waits_for_isolation_and_fails_closed() {
    let argv = network_gate_command(60, &["10.43.0.1:443".into(), "1.1.1.1:443".into()]);
    assert_eq!(&argv[..2], ["/bin/sh", "-c"]);
    assert_eq!(
        &argv[3..],
        ["campus-network-gate", "60", "10.43.0.1:443", "1.1.1.1:443"]
    );
    let script = &argv[2];
    assert!(script.contains("python3") && script.contains("node"));
    assert!(script.contains("exit 3"), "no probe tool fails closed");
    assert!(
        script.trim_end().ends_with("exit 1"),
        "timeout fails closed"
    );
}

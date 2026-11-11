//! Live qualification of the Kubernetes Campus provider against a real
//! cluster. It is ignored by default and driven by
//! `scripts/campus-session-jobs-cluster-test.sh`, which prepares the probe
//! slots, runs the stress probes between the phases and cleans up.
//!
//! Environment:
//! - `PHARNESS_CAMPUS_LIVE_PHASE`: `provision` or `delete`.
//! - `PHARNESS_CAMPUS_LIVE_RUN`: run id, part of every session id.
//! - `PHARNESS_CAMPUS_LIVE_SLOTS`: comma-separated probe slot namespaces; the
//!   slot id equals the namespace.
//! - `PHARNESS_CAMPUS_LIVE_REAL_SLOT`: an Argo-managed slot, which admits the
//!   approved research image (LUC-121) behind its own network policies.
//! - `PHARNESS_CAMPUS_LIVE_CONTROLLER_ARGS`: kubectl global arguments for the
//!   controller identity (`--context ... --as ...`).
//! - `PHARNESS_CAMPUS_LIVE_ADMIN_ARGS`: kubectl global arguments for an
//!   operator, used only to show a missing RuntimeClass is refused.
//! - `PHARNESS_CAMPUS_LIVE_OUT`: directory for the observed Jobs and Pods.
//!
//! The egress phases (LUC-130) are driven by
//! `scripts/campus-egress-cluster-test.sh`:
//! - `PHARNESS_CAMPUS_LIVE_PHASE`: `egress-provision`, `egress-delete` or
//!   `enforcement`.
//! - `PHARNESS_CAMPUS_LIVE_EGRESS_SLOTS`: three Argo-managed slots, for a
//!   Node session and a Python session with approved egress and a session
//!   without egress.
//! - `PHARNESS_CAMPUS_LIVE_EGRESS_CA_FILE`: the Campus egress CA certificate.
//! - `PHARNESS_CAMPUS_LIVE_EXPECT`: for `enforcement`, the refusal the
//!   prepared probe slot must produce.

use async_trait::async_trait;
use pharness_workspaces::campus_broker_token::{
    CampusBrokerAudience, CampusBrokerSecret, CampusBrokerTokenError, CampusBrokerTokenIssuer,
    CampusIssuedBrokerToken,
};
use pharness_workspaces::campus_capability::{
    CampusActorRef, CampusBrokerClock, CampusSessionAuthority, InMemoryCampusAuthoritySource,
    SystemCampusBrokerClock,
};
use pharness_workspaces::campus_lab_session::kubernetes::{
    session_resource_name, CampusEgressProxyConfig, CampusKubeApi, CampusKubeKind,
    KubectlCampusKubeApi, KubernetesCampusProvider, KubernetesCampusProviderConfig,
};
use pharness_workspaces::campus_policy::CAMPUS_POLICY_LAYER_SCHEMA;
use pharness_workspaces::{
    CampusDeleteOutcome, CampusExecutionProvider, CampusPolicyLayer, CampusProfile,
    CampusProvisionOutcome, CampusProvisionSpec, CampusWorkloadRef, EffectiveCampusPolicy,
    SECURITY_PROFILE_QUALIFICATION,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use url::Url;

const RESEARCH: &str = include_str!("../src/campus_profile/fixtures/research.json");
const CODING: &str = include_str!("../src/campus_profile/fixtures/coding.json");
const SECURITY: &str = include_str!("../src/campus_profile/fixtures/security.json");
const TENANT: &str = "tenant-probe";
const COURSE: &str = "course-probe";

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

fn args(name: &str) -> Vec<String> {
    env(name).split_whitespace().map(str::to_string).collect()
}

fn report(event: &str, detail: Value) {
    println!("@@CAMPUS {}", json!({ "event": event, "detail": detail }));
}

fn layer(authority: &str) -> CampusPolicyLayer {
    CampusPolicyLayer::parse(
        &json!({
            "schema_version": CAMPUS_POLICY_LAYER_SCHEMA,
            "authority": authority,
            "id": format!("{authority}-probe"),
            "version": "v1",
            "constraints": {},
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

fn policy(document: &str) -> EffectiveCampusPolicy {
    EffectiveCampusPolicy::compose(
        &profile(document),
        &[layer("platform"), layer("institution")],
        &BTreeSet::from([SECURITY_PROFILE_QUALIFICATION.to_string()]),
    )
    .expect("policy composes")
}

/// Issues well-formed tokens bound to the workload. The live provider
/// test has no journal; the broker-backed qualification is the pharness-api
/// `campus_broker_live` test.
struct LiveIssuer;

#[async_trait]
impl CampusBrokerTokenIssuer for LiveIssuer {
    async fn issue(
        &self,
        workload: &CampusWorkloadRef,
        audience: CampusBrokerAudience,
    ) -> Result<CampusIssuedBrokerToken, CampusBrokerTokenError> {
        let mut bytes = [0_u8; 32];
        std::io::Read::read_exact(
            &mut std::fs::File::open("/dev/urandom").expect("urandom"),
            &mut bytes,
        )
        .expect("random bytes");
        let now = SystemCampusBrokerClock.now();
        Ok(CampusIssuedBrokerToken {
            token_id: format!("cbt-live-{now}"),
            audience,
            workload: workload.clone(),
            lab_generation: 1,
            issued_at: now,
            expires_at: now + 900,
            secret: CampusBrokerSecret::from_random_bytes(&bytes),
        })
    }
}

struct Live {
    run: String,
    slots: Vec<String>,
    real_slot: String,
    authority: Arc<InMemoryCampusAuthoritySource>,
    clock: Arc<SystemCampusBrokerClock>,
    controller: Arc<KubectlCampusKubeApi>,
    egress_slots: Vec<String>,
}

impl Live {
    fn from_env() -> Self {
        Self {
            run: env("PHARNESS_CAMPUS_LIVE_RUN"),
            slots: env("PHARNESS_CAMPUS_LIVE_SLOTS")
                .split(',')
                .map(str::to_string)
                .collect(),
            real_slot: env("PHARNESS_CAMPUS_LIVE_REAL_SLOT"),
            authority: Arc::new(InMemoryCampusAuthoritySource::default()),
            clock: Arc::new(SystemCampusBrokerClock),
            controller: Arc::new(KubectlCampusKubeApi::new(
                "kubectl",
                args("PHARNESS_CAMPUS_LIVE_CONTROLLER_ARGS"),
            )),
            egress_slots: std::env::var("PHARNESS_CAMPUS_LIVE_EGRESS_SLOTS")
                .map(|slots| slots.split(',').map(str::to_string).collect())
                .unwrap_or_default(),
        }
    }

    /// The provider configuration with the Campus egress proxy.
    fn egress_config(&self) -> KubernetesCampusProviderConfig {
        let mut config = KubernetesCampusProviderConfig::m1_lab();
        config.slots = self
            .egress_slots
            .iter()
            .map(|slot| (slot.clone(), slot.clone()))
            .collect();
        config.egress = Some(CampusEgressProxyConfig {
            proxy_url: Url::parse("http://10.43.130.30:3128").expect("url"),
            ca_certificate_pem: std::fs::read_to_string(env("PHARNESS_CAMPUS_LIVE_EGRESS_CA_FILE"))
                .expect("egress CA"),
            network_policy: "campus-lab-session-egress".into(),
        });
        config
    }

    fn config(&self) -> KubernetesCampusProviderConfig {
        let mut config = KubernetesCampusProviderConfig::m1_lab();
        config.slots = self
            .slots
            .iter()
            .chain([&self.real_slot])
            .map(|slot| (slot.clone(), slot.clone()))
            .collect();
        config
    }

    fn provider(
        &self,
        config: KubernetesCampusProviderConfig,
        api: Arc<dyn CampusKubeApi>,
    ) -> KubernetesCampusProvider {
        KubernetesCampusProvider::new(
            config,
            [profile(RESEARCH), profile(SECURITY)],
            self.authority.clone(),
            Arc::new(LiveIssuer),
            api,
            self.clock.clone(),
        )
        .expect("provider config validates")
    }

    fn session_id(&self, slot: &str, purpose: &str) -> String {
        format!(
            "probe-{}-{purpose}-{}",
            self.run,
            slot.rsplit('-').next().unwrap_or("x")
        )
    }

    /// Admit a session and return its spec.
    fn admit(
        &self,
        slot: &str,
        purpose: &str,
        document: &str,
        generation: i64,
    ) -> CampusProvisionSpec {
        let session_id = self.session_id(slot, purpose);
        let policy = policy(document);
        let expires_at = self.clock.now() + 3_600;
        let spec = CampusProvisionSpec {
            workload: CampusWorkloadRef {
                session_id: session_id.clone(),
                slot_id: slot.into(),
                slot_generation: generation,
            },
            tenant_id: TENANT.into(),
            course_id: COURSE.into(),
            profile: policy.profile().clone(),
            expires_at,
        };
        self.authority.insert(CampusSessionAuthority {
            session_id,
            course_id: COURSE.into(),
            owner: CampusActorRef {
                tenant_id: TENANT.into(),
                identity_id: "idn-probe-student".into(),
            },
            policy,
            expires_at,
        });
        spec
    }

    fn op(spec: &CampusProvisionSpec, kind: &str) -> String {
        format!(
            "{kind}-{}-{}",
            spec.workload.session_id, spec.workload.slot_generation
        )
    }
}

async fn records(api: &KubectlCampusKubeApi, namespace: &str, session_id: &str) -> Vec<Value> {
    let mut found = Vec::new();
    for kind in [
        CampusKubeKind::ConfigMap,
        CampusKubeKind::Job,
        CampusKubeKind::Pod,
    ] {
        found.extend(
            api.list(
                kind,
                namespace,
                &format!("pharness.dev/campus-session={session_id}"),
            )
            .await
            .expect("list"),
        );
    }
    found
}

fn rejected(outcome: &CampusProvisionOutcome) -> &str {
    match outcome {
        CampusProvisionOutcome::Rejected { reason } => reason,
        other => panic!("expected a rejection, got {other:?}"),
    }
}

async fn provision_phase(live: &Live) {
    let provider = live.provider(live.config(), live.controller.clone());
    let first = &live.slots[0];

    // Missing or unreadable RuntimeClass: refused before anything is created.
    let mut missing = live.config();
    missing.runtime.class_name = "gvisor-unqualified".into();
    let spec = live.admit(first, "noruntime", RESEARCH, 1);
    for (identity, api) in [
        ("controller", live.controller.clone()),
        (
            "operator",
            Arc::new(KubectlCampusKubeApi::new(
                "kubectl",
                args("PHARNESS_CAMPUS_LIVE_ADMIN_ARGS"),
            )),
        ),
    ] {
        let outcome = live
            .provider(missing.clone(), api)
            .provision(&Live::op(&spec, "provision"), &spec)
            .await
            .expect("provision call");
        let reason = rejected(&outcome).to_string();
        assert!(
            reason.contains("RuntimeClass gvisor-unqualified"),
            "{reason}"
        );
        report(
            "missing_runtime_refused",
            json!({ "identity": identity, "reason": reason }),
        );
    }
    assert!(records(&live.controller, first, &spec.workload.session_id)
        .await
        .is_empty());

    // The security profile never launches in a lab slot.
    let spec = live.admit(first, "security", SECURITY, 1);
    let outcome = provider
        .provision(&Live::op(&spec, "provision"), &spec)
        .await
        .expect("provision call");
    report(
        "security_profile_refused",
        json!({ "reason": rejected(&outcome) }),
    );
    assert!(records(&live.controller, first, &spec.workload.session_id)
        .await
        .is_empty());

    // An Argo-managed slot admits the approved research image.
    let spec = live.admit(&live.real_slot, "real", RESEARCH, 1);
    let outcome = provider
        .provision(&Live::op(&spec, "provision"), &spec)
        .await
        .expect("provision call");
    assert!(
        !matches!(outcome, CampusProvisionOutcome::Rejected { .. }),
        "{outcome:?}"
    );
    report(
        "real_slot_admitted",
        json!({ "slot": live.real_slot, "outcome": format!("{outcome:?}") }),
    );
    let started = Instant::now();
    loop {
        match provider
            .delete(&Live::op(&spec, "delete"), &spec.workload)
            .await
            .expect("delete call")
        {
            CampusDeleteOutcome::Deleted => break,
            CampusDeleteOutcome::InProgress => {}
            other => panic!("real slot delete: {other:?}"),
        }
        assert!(started.elapsed() < Duration::from_secs(120));
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    // Approved sessions in every probe slot at once.
    let started = Instant::now();
    let specs = live
        .slots
        .iter()
        .map(|slot| live.admit(slot, "session", RESEARCH, 1))
        .collect::<Vec<_>>();
    let mut ready = BTreeSet::new();
    while ready.len() < specs.len() {
        assert!(
            started.elapsed() < Duration::from_secs(300),
            "sessions not ready: {ready:?}"
        );
        for spec in &specs {
            if ready.contains(&spec.workload.slot_id) {
                continue;
            }
            match provider
                .provision(&Live::op(spec, "provision"), spec)
                .await
                .expect("provision call")
            {
                CampusProvisionOutcome::Ready { workspace_id } => {
                    assert_eq!(workspace_id, session_resource_name(&spec.workload));
                    report(
                        "session_ready",
                        json!({
                            "slot": spec.workload.slot_id,
                            "workspace": workspace_id,
                            "seconds": started.elapsed().as_secs(),
                        }),
                    );
                    ready.insert(spec.workload.slot_id.clone());
                }
                CampusProvisionOutcome::InProgress { .. } => {}
                CampusProvisionOutcome::Rejected { reason } => {
                    panic!("{} rejected: {reason}", spec.workload.slot_id)
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    // Repeating every create changes nothing.
    let out = env("PHARNESS_CAMPUS_LIVE_OUT");
    for spec in &specs {
        let name = session_resource_name(&spec.workload);
        let namespace = &spec.workload.slot_id;
        let before = live
            .controller
            .get(CampusKubeKind::Job, namespace, &name)
            .await
            .expect("get")
            .expect("job exists");
        let again = provider
            .provision(&Live::op(spec, "provision"), spec)
            .await
            .expect("provision call");
        assert!(
            matches!(again, CampusProvisionOutcome::Ready { .. }),
            "{again:?}"
        );
        let after = records(&live.controller, namespace, &spec.workload.session_id).await;
        let kinds = after
            .iter()
            .map(|object| object["kind"].as_str().unwrap_or_default().to_string())
            .collect::<Vec<_>>();
        assert_eq!(kinds, ["ConfigMap", "Job", "Pod"], "{namespace}");
        assert_eq!(after[1]["metadata"]["uid"], before["metadata"]["uid"]);
        std::fs::write(
            format!("{out}/job-{namespace}.json"),
            serde_json::to_vec_pretty(&after[1]).expect("json"),
        )
        .expect("write job");
        std::fs::write(
            format!("{out}/pod-{namespace}.json"),
            serde_json::to_vec_pretty(&after[2]).expect("json"),
        )
        .expect("write pod");
    }
    report("idempotent_repeat", json!({ "slots": specs.len() }));
    let owned = provider.list_owned().await.expect("list owned");
    report("owned_resources", json!({ "count": owned.len() }));
    // Anchor, Job, Pod and broker token Secret.
    assert_eq!(owned.len(), specs.len() * 4);

    // A second occupant of a busy slot is held off by the slot quota, and the
    // first occupant is then fenced as stale.
    let intruder = live.admit(first, "intruder", RESEARCH, 2);
    let outcome = provider
        .provision(&Live::op(&intruder, "provision"), &intruder)
        .await
        .expect("provision call");
    let reason = rejected(&outcome).to_string();
    assert!(reason.contains("exceeded quota"), "{reason}");
    report("second_occupant_quota_refused", json!({ "reason": reason }));
    let stale = provider
        .provision(&Live::op(&specs[0], "provision"), &specs[0])
        .await
        .expect("provision call");
    let reason = rejected(&stale).to_string();
    assert!(reason.contains("newer occupant"), "{reason}");
    report("stale_generation_refused", json!({ "reason": reason }));
    assert_eq!(
        provider
            .delete(&Live::op(&intruder, "delete"), &intruder.workload)
            .await
            .expect("delete call"),
        CampusDeleteOutcome::Deleted
    );
}

async fn delete_phase(live: &Live) {
    let provider = live.provider(live.config(), live.controller.clone());
    let started = Instant::now();
    for slot in &live.slots {
        let spec = live.admit(slot, "session", RESEARCH, 1);
        let op = Live::op(&spec, "delete");
        loop {
            let outcome = provider
                .delete(&op, &spec.workload)
                .await
                .expect("delete call");
            match outcome {
                CampusDeleteOutcome::Deleted => break,
                CampusDeleteOutcome::InProgress => {}
                other => panic!("{slot}: {other:?}"),
            }
            assert!(
                started.elapsed() < Duration::from_secs(180),
                "{slot} not deleted"
            );
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        let remaining = records(&live.controller, slot, &spec.workload.session_id).await;
        let kinds = remaining
            .iter()
            .map(|object| object["data"]["state"].as_str().unwrap_or("workload"))
            .collect::<Vec<_>>();
        assert_eq!(kinds, ["tombstone"], "{slot}");
        let late = provider
            .provision(&Live::op(&spec, "provision"), &spec)
            .await
            .expect("provision call");
        // The first slot was later held by the generation-2 intruder, so its
        // fence reports the newer occupant before the session's own tombstone.
        let expected = if *slot == live.slots[0] {
            "newer occupant"
        } else {
            "already deleted"
        };
        let reason = rejected(&late);
        assert!(reason.contains(expected), "{slot}: {reason}");
        report(
            "session_deleted",
            json!({ "slot": slot, "seconds": started.elapsed().as_secs() }),
        );
    }
    let owned = provider.list_owned().await.expect("list owned");
    assert!(owned.is_empty(), "{owned:?}");
    report("owned_resources", json!({ "count": 0 }));
}

/// The coding profile sized to fit an M1 lab slot (2 CPU, 3 GiB including
/// the memory-backed workspace).
fn coding_profile() -> Value {
    let mut profile: Value = serde_json::from_str(CODING).expect("coding fixture");
    profile["resources"]["cpu_millicores"] = 1000.into();
    profile["resources"]["memory_mib"] = 1024.into();
    profile["resources"]["ephemeral_storage_mib"] = 1024.into();
    profile
}

fn node_egress_profile() -> String {
    coding_profile().to_string()
}

/// The coding profile on the Python runner, with every approved route, so
/// one session can exercise pip, git and raw-file retrieval.
fn python_egress_profile() -> String {
    let mut profile = coding_profile();
    let python: Value = serde_json::from_str(RESEARCH).expect("research fixture");
    profile["id"] = "campus-coding-python".into();
    profile["image"] = python["image"].clone();
    profile["network"]["egress"]
        .as_array_mut()
        .expect("egress list")
        .push(json!({ "host": "raw.githubusercontent.com", "port": 443 }));
    profile.to_string()
}

fn egress_sessions(live: &Live) -> Vec<(&'static str, String, CampusProvisionSpec)> {
    assert_eq!(live.egress_slots.len(), 3, "three egress slots");
    let python = python_egress_profile();
    [
        ("node", node_egress_profile()),
        ("python", python),
        ("denied", RESEARCH.to_string()),
    ]
    .into_iter()
    .zip(&live.egress_slots)
    .map(|((purpose, document), slot)| {
        let spec = live.admit(slot, &format!("egress-{purpose}"), &document, 1);
        (purpose, document, spec)
    })
    .collect()
}

fn egress_provider(
    live: &Live,
    config: KubernetesCampusProviderConfig,
) -> KubernetesCampusProvider {
    KubernetesCampusProvider::new(
        config,
        [
            profile(RESEARCH),
            profile(&node_egress_profile()),
            profile(&python_egress_profile()),
        ],
        live.authority.clone(),
        Arc::new(LiveIssuer),
        live.controller.clone(),
        live.clock.clone(),
    )
    .expect("provider config validates")
}

async fn egress_provision_phase(live: &Live) {
    let sessions = egress_sessions(live);
    let (_, _, node) = &sessions[0];

    // Each missing piece of enforcement refuses the launch before anything
    // is created.
    let mut no_proxy = live.egress_config();
    no_proxy.egress = None;
    let mut no_egress_policy = live.egress_config();
    if let Some(egress) = no_egress_policy.egress.as_mut() {
        egress.network_policy = "campus-lab-session-egress-absent".into();
    }
    let mut no_isolation_policy = live.egress_config();
    no_isolation_policy
        .required_network_policies
        .push("campus-lab-isolation-absent".into());
    for (case, config, needle) in [
        ("no_proxy_configured", no_proxy, "egress"),
        (
            "egress_policy_missing",
            no_egress_policy,
            "campus-lab-session-egress-absent",
        ),
        (
            "isolation_policy_missing",
            no_isolation_policy,
            "campus-lab-isolation-absent",
        ),
    ] {
        let outcome = egress_provider(live, config)
            .provision(&Live::op(node, "provision"), node)
            .await
            .expect("provision call");
        let reason = rejected(&outcome).to_string();
        assert!(reason.contains(needle), "{case}: {reason}");
        report(
            "enforcement_refused",
            json!({ "case": case, "reason": reason }),
        );
        assert!(
            records(
                &live.controller,
                &node.workload.slot_id,
                &node.workload.session_id
            )
            .await
            .is_empty(),
            "{case} created something"
        );
    }

    let provider = egress_provider(live, live.egress_config());
    let started = Instant::now();
    let mut ready = BTreeSet::new();
    while ready.len() < sessions.len() {
        assert!(
            started.elapsed() < Duration::from_secs(300),
            "egress sessions not ready: {ready:?}"
        );
        for (purpose, _, spec) in &sessions {
            if ready.contains(purpose) {
                continue;
            }
            match provider
                .provision(&Live::op(spec, "provision"), spec)
                .await
                .expect("provision call")
            {
                CampusProvisionOutcome::Ready { .. } => {
                    report(
                        "egress_session_ready",
                        json!({
                            "purpose": purpose,
                            "slot": spec.workload.slot_id,
                            "seconds": started.elapsed().as_secs(),
                        }),
                    );
                    ready.insert(*purpose);
                }
                CampusProvisionOutcome::InProgress { .. } => {}
                CampusProvisionOutcome::Rejected { reason } => {
                    panic!("{purpose} rejected: {reason}")
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    let out = env("PHARNESS_CAMPUS_LIVE_OUT");
    for (purpose, _, spec) in &sessions {
        let found = records(
            &live.controller,
            &spec.workload.slot_id,
            &spec.workload.session_id,
        )
        .await;
        let pod = found
            .iter()
            .find(|object| object["kind"] == "Pod")
            .expect("session Pod");
        std::fs::write(
            format!("{out}/egress-pod-{purpose}.json"),
            serde_json::to_vec_pretty(pod).expect("json"),
        )
        .expect("write pod");
        let job = found
            .iter()
            .find(|object| object["kind"] == "Job")
            .expect("session Job");
        std::fs::write(
            format!("{out}/egress-job-{purpose}.json"),
            serde_json::to_vec_pretty(job).expect("json"),
        )
        .expect("write job");
        report(
            "network_gate",
            json!({
                "purpose": purpose,
                "gate": pod.pointer("/status/initContainerStatuses/0/state"),
                "egress_label": pod.pointer("/metadata/labels/pharness.dev~1campus-egress"),
                "hosts": pod.pointer("/metadata/annotations/pharness.dev~1campus-egress-hosts"),
            }),
        );
    }
}

async fn egress_delete_phase(live: &Live) {
    let sessions = egress_sessions(live);
    let provider = egress_provider(live, live.egress_config());
    let started = Instant::now();
    for (purpose, _, spec) in &sessions {
        let op = Live::op(spec, "delete");
        loop {
            match provider
                .delete(&op, &spec.workload)
                .await
                .expect("delete call")
            {
                CampusDeleteOutcome::Deleted => break,
                CampusDeleteOutcome::InProgress => {}
                other => panic!("{purpose}: {other:?}"),
            }
            assert!(
                started.elapsed() < Duration::from_secs(180),
                "{purpose} not deleted"
            );
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        report(
            "egress_session_deleted",
            json!({ "purpose": purpose, "seconds": started.elapsed().as_secs() }),
        );
    }
    let owned = provider.list_owned().await.expect("list owned");
    let left = owned
        .iter()
        .filter(|resource| resource.workload.session_id.contains("-egress-"))
        .count();
    assert_eq!(left, 0, "{owned:?}");
}

/// The script has removed or weakened an isolation policy in the first
/// probe slot; a launch there must be refused before anything is created.
async fn enforcement_phase(live: &Live) {
    let slot = &live.slots[0];
    let expect = env("PHARNESS_CAMPUS_LIVE_EXPECT");
    let spec = live.admit(slot, "enforcement", RESEARCH, 1);
    let outcome = live
        .provider(live.config(), live.controller.clone())
        .provision(&Live::op(&spec, "provision"), &spec)
        .await
        .expect("provision call");
    let reason = rejected(&outcome).to_string();
    assert!(reason.contains(&expect), "{reason}");
    report(
        "enforcement_refused",
        json!({ "slot": slot, "reason": reason }),
    );
    assert!(records(&live.controller, slot, &spec.workload.session_id)
        .await
        .is_empty());
}

#[tokio::test]
#[ignore = "needs a cluster; run scripts/campus-session-jobs-cluster-test.sh"]
async fn live_campus_session_jobs() {
    let live = Live::from_env();
    match env("PHARNESS_CAMPUS_LIVE_PHASE").as_str() {
        "provision" => provision_phase(&live).await,
        "delete" => delete_phase(&live).await,
        "egress-provision" => egress_provision_phase(&live).await,
        "egress-delete" => egress_delete_phase(&live).await,
        "enforcement" => enforcement_phase(&live).await,
        other => panic!("unknown phase {other}"),
    }
}

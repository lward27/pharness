//! Live qualification of Campus teardown (LUC-133) against real lab slots.
//! Ignored by default; `scripts/campus-teardown-cluster-test.sh` runs it.
//!
//! The journal, the reconciler ([`CampusLabs`]) and the broker run in
//! process; the real Kubernetes provider launches and removes the sessions
//! through the controller identity. The harness clock follows real time, so
//! leases, backoff and the janitor interval behave as in production, and it
//! jumps ahead for expiry. The provider keeps real time, which is what it
//! compares deletion timestamps against.
//!
//! - `campus_teardown_live_cycles` runs create, expire and stop cycles in
//!   the M1 slots `campus-lab-slot-1..5`, listed as the catalog's
//!   `lab_slot_ids` as in the Campus PoC, and measures token revocation and
//!   resource removal.
//! - `campus_teardown_live_stuck` uses a probe namespace labelled as slot
//!   `campus-lab-slot-1`: leftovers of another journal are quarantined and
//!   removed by the janitor, and a Pod or Job held by a finalizer (added and
//!   removed by the operator, never by the controller) quarantines the
//!   cleanup until it can finish.
//!
//! Environment:
//! - `PHARNESS_CAMPUS_TEARDOWN_LIVE_CONTROLLER_ARGS` /
//!   `PHARNESS_CAMPUS_TEARDOWN_LIVE_ADMIN_ARGS`: kubectl global arguments for
//!   the controller identity and for an operator.
//! - `PHARNESS_CAMPUS_TEARDOWN_LIVE_OUT`: evidence directory.
//! - `PHARNESS_CAMPUS_TEARDOWN_LIVE_CYCLES`: cycles, a multiple of five
//!   (default 100).
//! - `PHARNESS_CAMPUS_TEARDOWN_LIVE_PROBE_NAMESPACE`: the probe namespace.

use super::super::labs::{CampusLabSweep, CampusLabs, JANITOR_INTERVAL_SECONDS};
use super::broker::denial;
use super::labs::{act, catalog_value, enroll_students, id, launched, parse};
use super::{Harness, Session, TestClock, TENANT};
use pharness_core::campus_capability::{CampusBrokerClock, SystemCampusBrokerClock};
use pharness_core::campus_lab_session::kubernetes::{
    KubectlCampusKubeApi, KubernetesCampusProvider, KubernetesCampusProviderConfig, LABEL_RECORD,
    LABEL_SESSION, RECORD_TOMBSTONE,
};
use pharness_core::campus_sandbox_pod::CAMPUS_BROKER_TOKEN_PATH;
use pharness_core::{CampusExecutionProvider, CampusLabSessionState as State, CampusProfile};
use pharness_store::{
    CampusLabSlotCondition, JournalCampusAuthoritySource, StoredCampusLabSession,
    CAMPUS_ORPHAN_GRACE_SECONDS,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const RESEARCH: &str =
    include_str!("../../../../../pharness-core/src/campus_profile/fixtures/research.json");
const CODING: &str =
    include_str!("../../../../../pharness-core/src/campus_profile/fixtures/coding.json");
const PROBE_FINALIZER: &str = "pharness.dev/luc-133-probe";
const PROBE_SLOT: &str = "campus-lab-slot-1";
const LEASE_SECONDS: i64 = 120;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

fn args(name: &str) -> Vec<String> {
    env(name).split_whitespace().map(str::to_string).collect()
}

fn report(event: &str, detail: Value) {
    println!("@@CAMPUS {}", json!({ "event": event, "detail": detail }));
}

struct HarnessClock(Arc<TestClock>);

impl CampusBrokerClock for HarnessClock {
    fn now(&self) -> i64 {
        use super::super::state::CampusClock;
        self.0.now()
    }
}

/// The node runner on the coding profile, without approved egress and with
/// the research profile's resources, so that both approved runner images
/// fit the slots and run without the egress proxy.
fn node_profile() -> Value {
    let research: Value = serde_json::from_str(RESEARCH).unwrap();
    let mut profile: Value = serde_json::from_str(CODING).unwrap();
    profile["id"] = json!("campus-node-offline");
    profile["network"] = json!({ "mode": "denied", "egress": [] });
    profile["resources"] = research["resources"].clone();
    profile
}

fn profiles() -> Vec<CampusProfile> {
    [RESEARCH.to_string(), node_profile().to_string()]
        .iter()
        .map(|document| CampusProfile::parse(document).expect("profile"))
        .collect()
}

async fn live_harness(slots: i64) -> Harness {
    let mut catalog = catalog_value(slots);
    catalog["lab_slot_ids"] = json!((1..=slots)
        .map(|index| format!("campus-lab-slot-{index}"))
        .collect::<Vec<_>>());
    catalog["profiles"]
        .as_array_mut()
        .unwrap()
        .push(node_profile());
    catalog["assignments"].as_array_mut().unwrap().push(json!({
        "course_id": "cs101",
        "assignment_id": "lab-node",
        "title": "cs101 node",
        "profile_id": "campus-node-offline",
    }));
    let harness = Harness::with_labs(parse(&catalog).unwrap(), true, false).await;
    let real = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    harness.clock.advance(real - harness.now());
    harness
}

/// Moves a harness clock forward with real time, on top of any jumps.
struct RealTime(Mutex<Instant>);

impl RealTime {
    fn new() -> Self {
        Self(Mutex::new(Instant::now()))
    }

    fn tick(&self, harness: &Harness) {
        let mut last = self.0.lock().unwrap();
        let elapsed = last.elapsed().as_secs();
        if elapsed > 0 {
            harness.clock.advance(elapsed as i64);
            *last += Duration::from_secs(elapsed);
        }
    }
}

struct Live {
    admin: Vec<String>,
    controller: Arc<KubectlCampusKubeApi>,
}

impl Live {
    fn from_env() -> Self {
        // Reconciler warnings are the only trace of a step that failed.
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::WARN)
            .try_init();
        Self {
            admin: args("PHARNESS_CAMPUS_TEARDOWN_LIVE_ADMIN_ARGS"),
            controller: Arc::new(KubectlCampusKubeApi::new(
                "kubectl",
                args("PHARNESS_CAMPUS_TEARDOWN_LIVE_CONTROLLER_ARGS"),
            )),
        }
    }

    fn kubectl(&self, extra: &[&str]) -> std::process::Output {
        std::process::Command::new("kubectl")
            .args(&self.admin)
            .args(extra)
            .output()
            .expect("kubectl runs")
    }

    fn stdout(&self, extra: &[&str]) -> String {
        let output = self.kubectl(extra);
        assert!(
            output.status.success(),
            "kubectl {extra:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn provider(
        &self,
        harness: &Harness,
        slots: &BTreeMap<String, String>,
    ) -> Arc<dyn CampusExecutionProvider> {
        let labs = harness.labs.clone().expect("labs");
        let mut config = KubernetesCampusProviderConfig::m1_lab();
        config.slots = slots.clone();
        let authority = Arc::new(JournalCampusAuthoritySource::new(
            (*harness.store).clone(),
            labs.catalog.clone(),
            Arc::new(HarnessClock(harness.clock.clone())),
        ));
        Arc::new(
            KubernetesCampusProvider::new(
                config,
                profiles(),
                authority,
                harness.broker_tokens.clone(),
                self.controller.clone(),
                Arc::new(SystemCampusBrokerClock),
            )
            .expect("provider config validates"),
        )
    }

    /// Jobs, Pods and token Secrets of a session.
    fn remaining(&self, namespace: &str, session_id: &str) -> String {
        self.stdout(&[
            "-n",
            namespace,
            "get",
            "jobs.batch,pods,secrets",
            "-l",
            &format!("{LABEL_SESSION}={session_id}"),
            "-o",
            "name",
        ])
    }

    fn object(&self, namespace: &str, kind: &str, session_id: &str) -> String {
        self.stdout(&[
            "-n",
            namespace,
            "get",
            kind,
            "-l",
            &format!("{LABEL_SESSION}={session_id}"),
            "-o",
            "jsonpath={.items[0].metadata.name}",
        ])
    }

    fn token_in_pod(&self, namespace: &str, session_id: &str) -> String {
        let pod = self.object(namespace, "pods", session_id);
        self.stdout(&[
            "-n",
            namespace,
            "exec",
            &pod,
            "-c",
            "workspace",
            "--",
            "cat",
            CAMPUS_BROKER_TOKEN_PATH,
        ])
    }

    /// Add or remove only the probe finalizer, as an operator. The patch
    /// tests the list it read, so a finalizer another controller changes
    /// meanwhile (the Job controller's Pod tracking finalizer) is kept.
    fn set_finalizer(&self, namespace: &str, kind: &str, name: &str, present: bool) {
        for _ in 0..5 {
            let current = self.stdout(&[
                "-n",
                namespace,
                "get",
                kind,
                name,
                "-o",
                "jsonpath={.metadata.finalizers}",
            ]);
            let current: Vec<String> = if current.is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(&current).unwrap()
            };
            let mut next = current
                .iter()
                .filter(|finalizer| *finalizer != PROBE_FINALIZER)
                .cloned()
                .collect::<Vec<_>>();
            if present {
                next.push(PROBE_FINALIZER.to_string());
            }
            let patch = if current.is_empty() {
                json!([{ "op": "add", "path": "/metadata/finalizers", "value": next }])
            } else {
                json!([
                    { "op": "test", "path": "/metadata/finalizers", "value": current },
                    { "op": "replace", "path": "/metadata/finalizers", "value": next },
                ])
            };
            let output = self.kubectl(&[
                "-n",
                namespace,
                "patch",
                kind,
                name,
                "--type=json",
                "-p",
                &patch.to_string(),
            ]);
            if output.status.success() {
                return;
            }
            println!(
                "finalizer patch retried: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        panic!("could not change the finalizers of {kind} {name}");
    }

    /// Remove the tombstones of `sessions`, the only records a finished
    /// session leaves, so the slots are as empty as before the test.
    fn remove_tombstones(&self, namespace: &str, sessions: &BTreeSet<String>) -> usize {
        let listed = self.stdout(&[
            "-n",
            namespace,
            "get",
            "configmaps",
            "-l",
            &format!("{LABEL_RECORD}={RECORD_TOMBSTONE}"),
            "-o",
            "json",
        ]);
        let listed: Value = serde_json::from_str(&listed).unwrap();
        let mut removed = 0;
        for item in listed["items"].as_array().unwrap() {
            let session = item["metadata"]["labels"][LABEL_SESSION]
                .as_str()
                .unwrap_or_default();
            if sessions.contains(session) {
                let name = item["metadata"]["name"].as_str().unwrap();
                self.stdout(&["-n", namespace, "delete", "configmap", name]);
                removed += 1;
            }
        }
        removed
    }
}

async fn stored(harness: &Harness, session_id: &str) -> StoredCampusLabSession {
    harness
        .store
        .get_campus_lab_session(session_id)
        .await
        .unwrap()
        .unwrap()
}

/// Whether the session is Ready; a session that ended instead fails the test
/// with its reason.
async fn ready(harness: &Harness, session_id: &str) -> bool {
    let record = stored(harness, session_id).await;
    assert!(
        !record.state.is_terminal(),
        "{session_id} ended: {} ({:?})",
        record.state,
        record.failure_reason
    );
    record.state == State::Ready
}

async fn slot_condition(harness: &Harness, condition: CampusLabSlotCondition) -> i64 {
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

async fn tokens_revoked(harness: &Harness, session_id: &str) -> bool {
    harness
        .store
        .list_campus_broker_tokens(session_id)
        .await
        .unwrap()
        .iter()
        .all(|token| token.revoked_at.is_some())
}

#[derive(Debug, Clone, Copy)]
enum Ending {
    Stop,
    Expire,
    StopWhileProvisioning,
    StopAcrossRestart,
}

struct Tracked {
    student: Session,
    session_id: String,
    namespace: String,
    expires_at: i64,
    token: Option<String>,
    revoked_ms: Option<u128>,
    removed_s: Option<u64>,
}

fn percentile(values: &mut [u128], p: usize) -> u128 {
    values.sort_unstable();
    values[(values.len() - 1) * p / 100]
}

#[tokio::test]
#[ignore = "needs a cluster; run scripts/campus-teardown-cluster-test.sh"]
async fn campus_teardown_live_cycles() {
    let live = Live::from_env();
    let out = env("PHARNESS_CAMPUS_TEARDOWN_LIVE_OUT");
    let cycles: usize = std::env::var("PHARNESS_CAMPUS_TEARDOWN_LIVE_CYCLES")
        .ok()
        .map(|value| value.parse().expect("cycles"))
        .unwrap_or(100);
    assert!(cycles > 0 && cycles % 5 == 0);
    let harness = live_harness(5).await;
    let slots = KubernetesCampusProviderConfig::m1_lab().slots;
    let students = ["s0", "s1", "s2", "s3", "s4"];
    enroll_students(&harness, "cs101", &students).await;
    let catalog = harness.labs.clone().unwrap().catalog.clone();
    let provider = live.provider(&harness, &slots);
    let mut labs = Arc::new(CampusLabs::with_shared_catalog(
        catalog.clone(),
        Some(provider.clone()),
    ));
    let time = RealTime::new();
    let mut sessions = BTreeSet::new();
    let mut revoked = Vec::new();
    let mut removed = Vec::new();
    let mut sweep_errors = 0;
    let started = Instant::now();

    for batch in 0..cycles / 5 {
        let ending = [
            Ending::Stop,
            Ending::Expire,
            Ending::StopWhileProvisioning,
            Ending::StopAcrossRestart,
        ][batch % 4];
        let mut tracked = Vec::new();
        for (index, subject) in students.iter().enumerate() {
            let student = harness.login(subject).await;
            let assignment = if (batch + index) % 2 == 0 {
                "lab-2"
            } else {
                "lab-node"
            };
            let session_id = id(&launched(&harness, &student, assignment).await);
            sessions.insert(session_id.clone());
            tracked.push(Tracked {
                student,
                session_id,
                namespace: String::new(),
                expires_at: 0,
                token: None,
                revoked_ms: None,
                removed_s: None,
            });
        }
        let launch = Instant::now();
        loop {
            time.tick(&harness);
            sweep_errors += labs.reconcile(&harness.store, harness.now()).await.errors;
            let mut up = 0;
            for session in &tracked {
                up += usize::from(ready(&harness, &session.session_id).await);
            }
            if matches!(ending, Ending::StopWhileProvisioning) || up == tracked.len() {
                break;
            }
            assert!(launch.elapsed() < Duration::from_secs(300), "not ready");
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        let ready_s = launch.elapsed().as_secs();
        for session in &mut tracked {
            let record = stored(&harness, &session.session_id).await;
            session.expires_at = record.expires_at;
            let slot = record.slot_id.clone().expect("slot");
            session.namespace = slots[&slot].clone();
            if record.state == State::Ready {
                let token = live.token_in_pod(&session.namespace, &session.session_id);
                assert_eq!(denial(&harness, &token).await, None);
                session.token = Some(token);
            }
        }

        let ended = Instant::now();
        match ending {
            Ending::Stop | Ending::StopWhileProvisioning => {
                for session in &tracked {
                    let response =
                        act(&harness, &session.student, &session.session_id, "stop").await;
                    assert!(response.status().is_success(), "{}", response.status());
                }
            }
            Ending::Expire => {
                let last = tracked
                    .iter()
                    .map(|session| session.expires_at)
                    .max()
                    .unwrap();
                harness.clock.advance(last - harness.now());
            }
            Ending::StopAcrossRestart => {
                for session in &tracked {
                    for _ in 0..2 {
                        let response =
                            act(&harness, &session.student, &session.session_id, "stop").await;
                        assert!(response.status().is_success(), "{}", response.status());
                    }
                }
                time.tick(&harness);
                sweep_errors += labs.reconcile(&harness.store, harness.now()).await.errors;
                labs = Arc::new(CampusLabs::with_shared_catalog(
                    catalog.clone(),
                    Some(provider.clone()),
                ));
                harness.clock.advance(LEASE_SECONDS + 1);
            }
        }
        // Access is checked before each sweep, so the measurement shows what
        // the stop transaction or the expiry itself revoked, not a sweep's
        // Kubernetes calls.
        loop {
            time.tick(&harness);
            for session in &mut tracked {
                if session.revoked_ms.is_none() {
                    let denied = match &session.token {
                        Some(token) => denial(&harness, token).await.is_some(),
                        None => tokens_revoked(&harness, &session.session_id).await,
                    };
                    if denied {
                        session.revoked_ms = Some(ended.elapsed().as_millis());
                    }
                }
                if session.removed_s.is_none()
                    && stored(&harness, &session.session_id)
                        .await
                        .state
                        .is_terminal()
                    && live
                        .remaining(&session.namespace, &session.session_id)
                        .is_empty()
                {
                    session.removed_s = Some(ended.elapsed().as_secs());
                }
            }
            if tracked
                .iter()
                .all(|session| session.revoked_ms.is_some() && session.removed_s.is_some())
            {
                break;
            }
            assert!(
                ended.elapsed() < Duration::from_secs(300),
                "teardown did not finish within five minutes"
            );
            sweep_errors += labs.reconcile(&harness.store, harness.now()).await.errors;
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        for session in &tracked {
            let record = stored(&harness, &session.session_id).await;
            assert_eq!(record.state, State::Terminated, "{}", session.session_id);
            assert!(
                tokens_revoked(&harness, &session.session_id).await,
                "{} kept an unrevoked broker token",
                session.session_id
            );
            assert_eq!(record.expires_at, session.expires_at, "expiry changed");
            revoked.push(session.revoked_ms.unwrap());
            removed.push(u128::from(session.removed_s.unwrap()));
        }
        report(
            "batch",
            json!({
                "batch": batch,
                "ending": format!("{ending:?}"),
                "ready_seconds": ready_s,
                "revoked_ms": tracked.iter().map(|s| s.revoked_ms).collect::<Vec<_>>(),
                "removed_seconds": tracked.iter().map(|s| s.removed_s).collect::<Vec<_>>(),
            }),
        );
        println!(
            "PASS batch {batch} ({ending:?}): 5 sessions ready in {ready_s}s, revoked within {} ms, removed within {} s",
            tracked.iter().filter_map(|s| s.revoked_ms).max().unwrap(),
            tracked.iter().filter_map(|s| s.removed_s).max().unwrap()
        );
    }

    // The janitor's inventory agrees with the journal: nothing owned is left.
    harness.clock.advance(JANITOR_INTERVAL_SECONDS);
    time.tick(&harness);
    let last: CampusLabSweep = labs.reconcile(&harness.store, harness.now()).await;
    assert_eq!(last.janitor_passes, 1);
    assert_eq!(last.orphaned_workloads, 0);
    assert_eq!(last.errors, 0);
    assert!(provider.list_owned().await.unwrap().is_empty());
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
    assert_eq!(
        slot_condition(&harness, CampusLabSlotCondition::Free).await,
        5
    );
    let mut tombstones = 0;
    for namespace in slots.values() {
        let left = live.stdout(&[
            "-n",
            namespace,
            "get",
            "jobs.batch,pods,secrets",
            "-l",
            "pharness.dev/campus-controller",
            "-o",
            "name",
        ]);
        assert!(left.is_empty(), "{namespace}: {left}");
        tombstones += live.remove_tombstones(namespace, &sessions);
    }
    let summary = json!({
        "cycles": cycles,
        "sessions": sessions.len(),
        "minutes": started.elapsed().as_secs() / 60,
        "revoked_ms": { "p50": percentile(&mut revoked, 50), "p95": percentile(&mut revoked, 95), "max": percentile(&mut revoked, 100) },
        "removed_seconds": { "p50": percentile(&mut removed, 50), "p95": percentile(&mut removed, 95), "max": percentile(&mut removed, 100) },
        "sweep_errors": sweep_errors,
        "final_tombstones_removed": tombstones,
    });
    std::fs::write(
        format!("{out}/teardown-cycles.json"),
        serde_json::to_string_pretty(&summary).unwrap(),
    )
    .unwrap();
    report("summary", summary.clone());
    assert!(percentile(&mut revoked, 100) <= 60_000);
    assert!(percentile(&mut removed, 100) <= 300);
    println!(
        "PASS {cycles} cycles: no owned resources, active tokens, holds or live sessions left; {summary}"
    );
}

/// Reconcile every five seconds, keeping the janitor due, until `done`.
async fn until<F, Fut>(
    harness: &Harness,
    labs: &CampusLabs,
    time: &RealTime,
    limit: Duration,
    what: &str,
    mut done: F,
) -> (u64, CampusLabSweep)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let started = Instant::now();
    let mut total = CampusLabSweep::default();
    loop {
        time.tick(harness);
        let sweep = labs.reconcile(&harness.store, harness.now()).await;
        total.orphaned_workloads += sweep.orphaned_workloads;
        total.orphans_deleted += sweep.orphans_deleted;
        total.cleanup_retries += sweep.cleanup_retries;
        total.janitor_passes += sweep.janitor_passes;
        if done().await {
            return (started.elapsed().as_secs(), total);
        }
        assert!(
            started.elapsed() < limit,
            "{what} took longer than {limit:?}"
        );
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

#[tokio::test]
#[ignore = "needs a cluster; run scripts/campus-teardown-cluster-test.sh"]
async fn campus_teardown_live_stuck() {
    let live = Live::from_env();
    let namespace = env("PHARNESS_CAMPUS_TEARDOWN_LIVE_PROBE_NAMESPACE");
    let slots = BTreeMap::from([(PROBE_SLOT.to_string(), namespace.clone())]);
    let mut sessions = BTreeSet::new();

    // Leftovers of another journal (a lost database) in this one's slot.
    let other = live_harness(1).await;
    enroll_students(&other, "cs101", &["ghost"]).await;
    let ghost = other.login("ghost").await;
    let other_labs = CampusLabs::with_shared_catalog(
        other.labs.clone().unwrap().catalog.clone(),
        Some(live.provider(&other, &slots)),
    );
    let ghost_id = id(&launched(&other, &ghost, "lab-2").await);
    sessions.insert(ghost_id.clone());
    let other_time = RealTime::new();
    until(
        &other,
        &other_labs,
        &other_time,
        Duration::from_secs(300),
        "ghost ready",
        || ready(&other, &ghost_id),
    )
    .await;

    let harness = live_harness(1).await;
    enroll_students(&harness, "cs101", &["alice"]).await;
    let alice = harness.login("alice").await;
    let labs = CampusLabs::with_shared_catalog(
        harness.labs.clone().unwrap().catalog.clone(),
        Some(live.provider(&harness, &slots)),
    );
    let time = RealTime::new();
    let first = labs.reconcile(&harness.store, harness.now()).await;
    assert_eq!(first.orphaned_workloads, 1);
    assert_eq!(
        slot_condition(&harness, CampusLabSlotCondition::Quarantined).await,
        1
    );
    assert!(
        !live.remaining(&namespace, &ghost_id).is_empty(),
        "nothing is deleted at once"
    );
    println!("PASS the janitor quarantined the slot holding another journal's session at once");
    harness.clock.advance(CAMPUS_ORPHAN_GRACE_SECONDS);
    let (seconds, janitor) = until(
        &harness,
        &labs,
        &time,
        Duration::from_secs(300),
        "orphan removal",
        || async {
            harness.clock.advance(JANITOR_INTERVAL_SECONDS);
            live.remaining(&namespace, &ghost_id).is_empty()
                && harness
                    .store
                    .list_campus_lab_slot_holds(TENANT)
                    .await
                    .unwrap()
                    .is_empty()
        },
    )
    .await;
    // The provider may report the delete in progress and the next inventory
    // empty, so the confirmation is the clean inventory, not a counter.
    assert!(janitor.janitor_passes >= 1);
    println!("PASS after the grace period the janitor removed the orphan and released the slot in {seconds}s");
    report(
        "orphan",
        json!({ "seconds": seconds, "janitor": format!("{janitor:?}") }),
    );

    for (kind, expected) in [
        ("pods", "has not confirmed termination"),
        ("jobs.batch", "is held by finalizers"),
    ] {
        let session_id = id(&launched(&harness, &alice, "lab-2").await);
        sessions.insert(session_id.clone());
        until(
            &harness,
            &labs,
            &time,
            Duration::from_secs(300),
            "ready",
            || ready(&harness, &session_id),
        )
        .await;
        let name = live.object(&namespace, kind, &session_id);
        live.set_finalizer(&namespace, kind, &name, true);
        let response = act(&harness, &alice, &session_id, "stop").await;
        assert!(response.status().is_success());
        assert!(tokens_revoked(&harness, &session_id).await);
        let (blocked_s, _) = until(
            &harness,
            &labs,
            &time,
            Duration::from_secs(300),
            "quarantine",
            || async { stored(&harness, &session_id).await.state == State::CleanupBlocked },
        )
        .await;
        let record = stored(&harness, &session_id).await;
        let reason = record.blocked_reason.clone().unwrap_or_default();
        assert!(reason.contains(expected), "{reason}");
        assert_eq!(
            slot_condition(&harness, CampusLabSlotCondition::CleanupBlocked).await,
            1
        );
        let finalizers = live.stdout(&[
            "-n",
            &namespace,
            "get",
            kind,
            &name,
            "-o",
            "jsonpath={.metadata.finalizers}",
        ]);
        assert!(finalizers.contains(PROBE_FINALIZER), "{finalizers}");
        println!("PASS {kind} {name} held by a finalizer: cleanup_blocked after {blocked_s}s ({reason}); the finalizer is untouched and the slot is not reused");

        // One more blocked retry after the 30 s backoff, then the operator
        // removes the finalizer and the next retry finishes.
        harness.clock.advance(30);
        let retry = labs.reconcile(&harness.store, harness.now()).await;
        assert_eq!(retry.cleanup_retries, 1);
        assert_eq!(
            stored(&harness, &session_id).await.state,
            State::CleanupBlocked
        );
        live.set_finalizer(&namespace, kind, &name, false);
        let released = Instant::now();
        let (_, finished) = until(
            &harness,
            &labs,
            &time,
            Duration::from_secs(300),
            "finish",
            || async {
                harness.clock.advance(60);
                stored(&harness, &session_id).await.state == State::Terminated
                    && live.remaining(&namespace, &session_id).is_empty()
            },
        )
        .await;
        assert!(finished.cleanup_retries >= 1);
        assert_eq!(
            slot_condition(&harness, CampusLabSlotCondition::Free).await,
            1
        );
        let seconds = released.elapsed().as_secs();
        println!("PASS after the finalizer was removed the retried cleanup finished in {seconds}s and freed the slot");
        report(
            "stuck",
            json!({ "kind": kind, "blocked_seconds": blocked_s, "reason": reason, "finished_seconds": seconds }),
        );
    }
    let removed = live.remove_tombstones(&namespace, &sessions);
    println!("PASS removed {removed} tombstones of this run");
}

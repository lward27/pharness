//! Live qualification of the broker token boundary (LUC-131) against a real
//! lab slot. Ignored by default; `scripts/campus-egress-cluster-test.sh`
//! runs it.
//!
//! The lab session lives in this harness's journal and the broker router
//! answers in process. The real Kubernetes provider, with the journal's
//! token issuer, launches the session in the slot through the controller
//! identity; the test then reads what a student could see from inside the
//! Pod (operator `kubectl exec`), rotates the token and stops the session.
//!
//! Environment:
//! - `PHARNESS_CAMPUS_BROKER_LIVE_SLOT`: a lab slot namespace whose
//!   `pharness.dev/campus-slot` label is the journal's slot id,
//!   `tenant-a-lab-slot-01`.
//! - `PHARNESS_CAMPUS_BROKER_LIVE_CONTROLLER_ARGS`: kubectl global arguments
//!   for the controller identity.
//! - `PHARNESS_CAMPUS_BROKER_LIVE_ADMIN_ARGS`: kubectl global arguments for an
//!   operator, used for `exec` and to read back the Secret's metadata.
//! - `PHARNESS_CAMPUS_BROKER_LIVE_OUT`: evidence directory.

use super::broker::{denial, ready_session, workload};
use super::labs::{act, enroll_students, labs_harness};
use super::{Harness, TestClock, TENANT};
use pharness_store::JournalCampusAuthoritySource;
use pharness_workspaces::campus_capability::{CampusAuthoritySource, CampusBrokerClock};
use pharness_workspaces::campus_lab_session::kubernetes::{
    KubectlCampusKubeApi, KubernetesCampusProvider, KubernetesCampusProviderConfig,
};
use pharness_workspaces::campus_sandbox_pod::{
    campus_broker_token_secret_name, CAMPUS_BROKER_TOKEN_ID_ANNOTATION, CAMPUS_BROKER_TOKEN_PATH,
};
use pharness_workspaces::{
    CampusDeleteOutcome, CampusExecutionProvider, CampusProvisionOutcome, CampusProvisionSpec,
    CampusWorkloadRef,
};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

const RESEARCH: &str =
    include_str!("../../../../../pharness-workspaces/src/campus_profile/fixtures/research.json");
const CODING: &str =
    include_str!("../../../../../pharness-workspaces/src/campus_profile/fixtures/coding.json");

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

fn args(name: &str) -> Vec<String> {
    env(name).split_whitespace().map(str::to_string).collect()
}

fn report(event: &str, detail: Value) {
    println!("@@CAMPUS {}", json!({ "event": event, "detail": detail }));
}

/// The harness clock, optionally ahead, as the provider's clock.
struct HarnessClock(Arc<TestClock>, i64);

impl CampusBrokerClock for HarnessClock {
    fn now(&self) -> i64 {
        use super::super::state::CampusClock;
        self.0.now() + self.1
    }
}

struct Live {
    slot: String,
    admin: Vec<String>,
    controller: Arc<KubectlCampusKubeApi>,
}

impl Live {
    fn kubectl(&self, extra: &[&str]) -> std::process::Output {
        std::process::Command::new("kubectl")
            .args(&self.admin)
            .args(extra)
            .output()
            .expect("kubectl runs")
    }

    /// Run a shell script in the session container as an operator.
    fn exec(&self, pod: &str, script: &str) -> String {
        let output = self.kubectl(&[
            "-n",
            &self.slot,
            "exec",
            pod,
            "-c",
            "workspace",
            "--",
            "/bin/sh",
            "-c",
            script,
        ]);
        assert!(
            output.status.success(),
            "exec failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn token_in_pod(&self, pod: &str) -> String {
        self.exec(pod, &format!("cat {CAMPUS_BROKER_TOKEN_PATH}"))
            .trim()
            .to_string()
    }

    fn provider(
        &self,
        harness: &Harness,
        slot_id: &str,
        clock: Arc<dyn CampusBrokerClock>,
    ) -> KubernetesCampusProvider {
        let labs = harness.labs.clone().expect("labs");
        let mut config = KubernetesCampusProviderConfig::m1_lab();
        config.slots = [(slot_id.to_string(), self.slot.clone())].into();
        let authority = Arc::new(JournalCampusAuthoritySource::new(
            (*harness.store).clone(),
            labs.catalog.clone(),
            Arc::new(HarnessClock(harness.clock.clone(), 0)),
        ));
        KubernetesCampusProvider::new(
            config,
            [
                pharness_workspaces::CampusProfile::parse(RESEARCH).expect("research"),
                pharness_workspaces::CampusProfile::parse(CODING).expect("coding"),
            ],
            authority,
            harness.broker_tokens.clone(),
            self.controller.clone(),
            clock,
        )
        .expect("provider config validates")
    }
}

/// Everything a student process can read about credentials.
/// Section markers are printed through `s`, so the script's own command line
/// (visible in `/proc/*/cmdline`) never contains one.
const SCRAPE: &str = r#"
s() { printf '@@%s\n' "$1"; }
s env; env | sort
s environ
for p in /proc/[0-9]*; do
  echo "-- ${p#/proc/} $(tr '\0' ' ' < "$p/cmdline" 2>/dev/null)"
  tr '\0' '\n' < "$p/environ" 2>/dev/null
done
s mounts; cat /proc/mounts
s service-account; ls -la /var/run/secrets /run/secrets 2>&1
s broker-mount; ls -laL /var/run/campus-broker 2>&1
s credential-files
find / -xdev -type f \( -name '*token*' -o -name 'kubeconfig' -o -name '*credential*' -o -name '*.key' \) \
  -not -path '/usr/*' -not -path '/proc/*' 2>/dev/null
find /workspace /tmp -type f 2>/dev/null
s processes
for p in /proc/[0-9]*; do printf '%s %s\n' "${p#/proc/}" "$(tr '\0' ' ' < "$p/cmdline" 2>/dev/null)"; done
"#;

fn section<'a>(scrape: &'a str, name: &str) -> &'a str {
    let marker = format!("@@{name}\n");
    let start = scrape
        .find(&marker)
        .map(|index| index + marker.len())
        .unwrap_or(scrape.len());
    let rest = &scrape[start..];
    let end = if rest.starts_with("@@") {
        0
    } else {
        rest.find("\n@@").unwrap_or(rest.len())
    };
    &rest[..end]
}

async fn wait_ready(provider: &KubernetesCampusProvider, spec: &CampusProvisionSpec) -> u64 {
    let started = Instant::now();
    loop {
        match provider
            .provision(&format!("provision-{}", spec.workload.session_id), spec)
            .await
            .expect("provision call")
        {
            CampusProvisionOutcome::Ready { .. } => return started.elapsed().as_secs(),
            CampusProvisionOutcome::InProgress { .. } => {}
            CampusProvisionOutcome::Rejected { reason } => panic!("rejected: {reason}"),
        }
        assert!(started.elapsed() < Duration::from_secs(300), "not ready");
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn delete(provider: &KubernetesCampusProvider, workload: &CampusWorkloadRef) -> u64 {
    let started = Instant::now();
    loop {
        match provider
            .delete(&format!("delete-{}", workload.session_id), workload)
            .await
            .expect("delete call")
        {
            CampusDeleteOutcome::Deleted => return started.elapsed().as_secs(),
            CampusDeleteOutcome::InProgress => {}
            other => panic!("delete: {other:?}"),
        }
        assert!(started.elapsed() < Duration::from_secs(180), "not deleted");
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

#[tokio::test]
#[ignore = "needs a cluster; run scripts/campus-egress-cluster-test.sh"]
async fn campus_broker_live() {
    let live = Live {
        slot: env("PHARNESS_CAMPUS_BROKER_LIVE_SLOT"),
        admin: args("PHARNESS_CAMPUS_BROKER_LIVE_ADMIN_ARGS"),
        controller: Arc::new(KubectlCampusKubeApi::new(
            "kubectl",
            args("PHARNESS_CAMPUS_BROKER_LIVE_CONTROLLER_ARGS"),
        )),
    };
    let out = env("PHARNESS_CAMPUS_BROKER_LIVE_OUT");

    // A ready lab session in the journal, on the research profile (no
    // egress), then the real provider in the real slot.
    let harness = labs_harness(1).await;
    enroll_students(&harness, "cs101", &["broker-live"]).await;
    let student = harness.login("broker-live").await;
    let session_id = ready_session(&harness, &student, "lab-2").await;
    let workload = workload(&harness, &session_id).await;
    let source = JournalCampusAuthoritySource::new(
        (*harness.store).clone(),
        harness.labs.clone().expect("labs").catalog.clone(),
        Arc::new(HarnessClock(harness.clock.clone(), 0)),
    );
    let authority = source
        .session(&session_id)
        .await
        .expect("authority")
        .expect("session")
        .authority;
    let spec = CampusProvisionSpec {
        workload: workload.clone(),
        tenant_id: TENANT.into(),
        course_id: authority.course_id.clone(),
        profile: authority.policy.profile().clone(),
        expires_at: authority.expires_at,
    };
    let provider = live.provider(
        &harness,
        &workload.slot_id,
        Arc::new(HarnessClock(harness.clock.clone(), 0)),
    );
    let seconds = wait_ready(&provider, &spec).await;
    let pods = live.kubectl(&[
        "-n",
        &live.slot,
        "get",
        "pods",
        "-l",
        &format!("pharness.dev/campus-session={session_id}"),
        "-o",
        "jsonpath={.items[0].metadata.name}",
    ]);
    let pod = String::from_utf8_lossy(&pods.stdout).trim().to_string();
    assert!(!pod.is_empty(), "no Pod for {session_id}");
    report(
        "broker_session_ready",
        json!({ "slot": live.slot, "pod": pod, "seconds": seconds }),
    );

    // The delivered token is the session's and works at the broker.
    let first = live.token_in_pod(&pod);
    assert!(
        first.starts_with("pcbt1_"),
        "token file holds no broker token"
    );
    assert_eq!(denial(&harness, &first).await, None);
    println!("PASS the token mounted in the Pod authenticates its own session at the broker");

    // What a student can see.
    let scrape = live.exec(&pod, SCRAPE);
    std::fs::write(
        format!("{out}/broker-scrape.txt"),
        scrape.replace(&first, "<broker token>"),
    )
    .expect("write scrape");
    assert!(
        !scrape.contains("pcbt1_"),
        "a broker token appears in the environment, process list or file names"
    );
    println!("PASS no broker token in env, /proc/*/environ, process arguments or file names");
    let env_names = section(&scrape, "env")
        .lines()
        .filter_map(|line| line.split_once('=').map(|(name, _)| name.to_string()))
        .collect::<Vec<_>>();
    let shaped = env_names
        .iter()
        .filter(|name| {
            let upper = name.to_ascii_uppercase();
            [
                "TOKEN",
                "SECRET",
                "PASSWORD",
                "CREDENTIAL",
                "API_KEY",
                "AUTH",
            ]
            .iter()
            .any(|word| upper.contains(word))
        })
        .collect::<Vec<_>>();
    assert!(shaped.is_empty(), "credential-shaped variables: {shaped:?}");
    println!(
        "PASS no credential-shaped environment variable ({} variables)",
        env_names.len()
    );
    let mounts = section(&scrape, "mounts");
    assert!(!mounts.contains("serviceaccount"), "{mounts}");
    let broker_mount = mounts
        .lines()
        .find(|line| {
            // /var/run is a symlink to /run in the runner images.
            matches!(
                line.split(' ').nth(1),
                Some("/var/run/campus-broker" | "/run/campus-broker")
            )
        })
        .expect("broker mount");
    assert!(
        broker_mount
            .split(' ')
            .nth(3)
            .is_some_and(|options| options.split(',').any(|option| option == "ro")),
        "{broker_mount}"
    );
    println!("PASS no service-account mount; the broker token mount is read-only: {broker_mount}");
    let service_account = section(&scrape, "service-account");
    assert!(
        service_account.matches("No such file or directory").count() == 2,
        "{service_account}"
    );
    println!("PASS /var/run/secrets and /run/secrets do not exist");
    let files = section(&scrape, "credential-files").trim();
    assert!(files.is_empty(), "{files}");
    println!("PASS no other credential-shaped file on the root filesystem, /workspace or /tmp");
    let broker_dir = section(&scrape, "broker-mount");
    assert!(
        broker_dir.contains(" token\n") || broker_dir.ends_with(" token"),
        "{broker_dir}"
    );
    assert!(!broker_dir.contains("egress-ca.pem"), "{broker_dir}");
    let processes = section(&scrape, "processes").trim().lines().count();
    println!("PASS {CAMPUS_BROKER_TOKEN_PATH} is the only file in the broker mount; {processes} processes visible");
    report(
        "scrape",
        json!({ "env": env_names, "broker_mount": broker_mount, "processes": processes }),
    );

    // Rotation: a provider whose clock is past rotate_at replaces the Secret;
    // the kubelet refreshes the mounted file in place.
    let secret = campus_broker_token_secret_name(&workload);
    let token_id = |live: &Live| {
        let output = live.kubectl(&[
            "-n",
            &live.slot,
            "get",
            "secret",
            &secret,
            "-o",
            &format!(
                "jsonpath={{.metadata.annotations.{}}}",
                CAMPUS_BROKER_TOKEN_ID_ANNOTATION.replace('.', "\\.")
            ),
        ]);
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };
    let before_id = token_id(&live);
    let later = live.provider(
        &harness,
        &workload.slot_id,
        Arc::new(HarnessClock(harness.clock.clone(), 460)),
    );
    assert!(later
        .rotate_broker_token(&workload)
        .await
        .expect("rotation"));
    let after_id = token_id(&live);
    assert_ne!(before_id, after_id);
    let rotated_at = Instant::now();
    let second = loop {
        let current = live.token_in_pod(&pod);
        if current != first {
            break current;
        }
        assert!(
            rotated_at.elapsed() < Duration::from_secs(180),
            "the mounted token did not change"
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
    };
    let propagation = rotated_at.elapsed().as_secs();
    assert_eq!(denial(&harness, &second).await, None);
    assert_eq!(denial(&harness, &first).await, None);
    println!(
        "PASS rotation: Secret {before_id} -> {after_id}, the mounted file changed in place after {propagation}s; both tokens valid during the overlap"
    );
    report(
        "rotation",
        json!({ "from": before_id, "to": after_id, "propagation_seconds": propagation }),
    );

    // Stop: both tokens fail on their next request.
    let response = act(&harness, &student, &session_id, "stop").await;
    let stopped = Instant::now();
    assert!(response.status().is_success(), "{}", response.status());
    let current = denial(&harness, &second).await;
    let previous = denial(&harness, &first).await;
    let elapsed_ms = stopped.elapsed().as_millis();
    assert!(current.is_some() && previous.is_some());
    assert!(elapsed_ms < 60_000);
    println!(
        "PASS stop: the broker refuses the mounted token ({}) and the previous one ({}) {elapsed_ms} ms after the stop",
        current.as_deref().unwrap_or_default(),
        previous.as_deref().unwrap_or_default()
    );
    report(
        "revocation",
        json!({ "current": current, "previous": previous, "elapsed_ms": elapsed_ms }),
    );
    // The provider can no longer deliver a token to the stopped session.
    let refused = later.rotate_broker_token(&workload).await;
    println!("PASS rotation after stop: {refused:?}");
    assert!(refused.is_err() || refused == Ok(false));

    // Teardown removes the Job, the Pod and the token Secret.
    let teardown = live.provider(
        &harness,
        &workload.slot_id,
        Arc::new(pharness_workspaces::campus_capability::SystemCampusBrokerClock),
    );
    let seconds = delete(&teardown, &workload).await;
    let left = live.kubectl(&[
        "-n",
        &live.slot,
        "get",
        "jobs.batch,pods,secrets",
        "-l",
        &format!("pharness.dev/campus-session={session_id}"),
        "-o",
        "name",
    ]);
    let left = String::from_utf8_lossy(&left.stdout).trim().to_string();
    assert!(left.is_empty(), "{left}");
    println!("PASS delete removed the Job, Pod and token Secret in {seconds}s");
}

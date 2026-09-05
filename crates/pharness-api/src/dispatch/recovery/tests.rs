use super::{bind_manifest, create_or_reconcile_job, find_exact_job, validate_observed_job};
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

fn manifest() -> Value {
    bind_manifest(json!({
        "apiVersion":"batch/v1", "kind":"Job",
        "metadata":{"name":"pharness-run-fixed", "namespace":"fixture", "labels":{"run":"fixed"}},
        "spec":{"backoffLimit":0,"template":{"spec":{
            "restartPolicy":"Never", "containers":[{"name":"worker","image":"registry.test/worker@sha256:fixed",
                "command":["pharness-worker"],"env":[{"name":"PHARNESS_RUN_ID","value":"run_fixed"}]}]}}}
    }))
}

pub(crate) struct KubectlFixture {
    pub(crate) dir: PathBuf,
    pub(crate) command: String,
}

impl KubectlFixture {
    pub(crate) fn new(unavailable: bool) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "pharness-dispatch-recovery-{}",
            uuid::Uuid::now_v7().simple()
        ));
        std::fs::create_dir(&dir).unwrap();
        let command = dir.join("kubectl");
        // These are owned UUID paths without shell metacharacters. The fixture
        // has no Kubernetes credentials and cannot contact an external API.
        let script = format!(
            r#"#!/bin/sh
case "$1" in
get)
  if [ "{unavailable}" = "true" ]; then exit 9; fi
  if [ "$2" = "jobs" ]; then printf '{{"items":[]}}'; exit 0; fi
  if [ "$2" = "persistentvolumeclaim" ]; then printf 'existing-workspace'; exit 0; fi
  if [ -f '{dir}/job.json' ]; then cat '{dir}/job.json'; fi
  exit 0;;
create)
  cat > '{dir}/input.json'
  if mkdir '{dir}/created' 2>/dev/null; then
    cp '{dir}/input.json' '{dir}/job.json'
    printf x >> '{dir}/creates'
  fi
  exit 1;;
*) exit 99;;
esac
"#,
            dir = dir.display()
        );
        std::fs::write(&command, script).unwrap();
        std::fs::set_permissions(&command, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            dir,
            command: command.to_str().unwrap().into(),
        }
    }

    pub(crate) fn creates(&self) -> usize {
        std::fs::read(self.dir.join("creates"))
            .map(|bytes| bytes.len())
            .unwrap_or(0)
    }
}

impl Drop for KubectlFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn observed_job_requires_exact_dispatch_and_requested_execution_fields() {
    let expected = manifest();
    let mut observed = expected.clone();
    observed["metadata"]["uid"] = json!("server-uid");
    observed["spec"]["selector"] = json!({"matchLabels":{"controller-uid":"server-uid"}});
    observed["spec"]["template"]["spec"]["dnsPolicy"] = json!("ClusterFirst");
    observed["status"] = json!({"active":1});
    validate_observed_job(&expected, &observed).unwrap();
    for pointer in [
        "/spec/template/spec/containers/0/image",
        "/metadata/name",
        "/metadata/namespace",
        "/metadata/annotations/pharness.lucas.engineering~1dispatch-hash",
    ] {
        let mut changed = observed.clone();
        *changed.pointer_mut(pointer).unwrap() = json!("changed");
        assert!(
            validate_observed_job(&expected, &changed).is_err(),
            "{pointer}"
        );
    }
    observed["spec"]["template"]["spec"]["containers"][0]["env"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":"UNEXPECTED","value":"extra"}));
    assert!(validate_observed_job(&expected, &observed).is_err());
    let mut terminating = expected.clone();
    terminating["metadata"]["deletionTimestamp"] = json!("2026-09-05T12:00:00Z");
    assert!(validate_observed_job(&expected, &terminating).is_err());
}

#[tokio::test]
async fn lost_create_acknowledgment_and_repeated_dispatch_reuse_one_job() {
    let fixture = KubectlFixture::new(false);
    let expected = manifest();
    assert!(find_exact_job(&fixture.command, "fixture", &expected)
        .await
        .unwrap()
        .is_none());
    create_or_reconcile_job(&fixture.command, "fixture", &expected)
        .await
        .unwrap();
    create_or_reconcile_job(&fixture.command, "fixture", &expected)
        .await
        .unwrap();
    assert_eq!(fixture.creates(), 1);
}

#[tokio::test]
async fn failed_observation_never_dispatches_and_conflicting_job_is_never_replaced() {
    let unavailable = KubectlFixture::new(true);
    assert!(
        create_or_reconcile_job(&unavailable.command, "fixture", &manifest())
            .await
            .is_err()
    );
    assert_eq!(unavailable.creates(), 0);
    let conflicting = KubectlFixture::new(false);
    let mut wrong = manifest();
    wrong["spec"]["template"]["spec"]["containers"][0]["image"] = json!("wrong-image");
    let before = wrong.to_string();
    std::fs::write(conflicting.dir.join("job.json"), &before).unwrap();
    assert!(
        create_or_reconcile_job(&conflicting.command, "fixture", &manifest())
            .await
            .is_err()
    );
    assert_eq!(conflicting.creates(), 0);
    assert_eq!(
        std::fs::read_to_string(conflicting.dir.join("job.json")).unwrap(),
        before
    );
}

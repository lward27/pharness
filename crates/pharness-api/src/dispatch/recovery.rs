use pharness_store::StoredRun;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

const DISPATCH_HASH: &str = "pharness.lucas.engineering/dispatch-hash";

pub(super) fn is_hosted(run: &StoredRun) -> bool {
    run.execution_target_json
        .get("hosted_workflow_policy_hash")
        .and_then(Value::as_str)
        .is_some()
}

pub(super) fn bind_manifest(mut manifest: Value) -> Value {
    let hash = format!(
        "sha256:{:x}",
        Sha256::digest(manifest.to_string().as_bytes())
    );
    manifest["metadata"]["annotations"][DISPATCH_HASH] = Value::String(hash);
    manifest
}

/// Read the exact deterministic name. A matching name alone is insufficient:
/// require the recorded dispatch hash and the requested immutable manifest.
pub(super) async fn find_exact_job(
    kubectl: &str,
    namespace: &str,
    expected: &Value,
) -> anyhow::Result<Option<Value>> {
    let name = expected
        .pointer("/metadata/name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("hosted Job requires a deterministic name"))?;
    let mut command = tokio::process::Command::new(kubectl);
    command
        .args([
            "get",
            "job",
            name,
            "-n",
            namespace,
            "--ignore-not-found=true",
            "-o",
            "json",
            "--request-timeout=10s",
        ])
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(12), command.output()).await??;
    if !output.status.success() {
        // Do not expose credential-bearing client errors or assume absence on a
        // timeout/authentication failure.
        anyhow::bail!("the hosted Job could not be observed; dispatch outcome is unknown");
    }
    if output.stdout.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    let observed: Value = serde_json::from_slice(&output.stdout)?;
    validate_observed_job(expected, &observed)?;
    Ok(Some(observed))
}

fn validate_observed_job(expected: &Value, observed: &Value) -> anyhow::Result<()> {
    if expected["metadata"]["annotations"][DISPATCH_HASH]
        .as_str()
        .is_none()
        || expected["metadata"]["annotations"][DISPATCH_HASH]
            != observed["metadata"]["annotations"][DISPATCH_HASH]
        || observed
            .pointer("/metadata/deletionTimestamp")
            .is_some_and(|value| !value.is_null())
        || !contains_requested_fields(expected, observed)
    {
        anyhow::bail!(
            "existing hosted Job does not match its recorded dispatch; intervention is required"
        );
    }
    Ok(())
}

// Kubernetes adds default fields, labels, selectors and status. Preserve all
// requested values while allowing those additions; array membership/order is
// exact so an extra container or environment entry is never ignored.
fn contains_requested_fields(expected: &Value, observed: &Value) -> bool {
    match (expected, observed) {
        (Value::Object(expected), Value::Object(observed)) => {
            expected.iter().all(|(key, value)| {
                observed
                    .get(key)
                    .is_some_and(|actual| contains_requested_fields(value, actual))
            })
        }
        (Value::Array(expected), Value::Array(observed)) => {
            expected.len() == observed.len()
                && expected
                    .iter()
                    .zip(observed)
                    .all(|(a, b)| contains_requested_fields(a, b))
        }
        _ => expected == observed,
    }
}

pub(super) async fn create_or_reconcile_job(
    kubectl: &str,
    namespace: &str,
    manifest: &Value,
) -> anyhow::Result<()> {
    if find_exact_job(kubectl, namespace, manifest)
        .await?
        .is_some()
    {
        return Ok(());
    }
    let payload = serde_json::to_vec(manifest)?;
    let result = tokio::time::timeout(Duration::from_secs(12), async {
        let mut child = tokio::process::Command::new(kubectl)
            .args([
                "create",
                "-n",
                namespace,
                "-f",
                "-",
                "--request-timeout=10s",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(&payload).await?;
        }
        child.wait().await
    })
    .await;
    if matches!(result, Ok(Ok(status)) if status.success()) {
        return Ok(());
    }
    // The create request may have succeeded even when its response was lost.
    // Re-read before any retry, and never delete or replace the existing Job.
    if find_exact_job(kubectl, namespace, manifest)
        .await?
        .is_some()
    {
        return Ok(());
    }
    anyhow::bail!("hosted dispatch was not acknowledged and no matching Job is visible; retain the operation for reconciliation")
}

#[cfg(test)]
mod tests;

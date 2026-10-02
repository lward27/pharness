use super::{FinanceDeploymentExpectation, ReadOnlyClusterTools};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command};

pub(super) struct Resources {
    pub argo: Value,
    pub deployment: Value,
    pub replicasets: Value,
    pub pods: Value,
    pub service: Value,
    pub endpoints: Value,
    pub receipts: Vec<Value>,
}

pub(super) async fn read(
    tools: &ReadOnlyClusterTools,
    expected: &FinanceDeploymentExpectation,
) -> Result<Resources, &'static str> {
    // Concurrent, independently bounded reads. The identity validator fences
    // their owner UIDs, resource versions and desired/observed generations.
    let selector = format!("app={}", expected.workload());
    let endpoints = format!("kubernetes.io/service-name={}", expected.workload());
    let n = expected.namespace();
    let (a, d, r, p, s, e) = tokio::join!(
        get(
            tools,
            "applications.argoproj.io",
            &tools.argocd_namespace,
            Some(expected.argo_application()),
            None
        ),
        get(
            tools,
            "deployments.apps",
            n,
            Some(expected.workload()),
            None
        ),
        get(tools, "replicasets.apps", n, None, Some(&selector)),
        get(tools, "pods", n, None, Some(&selector)),
        get(tools, "services", n, Some(expected.workload()), None),
        get(
            tools,
            "endpointslices.discovery.k8s.io",
            n,
            None,
            Some(&endpoints)
        ),
    );
    let (a, d, r, p, s, e) = (a?, d?, r?, p?, s?, e?);
    Ok(Resources {
        argo: a.0,
        deployment: d.0,
        replicasets: r.0,
        pods: p.0,
        service: s.0,
        endpoints: e.0,
        receipts: vec![a.1, d.1, r.1, p.1, s.1, e.1],
    })
}

async fn get(
    tools: &ReadOnlyClusterTools,
    resource: &str,
    namespace: &str,
    name: Option<&str>,
    selector: Option<&str>,
) -> Result<(Value, Value), &'static str> {
    let limit = tools.max_output_bytes.clamp(1, 512 * 1024);
    let timeout_ms = tools.timeout_ms.clamp(1, 15_000);
    let mut command = Command::new(&tools.kubectl_bin);
    command.args([
        "get",
        resource,
        "-n",
        namespace,
        "-o",
        "json",
        "--request-timeout=10s",
        "--chunk-size=50",
    ]);
    if let Some(name) = name {
        command.arg(name);
    }
    if let Some(selector) = selector {
        command.args(["-l", selector]);
    }
    // Raw Kubernetes errors or pod specs may contain credentials. Only hashes,
    // fixed diagnostics and whitelisted identity fields leave this reader.
    command
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let bytes = tokio::time::timeout(Duration::from_millis(timeout_ms), async {
        let mut child = command
            .spawn()
            .map_err(|_| "kubernetes_reader_unavailable")?;
        let mut stdout = child.stdout.take().ok_or("kubernetes_stdout_unavailable")?;
        let mut bytes = Vec::new();
        let mut chunk = [0_u8; 8192];
        loop {
            let count = stdout
                .read(&mut chunk)
                .await
                .map_err(|_| "kubernetes_response_read_failed")?;
            if count == 0 {
                break;
            }
            if count > limit.saturating_sub(bytes.len()) {
                let _ = child.kill().await;
                return Err("kubernetes_response_too_large");
            }
            bytes.extend_from_slice(&chunk[..count]);
        }
        if !child
            .wait()
            .await
            .map_err(|_| "kubernetes_reader_wait_failed")?
            .success()
        {
            return Err("kubernetes_read_failed");
        }
        Ok(bytes)
    })
    .await
    .map_err(|_| "kubernetes_read_timed_out")??;
    let body: Value =
        serde_json::from_slice(&bytes).map_err(|_| "malformed_kubernetes_response")?;
    let receipt = json!({"resource":resource,"namespace":namespace,"name":name,"selector":selector,"bytes":bytes.len(),"sha256":format!("sha256:{:x}",Sha256::digest(&bytes))});
    Ok((body, receipt))
}

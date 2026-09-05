//! The existing Tekton executor with one durable hosted create admission.
use anyhow::{Context, Result};
use pharness_core::hosted_sdlc::build::{validate_manifest, validate_observed_run, DEADLINE};
use serde_json::{json, Value};
use std::{
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::io::AsyncWriteExt;

pub(super) async fn execute() -> Result<()> {
    let api = super::required_env("PHARNESS_API_URL")?
        .trim_end_matches('/')
        .to_string();
    let intent = super::required_env("PHARNESS_PIPELINE_INTENT_ID")?;
    let execution = super::required_env("PHARNESS_EXECUTION_ID")?;
    let token = super::required_env("PHARNESS_WORKER_TOKEN")?;
    let manifest: Value =
        serde_json::from_str(&super::required_env("PHARNESS_TEKTON_PIPELINERUN_JSON")?)?;
    validate_manifest(&manifest).map_err(anyhow::Error::msg)?;
    let manifest_hash =
        pharness_core::canonical_json_sha256(&manifest).map_err(anyhow::Error::msg)?;
    let observe_only = std::env::var("PHARNESS_HOSTED_BUILD_OBSERVE_ONLY").as_deref() == Ok("true");
    let deadline = manifest["metadata"]["annotations"][DEADLINE]
        .as_str()
        .unwrap()
        .parse::<u128>()?;
    let poll = super::tekton_poll_interval()?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let transport = Transport {
        client,
        api,
        intent,
        execution,
        token,
        manifest_hash,
        observe_only,
    };
    let mut run = read_run(&manifest).await?;
    if run.is_none() && !observe_only {
        anyhow::ensure!(
            now() < deadline,
            "original hosted build deadline expired before admission"
        );
        // Do not retry admission. An uncertain response cannot authorize a
        // PipelineRun create even when a record may have reached the API.
        let admitted=transport.client.post(format!("{}/api/internal/pipeline-intents/{}/execution-attempt",transport.api,transport.intent))
            .bearer_auth(&transport.token).json(&json!({"execution_id":transport.execution,"manifest_hash":transport.manifest_hash})).send().await;
        let allowed = match admitted {
            Ok(response) if response.status().is_success() => {
                response.json::<Value>().await.is_ok_and(|v| {
                    v["admitted"] == true && v["manifest_hash"] == transport.manifest_hash
                })
            }
            _ => false,
        };
        if allowed && now() < deadline {
            // The API admitted exactly one provider attempt. Always observe
            // afterwards; a failed client acknowledgement is not proof of absence.
            create_once(&manifest).await;
        }
        run = read_run(&manifest).await?;
        if !allowed && run.is_none() {
            return transport.unavailable("build_admission_unconfirmed").await;
        }
    }
    let appear_deadline = now().saturating_add(60_000).min(deadline);
    while run.is_none() && now() < appear_deadline {
        tokio::time::sleep(poll).await;
        run = read_run(&manifest).await?;
    }
    let Some(mut run) = run else {
        return transport
            .unavailable("original_pipeline_run_unavailable")
            .await;
    };
    let uid = run["metadata"]["uid"]
        .as_str()
        .context("observed PipelineRun has no UID")?
        .to_string();
    let mut submitted = false;
    loop {
        validate_observed_run(&manifest, &run, Some(&uid)).map_err(anyhow::Error::msg)?;
        if super::pipeline_run_terminal(&run).is_some() {
            let namespace = manifest["metadata"]["namespace"].as_str().unwrap();
            let name = manifest["metadata"]["name"].as_str().unwrap();
            return match super::analyze_terminal_pipeline_run(namespace, name).await {
                Ok(analysis) => transport.observed(&run, Some(analysis)).await,
                Err(_) => {
                    transport
                        .unavailable("terminal_build_analysis_unavailable")
                        .await
                }
            };
        }
        if !submitted {
            transport.observed(&run, None).await?;
            submitted = true;
        }
        if now() >= deadline {
            return transport
                .unavailable("original_build_observation_deadline")
                .await;
        }
        tokio::time::sleep(poll).await;
        run = read_run(&manifest)
            .await?
            .context("original PipelineRun disappeared during observation")?;
    }
}

struct Transport {
    client: reqwest::Client,
    api: String,
    intent: String,
    execution: String,
    token: String,
    manifest_hash: String,
    observe_only: bool,
}
impl Transport {
    async fn observed(&self, run: &Value, analysis: Option<Value>) -> Result<()> {
        self.post(json!({"execution_id":self.execution,"manifest_hash":self.manifest_hash,"pipeline_run":run,"analysis":analysis,"observe_only":self.observe_only})).await
    }
    async fn unavailable(&self, code: &str) -> Result<()> {
        self.post(json!({"execution_id":self.execution,"manifest_hash":self.manifest_hash,"error_code":code,"observe_only":self.observe_only})).await
    }
    async fn post(&self, body: Value) -> Result<()> {
        for attempt in 0..3 {
            let response = self
                .client
                .post(format!(
                    "{}/api/internal/pipeline-intents/{}/hosted-execution-outcome",
                    self.api, self.intent
                ))
                .bearer_auth(&self.token)
                .json(&body)
                .send()
                .await;
            match response {
                Ok(response) if response.status().is_success() => return Ok(()),
                Ok(response) if response.status().is_client_error() => anyhow::bail!(
                    "hosted build outcome rejected with HTTP {}",
                    response.status().as_u16()
                ),
                _ => {}
            }
            if attempt < 2 {
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
        anyhow::bail!(
            "hosted build outcome was not acknowledged; original evidence remains recoverable"
        )
    }
}
fn now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
async fn read_run(expected: &Value) -> Result<Option<Value>> {
    let mut command = tokio::process::Command::new("kubectl");
    command
        .args([
            "get",
            "pipelinerun",
            expected["metadata"]["name"].as_str().unwrap(),
            "-n",
            expected["metadata"]["namespace"].as_str().unwrap(),
            "--ignore-not-found=true",
            "-o",
            "json",
            "--request-timeout=10s",
        ])
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(12), command.output()).await??;
    anyhow::ensure!(
        output.status.success(),
        "original PipelineRun could not be observed; absence is unproven"
    );
    if output.stdout.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    let value: Value = serde_json::from_slice(&output.stdout)?;
    validate_observed_run(expected, &value, None).map_err(anyhow::Error::msg)?;
    Ok(Some(value))
}
async fn create_once(manifest: &Value) {
    let attempt = async {
        let mut child = tokio::process::Command::new("kubectl")
            .args(["create", "-f", "-", "--request-timeout=10s"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        if let Some(mut input) = child.stdin.take() {
            input.write_all(manifest.to_string().as_bytes()).await?;
        }
        child.wait().await
    };
    let _ = tokio::time::timeout(Duration::from_secs(12), attempt).await;
}

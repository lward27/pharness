#![cfg(unix)]
use serde_json::{json, Value};
use std::{
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn hosted_worker_never_retries_creation_after_uncertain_admission_or_observation_recovery() {
    for scenario in [
        "lost_create_ack",
        "denied_admission",
        "lost_admission_ack",
        "observe_existing",
    ] {
        let dir = std::env::temp_dir().join(format!(
            "pharness-hosted-build-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let fixture = Fixture(dir);
        let deadline = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
            + 60_000;
        let manifest = json!({"apiVersion":"tekton.dev/v1","kind":"PipelineRun","metadata":{"name":"pharness-build-fixture","namespace":"tekton-pipelines","annotations":{"pharness.lucas.engineering/workflow-operation-id":"operation_test","pharness.lucas.engineering/execution-id":"execution_test","pharness.lucas.engineering/build-deadline-ms":deadline.to_string()}},"spec":{"pipelineRef":{"name":"pharness-yfinance-build"},"params":[{"name":"revision","value":"c".repeat(40)}],"taskRunTemplate":{"serviceAccountName":"pharness-finance-build"}}});
        let manifest_hash = pharness_core::canonical_json_sha256(&manifest).unwrap();
        let mut observed = manifest.clone();
        observed["metadata"]["uid"] = json!("pipeline-fixture-uid");
        observed["status"] = json!({"conditions":[{"type":"Succeeded","status":"True"}],"results":[{"name":"SOURCE_COMMIT","value":"c".repeat(40)},{"name":"IMAGE_URL","value":format!("registry.lucas.engineering/yfinance_wrapper:git-{}","c".repeat(40))},{"name":"IMAGE_DIGEST","value":format!("sha256:{}","d".repeat(64))}]});
        std::fs::write(fixture.0.join("result.json"), observed.to_string()).unwrap();
        if scenario == "observe_existing" {
            std::fs::write(fixture.0.join("created"), "existing provider operation").unwrap();
        }
        let root = serde_json::to_string(fixture.0.to_str().unwrap()).unwrap();
        let script = format!(
            r#"#!/usr/bin/env python3
import sys,json
from pathlib import Path
root=Path({root})
if sys.argv[1]=='create':
    json.load(sys.stdin)
    with (root/'create-count').open('a') as f:f.write('create\n')
    (root/'created').write_text('created once')
    sys.exit(1)
if sys.argv[1]=='get':
    if sys.argv[2] in ('taskrun','taskruns'):
        print(json.dumps({{'items':[{{'kind':'TaskRun','metadata':{{'name':'build-task','namespace':'tekton-pipelines','uid':'task-uid'}},'status':{{'conditions':[{{'type':'Succeeded','status':'True'}}]}}}}]}}))
    elif (root/'created').exists():print((root/'result.json').read_text())
    sys.exit(0)
sys.exit(99)
"#
        );
        let kubectl = fixture.0.join("kubectl");
        std::fs::write(&kubectl, script).unwrap();
        std::fs::set_permissions(&kubectl, std::fs::Permissions::from_mode(0o700)).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let captured = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
        let records = captured.clone();
        let hash = manifest_hash.clone();
        let server = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let mut chunk = [0; 4096];
                let (head_end, length) = loop {
                    let size = stream.read(&mut chunk).await.unwrap();
                    if size == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&chunk[..size]);
                    if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]);
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap();
                        break (end + 4, length);
                    }
                };
                while bytes.len() < head_end + length {
                    let size = stream.read(&mut chunk).await.unwrap();
                    assert!(size > 0);
                    bytes.extend_from_slice(&chunk[..size]);
                }
                let path = String::from_utf8_lossy(&bytes[..head_end])
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .to_string();
                let body: Value =
                    serde_json::from_slice(&bytes[head_end..head_end + length]).unwrap();
                records.lock().unwrap().push((path.clone(), body));
                let admission = path.ends_with("/execution-attempt");
                if admission && scenario == "lost_admission_ack" {
                    drop(stream);
                    continue;
                }
                let status = if admission && scenario == "denied_admission" {
                    "409 Conflict"
                } else {
                    "200 OK"
                };
                let response = if admission {
                    json!({"admitted":scenario!="denied_admission","manifest_hash":hash})
                } else {
                    json!({"recorded":true})
                }
                .to_string();
                stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",response.len()).as_bytes()).await.unwrap();
            }
        });
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_pharness-worker"));
        child
            .env_clear()
            .env(
                "PATH",
                format!("{}:{}", fixture.0.display(), std::env::var("PATH").unwrap()),
            )
            .env("HOME", &fixture.0)
            .env("PHARNESS_KUBECTL_BIN", &kubectl)
            .env("PHARNESS_EXECUTION_KIND", "tekton_trigger")
            .env("PHARNESS_HOSTED_BUILD", "true")
            .env(
                "PHARNESS_HOSTED_BUILD_OBSERVE_ONLY",
                if scenario == "observe_existing" {
                    "true"
                } else {
                    "false"
                },
            )
            .env("PHARNESS_API_URL", format!("http://{address}"))
            .env("PHARNESS_PIPELINE_INTENT_ID", "intent_test")
            .env("PHARNESS_EXECUTION_ID", "execution_test")
            .env(
                "PHARNESS_WORKER_TOKEN",
                "fixture-token-with-no-external-access",
            )
            .env("PHARNESS_TEKTON_PIPELINERUN_JSON", manifest.to_string())
            .env("PHARNESS_TEKTON_EXECUTOR_POLL_SECONDS", "1")
            .kill_on_drop(true);
        let result = tokio::time::timeout(Duration::from_secs(20), child.output())
            .await
            .unwrap()
            .unwrap();
        server.abort();
        assert!(
            result.status.success(),
            "{scenario}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let count = std::fs::read_to_string(fixture.0.join("create-count"))
            .unwrap_or_default()
            .lines()
            .count();
        assert_eq!(
            count,
            usize::from(scenario == "lost_create_ack"),
            "{scenario}"
        );
        let records = captured.lock().unwrap();
        assert_eq!(
            records
                .iter()
                .filter(|(p, _)| p.ends_with("/execution-attempt"))
                .count(),
            usize::from(scenario != "observe_existing"),
            "admission must never be retried"
        );
        let outcome = &records.last().unwrap().1;
        if matches!(scenario, "lost_create_ack" | "observe_existing") {
            assert_eq!(
                outcome["pipeline_run"]["metadata"]["uid"],
                "pipeline-fixture-uid"
            );
            assert_eq!(
                outcome["analysis"]["outputs"]["source_commit"],
                "c".repeat(40)
            );
        } else {
            assert!(outcome.get("pipeline_run").is_none());
            assert_eq!(outcome["error_code"], "build_admission_unconfirmed");
        }
    }
}

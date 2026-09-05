use super::*;
use pharness_core::hosted_sdlc::{
    gitops_patch::update_kustomization_image, staging::AUTHORITY_SCHEMA,
};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
mod flow;

struct Mock {
    git: GitHub,
    requests: Arc<Mutex<Vec<(String, Value)>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn mock(responses: Vec<(u16, Value)>) -> Mock {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = requests.clone();
    let queue = Arc::new(Mutex::new(std::collections::VecDeque::from(responses)));
    let task = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let mut chunk = [0; 4096];
            let (header_end, length) = loop {
                let n = socket.read(&mut chunk).await.unwrap();
                if n == 0 {
                    return;
                }
                bytes.extend_from_slice(&chunk[..n]);
                if let Some(i) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&bytes[..i]);
                    let length = header
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|s| s.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    break (i + 4, length);
                }
            };
            while bytes.len() < header_end + length {
                let n = socket.read(&mut chunk).await.unwrap();
                if n == 0 {
                    return;
                }
                bytes.extend_from_slice(&chunk[..n]);
            }
            let first = String::from_utf8_lossy(&bytes[..header_end])
                .lines()
                .next()
                .unwrap()
                .to_string();
            let body = if length == 0 {
                Value::Null
            } else {
                serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap()
            };
            seen.lock().unwrap().push((first, body));
            let (status, payload) = queue
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or((500, json!({"unexpected":true})));
            let body = serde_json::to_vec(&payload).unwrap();
            let header = format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len());
            let _ = socket.write_all(header.as_bytes()).await;
            let _ = socket.write_all(&body).await;
        }
    });
    let mut git = GitHub::new("fixture-only-token".into()).unwrap();
    git.rest = format!("http://{addr}/repos/lward27/lucas_engineering");
    git.graphql = format!("http://{addr}/graphql");
    Mock {
        git,
        requests,
        task,
    }
}

fn authority() -> HostedStagingAuthority {
    serde_json::from_value(json!({"schema_version":AUTHORITY_SCHEMA,"work_item_id":"work_fixture","operation_id":"operation_fixture",
        "execution_id":"execution_fixture","pipeline_intent_id":"pipeline_fixture","deployment_intent_id":"deployment_fixture",
        "gitops_change_set_id":"gitops_fixture","workflow_policy_hash":format!("sha256:{}","a".repeat(64)),
        "build_evidence_hash":format!("sha256:{}","b".repeat(64)),"application_repository":"https://github.com/lward27/yfinance_wrapper.git",
        "source_commit_sha":"c".repeat(40),"image_digest":format!("sha256:{}","d".repeat(64)),"created_at_ms":1000,"expires_at_ms":3601000})).unwrap()
}

fn plan(a: &HostedStagingAuthority) -> StagingGitOpsPlan {
    let original = format!("apiVersion: kustomize.config.k8s.io/v1beta1\nkind: Kustomization\nnamespace: apps-staging\nimages:\n  - name: registry.lucas.engineering/yfinance_wrapper\n    digest: sha256:{}\n", "0".repeat(64));
    StagingGitOpsPlan {
        schema_version: PLAN_SCHEMA.into(),
        authority_hash: a.material_hash().unwrap(),
        base_commit_sha: "a".repeat(40),
        original_blob_sha: "b".repeat(40),
        updated_content: update_kustomization_image(
            &original,
            a.coordinates().unwrap().1,
            &a.image_ref().unwrap(),
        )
        .unwrap(),
        original_content: original,
    }
}
fn branch(sha: &str) -> Value {
    json!({"name":"main","protected":false,"commit":{"sha":sha}})
}
fn file(a: &HostedStagingAuthority, body: &str) -> Value {
    let blob = if body.contains(&a.image_digest) {
        "d"
    } else {
        "b"
    };
    json!({"type":"file","path":a.coordinates().unwrap().0,"encoding":"base64","sha":blob.repeat(40),"size":body.len(),"content":STANDARD.encode(body)})
}
fn commit(a: &HostedStagingAuthority, p: &StagingGitOpsPlan) -> Value {
    json!({"sha":"c".repeat(40),"parents":[{"sha":p.base_commit_sha}],
    "commit":{"message":format!("{}\n\n{}\n",p.commit_headline(a),p.commit_body(a).unwrap())},"files":[{"filename":a.coordinates().unwrap().0,"status":"modified"}]})
}

#[tokio::test]
async fn native_staging_reads_a_pinned_file_and_submits_one_expected_head_mutation() {
    let a = authority();
    let p = plan(&a);
    let m=mock(vec![(200,branch(&p.base_commit_sha)),(200,file(&a,&p.original_content)),
        (200,json!({"data":{"createCommitOnBranch":{"clientMutationId":a.execution_id,"commit":{"oid":"c".repeat(40)}}}}))]).await;
    let observed = m.git.plan(&a).await.unwrap();
    assert_eq!(observed, p);
    assert_eq!(
        m.git.commit_once(&a, &p, 2000).await.unwrap(),
        "c".repeat(40)
    );
    let requests = m.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests[1]
        .0
        .contains(&format!("?ref={}", p.base_commit_sha)));
    assert!(requests[2].0.starts_with("POST /graphql "));
    assert_eq!(requests[2].1, p.commit_request(&a, 2000).unwrap());
}

#[tokio::test]
async fn rejected_or_ambiguous_mutations_are_not_retried_and_do_not_echo_provider_bodies() {
    let a = authority();
    let p = plan(&a);
    for response in [
        (500, json!({"provider_detail":"must-not-be-retained"})),
        (200, json!({"errors":[{"message":"must-not-be-retained"}]})),
    ] {
        let m = mock(vec![response]).await;
        let error = m
            .git
            .commit_once(&a, &p, 2000)
            .await
            .unwrap_err()
            .to_string();
        assert_eq!(error, "staging_commit_unconfirmed");
        assert!(!error.contains("must-not-be-retained"));
        assert_eq!(m.requests.lock().unwrap().len(), 1);
    }
    let m = mock(vec![]).await;
    assert!(m.git.commit_once(&a, &p, a.expires_at_ms).await.is_err());
    assert!(m.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn recovery_observes_the_original_commit_without_sending_a_mutation() {
    let a = authority();
    let p = plan(&a);
    let c = commit(&a, &p);
    for known in [None, Some("c".repeat(40))] {
        let mut responses = vec![(200, branch(&"c".repeat(40)))];
        if known.is_none() {
            responses.push((200, json!([c])));
        }
        responses.extend([(200, c.clone()), (200, file(&a, &p.updated_content))]);
        let m = mock(responses).await;
        let result = m.git.observe(&a, &p, known.as_deref()).await.unwrap();
        assert_eq!(result["status"], "committed");
        assert_eq!(result["gitops_commit_sha"], "c".repeat(40));
        assert_eq!(result["plan_hash"], p.material_hash().unwrap());
        assert!(m
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|(line, _)| line.starts_with("GET ")));
    }
}

#[tokio::test]
async fn observation_rejects_unrelated_history_wrong_parents_and_unapproved_files() {
    let a = authority();
    let p = plan(&a);
    let empty = mock(vec![(200, branch(&"c".repeat(40))), (200, json!([]))]).await;
    assert!(empty.git.observe(&a, &p, None).await.is_err());
    for field in ["parent", "message", "file", "extra_file"] {
        let mut c = commit(&a, &p);
        match field {
            "parent" => c["parents"][0]["sha"] = json!("f".repeat(40)),
            "message" => c["commit"]["message"] = json!("Unrelated commit"),
            "file" => {
                c["files"][0]["filename"] = json!("charts/yfinance-wrapper/kustomization.yaml")
            }
            "extra_file" => c["files"]
                .as_array_mut()
                .unwrap()
                .push(json!({"filename":"other","status":"modified"})),
            _ => unreachable!(),
        }
        let m = mock(vec![(200, branch(&"c".repeat(40))), (200, c)]).await;
        assert!(
            m.git.observe(&a, &p, Some(&"c".repeat(40))).await.is_err(),
            "{field}"
        );
        assert!(m
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|(line, _)| line.starts_with("GET ")));
    }
}

#[tokio::test]
async fn bounded_reads_reject_oversized_content_and_changed_branch_protection() {
    let a = authority();
    let p = plan(&a);
    let mut protected = branch(&p.base_commit_sha);
    protected["protected"] = json!(true);
    let m = mock(vec![(200, protected)]).await;
    assert!(m.git.plan(&a).await.is_err());
    let m = mock(vec![(200, json!({"padding":"x".repeat(262145)}))]).await;
    assert!(m.git.branch().await.is_err());
    let mut oversize = file(&a, &p.original_content);
    oversize["size"] = json!(MAX_FILE_BYTES + 1);
    let m = mock(vec![(200, branch(&p.base_commit_sha)), (200, oversize)]).await;
    assert!(m.git.plan(&a).await.is_err());
}

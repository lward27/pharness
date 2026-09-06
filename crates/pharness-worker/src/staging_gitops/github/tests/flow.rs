use super::*;
use crate::staging_gitops::{StagingContext, Transport};

fn current_authority() -> HostedStagingAuthority {
    let mut a = authority();
    a.created_at_ms = crate::staging_gitops::now() - 1000;
    a.expires_at_ms = a.created_at_ms + 600_000;
    a
}
fn context(a: &HostedStagingAuthority, p: &StagingGitOpsPlan, admitted: bool) -> Value {
    json!({"authority":a,"authority_hash":a.material_hash().unwrap(),"plan":p,"plan_hash":p.material_hash().unwrap(),"may_advance":!admitted,"admission_recorded":admitted})
}
fn admission(a: &HostedStagingAuthority, p: &StagingGitOpsPlan) -> Value {
    json!({"admitted":true,"authority_hash":a.material_hash().unwrap(),"plan_hash":p.material_hash().unwrap(),"baseline":{
        "baseline_artifact_id":format!("staging_baseline_result_{}",a.execution_id),
        "identity_artifact_id":format!("staging_baseline_admission_identity_{}",a.execution_id),
        "baseline_sha256":format!("sha256:{}","1".repeat(64)),"identity_sha256":format!("sha256:{}","2".repeat(64)),
        "admission_expires_at_ms":crate::staging_gitops::now()+30_000}})
}
fn transport(m: &Mock, a: &HostedStagingAuthority, observer: bool) -> Transport {
    Transport {
        client: m.git.client.clone(),
        url: format!("{}/staging", m.git.rest),
        token: "fixture-only-worker-token".into(),
        execution: a.execution_id.clone(),
        deployment: a.deployment_intent_id.clone(),
        observe_only: observer,
    }
}

#[tokio::test]
async fn unknown_or_denied_admission_never_reaches_github_mutation() {
    for response in [
        (500, json!({"error":"unknown acknowledgement"})),
        (200, json!({"admitted":false})),
        (
            200,
            json!({"admitted":true,"authority_hash":"different","plan_hash":"different"}),
        ),
    ] {
        let a = current_authority();
        let p = plan(&a);
        let body = context(&a, &p, false);
        let initial: StagingContext = serde_json::from_value(body.clone()).unwrap();
        let m = mock(vec![
            (200, body),
            (200, branch(&p.base_commit_sha)),
            response,
        ])
        .await;
        assert!(transport(&m, &a, false)
            .run(&m.git, &initial)
            .await
            .is_err());
        let requests = m.requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(
            requests
                .iter()
                .filter(|(path, _)| path.starts_with("POST "))
                .count(),
            1
        );
        assert!(requests[2].0.contains("/staging/attempt "));
        assert!(!requests.iter().any(|(path, _)| path.contains("/graphql")));
    }
}

#[tokio::test]
async fn lost_commit_acknowledgement_recovers_by_reading_the_admitted_history() {
    let a = current_authority();
    let p = plan(&a);
    let body = context(&a, &p, false);
    let initial: StagingContext = serde_json::from_value(body.clone()).unwrap();
    let admitted = admission(&a, &p);
    let m = mock(vec![
        (200, body),
        (200, branch(&p.base_commit_sha)),
        (200, admitted),
        (500, json!({"server_error":"response lost after effect"})),
        (200, branch(&"c".repeat(40))),
        (200, json!([commit(&a, &p)])),
        (200, commit(&a, &p)),
        (200, file(&a, &p.updated_content)),
    ])
    .await;
    assert_eq!(
        transport(&m, &a, false)
            .run(&m.git, &initial)
            .await
            .unwrap()["status"],
        "committed"
    );
    let requests = m.requests.lock().unwrap();
    assert_eq!(requests.len(), 8);
    assert_eq!(
        requests
            .iter()
            .filter(|(path, _)| path.starts_with("POST /graphql "))
            .count(),
        1
    );
    assert!(requests
        .iter()
        .skip(4)
        .all(|(path, _)| path.starts_with("GET ")));
}

#[tokio::test]
async fn expired_or_missing_baseline_admission_cannot_send_the_github_post() {
    for expired in [false, true] {
        let a = current_authority();
        let p = plan(&a);
        let body = context(&a, &p, false);
        let initial: StagingContext = serde_json::from_value(body.clone()).unwrap();
        let mut admitted = admission(&a, &p);
        if expired {
            admitted["baseline"]["admission_expires_at_ms"] =
                json!(crate::staging_gitops::now() - 1);
        } else {
            admitted.as_object_mut().unwrap().remove("baseline");
        }
        let m = mock(vec![
            (200, body),
            (200, branch(&p.base_commit_sha)),
            (200, admitted),
        ])
        .await;
        assert!(transport(&m, &a, false)
            .run(&m.git, &initial)
            .await
            .unwrap_err()
            .to_string()
            .contains("baseline_admission"));
        let requests = m.requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(!requests.iter().any(|(path, _)| path.contains("/graphql")));
    }
}

#[tokio::test]
async fn recorded_admission_and_observer_flag_allow_only_read_only_recovery() {
    for observer in [false, true] {
        let a = current_authority();
        let p = plan(&a);
        let initial: StagingContext = serde_json::from_value(context(&a, &p, true)).unwrap();
        let mut protected = branch(&"c".repeat(40));
        protected["protected"] = json!(true);
        let m = mock(vec![
            (200, protected),
            (200, json!([commit(&a, &p)])),
            (200, commit(&a, &p)),
            (200, file(&a, &p.updated_content)),
        ])
        .await;
        assert_eq!(
            transport(&m, &a, observer)
                .run(&m.git, &initial)
                .await
                .unwrap()["status"],
            "committed"
        );
        assert!(m
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|(path, _)| path.starts_with("GET ")));
    }
}

#[tokio::test]
async fn stale_gitops_base_and_unacknowledged_plan_stop_before_admission() {
    let a = current_authority();
    let p = plan(&a);
    let body = context(&a, &p, false);
    let initial: StagingContext = serde_json::from_value(body.clone()).unwrap();
    let m = mock(vec![(200, body), (200, branch(&"9".repeat(40)))]).await;
    assert!(transport(&m, &a, false)
        .run(&m.git, &initial)
        .await
        .is_err());
    assert!(m
        .requests
        .lock()
        .unwrap()
        .iter()
        .all(|(path, _)| path.starts_with("GET ")));
    let initial = StagingContext {
        authority: a.clone(),
        authority_hash: a.material_hash().unwrap(),
        plan: None,
        plan_hash: None,
        may_advance: true,
        admission_recorded: false,
    };
    let m = mock(vec![
        (200, branch(&p.base_commit_sha)),
        (200, file(&a, &p.original_content)),
        (500, json!({"unknown":true})),
    ])
    .await;
    assert!(transport(&m, &a, false)
        .run(&m.git, &initial)
        .await
        .is_err());
    let requests = m.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests[2].0.contains("/staging/plan "));
    assert!(!requests
        .iter()
        .any(|(path, _)| path.contains("/staging/attempt") || path.contains("/graphql")));
}

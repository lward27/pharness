use super::{identity::verify, kubernetes::Resources, *};
use serde_json::{json, Value};

fn expectation() -> FinanceDeploymentExpectation {
    FinanceDeploymentExpectation {
        application: FinanceApplication::Yfinance,
        environment: FinanceEnvironment::Staging,
        gitops_commit_sha: "a".repeat(40),
        image_digest: format!("sha256:{}", "b".repeat(64)),
    }
}

fn metadata(name: &str, uid: &str) -> Value {
    json!({"name":name,"namespace":"apps-staging","uid":uid,"resourceVersion":"120"})
}

fn own(v: &mut Value, kind: &str, name: &str, uid: &str) {
    v["metadata"]["ownerReferences"] =
        json!([{"kind":kind,"name":name,"uid":uid,"controller":true}]);
}

fn resources(e: &FinanceDeploymentExpectation) -> Resources {
    let name = e.workload();
    let ns = e.namespace();
    let mut deployment = json!({"kind":"Deployment","metadata":metadata(name,"deployment-uid"),"spec":{"replicas":1,"selector":{"matchLabels":{"app":name}},"template":{"metadata":{"labels":{"app":name}},"spec":{"containers":[{"name":name,"image":e.image_ref(),"env":[{"name":"TOKEN","value":"credential-canary-must-never-appear"}]}]}}},"status":{"observedGeneration":3,"replicas":1,"readyReplicas":1,"availableReplicas":1,"updatedReplicas":1}});
    deployment["metadata"]["namespace"] = json!(ns);
    deployment["metadata"]["generation"] = json!(3);
    deployment["metadata"]["annotations"] = json!({"deployment.kubernetes.io/revision":"2"});
    let mut rs = json!({"kind":"ReplicaSet","metadata":metadata(&format!("{name}-abc"),"rs-uid"),"spec":{"template":deployment["spec"]["template"]}});
    rs["metadata"]["namespace"] = json!(ns);
    rs["metadata"]["annotations"] = deployment["metadata"]["annotations"].clone();
    rs["spec"]["template"]["metadata"]["labels"]["pod-template-hash"] = json!("abc");
    own(&mut rs, "Deployment", name, "deployment-uid");
    let pod_name = format!("{name}-abc-one");
    let mut pod = json!({"kind":"Pod","metadata":metadata(&pod_name,"pod-uid"),"spec":{"containers":[{"name":name,"image":e.image_ref()}]},"status":{"phase":"Running","podIP":"10.42.0.10","conditions":[{"type":"Ready","status":"True"}],"containerStatuses":[{"name":name,"imageID":e.image_ref(),"ready":true,"restartCount":0,"state":{"running":{"startedAt":"2026-09-05T10:00:00Z"}}}]}});
    pod["metadata"]["namespace"] = json!(ns);
    own(&mut pod, "ReplicaSet", &format!("{name}-abc"), "rs-uid");
    let mut service = json!({"kind":"Service","metadata":metadata(name,"service-uid"),"spec":{"type":"ClusterIP","selector":{"app":name},"ports":[{"port":e.service_port(),"targetPort":e.container_port(),"protocol":"TCP"}]}});
    service["metadata"]["namespace"] = json!(ns);
    let mut slice = json!({"kind":"EndpointSlice","metadata":metadata(&format!("{name}-slice"),"slice-uid"),"ports":[{"port":e.container_port(),"protocol":"TCP"}],"endpoints":[{"addresses":["10.42.0.10"],"conditions":{"ready":true,"terminating":false},"targetRef":{"kind":"Pod","name":pod_name,"namespace":ns,"uid":"pod-uid"}}]});
    slice["metadata"]["namespace"] = json!(ns);
    slice["metadata"]["labels"] = json!({"kubernetes.io/service-name":name});
    own(&mut slice, "Service", name, "service-uid");
    let source = json!({"repoURL":crate::hosted_sdlc::staging::GITOPS_REPOSITORY,"path":e.gitops_path(),"targetRevision":"main"});
    let destination = json!({"namespace":ns,"server":"https://kubernetes.default.svc"});
    let argo = json!({"kind":"Application","metadata":{"name":e.argo_application(),"namespace":"argocd","uid":"argo-uid","resourceVersion":"125"},"spec":{"source":source,"destination":destination},"status":{"sync":{"status":"Synced","revision":e.gitops_commit_sha,"comparedTo":{"source":source,"destination":destination}},"health":{"status":"Healthy"},"resources":[{"kind":"Deployment","namespace":ns,"name":name,"status":"Synced"}]}});
    Resources {
        argo,
        deployment,
        replicasets: json!({"items":[rs]}),
        pods: json!({"items":[pod]}),
        service,
        endpoints: json!({"items":[slice]}),
        receipts: vec![],
    }
}

#[test]
fn all_finite_targets_require_full_source_and_image_identity() {
    for application in [FinanceApplication::Yfinance, FinanceApplication::Frontend] {
        for environment in [FinanceEnvironment::Staging, FinanceEnvironment::Production] {
            let e = FinanceDeploymentExpectation {
                application,
                environment,
                ..expectation()
            };
            let identity = verify(&e, &resources(&e)).unwrap();
            assert_eq!(identity["runtime_verification"], "not_evaluated");
            assert_eq!(identity["observation_is_atomic"], false);
            assert!(!identity.to_string().contains("credential-canary"));
            let mut invalid = e.clone();
            invalid.gitops_commit_sha = "main".into();
            assert!(invalid.validate().is_err());
            invalid = e.clone();
            invalid.image_digest = "latest".into();
            assert!(invalid.validate().is_err());
        }
    }
    assert!(serde_json::from_value::<FinanceDeploymentExpectation>(json!({"application":"arbitrary","environment":"staging","gitops_commit_sha":"a".repeat(40),"image_digest":expectation().image_digest})).is_err());
}

#[test]
fn healthy_argo_and_pods_cannot_mask_wrong_gitops_or_target() {
    let e = expectation();
    for (pointer, value) in [
        ("/status/sync/revision", json!("c".repeat(40))),
        ("/status/sync/status", json!("OutOfSync")),
        ("/status/health/status", json!("Progressing")),
        ("/spec/source/path", json!("charts/finance-frontend")),
        ("/spec/destination/namespace", json!("apps-prod")),
        (
            "/status/sync/comparedTo/source/targetRevision",
            json!("old-branch"),
        ),
        ("/status/resources/0/name", json!("another-app")),
    ] {
        let mut r = resources(&e);
        *r.argo.pointer_mut(pointer).unwrap() = value;
        assert!(verify(&e, &r).is_err(), "{pointer}");
    }
}

#[test]
fn rollout_must_match_generation_image_and_template() {
    let e = expectation();
    for (pointer, value) in [
        ("/status/observedGeneration", json!(2)),
        ("/status/updatedReplicas", json!(0)),
        (
            "/spec/template/spec/containers/0/image",
            json!("registry.lucas.engineering/yfinance_wrapper:latest"),
        ),
        ("/spec/selector/matchLabels/app", json!("other")),
    ] {
        let mut r = resources(&e);
        *r.deployment.pointer_mut(pointer).unwrap() = value;
        assert!(verify(&e, &r).is_err(), "{pointer}");
    }
    let mut r = resources(&e);
    r.replicasets["items"][0]["metadata"]["ownerReferences"][0]["uid"] = json!("old-deployment");
    assert_eq!(verify(&e, &r).unwrap_err(), "current_replicaset_unproven");
    let mut r = resources(&e);
    r.replicasets["items"][0]["spec"]["template"]["spec"]["containers"][0]["env"][0]["value"] =
        json!("changed-config");
    assert_eq!(verify(&e, &r).unwrap_err(), "replicaset_template_changed");
}

#[test]
fn ready_pod_must_have_exact_runtime_digest_and_current_owner() {
    let e = expectation();
    for (pointer, value) in [
        (
            "/items/0/status/containerStatuses/0/imageID",
            json!(format!(
                "registry.lucas.engineering/yfinance_wrapper@sha256:{}",
                "c".repeat(64)
            )),
        ),
        ("/items/0/metadata/ownerReferences/0/uid", json!("old-rs")),
        ("/items/0/status/containerStatuses/0/ready", json!(false)),
        ("/items/0/status/conditions/0/status", json!("False")),
        ("/items/0/status/phase", json!("Succeeded")),
    ] {
        let mut r = resources(&e);
        *r.pods.pointer_mut(pointer).unwrap() = value;
        assert!(verify(&e, &r).is_err(), "{pointer}");
    }
    let mut r = resources(&e);
    r.pods["items"][0]["status"]["containerStatuses"][0]["imageID"] =
        json!(format!("docker-pullable://{}", e.image_ref()));
    assert!(verify(&e, &r).is_ok());
}

#[test]
fn service_must_route_only_to_verified_ready_pods() {
    let e = expectation();
    for (pointer, value) in [
        (
            "/items/0/metadata/ownerReferences/0/uid",
            json!("other-service"),
        ),
        ("/items/0/ports/0/port", json!(9000)),
        ("/items/0/endpoints/0/targetRef/uid", json!("other-pod")),
        (
            "/items/0/endpoints/0/targetRef/namespace",
            json!("apps-prod"),
        ),
        ("/items/0/endpoints/0/addresses", json!(["10.42.0.11"])),
        ("/items/0/endpoints/0/conditions/ready", Value::Null),
        ("/items/0/endpoints/0/conditions/terminating", json!(true)),
    ] {
        let mut r = resources(&e);
        *r.endpoints.pointer_mut(pointer).unwrap() = value;
        assert!(verify(&e, &r).is_err(), "{pointer}");
    }
    let mut r = resources(&e);
    r.service["spec"]["ports"][0]["targetPort"] = json!(9000);
    assert!(verify(&e, &r).is_err());
    let mut r = resources(&e);
    r.endpoints["items"][0]["endpoints"] = json!([]);
    assert_eq!(verify(&e, &r).unwrap_err(), "ready_endpoints_incomplete");
}

#[test]
fn retained_unready_probe_does_not_count_as_a_rollout_pod_or_endpoint() {
    let e = expectation();
    let mut r = resources(&e);
    let mut probe = r.pods["items"][0].clone();
    probe["metadata"]["uid"] = json!("probe-uid");
    probe["metadata"]["name"] = json!("old-probe");
    probe["status"]["phase"] = json!("Succeeded");
    own(&mut probe, "Job", "old-probe", "job-uid");
    r.pods["items"].as_array_mut().unwrap().push(probe);
    let mut endpoint = r.endpoints["items"][0]["endpoints"][0].clone();
    endpoint["targetRef"]["uid"] = json!("probe-uid");
    endpoint["targetRef"]["name"] = json!("old-probe");
    endpoint["conditions"]["ready"] = json!(false);
    r.endpoints["items"][0]["endpoints"]
        .as_array_mut()
        .unwrap()
        .push(endpoint);
    let result = verify(&e, &r).unwrap();
    assert_eq!(result["pods"].as_array().unwrap().len(), 1);
    assert_eq!(result["not_ready_endpoints_excluded"], 1);
    r.endpoints["items"][0]["endpoints"][1]["conditions"]["ready"] = json!(true);
    assert_eq!(verify(&e, &r).unwrap_err(), "endpoint_targets_other_pod");
}

#[test]
fn missing_truncated_duplicate_and_malformed_resources_are_inconclusive() {
    let e = expectation();
    let mut r = resources(&e);
    r.pods["metadata"] = json!({"continue":"next-page"});
    assert_eq!(verify(&e, &r).unwrap_err(), "incomplete_kubernetes_list");
    let mut r = resources(&e);
    r.pods["items"] = json!(vec![r.pods["items"][0].clone(); 33]);
    assert_eq!(
        verify(&e, &r).unwrap_err(),
        "missing_or_oversized_kubernetes_list"
    );
    let mut r = resources(&e);
    let pod = r.pods["items"][0].clone();
    r.pods["items"].as_array_mut().unwrap().push(pod);
    assert_eq!(verify(&e, &r).unwrap_err(), "duplicate_pod_identity");
    let mut r = resources(&e);
    r.service["metadata"]["resourceVersion"] = Value::Null;
    assert!(verify(&e, &r).is_err());
}

#[tokio::test]
async fn unavailable_kubernetes_produces_fixed_inconclusive_diagnostics() {
    let result = ReadOnlyClusterTools::default()
        .with_kubectl_bin("/nonexistent/credential-canary")
        .observe_finance_deployment(&expectation())
        .await
        .unwrap();
    assert_eq!(result.content["identity_state"], "inconclusive");
    assert_eq!(result.content["runtime_verification"], "not_evaluated");
    assert!(!result.content.to_string().contains("credential-canary"));
}

#[cfg(unix)]
struct FakeKubectl(std::path::PathBuf);

#[cfg(unix)]
impl FakeKubectl {
    fn new(script: &str) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "pharness-finance-reader-{}-{nonce}",
            std::process::id()
        ));
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
    fn tools(&self) -> ReadOnlyClusterTools {
        ReadOnlyClusterTools::default().with_kubectl_bin(self.0.to_string_lossy().as_ref())
    }
}

#[cfg(unix)]
impl Drop for FakeKubectl {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(unix)]
#[tokio::test]
async fn native_reads_are_bounded_and_do_not_export_process_errors() {
    for (script, limit, timeout, reason) in [
        (
            "printf credential-canary >&2; exit 1",
            512 * 1024,
            1500,
            "kubernetes_read_failed",
        ),
        (
            "printf invalid-credential-canary",
            512 * 1024,
            1500,
            "malformed_kubernetes_response",
        ),
        (
            "while :; do printf 0123456789abcdef; done",
            32,
            1500,
            "kubernetes_response_too_large",
        ),
        ("exec sleep 5", 512 * 1024, 25, "kubernetes_read_timed_out"),
    ] {
        let fake = FakeKubectl::new(script);
        let mut tools = fake.tools().with_timeout_ms(timeout);
        tools.max_output_bytes = limit;
        let output = tools
            .observe_finance_deployment(&expectation())
            .await
            .unwrap();
        assert_eq!(output.content["identity_state"], "inconclusive");
        assert_eq!(output.content["reasons"], json!([reason]));
        assert!(!output.content.to_string().contains("credential-canary"));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn native_transport_keeps_only_identity_and_six_content_hashes() {
    let e = expectation();
    let r = resources(&e);
    let mut script = String::from("test \"$1\" = get || exit 4\ncase \"$2\" in\n");
    for (kind, value) in [
        ("applications.argoproj.io", r.argo),
        ("deployments.apps", r.deployment),
        ("replicasets.apps", r.replicasets),
        ("pods", r.pods),
        ("services", r.service),
        ("endpointslices.discovery.k8s.io", r.endpoints),
    ] {
        let quoted = value.to_string().replace('\'', "'\\''");
        script.push_str(&format!("{kind}) printf '%s' '{quoted}' ;;\n"));
    }
    script.push_str("*) exit 4 ;;\nesac");
    let fake = FakeKubectl::new(&script);
    let output = fake.tools().observe_finance_deployment(&e).await.unwrap();
    assert_eq!(output.content["identity_state"], "verified");
    assert_eq!(output.content["read_receipts"].as_array().unwrap().len(), 6);
    assert!(output.content["read_receipts"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["bytes"].as_u64().unwrap() > 0 && r["sha256"].as_str().unwrap().len() == 71));
    assert!(!output.content.to_string().contains("credential-canary"));
}

#[tokio::test]
#[ignore = "explicitly authorized read-only Finance deployment access and exact expected identities required"]
async fn live_finance_deployment_identity() {
    let expected: FinanceDeploymentExpectation = serde_json::from_str(
        &std::env::var("PHARNESS_FINANCE_LIVE_EXPECTATION")
            .expect("explicit expected identity JSON required"),
    )
    .unwrap();
    let binary = std::env::var("PHARNESS_FINANCE_LIVE_KUBECTL")
        .expect("explicit cluster-scoped kubectl wrapper required");
    let result = ReadOnlyClusterTools::default()
        .with_kubectl_bin(binary)
        .observe_finance_deployment(&expected)
        .await
        .unwrap();
    println!("{}", serde_json::to_string(&result).unwrap());
    assert_eq!(result.content["identity_state"], "verified");
    assert_eq!(result.content["runtime_verification"], "not_evaluated");
}

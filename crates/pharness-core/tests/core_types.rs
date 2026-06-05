use camino::Utf8PathBuf;
use pharness_core::{
    ArtifactRef, CapabilityKind, EnvironmentRef, EnvironmentTier, ExecutionTarget, ResourceRef,
    RunScope, ToolCapability, WorkspaceMount,
};

#[test]
fn execution_target_round_trips_local_process() {
    let target = ExecutionTarget::LocalProcess {
        cwd: Utf8PathBuf::from("/workspace/app"),
        shell: "zsh".to_string(),
    };

    let json = serde_json::to_string(&target).unwrap();
    assert!(json.contains("local_process"));

    let restored: ExecutionTarget = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, target);
}

#[test]
fn execution_target_round_trips_kubernetes_job() {
    let target = ExecutionTarget::KubernetesJob {
        cluster: "homelab".to_string(),
        namespace: "pharness-runs".to_string(),
        service_account: "pharness-worker".to_string(),
        workspace: WorkspaceMount::EmptyDir {
            mount_path: Utf8PathBuf::from("/workspace"),
        },
        network_profile: "restricted".to_string(),
        resource_profile: "medium".to_string(),
    };

    let json = serde_json::to_string(&target).unwrap();
    assert!(json.contains("kubernetes_job"));

    let restored: ExecutionTarget = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, target);
}

#[test]
fn resource_and_artifact_refs_round_trip() {
    let resource = ResourceRef::new("argocd", "application", "checkout")
        .with_namespace("argocd")
        .with_uri("argocd://applications/checkout")
        .with_metadata(serde_json::json!({ "environment": "production" }));

    let artifact = ArtifactRef {
        artifact_id: "art_test".into(),
        kind: "deployment_verification".to_string(),
        label: "Checkout production rollout".to_string(),
        uri: Some("pharness://artifacts/art_test".to_string()),
        resource_ref: Some(resource),
    };

    let restored: ArtifactRef =
        serde_json::from_str(&serde_json::to_string(&artifact).unwrap()).unwrap();
    assert_eq!(restored, artifact);
}

#[test]
fn environment_ref_marks_production_context() {
    let environment = EnvironmentRef {
        id: "prod".to_string(),
        name: "Production".to_string(),
        tier: EnvironmentTier::Production,
        cluster: Some("homelab".to_string()),
        namespace: Some("checkout".to_string()),
    };

    let restored: EnvironmentRef =
        serde_json::from_str(&serde_json::to_string(&environment).unwrap()).unwrap();
    assert_eq!(restored.tier, EnvironmentTier::Production);
}

#[test]
fn run_scope_carries_sdlc_metadata() {
    let scope = RunScope {
        namespace: Some("apps-dev".to_string()),
        repo: Some("git@example.test/team/app.git".to_string()),
        branch: Some("feature/pharness".to_string()),
        work_plan_id: Some("wplan_1".to_string()),
        change_set_id: Some("cset_1".to_string()),
        production_impacting: false,
    };

    let restored: RunScope = serde_json::from_str(&serde_json::to_string(&scope).unwrap()).unwrap();

    assert_eq!(restored, scope);
    assert!(!restored.is_empty());
    assert!(RunScope::default().is_empty());
    assert_eq!(RunScope::default().to_optional_json(), None);
    assert_eq!(
        scope.to_optional_json().unwrap()["namespace"],
        serde_json::json!("apps-dev")
    );
    assert_eq!(
        scope.to_optional_json().unwrap()["change_set_id"],
        serde_json::json!("cset_1")
    );
}

#[test]
fn future_cluster_capabilities_are_typed() {
    let capabilities = [
        ToolCapability::read_only(CapabilityKind::KubernetesRead),
        ToolCapability::mutating(CapabilityKind::RegistryWrite, true),
        ToolCapability::mutating(CapabilityKind::DatabaseMigration, true),
        ToolCapability::read_only(CapabilityKind::ObservabilityRead),
        ToolCapability::read_only(CapabilityKind::RagRead),
    ];

    assert_eq!(capabilities.len(), 5);
    assert!(capabilities
        .iter()
        .any(|capability| capability.production_risk));
}

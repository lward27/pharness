//! Lab slot ids in the catalog, the Kubernetes provider configuration file
//! and how startup builds the provider from them.

use super::super::lab_catalog::{
    load_campus_labs, CampusLabCatalog, CampusLabProviderKind, CampusLabsConfig,
    KubernetesLabProviderFile, CAMPUS_LAB_CATALOG_SCHEMA, ENV_LABS_FILE, ENV_LAB_PROVIDER,
    ENV_LAB_PROVIDER_CONFIG_FILE,
};
use super::super::lab_provider::build_campus_labs;
use super::super::state::{CampusRuntime, SystemClock};
use super::{test_config, MockIdp, TENANT};
use pharness_store::SqliteStore;
use pharness_workspaces::campus_lab_session::kubernetes::KubernetesCampusProviderConfig;
use pharness_workspaces::campus_terminal::KubectlCampusTerminalConfig;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;

const RESEARCH: &str =
    include_str!("../../../../../pharness-workspaces/src/campus_profile/fixtures/research.json");
const CODING: &str =
    include_str!("../../../../../pharness-workspaces/src/campus_profile/fixtures/coding.json");

fn layer(authority: &str, constraints: Value) -> Value {
    json!({
        "schema_version": "pharness.dev/campus-policy-layer/v1alpha1",
        "authority": authority,
        "id": format!("{authority}-policy"),
        "version": "v1",
        "constraints": constraints,
        "mandatory_denies": {
            "tools": [], "egress_hosts": [], "model_aliases": [], "approval_kinds": []
        },
    })
}

fn m1_slot_ids() -> Vec<String> {
    (1..=5)
        .map(|index| format!("campus-lab-slot-{index}"))
        .collect()
}

/// Research and coding assignments; the institution layer sizes coding
/// down to fit an M1 lab slot.
fn catalog_value() -> Value {
    json!({
        "schema_version": CAMPUS_LAB_CATALOG_SCHEMA,
        "lab_slot_ids": m1_slot_ids(),
        "profiles": [
            serde_json::from_str::<Value>(RESEARCH).unwrap(),
            serde_json::from_str::<Value>(CODING).unwrap(),
        ],
        "policy_layers": [
            layer("platform", json!({})),
            layer("institution", json!({
                "cpu_millicores": 1500, "memory_mib": 1536, "ephemeral_storage_mib": 1024
            })),
        ],
        "assignments": [
            {
                "course_id": "cs101", "assignment_id": "research",
                "title": "Research", "profile_id": "campus-research",
            },
            {
                "course_id": "cs101", "assignment_id": "coding",
                "title": "Coding", "profile_id": "campus-coding",
            },
        ],
    })
}

fn parse(value: &Value) -> anyhow::Result<CampusLabCatalog> {
    CampusLabCatalog::parse(TENANT, &value.to_string())
}

fn provider_file() -> Value {
    json!({
        "kubectl": "kubectl",
        "global_args": ["--kubeconfig=/etc/pharness/campus-controller/kubeconfig"],
        "provider": KubernetesCampusProviderConfig::m1_lab(),
    })
}

fn parse_file(
    file: &Value,
    catalog: &CampusLabCatalog,
) -> anyhow::Result<KubernetesLabProviderFile> {
    KubernetesLabProviderFile::parse(&file.to_string(), catalog)
}

#[test]
fn catalog_slot_ids_default_to_the_tenant_and_may_be_listed() {
    let mut defaulted = catalog_value();
    defaulted.as_object_mut().unwrap().remove("lab_slot_ids");
    defaulted["lab_slots"] = json!(2);
    assert_eq!(
        parse(&defaulted).unwrap().slot_ids,
        ["tenant-a-lab-slot-01", "tenant-a-lab-slot-02"]
    );

    let listed = parse(&catalog_value()).unwrap();
    assert_eq!(listed.slot_ids, m1_slot_ids());
    assert_eq!(listed.limits.lab_slots, 5);
    assert_eq!(listed.profiles.len(), 2);

    let mut consistent = catalog_value();
    consistent["lab_slots"] = json!(5);
    assert!(parse(&consistent).is_ok());
    for ids in [
        json!(["campus-lab-slot-1", "campus-lab-slot-1"]),
        json!([]),
        json!(["Campus Lab"]),
        json!(["-campus"]),
    ] {
        let mut invalid = catalog_value();
        invalid["lab_slot_ids"] = ids.clone();
        assert!(parse(&invalid).is_err(), "{ids} must be refused");
    }
    let mut miscounted = catalog_value();
    miscounted["lab_slots"] = json!(4);
    assert!(parse(&miscounted).is_err());
}

#[test]
fn provider_file_must_match_the_catalog_slots_and_fit_every_assignment() {
    let catalog = parse(&catalog_value()).unwrap();
    let file = parse_file(&provider_file(), &catalog).unwrap();
    assert_eq!(file.provider.slots.len(), 5);

    let mut fewer = catalog_value();
    fewer["lab_slot_ids"] = json!(["campus-lab-slot-1"]);
    assert!(parse_file(&provider_file(), &parse(&fewer).unwrap()).is_err());

    let mut oversized = catalog_value();
    oversized["policy_layers"][1] = layer("institution", json!({}));
    let error = parse_file(&provider_file(), &parse(&oversized).unwrap()).unwrap_err();
    assert!(format!("{error:#}").contains("cs101/coding does not fit a lab slot"));

    let mut no_kubectl = provider_file();
    no_kubectl["kubectl"] = json!(" ");
    assert!(parse_file(&no_kubectl, &catalog).is_err());

    let mut invalid = provider_file();
    invalid["provider"]["required_network_policies"] = json!([]);
    assert!(parse_file(&invalid, &catalog).is_err());

    let mut unknown = provider_file();
    unknown["kubeconfig"] = json!("/somewhere");
    assert!(parse_file(&unknown, &catalog).is_err());
}

#[test]
fn kubernetes_provider_is_selected_with_its_configuration_file() {
    let dir = std::env::temp_dir().join(format!(
        "pharness-campus-provider-{}",
        uuid::Uuid::now_v7().simple()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let catalog_path = dir.join("labs.json");
    let provider_path = dir.join("provider.json");
    std::fs::write(&catalog_path, catalog_value().to_string()).unwrap();
    std::fs::write(&provider_path, provider_file().to_string()).unwrap();
    let catalog = catalog_path.display().to_string();
    let provider = provider_path.display().to_string();
    let env = |pairs: &[(&str, &str)]| {
        let pairs = pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect::<BTreeMap<_, _>>();
        move |name: &str| pairs.get(name).cloned()
    };
    let config = test_config();

    let loaded = load_campus_labs(
        &config,
        env(&[
            (ENV_LABS_FILE, &catalog),
            (ENV_LAB_PROVIDER, "kubernetes"),
            (ENV_LAB_PROVIDER_CONFIG_FILE, &provider),
        ]),
    )
    .unwrap()
    .unwrap();
    assert_eq!(loaded.provider, CampusLabProviderKind::Kubernetes);
    assert!(loaded.kubernetes.is_some());

    assert!(load_campus_labs(
        &config,
        env(&[(ENV_LABS_FILE, &catalog), (ENV_LAB_PROVIDER, "kubernetes")])
    )
    .is_err());
    assert!(load_campus_labs(
        &config,
        env(&[
            (ENV_LABS_FILE, &catalog),
            (ENV_LAB_PROVIDER_CONFIG_FILE, &provider)
        ])
    )
    .is_err());
    assert!(load_campus_labs(
        &config,
        env(&[
            (ENV_LAB_PROVIDER, "kubernetes"),
            (ENV_LAB_PROVIDER_CONFIG_FILE, &provider)
        ])
    )
    .is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn startup_builds_the_kubernetes_provider_on_the_catalog_slots() {
    let store = SqliteStore::connect_in_memory().await.unwrap();
    let runtime = CampusRuntime::new(
        test_config(),
        MockIdp::with_keys(&[]),
        Arc::new(SystemClock),
    );
    let catalog = parse(&catalog_value()).unwrap();
    let kubernetes = parse_file(&provider_file(), &catalog).unwrap();
    let labs = build_campus_labs(
        &runtime,
        &store,
        CampusLabsConfig {
            catalog,
            provider: CampusLabProviderKind::Kubernetes,
            kubernetes: Some(kubernetes),
        },
    )
    .unwrap();
    assert!(labs.can_launch());
    assert_eq!(labs.slot_ids(), m1_slot_ids());
    labs.register_slots(&store, 1_900_000_000).await.unwrap();

    let none = build_campus_labs(
        &runtime,
        &store,
        CampusLabsConfig {
            catalog: parse(&catalog_value()).unwrap(),
            provider: CampusLabProviderKind::None,
            kubernetes: None,
        },
    )
    .unwrap();
    assert!(!none.can_launch());
}

const CHART_CATALOG: &str =
    include_str!("../../../../../../deploy/helm/pharness-campus/files/campus-labs.json");
const CHART_PROVIDER: &str =
    include_str!("../../../../../../deploy/helm/pharness-campus/files/lab-provider.json");
const CHART_TERMINAL: &str =
    include_str!("../../../../../../deploy/helm/pharness-campus/files/terminal.json");

/// The configuration `deploy/helm/pharness-campus` ships: a startup error
/// here would otherwise only show up as a crash-looping Pod.
#[tokio::test]
async fn the_campus_chart_configuration_starts_the_kubernetes_provider() {
    let catalog = CampusLabCatalog::parse("campus-poc", CHART_CATALOG).unwrap();
    assert!(catalog.assignment("cs101", "research-lab").is_some());
    assert!(catalog.assignment("cs101", "coding-lab").is_some());
    // The M1 demonstration (planning/operations/campus-m1-demo.md) waits
    // for this assignment's absolute expiry.
    let timeout_demo = catalog
        .assignment("cs101", "research-timeout-demo")
        .unwrap();
    assert_eq!(timeout_demo.session_seconds, 300);
    let file = KubernetesLabProviderFile::parse(CHART_PROVIDER, &catalog).unwrap();
    assert!(file.provider.egress.is_some());

    let terminal: KubectlCampusTerminalConfig = serde_json::from_str(CHART_TERMINAL).unwrap();
    terminal.validate().unwrap();
    assert_eq!(terminal.slots, file.provider.slots);
    assert_eq!(terminal.controller, file.provider.controller);
    assert_ne!(terminal.global_args, file.global_args);

    let profiles = |document: &str| serde_json::from_str::<Value>(document).unwrap();
    let shipped = serde_json::from_str::<Value>(CHART_CATALOG).unwrap();
    assert_eq!(
        shipped["profiles"],
        json!([profiles(RESEARCH), profiles(CODING)])
    );

    let store = SqliteStore::connect_in_memory().await.unwrap();
    let runtime = CampusRuntime::new(
        test_config(),
        MockIdp::with_keys(&[]),
        Arc::new(SystemClock),
    );
    let labs = build_campus_labs(
        &runtime,
        &store,
        CampusLabsConfig {
            catalog,
            provider: CampusLabProviderKind::Kubernetes,
            kubernetes: Some(file),
        },
    )
    .unwrap();
    assert!(labs.can_launch());
}

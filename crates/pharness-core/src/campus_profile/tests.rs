use super::*;
use serde_json::{json, Value};

const RESEARCH: &str = include_str!("fixtures/research.json");
const CODING: &str = include_str!("fixtures/coding.json");
const SECURITY: &str = include_str!("fixtures/security.json");

fn fixture(document: &str) -> Value {
    serde_json::from_str(document).expect("fixture is JSON")
}

fn parse(value: &Value) -> Result<CampusProfile, CampusProfileError> {
    CampusProfile::parse(&value.to_string())
}

fn rejected_field(value: &Value) -> String {
    match parse(value) {
        Err(CampusProfileError::Invalid { field, .. }) => field,
        other => panic!("expected an invalid-field rejection, got {other:?}"),
    }
}

fn assert_malformed(value: &Value) {
    assert!(
        matches!(parse(value), Err(CampusProfileError::Malformed(_))),
        "expected a schema rejection for {value}"
    );
}

#[test]
fn reference_fixtures_validate_with_enforceable_bounds() {
    for (document, kind) in [
        (RESEARCH, CampusProfileKind::Research),
        (CODING, CampusProfileKind::Coding),
        (SECURITY, CampusProfileKind::Security),
    ] {
        let profile = CampusProfile::parse(document).expect("fixture validates");
        assert_eq!(profile.kind, kind);
        assert!(profile.resources.cpu_millicores <= MAX_CPU_MILLICORES);
        assert!(profile.agent_budget.active_execution_seconds <= u64::from(profile.ttl_seconds));
    }
}

#[test]
fn research_and_coding_launch_but_security_needs_its_qualification() {
    let none = BTreeSet::new();
    for document in [RESEARCH, CODING] {
        let profile = CampusProfile::parse(document).unwrap();
        assert_eq!(profile.check_launchable(&none), Ok(()));
    }
    let security = CampusProfile::parse(SECURITY).unwrap();
    assert_eq!(
        security.check_launchable(&none),
        Err(CampusProfileError::LaunchRefused {
            profile: "campus-security".into(),
            qualification: SECURITY_PROFILE_QUALIFICATION.into(),
        })
    );
    let unrelated = BTreeSet::from(["campus-runtime-gvisor".to_string()]);
    assert!(security.check_launchable(&unrelated).is_err());
    let qualified = BTreeSet::from([SECURITY_PROFILE_QUALIFICATION.to_string()]);
    assert_eq!(security.check_launchable(&qualified), Ok(()));
}

#[test]
fn security_profile_cannot_be_enabled_or_weakened() {
    let mut enabled = fixture(SECURITY);
    enabled["launch_gate"] = json!("enabled");
    assert_eq!(rejected_field(&enabled), "launch_gate");

    let mut other_gate = fixture(SECURITY);
    other_gate["launch_gate"] = json!({ "requires_qualification": "campus-anything" });
    assert_eq!(rejected_field(&other_gate), "launch_gate");

    let mut gvisor = fixture(SECURITY);
    gvisor["isolation"] = json!("gvisor");
    assert_eq!(rejected_field(&gvisor), "isolation");

    let mut egress = fixture(SECURITY);
    egress["network"] =
        json!({ "mode": "approved_egress", "egress": [{ "host": "example.com", "port": 443 }] });
    assert_eq!(rejected_field(&egress), "network.mode");
}

#[test]
fn unknown_fields_are_rejected_at_every_level() {
    let paths: [&[&str]; 8] = [
        &[],
        &["resources"],
        &["network"],
        &["agent_budget"],
        &["model_policy"],
        &["student_choices"],
        &["capabilities", "0"],
        &["network", "egress", "0"],
    ];
    for path in paths {
        let mut value = fixture(CODING);
        let mut target = &mut value;
        for segment in path {
            target = match segment.parse::<usize>() {
                Ok(index) => &mut target[index],
                Err(_) => &mut target[*segment],
            };
        }
        target["privileged"] = json!(true);
        assert_malformed(&value);
    }
}

#[test]
fn omitted_fields_never_fall_back_to_defaults() {
    for field in [
        "isolation",
        "network",
        "ttl_seconds",
        "agent_budget",
        "launch_gate",
        "student_choices",
    ] {
        let mut value = fixture(RESEARCH);
        value.as_object_mut().unwrap().remove(field);
        assert_malformed(&value);
    }
    let mut budget = fixture(RESEARCH);
    budget["agent_budget"]
        .as_object_mut()
        .unwrap()
        .remove("hard_tokens");
    assert_malformed(&budget);
}

#[test]
fn duplicate_keys_are_rejected() {
    let document = RESEARCH.replacen(
        "\"ttl_seconds\": 1800,",
        "\"ttl_seconds\": 1800, \"ttl_seconds\": 14400,",
        1,
    );
    assert!(matches!(
        CampusProfile::parse(&document),
        Err(CampusProfileError::Malformed(_))
    ));
}

#[test]
fn mutable_or_malformed_image_references_are_rejected() {
    let digest = "fba7af5624c6806d7401420f777915ea5afd5198408fa431ebb78be585806731";
    for image in [
        "registry.lucas.engineering/pharness-python-runner:latest".to_string(),
        "registry.lucas.engineering/pharness-python-runner".to_string(),
        format!("registry.lucas.engineering/pharness-python-runner:3.11@sha256:{digest}"),
        format!(
            "registry.lucas.engineering/pharness-python-runner@sha256:{}",
            &digest[..63]
        ),
        format!(
            "registry.lucas.engineering/pharness-python-runner@sha256:{}",
            digest.to_uppercase()
        ),
        format!("pharness-python-runner@sha256:{digest}"),
        format!("registry.lucas.engineering/Pharness@sha256:{digest}"),
        format!("registry.lucas.engineering/pharness@sha256:{digest}@sha256:{digest}"),
    ] {
        let mut value = fixture(RESEARCH);
        value["image"] = json!(image);
        assert_eq!(rejected_field(&value), "image", "{image}");
    }
    let mut with_port = fixture(RESEARCH);
    with_port["image"] = json!(format!("registry.local:5000/team/runner@sha256:{digest}"));
    assert!(parse(&with_port).is_ok());
}

#[test]
fn resource_ttl_and_budget_limits_are_enforced() {
    let cases: Vec<(&str, Value, &str)> = vec![
        (
            "/resources/cpu_millicores",
            json!(0),
            "resources.cpu_millicores",
        ),
        (
            "/resources/cpu_millicores",
            json!(MAX_CPU_MILLICORES + 1),
            "resources.cpu_millicores",
        ),
        (
            "/resources/memory_mib",
            json!(MAX_MEMORY_MIB + 1),
            "resources.memory_mib",
        ),
        (
            "/resources/ephemeral_storage_mib",
            json!(0),
            "resources.ephemeral_storage_mib",
        ),
        ("/resources/pids", json!(MIN_PIDS - 1), "resources.pids"),
        ("/resources/pids", json!(MAX_PIDS + 1), "resources.pids"),
        ("/ttl_seconds", json!(MAX_TTL_SECONDS + 1), "ttl_seconds"),
        ("/ttl_seconds", json!(MIN_TTL_SECONDS - 1), "ttl_seconds"),
        ("/agent_budget/hard_turns", json!(8), "agent_budget"),
        (
            "/agent_budget/hard_tokens",
            json!(2_000_000),
            "agent_budget",
        ),
        (
            "/agent_budget/active_execution_seconds",
            json!(1801),
            "agent_budget.active_execution_seconds",
        ),
        (
            "/model_policy/session_allocation_micro_usd",
            json!(0),
            "model_policy.session_allocation_micro_usd",
        ),
        (
            "/model_policy/session_allocation_micro_usd",
            json!(MAX_SESSION_ALLOCATION_MICRO_USD + 1),
            "model_policy.session_allocation_micro_usd",
        ),
        (
            "/model_policy/max_output_tokens",
            json!(MAX_OUTPUT_TOKENS + 1),
            "model_policy.max_output_tokens",
        ),
        (
            "/model_policy/max_concurrent_requests",
            json!(0),
            "model_policy.max_concurrent_requests",
        ),
        (
            "/model_policy/model_aliases",
            json!([]),
            "model_policy.model_aliases",
        ),
        (
            "/model_policy/model_aliases",
            json!(["Campus Default"]),
            "model_policy.model_aliases",
        ),
    ];
    for (pointer, replacement, field) in cases {
        let mut value = fixture(RESEARCH);
        *value.pointer_mut(pointer).unwrap() = replacement;
        assert_eq!(rejected_field(&value), field, "{pointer}");
    }
    for (pointer, replacement) in [
        ("/resources/pids", json!(-1)),
        ("/resources/memory_mib", json!(1.5)),
        ("/resources/cpu_millicores", json!("500m")),
        (
            "/network/egress",
            json!([{ "host": "pypi.org", "port": 70000 }]),
        ),
    ] {
        let mut value = fixture(RESEARCH);
        *value.pointer_mut(pointer).unwrap() = replacement;
        assert_malformed(&value);
    }
}

#[test]
fn malformed_capability_bindings_are_rejected() {
    let cases = [
        json!([]),
        json!([{ "tool": "submit_implementation", "scope": "session_workspace" }]),
        json!([{ "tool": "kubernetes_get", "scope": "session_shell" }]),
        json!([{ "tool": "get_evidence", "scope": "session_workspace" }]),
        json!([{ "tool": "run_workspace_command", "scope": "session_workspace" }]),
        json!([{ "tool": "read_file", "scope": "agent_control" }]),
        json!([
            { "tool": "read_file", "scope": "session_workspace" },
            { "tool": "read_file", "scope": "session_workspace" }
        ]),
    ];
    for capabilities in cases {
        let mut value = fixture(RESEARCH);
        value["capabilities"] = capabilities.clone();
        let field = rejected_field(&value);
        assert!(field.starts_with("capabilities"), "{capabilities}: {field}");
    }
    let mut unknown_scope = fixture(RESEARCH);
    unknown_scope["capabilities"] = json!([{ "tool": "read_file", "scope": "cluster_admin" }]);
    assert_malformed(&unknown_scope);
}

#[test]
fn egress_destinations_must_be_public_dns_names() {
    for host in [
        "169.254.169.254",
        "10.0.0.1",
        "[fd00::1]",
        "::1",
        "localhost",
        "pharness-api.pharness.svc.cluster.local",
        "metadata.google.internal",
        "*.github.com",
        "GitHub.com",
        "github",
        "-bad.example.com",
        "",
    ] {
        let mut value = fixture(CODING);
        value["network"]["egress"] = json!([{ "host": host, "port": 443 }]);
        assert_eq!(rejected_field(&value), "network.egress[0]", "{host}");
    }
    let mut port_zero = fixture(CODING);
    port_zero["network"]["egress"] = json!([{ "host": "pypi.org", "port": 0 }]);
    assert_eq!(rejected_field(&port_zero), "network.egress[0]");

    let mut duplicate = fixture(CODING);
    duplicate["network"]["egress"] = json!([
        { "host": "pypi.org", "port": 443 },
        { "host": "pypi.org", "port": 443 }
    ]);
    assert_eq!(rejected_field(&duplicate), "network.egress[1]");

    let mut denied_with_hosts = fixture(RESEARCH);
    denied_with_hosts["network"]["egress"] = json!([{ "host": "pypi.org", "port": 443 }]);
    assert_eq!(rejected_field(&denied_with_hosts), "network.egress");

    let mut approved_without_hosts = fixture(CODING);
    approved_without_hosts["network"]["egress"] = json!([]);
    assert_eq!(rejected_field(&approved_without_hosts), "network.egress");
}

#[test]
fn student_choices_can_only_narrow() {
    let mut alias = fixture(RESEARCH);
    alias["student_choices"]["model_aliases"] = json!(["campus-premium"]);
    assert_eq!(rejected_field(&alias), "student_choices.model_aliases");

    let mut ttl = fixture(RESEARCH);
    ttl["student_choices"]["min_ttl_seconds"] = json!(1801);
    assert_eq!(rejected_field(&ttl), "student_choices.min_ttl_seconds");
}

#[test]
fn identity_and_schema_are_strict() {
    let mut schema = fixture(RESEARCH);
    schema["schema_version"] = json!("pharness.dev/campus-profile/v2");
    assert_eq!(rejected_field(&schema), "schema_version");

    let mut id = fixture(RESEARCH);
    id["id"] = json!("repo-builder");
    assert_eq!(rejected_field(&id), "id");

    let mut kind = fixture(RESEARCH);
    kind["kind"] = json!("admin");
    assert_malformed(&kind);
}

#[test]
fn profile_hash_is_canonical_and_content_bound() {
    let profile = CampusProfile::parse(RESEARCH).unwrap();
    let reordered: Value = fixture(RESEARCH);
    let mut keys = reordered.as_object().unwrap().iter().collect::<Vec<_>>();
    keys.reverse();
    let reordered_document = format!(
        "{{{}}}",
        keys.iter()
            .map(|(key, value)| format!("{}:{}", json!(key), value))
            .collect::<Vec<_>>()
            .join(",")
    );
    let reparsed = CampusProfile::parse(&reordered_document).unwrap();
    assert_eq!(profile.profile_hash(), reparsed.profile_hash());
    assert_eq!(profile.profile_hash(), RESEARCH_PROFILE_HASH);

    let mut changed = fixture(RESEARCH);
    changed["resources"]["memory_mib"] = json!(2048);
    assert_ne!(
        parse(&changed).unwrap().profile_hash(),
        profile.profile_hash()
    );

    assert_eq!(
        profile.profile_ref(),
        CampusProfileRef {
            id: "campus-research".into(),
            version: "v1".into(),
            profile_hash: RESEARCH_PROFILE_HASH.into(),
        }
    );
}

// Changing this value means the canonical form of an existing profile moved;
// recorded session hashes would no longer match their profile.
const RESEARCH_PROFILE_HASH: &str =
    "sha256:3446f702110bedcf821fbc7991d1e111c409357323560119deb25f0922f56d10";

#[test]
fn agent_profile_is_derived_only_from_the_envelope() {
    let profile = CampusProfile::parse(CODING).unwrap();
    let agent = profile
        .agent_profile("campus-default", "campus-v1")
        .unwrap();
    assert_eq!(agent.id, "campus-coding");
    assert_eq!(agent.profile_hash, profile.profile_hash());
    assert_eq!(agent.model, "campus-default");
    assert_eq!(
        agent.tools,
        profile
            .capabilities
            .iter()
            .map(|binding| binding.tool.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(agent.budget.hard_turns, 100);
    assert_eq!(agent.budget.verification_reserve_turns, 0);
    assert!(agent.budget.validate().is_ok());

    assert_eq!(
        profile
            .agent_profile("gpt-unapproved", "campus-v1")
            .unwrap_err(),
        CampusProfileError::Invalid {
            field: "model_alias".into(),
            reason: "gpt-unapproved is not approved for campus-coding".into(),
        }
    );
}

#[test]
fn oversized_documents_are_rejected_before_parsing() {
    let document = format!("{}{}", RESEARCH, " ".repeat(MAX_PROFILE_BYTES));
    assert!(matches!(
        CampusProfile::parse(&document),
        Err(CampusProfileError::Malformed(_))
    ));
}

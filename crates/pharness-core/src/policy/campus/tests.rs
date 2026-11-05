use super::*;
use crate::{
    CapabilityKind, PermissionGrant, PermissionGrantPolicy, PermissionGrantScope, PolicyMode,
};
use serde_json::{json, Value};

const RESEARCH: &str = include_str!("../../campus_profile/fixtures/research.json");
const CODING: &str = include_str!("../../campus_profile/fixtures/coding.json");
const SECURITY: &str = include_str!("../../campus_profile/fixtures/security.json");

// Changing this value means the canonical effective form of an unchanged
// profile and layer set moved; recorded decisions would no longer match.
const RESEARCH_SAMPLE_EFFECTIVE_POLICY_HASH: &str =
    "sha256:23071d947872d3463d4dfe17548a1432415829512c00682c18b28ebb974eb20b";

const RESEARCH_TOOLS: [&str; 7] = [
    "environment_info",
    "list_dir",
    "read_file",
    "search_files",
    "create_directory",
    "write_file",
    "finish",
];

fn profile(document: &str) -> CampusProfile {
    CampusProfile::parse(document).expect("fixture validates")
}

fn no_denies() -> Value {
    json!({ "tools": [], "egress_hosts": [], "model_aliases": [], "approval_kinds": [] })
}

fn layer_value(authority: &str, constraints: Value, denies: Value) -> Value {
    json!({
        "schema_version": CAMPUS_POLICY_LAYER_SCHEMA,
        "authority": authority,
        "id": format!("{authority}-policy"),
        "version": "v1",
        "constraints": constraints,
        "mandatory_denies": denies,
    })
}

fn parse_layer(value: &Value) -> Result<CampusPolicyLayer, CampusPolicyError> {
    CampusPolicyLayer::parse(&value.to_string())
}

fn layer(authority: &str, constraints: Value) -> CampusPolicyLayer {
    layer_with_denies(authority, constraints, no_denies())
}

fn layer_with_denies(authority: &str, constraints: Value, denies: Value) -> CampusPolicyLayer {
    parse_layer(&layer_value(authority, constraints, denies)).expect("layer validates")
}

fn required() -> Vec<CampusPolicyLayer> {
    vec![
        layer("platform", json!({})),
        layer("institution", json!({})),
    ]
}

fn with(mut layers: Vec<CampusPolicyLayer>, extra: CampusPolicyLayer) -> Vec<CampusPolicyLayer> {
    layers.push(extra);
    layers
}

fn compose(
    document: &str,
    layers: &[CampusPolicyLayer],
) -> Result<EffectiveCampusPolicy, CampusPolicyError> {
    EffectiveCampusPolicy::compose(&profile(document), layers, &BTreeSet::new())
}

fn sample_layers() -> Vec<CampusPolicyLayer> {
    vec![
        layer_with_denies(
            "platform",
            json!({
                "cpu_millicores": 4000,
                "memory_mib": 8192,
                "ephemeral_storage_mib": 16384,
                "ttl_seconds": 14400,
                "session_allocation_micro_usd": 10000000,
                "hard_tokens": 1000000
            }),
            json!({
                "tools": ["run_shell"],
                "egress_hosts": [],
                "model_aliases": [],
                "approval_kinds": ["privileged", "secret_access"]
            }),
        ),
        layer_with_denies(
            "institution",
            json!({
                "memory_mib": 2048,
                "model_aliases": ["campus-default", "campus-small"]
            }),
            json!({
                "tools": [],
                "egress_hosts": ["pastebin.com"],
                "model_aliases": [],
                "approval_kinds": ["network"]
            }),
        ),
        layer(
            "assignment",
            json!({
                "tools": ["environment_info", "list_dir", "read_file", "search_files", "write_file", "finish"],
                "ttl_seconds": 1200,
                "session_allocation_micro_usd": 250000,
                "max_output_tokens": 2048
            }),
        ),
        layer("student", json!({ "ttl_seconds": 900 })),
    ]
}

fn strings(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| value.to_string()).collect()
}

fn write_readme() -> AgentAction {
    AgentAction::WriteFile {
        id: "act_write".into(),
        reason: "write".into(),
        path: "README.md".into(),
        content: "hello".into(),
    }
}

fn workspace_command(executable: &str, args: &[&str]) -> AgentAction {
    AgentAction::RunWorkspaceCommand {
        id: "act_command".into(),
        reason: "run".into(),
        executable: executable.into(),
        args: args.iter().map(|arg| arg.to_string()).collect(),
        cwd: None,
        timeout_ms: None,
    }
}

fn readme_grant() -> PermissionGrant {
    PermissionGrant {
        id: "grant_readme".into(),
        subject: "agent:local-worker".into(),
        scope: PermissionGrantScope {
            environment: Some("local".into()),
            capability_kinds: vec![CapabilityKind::Filesystem],
            actions: vec!["write_file".into()],
            max_risk: Some(RiskLevel::Medium),
            writable_path_globs: vec!["README.md".into()],
            ..PermissionGrantScope::default()
        },
        policy: PermissionGrantPolicy {
            policy_mode: PolicyMode::TrustedWrites,
        },
        expires_at: None,
    }
}

fn approval(role: CampusApproverRole) -> CampusApproval {
    CampusApproval {
        action_id: "act_write".into(),
        approval_kind: ApprovalKind::FileWrite,
        role,
    }
}

/// Rewrites a JSON document with every object's keys reversed and
/// pretty-printed, so only key order and whitespace differ.
fn reordered(value: &Value) -> String {
    fn reverse(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut entries = map.iter().collect::<Vec<_>>();
                entries.reverse();
                let mut out = serde_json::Map::new();
                for (key, value) in entries {
                    out.insert(key.clone(), reverse(value));
                }
                Value::Object(out)
            }
            Value::Array(values) => Value::Array(values.iter().map(reverse).collect()),
            other => other.clone(),
        }
    }
    serde_json::to_string_pretty(&reverse(value)).unwrap()
}

#[test]
fn required_layers_alone_reproduce_the_profile() {
    for document in [RESEARCH, CODING] {
        let source = profile(document);
        let policy = compose(document, &required()).unwrap();
        let constraints = policy.constraints();
        assert_eq!(
            constraints.tools,
            source
                .capabilities
                .iter()
                .map(|binding| binding.tool.clone())
                .collect()
        );
        assert_eq!(
            constraints.egress,
            source.network.egress.iter().cloned().collect()
        );
        assert_eq!(constraints.ttl_seconds, source.ttl_seconds);
        assert_eq!(constraints.cpu_millicores, source.resources.cpu_millicores);
        assert_eq!(
            constraints.session_allocation_micro_usd,
            source.model_policy.session_allocation_micro_usd
        );
        assert!(policy
            .provenance()
            .values()
            .all(|source| *source == CampusPolicySource::Profile));
        assert_eq!(policy.profile_hash(), source.profile_hash());
        assert_eq!(policy.layers().len(), 2);
    }
}

#[test]
fn narrowing_intersects_sets_and_takes_minimum_limits() {
    use CampusPolicyDimension as D;
    use CampusPolicySource as S;
    let layers = vec![
        layer_with_denies(
            "platform",
            json!({ "memory_mib": 2048, "cpu_millicores": 4000 }),
            json!({ "tools": [], "egress_hosts": ["registry.npmjs.org"], "model_aliases": [], "approval_kinds": [] }),
        ),
        layer(
            "institution",
            json!({ "egress": [
                { "host": "pypi.org", "port": 443 },
                { "host": "files.pythonhosted.org", "port": 443 },
                { "host": "github.com", "port": 443 },
                { "host": "codeload.github.com", "port": 443 },
                { "host": "crates.io", "port": 443 }
            ] }),
        ),
        layer(
            "assignment",
            json!({
                "tools": ["environment_info", "list_dir", "read_file", "search_files", "create_directory",
                          "write_file", "patch_file", "git_diff", "git_status", "finish"],
                "egress": [
                    { "host": "pypi.org", "port": 443 },
                    { "host": "files.pythonhosted.org", "port": 443 }
                ],
                "ttl_seconds": 3600,
                "cpu_millicores": 1000,
                "session_allocation_micro_usd": 1000000,
                "hard_tokens": 500000
            }),
        ),
        layer(
            "student",
            json!({ "ttl_seconds": 1800, "model_aliases": ["campus-default"] }),
        ),
    ];
    let policy = compose(CODING, &layers).unwrap();
    let constraints = policy.constraints();
    assert_eq!(constraints.tools.len(), 10);
    assert!(!constraints.tools.contains("run_workspace_command"));
    assert!(!constraints.tools.contains("apply_patch"));
    assert_eq!(
        constraints.egress,
        BTreeSet::from([
            CampusEgressDestination {
                host: "files.pythonhosted.org".into(),
                port: 443
            },
            CampusEgressDestination {
                host: "pypi.org".into(),
                port: 443
            },
        ])
    );
    assert_eq!(constraints.ttl_seconds, 1800);
    assert_eq!(constraints.active_execution_seconds, 1800);
    assert_eq!(constraints.cpu_millicores, 1000);
    assert_eq!(constraints.memory_mib, 2048);
    assert_eq!(constraints.ephemeral_storage_mib, 4096);
    assert_eq!(constraints.session_allocation_micro_usd, 1_000_000);
    assert_eq!(constraints.hard_tokens, 500_000);
    assert_eq!(constraints.model_aliases, strings(&["campus-default"]));

    for (dimension, source) in [
        (D::Tools, S::Assignment),
        (D::Egress, S::Assignment),
        (D::TtlSeconds, S::Student),
        (D::ActiveExecutionSeconds, S::Student),
        (D::CpuMillicores, S::Assignment),
        (D::MemoryMib, S::Platform),
        (D::EphemeralStorageMib, S::Profile),
        (D::SessionAllocationMicroUsd, S::Assignment),
        (D::HardTokens, S::Assignment),
        (D::ModelAliases, S::Profile),
        (D::MaxConcurrentRequests, S::Profile),
    ] {
        assert_eq!(policy.source(dimension), source, "{dimension}");
    }

    let without_assignment = compose(
        CODING,
        &[layers[0].clone(), layers[1].clone(), layers[3].clone()],
    )
    .unwrap();
    assert_eq!(
        without_assignment.constraints().egress.len(),
        4,
        "the platform deny removes npm and the institution set adds nothing"
    );
    assert_eq!(without_assignment.source(D::Egress), S::Platform);
    assert_eq!(
        policy.removed()[&D::Egress],
        BTreeMap::from([
            ("codeload.github.com:443".to_string(), S::Assignment),
            ("github.com:443".to_string(), S::Assignment),
            ("registry.npmjs.org:443".to_string(), S::Platform),
        ])
    );
    assert_eq!(
        policy.exclusion_source(D::Tools, "run_workspace_command"),
        S::Assignment
    );
    assert_eq!(policy.exclusion_source(D::Tools, "run_shell"), S::Profile);
}

fn widening_cases() -> Vec<(CampusPolicyDimension, &'static str, Value)> {
    use CampusPolicyDimension as D;
    let mut tools = RESEARCH_TOOLS.to_vec();
    tools.push("run_workspace_command");
    vec![
        (D::Tools, "tools", json!(tools)),
        (
            D::Egress,
            "egress",
            json!([{ "host": "pypi.org", "port": 443 }]),
        ),
        (
            D::ModelAliases,
            "model_aliases",
            json!(["campus-default", "campus-premium"]),
        ),
        (D::TtlSeconds, "ttl_seconds", json!(3600)),
        (
            D::ActiveExecutionSeconds,
            "active_execution_seconds",
            json!(1600),
        ),
        (D::CpuMillicores, "cpu_millicores", json!(1000)),
        (D::MemoryMib, "memory_mib", json!(2048)),
        (D::EphemeralStorageMib, "ephemeral_storage_mib", json!(2048)),
        (D::Pids, "pids", json!(256)),
        (
            D::SessionAllocationMicroUsd,
            "session_allocation_micro_usd",
            json!(600000),
        ),
        (D::HardTokens, "hard_tokens", json!(300000)),
        (D::MaxOutputTokens, "max_output_tokens", json!(8192)),
        (
            D::MaxConcurrentRequests,
            "max_concurrent_requests",
            json!(3),
        ),
    ]
}

#[test]
fn assignment_and_student_cannot_widen_any_dimension() {
    for authority in ["assignment", "student"] {
        for (dimension, key, value) in widening_cases() {
            let mut constraints = json!({});
            constraints[key] = value;
            let layers = with(required(), layer(authority, constraints));
            match compose(RESEARCH, &layers) {
                Err(CampusPolicyError::Widening {
                    authority: refused,
                    dimension: refused_dimension,
                    bounded_by,
                    ..
                }) => {
                    assert_eq!(refused.as_str(), authority);
                    assert_eq!(refused_dimension, dimension);
                    assert_eq!(bounded_by, CampusPolicySource::Profile);
                }
                other => panic!("{authority} widening {dimension} was not refused: {other:?}"),
            }
        }
    }
}

#[test]
fn student_cannot_widen_what_the_assignment_narrowed() {
    let layers = vec![
        layer("platform", json!({})),
        layer("institution", json!({})),
        layer("assignment", json!({ "ttl_seconds": 1200 })),
        layer("student", json!({ "ttl_seconds": 1500 })),
    ];
    assert_eq!(
        compose(RESEARCH, &layers).unwrap_err(),
        CampusPolicyError::Widening {
            authority: CampusAuthority::Student,
            dimension: CampusPolicyDimension::TtlSeconds,
            requested: "1500".into(),
            permitted: "1200".into(),
            bounded_by: CampusPolicySource::Assignment,
        }
    );
}

#[test]
fn institution_cannot_widen_platform_but_shared_ceilings_may_exceed_the_profile() {
    let platform = layer(
        "platform",
        json!({ "cpu_millicores": 1000, "tools": RESEARCH_TOOLS }),
    );
    let refused = compose(
        RESEARCH,
        &[
            platform.clone(),
            layer("institution", json!({ "cpu_millicores": 2000 })),
        ],
    );
    assert!(matches!(
        refused,
        Err(CampusPolicyError::Widening {
            authority: CampusAuthority::Institution,
            dimension: CampusPolicyDimension::CpuMillicores,
            bounded_by: CampusPolicySource::Platform,
            ..
        })
    ));
    let refused_tool = compose(
        RESEARCH,
        &[
            platform.clone(),
            layer(
                "institution",
                json!({ "tools": ["read_file", "run_workspace_command"] }),
            ),
        ],
    );
    assert!(matches!(
        refused_tool,
        Err(CampusPolicyError::Widening {
            authority: CampusAuthority::Institution,
            dimension: CampusPolicyDimension::Tools,
            ref requested,
            ..
        }) if requested == "run_workspace_command"
    ));

    // A shared ceiling written for every profile is not a widening of a
    // smaller profile: Research keeps its own 500m CPU and 1 GiB memory.
    let shared = compose(
        RESEARCH,
        &[
            layer(
                "platform",
                json!({ "cpu_millicores": 4000, "memory_mib": 8192 }),
            ),
            layer("institution", json!({ "cpu_millicores": 2000 })),
        ],
    )
    .unwrap();
    assert_eq!(shared.constraints().cpu_millicores, 500);
    assert_eq!(shared.constraints().memory_mib, 1024);
    assert_eq!(
        shared.source(CampusPolicyDimension::CpuMillicores),
        CampusPolicySource::Profile
    );

    // A shared ceiling below the profile narrows it and is recorded.
    let narrowed = compose(
        CODING,
        &[
            layer("platform", json!({ "memory_mib": 1024 })),
            layer("institution", json!({})),
        ],
    )
    .unwrap();
    assert_eq!(narrowed.constraints().memory_mib, 1024);
    assert_eq!(
        narrowed.source(CampusPolicyDimension::MemoryMib),
        CampusPolicySource::Platform
    );
}

#[test]
fn student_layer_is_bounded_to_profile_choices() {
    let narrowing_cpu = with(
        required(),
        layer("student", json!({ "cpu_millicores": 250 })),
    );
    assert_eq!(
        compose(RESEARCH, &narrowing_cpu).unwrap_err(),
        CampusPolicyError::DimensionNotPermitted {
            authority: CampusAuthority::Student,
            dimension: CampusPolicyDimension::CpuMillicores,
        }
    );

    let short_ttl = with(required(), layer("student", json!({ "ttl_seconds": 300 })));
    assert!(matches!(
        compose(RESEARCH, &short_ttl),
        Err(CampusPolicyError::InvalidLayer {
            authority: CampusAuthority::Student,
            ref field,
            ..
        }) if field == "constraints.ttl_seconds"
    ));

    let mut wider: Value = serde_json::from_str(RESEARCH).unwrap();
    wider["model_policy"]["model_aliases"] = json!(["campus-default", "campus-large"]);
    let wider_profile = CampusProfile::parse(&wider.to_string()).unwrap();
    let pick_large = with(
        required(),
        layer("student", json!({ "model_aliases": ["campus-large"] })),
    );
    assert_eq!(
        EffectiveCampusPolicy::compose(&wider_profile, &pick_large, &BTreeSet::new()).unwrap_err(),
        CampusPolicyError::Widening {
            authority: CampusAuthority::Student,
            dimension: CampusPolicyDimension::ModelAliases,
            requested: "campus-large".into(),
            permitted: "campus-default".into(),
            bounded_by: CampusPolicySource::Profile,
        }
    );
    let assignment_may = with(
        required(),
        layer("assignment", json!({ "model_aliases": ["campus-large"] })),
    );
    let policy =
        EffectiveCampusPolicy::compose(&wider_profile, &assignment_may, &BTreeSet::new()).unwrap();
    assert_eq!(
        policy.constraints().model_aliases,
        strings(&["campus-large"])
    );
}

#[test]
fn mandatory_denies_narrow_and_cannot_be_reasserted_below() {
    let deny_write = layer_with_denies(
        "institution",
        json!({}),
        json!({ "tools": ["write_file"], "egress_hosts": [], "model_aliases": [], "approval_kinds": [] }),
    );
    let policy = compose(
        RESEARCH,
        &[layer("platform", json!({})), deny_write.clone()],
    )
    .unwrap();
    assert!(!policy.constraints().tools.contains("write_file"));
    assert_eq!(
        policy.source(CampusPolicyDimension::Tools),
        CampusPolicySource::Institution
    );

    let reasserted = compose(
        RESEARCH,
        &[
            layer("platform", json!({})),
            deny_write,
            layer(
                "assignment",
                json!({ "tools": ["read_file", "write_file"] }),
            ),
        ],
    );
    assert_eq!(
        reasserted.unwrap_err(),
        CampusPolicyError::MandatoryDenyConflict {
            authority: CampusAuthority::Assignment,
            dimension: CampusPolicyDimension::Tools,
            value: "write_file".into(),
            denied_by: CampusAuthority::Institution,
        }
    );

    let platform_denies_model = layer_with_denies(
        "platform",
        json!({}),
        json!({ "tools": [], "egress_hosts": [], "model_aliases": ["campus-small"], "approval_kinds": [] }),
    );
    let institution_lists_it = layer(
        "institution",
        json!({ "model_aliases": ["campus-default", "campus-small"] }),
    );
    assert!(matches!(
        compose(RESEARCH, &[platform_denies_model, institution_lists_it]),
        Err(CampusPolicyError::MandatoryDenyConflict {
            authority: CampusAuthority::Institution,
            dimension: CampusPolicyDimension::ModelAliases,
            denied_by: CampusAuthority::Platform,
            ..
        })
    ));

    let contradictory = layer_with_denies(
        "institution",
        json!({ "egress": [{ "host": "pastebin.com", "port": 443 }] }),
        json!({ "tools": [], "egress_hosts": ["pastebin.com"], "model_aliases": [], "approval_kinds": [] }),
    );
    assert!(matches!(
        compose(CODING, &[layer("platform", json!({})), contradictory]),
        Err(CampusPolicyError::MandatoryDenyConflict {
            denied_by: CampusAuthority::Institution,
            ..
        })
    ));

    let deny_every_model = layer_with_denies(
        "platform",
        json!({}),
        json!({ "tools": [], "egress_hosts": [], "model_aliases": ["campus-default"], "approval_kinds": [] }),
    );
    assert_eq!(
        compose(
            RESEARCH,
            &[deny_every_model, layer("institution", json!({}))]
        )
        .unwrap_err(),
        CampusPolicyError::EmptyDimension {
            dimension: CampusPolicyDimension::ModelAliases
        }
    );

    for authority in ["assignment", "student"] {
        let value = layer_value(
            authority,
            json!({}),
            json!({ "tools": ["write_file"], "egress_hosts": [], "model_aliases": [], "approval_kinds": [] }),
        );
        assert!(matches!(
            parse_layer(&value),
            Err(CampusPolicyError::InvalidLayer { ref field, .. }) if field == "mandatory_denies"
        ));
    }
}

#[test]
fn missing_duplicate_and_out_of_order_layers_refuse_launch() {
    use CampusAuthority as A;
    let platform = layer("platform", json!({}));
    let institution = layer("institution", json!({}));
    let assignment = layer("assignment", json!({}));
    let student = layer("student", json!({}));
    let cases: Vec<(Vec<CampusPolicyLayer>, CampusPolicyError)> = vec![
        (
            vec![],
            CampusPolicyError::MissingLayer {
                authority: A::Platform,
            },
        ),
        (
            vec![platform.clone()],
            CampusPolicyError::MissingLayer {
                authority: A::Institution,
            },
        ),
        (
            vec![institution.clone(), assignment.clone()],
            CampusPolicyError::MissingLayer {
                authority: A::Platform,
            },
        ),
        (
            vec![institution.clone(), platform.clone()],
            CampusPolicyError::OutOfOrder {
                authority: A::Platform,
                after: A::Institution,
            },
        ),
        (
            vec![
                platform.clone(),
                institution.clone(),
                student.clone(),
                assignment.clone(),
            ],
            CampusPolicyError::OutOfOrder {
                authority: A::Assignment,
                after: A::Student,
            },
        ),
        (
            vec![platform.clone(), platform.clone(), institution.clone()],
            CampusPolicyError::DuplicateAuthority {
                authority: A::Platform,
            },
        ),
        (
            vec![platform, institution, assignment.clone(), assignment],
            CampusPolicyError::DuplicateAuthority {
                authority: A::Assignment,
            },
        ),
    ];
    for (layers, expected) in cases {
        assert_eq!(compose(RESEARCH, &layers).unwrap_err(), expected);
    }

    // Layers built in code are revalidated before composition.
    let mut invalid = layer("institution", json!({}));
    invalid.constraints.cpu_millicores = Some(50);
    assert!(matches!(
        compose(RESEARCH, &[layer("platform", json!({})), invalid]),
        Err(CampusPolicyError::InvalidLayer { ref field, .. }) if field == "constraints.cpu_millicores"
    ));
    let mut smuggled = layer("student", json!({}));
    smuggled.mandatory_denies.tools = vec!["read_file".into()];
    assert!(matches!(
        compose(RESEARCH, &with(required(), smuggled)),
        Err(CampusPolicyError::InvalidLayer { ref field, .. }) if field == "mandatory_denies"
    ));
}

#[test]
fn malformed_layer_documents_are_rejected() {
    let base = layer_value("institution", json!({}), no_denies());
    let mut malformed = Vec::new();
    for pointer in ["", "/constraints", "/mandatory_denies"] {
        let mut value = base.clone();
        value.pointer_mut(pointer).unwrap()["privileged"] = json!(true);
        malformed.push(value);
    }
    let mut egress_extra = base.clone();
    egress_extra["constraints"]["egress"] =
        json!([{ "host": "pypi.org", "port": 443, "tls": false }]);
    malformed.push(egress_extra);
    let mut no_denies_field = base.clone();
    no_denies_field
        .as_object_mut()
        .unwrap()
        .remove("mandatory_denies");
    malformed.push(no_denies_field);
    let mut partial_denies = base.clone();
    partial_denies["mandatory_denies"]
        .as_object_mut()
        .unwrap()
        .remove("approval_kinds");
    malformed.push(partial_denies);
    let mut unknown_authority = base.clone();
    unknown_authority["authority"] = json!("department");
    malformed.push(unknown_authority);
    let mut unknown_kind = base.clone();
    unknown_kind["mandatory_denies"]["approval_kinds"] = json!(["everything"]);
    malformed.push(unknown_kind);
    for (pointer, replacement) in [
        ("/constraints/ttl_seconds", json!(-1)),
        ("/constraints/memory_mib", json!(1.5)),
        ("/constraints/cpu_millicores", json!("500m")),
    ] {
        let mut value = base.clone();
        value["constraints"][pointer.rsplit('/').next().unwrap()] = replacement;
        malformed.push(value);
    }
    for value in malformed {
        assert!(
            matches!(parse_layer(&value), Err(CampusPolicyError::Malformed(_))),
            "expected a schema rejection for {value}"
        );
    }

    let duplicate_key = base.to_string().replacen(
        "\"version\":\"v1\"",
        "\"version\":\"v1\",\"version\":\"v2\"",
        1,
    );
    assert!(matches!(
        CampusPolicyLayer::parse(&duplicate_key),
        Err(CampusPolicyError::Malformed(_))
    ));
    let oversized = format!("{}{}", base, " ".repeat(MAX_LAYER_BYTES));
    assert!(matches!(
        CampusPolicyLayer::parse(&oversized),
        Err(CampusPolicyError::Malformed(_))
    ));

    let invalid_cases: Vec<(&str, Value, &str)> = vec![
        (
            "/schema_version",
            json!("pharness.dev/campus-policy-layer/v2"),
            "schema_version",
        ),
        ("/id", json!("Institution Policy"), "id"),
        (
            "/constraints/tools",
            json!(["run_shell"]),
            "constraints.tools",
        ),
        (
            "/constraints/tools",
            json!(["read_file", "read_file"]),
            "constraints.tools",
        ),
        (
            "/constraints/egress",
            json!([{ "host": "169.254.169.254", "port": 443 }]),
            "constraints.egress[0]",
        ),
        (
            "/constraints/egress",
            json!([{ "host": "pypi.org", "port": 0 }]),
            "constraints.egress[0]",
        ),
        (
            "/constraints/ttl_seconds",
            json!(100),
            "constraints.ttl_seconds",
        ),
        (
            "/constraints/cpu_millicores",
            json!(0),
            "constraints.cpu_millicores",
        ),
        (
            "/constraints/hard_tokens",
            json!(2_000_000),
            "constraints.hard_tokens",
        ),
        (
            "/constraints/session_allocation_micro_usd",
            json!(0),
            "constraints.session_allocation_micro_usd",
        ),
        (
            "/mandatory_denies/egress_hosts",
            json!(["localhost"]),
            "mandatory_denies.egress_hosts[0]",
        ),
        (
            "/mandatory_denies/approval_kinds",
            json!(["network", "network"]),
            "mandatory_denies.approval_kinds",
        ),
    ];
    for (pointer, replacement, field) in invalid_cases {
        let mut value = base.clone();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        value
            .pointer_mut(if parent.is_empty() { "" } else { parent })
            .unwrap()[key] = replacement;
        match parse_layer(&value) {
            Err(CampusPolicyError::InvalidLayer {
                field: rejected, ..
            }) => assert_eq!(rejected, field, "{pointer}"),
            other => panic!("expected {field} to be rejected, got {other:?}"),
        }
    }
}

#[test]
fn security_profile_remains_gated() {
    assert_eq!(
        compose(SECURITY, &required()).unwrap_err(),
        CampusPolicyError::Profile(CampusProfileError::LaunchRefused {
            profile: "campus-security".into(),
            qualification: crate::SECURITY_PROFILE_QUALIFICATION.into(),
        })
    );
    let qualified = BTreeSet::from([crate::SECURITY_PROFILE_QUALIFICATION.to_string()]);
    let policy =
        EffectiveCampusPolicy::compose(&profile(SECURITY), &required(), &qualified).unwrap();
    assert!(policy.constraints().egress.is_empty());

    let mut tampered = profile(RESEARCH);
    tampered.ttl_seconds = MAX_TTL_SECONDS + 1;
    assert!(matches!(
        EffectiveCampusPolicy::compose(&tampered, &required(), &BTreeSet::new()),
        Err(CampusPolicyError::Profile(
            CampusProfileError::Invalid { .. }
        ))
    ));
}

#[test]
fn hashes_ignore_key_order_whitespace_and_list_order() {
    let layers = sample_layers();
    let policy = compose(RESEARCH, &layers).unwrap();

    let reparsed = layers
        .iter()
        .map(|layer| {
            let mut value = serde_json::to_value(layer).unwrap();
            if let Some(tools) = value["constraints"]["tools"].as_array_mut() {
                tools.reverse();
            }
            if let Some(kinds) = value["mandatory_denies"]["approval_kinds"].as_array_mut() {
                kinds.reverse();
            }
            CampusPolicyLayer::parse(&reordered(&value)).unwrap()
        })
        .collect::<Vec<_>>();
    for (original, reparsed) in layers.iter().zip(&reparsed) {
        assert_eq!(original.layer_hash(), reparsed.layer_hash());
    }
    let reordered_profile =
        CampusProfile::parse(&reordered(&serde_json::from_str(RESEARCH).unwrap())).unwrap();
    let recomposed =
        EffectiveCampusPolicy::compose(&reordered_profile, &reparsed, &BTreeSet::new()).unwrap();
    assert_eq!(
        recomposed.effective_policy_hash(),
        policy.effective_policy_hash()
    );
    assert_eq!(recomposed, policy);

    let mut changed = layers.clone();
    changed[3].constraints.ttl_seconds = Some(901);
    assert_ne!(changed[3].layer_hash(), layers[3].layer_hash());
    assert_ne!(
        compose(RESEARCH, &changed).unwrap().effective_policy_hash(),
        policy.effective_policy_hash()
    );
    let mut renamed = layers.clone();
    renamed[2].version = "v2".into();
    assert_ne!(
        compose(RESEARCH, &renamed).unwrap().effective_policy_hash(),
        policy.effective_policy_hash(),
        "layer identity is bound even when the bounds are unchanged"
    );
    assert!(policy.effective_policy_hash().starts_with("sha256:"));
}

#[test]
fn research_sample_effective_policy_hash_is_pinned() {
    let policy = compose(RESEARCH, &sample_layers()).unwrap();
    assert_eq!(
        policy.effective_policy_hash(),
        RESEARCH_SAMPLE_EFFECTIVE_POLICY_HASH
    );
    let constraints = policy.constraints();
    assert_eq!(constraints.ttl_seconds, 900);
    assert_eq!(constraints.active_execution_seconds, 900);
    assert_eq!(constraints.memory_mib, 1024);
    assert_eq!(constraints.session_allocation_micro_usd, 250_000);
    assert_eq!(constraints.max_output_tokens, 2048);
    assert!(!constraints.tools.contains("create_directory"));
    assert_eq!(policy.mandatory_denies().len(), 2);
    let serialized = serde_json::to_value(&policy).unwrap();
    assert_eq!(
        serialized["effective_policy_hash"],
        json!(RESEARCH_SAMPLE_EFFECTIVE_POLICY_HASH)
    );
    assert_eq!(serialized["provenance"]["ttl_seconds"], json!("student"));
    assert_eq!(serialized["layers"][0]["authority"], json!("platform"));
}

#[test]
fn decision_records_carry_authority_reason_and_exact_hashes() {
    use CampusDecisionOutcome as O;
    use CampusDecisionReason as R;
    use CampusPolicySource as S;
    let policy = compose(RESEARCH, &sample_layers()).unwrap();
    let baseline = SafetyPolicy::default();
    let scope = RunScope::default();
    let read = AgentAction::ReadFile {
        id: "act_read".into(),
        reason: "read".into(),
        path: "README.md".into(),
        max_bytes: None,
        start_line: None,
        line_count: None,
    };
    let mkdir = AgentAction::CreateDirectory {
        id: "act_mkdir".into(),
        reason: "mkdir".into(),
        path: "out".into(),
    };
    let shell = AgentAction::RunShell {
        id: "act_shell".into(),
        reason: "shell".into(),
        cmd: "ls".into(),
        cwd: None,
        timeout_ms: None,
        dry_run: false,
    };
    let cases = [
        (
            policy.evaluate_action(&baseline, &read, &scope, None),
            O::Allow,
            S::Platform,
            R::BaselineAllow,
        ),
        (
            policy.evaluate_action(&baseline, &write_readme(), &scope, None),
            O::Ask,
            S::Platform,
            R::ApprovalRequired,
        ),
        (
            policy.evaluate_action(&baseline, &mkdir, &scope, None),
            O::Deny,
            S::Assignment,
            R::OutsideEffectivePolicy,
        ),
        (
            policy.evaluate_action(&baseline, &workspace_command("pytest", &[]), &scope, None),
            O::Deny,
            S::Profile,
            R::OutsideEffectivePolicy,
        ),
        (
            policy.evaluate_action(&baseline, &shell, &scope, None),
            O::Deny,
            S::Platform,
            R::MandatoryDeny,
        ),
        (
            policy.authorize_model_alias("campus-default"),
            O::Allow,
            S::Profile,
            R::WithinEffectivePolicy,
        ),
        (
            policy.authorize_model_alias("campus-small"),
            O::Deny,
            S::Profile,
            R::OutsideEffectivePolicy,
        ),
        (
            policy.authorize_egress("pypi.org", 443),
            O::Deny,
            S::Profile,
            R::OutsideEffectivePolicy,
        ),
        (
            policy.authorize_egress("pastebin.com", 443),
            O::Deny,
            S::Institution,
            R::MandatoryDeny,
        ),
    ];
    for (record, outcome, authority, reason) in cases {
        assert_eq!(
            (record.outcome, record.authority, record.reason),
            (outcome, authority, reason),
            "{}",
            record.subject
        );
        assert_eq!(
            record.effective_policy_hash,
            RESEARCH_SAMPLE_EFFECTIVE_POLICY_HASH
        );
        assert_eq!(record.profile_hash, profile(RESEARCH).profile_hash());
    }

    let ask = policy.evaluate_action(&baseline, &write_readme(), &scope, None);
    assert_eq!(
        serde_json::to_value(&ask).unwrap(),
        json!({
            "outcome": "ask",
            "subject": "action:write_file",
            "authority": "platform",
            "reason": "approval_required",
            "summary": "file write requires approval: README.md",
            "risk": "medium",
            "approval_kind": "file_write",
            "effective_policy_hash": RESEARCH_SAMPLE_EFFECTIVE_POLICY_HASH,
            "profile_hash": profile(RESEARCH).profile_hash(),
        })
    );

    let coding = compose(CODING, &required()).unwrap();
    let allowed = coding.authorize_egress("pypi.org", 443);
    assert!(allowed.is_allow());
    assert_eq!(
        allowed.effective_policy_hash,
        coding.effective_policy_hash()
    );
    assert_ne!(allowed.effective_policy_hash, ask.effective_policy_hash);
}

#[test]
fn grants_and_approvals_cannot_override_mandatory_denies() {
    let scope = RunScope::default();
    let granted = SafetyPolicy {
        permission_grants: vec![readme_grant()],
        ..SafetyPolicy::default()
    };
    // The SDLC path and a Campus policy without the deny both honour the grant.
    assert!(matches!(
        granted.evaluate_action_in_scope(&write_readme(), &scope),
        PolicyDecision::Allow { grant_id: Some(ref id), .. } if id == "grant_readme"
    ));
    let open = compose(RESEARCH, &required()).unwrap();
    let by_grant = open.evaluate_action(&granted, &write_readme(), &scope, None);
    assert_eq!(by_grant.outcome, CampusDecisionOutcome::Allow);
    assert_eq!(by_grant.reason, CampusDecisionReason::PermissionGrant);
    assert_eq!(by_grant.grant_id.as_deref(), Some("grant_readme"));

    let deny_writes = layer_with_denies(
        "institution",
        json!({}),
        json!({ "tools": [], "egress_hosts": [], "model_aliases": [], "approval_kinds": ["file_write"] }),
    );
    let locked = compose(RESEARCH, &[layer("platform", json!({})), deny_writes]).unwrap();
    let trusted = SafetyPolicy {
        mode: PolicyMode::TrustedWrites,
        require_approval_for_writes: false,
        ..granted.clone()
    };
    for baseline in [&granted, &trusted] {
        for approval in [
            None,
            Some(approval(CampusApproverRole::Instructor)),
            Some(approval(CampusApproverRole::Student)),
        ] {
            let record =
                locked.evaluate_action(baseline, &write_readme(), &scope, approval.as_ref());
            assert_eq!(record.outcome, CampusDecisionOutcome::Deny);
            assert_eq!(record.authority, CampusPolicySource::Institution);
            assert_eq!(record.reason, CampusDecisionReason::MandatoryDeny);
            assert_eq!(record.approval_kind, Some(ApprovalKind::FileWrite));
            assert_eq!(record.grant_id, None);
            assert_eq!(record.approved_by, None);
        }
    }

    let deny_tool = layer_with_denies(
        "platform",
        json!({}),
        json!({ "tools": ["write_file"], "egress_hosts": [], "model_aliases": [], "approval_kinds": [] }),
    );
    let no_write_tool = compose(RESEARCH, &[deny_tool, layer("institution", json!({}))]).unwrap();
    let record = no_write_tool.evaluate_action(
        &granted,
        &write_readme(),
        &scope,
        Some(&approval(CampusApproverRole::Instructor)),
    );
    assert_eq!(
        (record.outcome, record.authority, record.reason),
        (
            CampusDecisionOutcome::Deny,
            CampusPolicySource::Platform,
            CampusDecisionReason::MandatoryDeny
        )
    );

    // Workspace commands are allowed by the SDLC baseline, so mandatory
    // operation denies are matched on the classified command line.
    let permissive = SafetyPolicy {
        deny_privileged: false,
        require_approval_for_network: false,
        ..SafetyPolicy::default()
    };
    let sudo = workspace_command("sudo", &["id"]);
    let curl = workspace_command("curl", &["https://example.com"]);
    assert!(permissive
        .evaluate_action_in_scope(&sudo, &scope)
        .is_allow());
    let coding = compose(CODING, &sample_layers()[..2]).unwrap();
    for (action, authority, kind) in [
        (
            &sudo,
            CampusPolicySource::Platform,
            ApprovalKind::Privileged,
        ),
        (
            &curl,
            CampusPolicySource::Institution,
            ApprovalKind::Network,
        ),
    ] {
        let record = coding.evaluate_action(&permissive, action, &scope, None);
        assert_eq!(record.outcome, CampusDecisionOutcome::Deny);
        assert_eq!(record.authority, authority);
        assert_eq!(record.reason, CampusDecisionReason::MandatoryDeny);
        assert_eq!(record.approval_kind, Some(kind));
    }
    assert!(coding
        .evaluate_action(
            &permissive,
            &workspace_command("pytest", &["-q"]),
            &scope,
            None
        )
        .is_allow());
}

#[test]
fn approvals_are_bounded_by_role_and_bound_to_the_action() {
    let policy = compose(RESEARCH, &required()).unwrap();
    let baseline = SafetyPolicy::default();
    let scope = RunScope::default();
    for (role, authority) in [
        (CampusApproverRole::Student, CampusPolicySource::Student),
        (
            CampusApproverRole::Instructor,
            CampusPolicySource::Assignment,
        ),
    ] {
        let record =
            policy.evaluate_action(&baseline, &write_readme(), &scope, Some(&approval(role)));
        assert_eq!(record.outcome, CampusDecisionOutcome::Allow);
        assert_eq!(record.reason, CampusDecisionReason::RoleApproval);
        assert_eq!(record.authority, authority);
        assert_eq!(record.approved_by, Some(role));
    }

    let other_action = CampusApproval {
        action_id: "act_other".into(),
        ..approval(CampusApproverRole::Instructor)
    };
    let other_kind = CampusApproval {
        approval_kind: ApprovalKind::ShellCommand,
        ..approval(CampusApproverRole::Instructor)
    };
    for mismatched in [other_action, other_kind] {
        let record = policy.evaluate_action(&baseline, &write_readme(), &scope, Some(&mismatched));
        assert_eq!(record.outcome, CampusDecisionOutcome::Ask);
        assert_eq!(record.reason, CampusDecisionReason::ApprovalMismatch);
    }

    use ApprovalKind as K;
    for kind in [K::FileWrite, K::ShellCommand] {
        assert!(CampusApproverRole::Student.may_approve(kind));
        assert!(CampusApproverRole::Instructor.may_approve(kind));
    }
    assert!(!CampusApproverRole::Student.may_approve(K::Destructive));
    assert!(CampusApproverRole::Instructor.may_approve(K::Destructive));
    for kind in [
        K::Network,
        K::Privileged,
        K::SecretAccess,
        K::RegistryWrite,
        K::TektonRun,
        K::ArgoSync,
        K::DatabaseChange,
        K::ProductionChange,
    ] {
        for role in CampusApproverRole::ALL {
            assert!(
                !role.may_approve(kind),
                "{} may approve {kind:?}",
                role.as_str()
            );
        }
    }
    assert!(serde_json::from_value::<CampusApproval>(json!({
        "action_id": "act_write",
        "approval_kind": "file_write",
        "role": "administrator"
    }))
    .is_err());
}

/// Compatibility boundary: the SDLC entry point must keep returning exactly
/// these decisions, and a Campus evaluation must not alter the baseline it
/// wraps.
#[test]
fn sdlc_entry_point_results_are_unchanged() {
    let scope = RunScope::default();
    let granted = SafetyPolicy {
        permission_grants: vec![readme_grant()],
        ..SafetyPolicy::default()
    };
    let deny_all_writes = SafetyPolicy {
        mode: PolicyMode::DenyAllWrites,
        ..granted.clone()
    };
    let kube_secret = AgentAction::KubernetesGet {
        id: "act_kube".into(),
        reason: "read".into(),
        resource: "secrets".into(),
        namespace: Some("apps".into()),
        name: None,
        all_namespaces: false,
        label_selector: None,
    };
    let sudo_shell = AgentAction::RunShell {
        id: "act_shell".into(),
        reason: "shell".into(),
        cmd: "sudo whoami".into(),
        cwd: None,
        timeout_ms: None,
        dry_run: false,
    };
    let pinned = |policy: &SafetyPolicy, action: &AgentAction| {
        serde_json::to_value(policy.evaluate_action_in_scope(action, &scope)).unwrap()
    };
    let expectations = [
        (
            SafetyPolicy::default(),
            write_readme(),
            json!({ "decision": "ask", "approval_kind": "file_write", "risk": "medium",
                    "summary": "file write requires approval: README.md" }),
        ),
        (
            granted.clone(),
            write_readme(),
            json!({ "decision": "allow", "risk": "medium", "grant_id": "grant_readme",
                    "summary": "file write requires approval: README.md; allowed by permission grant grant_readme" }),
        ),
        (
            deny_all_writes,
            write_readme(),
            json!({ "decision": "deny", "risk": "medium",
                    "summary": "file write to README.md blocked by policy mode DenyAllWrites" }),
        ),
        (
            SafetyPolicy::default(),
            sudo_shell,
            json!({ "decision": "deny", "risk": "critical",
                    "summary": "privileged command denied: sudo whoami" }),
        ),
        (
            SafetyPolicy::default(),
            workspace_command("sudo", &["id"]),
            json!({ "decision": "allow", "risk": "low",
                    "summary": "run_workspace_command is read-only" }),
        ),
        (
            SafetyPolicy::default(),
            kube_secret,
            json!({ "decision": "deny", "risk": "critical",
                    "summary": "kubernetes_get denied because it appears to access secrets" }),
        ),
    ];
    let campus = compose(CODING, &sample_layers()[..2]).unwrap();
    for (policy, action, expected) in expectations {
        assert_eq!(pinned(&policy, &action), expected, "{}", action.kind_name());
        let before = policy.clone();
        let _ = campus.evaluate_action(
            &policy,
            &action,
            &scope,
            Some(&approval(CampusApproverRole::Instructor)),
        );
        assert_eq!(policy, before);
        assert_eq!(pinned(&policy, &action), expected, "{}", action.kind_name());
    }
}

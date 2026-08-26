use pharness_core::SessionId;
use pharness_store::StoredDeploymentIntent;
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub(super) const V3_RELEASE_COMMIT: &str = "1aedc319e30c04f6fabfbb1ac6bde0f2f6cc3ec9";
pub(super) const V3_SOURCE_REVISION: &str = "97d2935933b872b76f7a2d8aa98e82d72f1f4e17";
pub(super) const V3_RUNTIME_DIGEST: &str =
    "sha256:0b8a64e847b1558ee976364a1b615576cb9acf8b8c32a3c675ef59c810c7341b";
pub(super) const V3_UI_DIGEST: &str =
    "sha256:e886457a846a19317fcdef8b291be634f85ac80dbb7b14b20de01991610ed3e4";
pub(super) const V3_RUNNER_DIGEST: &str =
    "sha256:abde65aab67c3f0b72da5bca0b211af66f9946dc5e291a2b63818e38f90f214b";
pub(super) const V3_WORK_ITEM_ID: &str = "witem_1787004039044254173";
pub(super) const V3_RUN_ID: &str = "run_1787004203328241892";
pub(super) const V3_RELEASE_ID: &str = "rel_1787319729524364759";
pub(super) const V3_ROLLBACK_INTENT_ID: &str = "rollback_1787101080007131097";
pub(super) const V3_RUNNING_YFINANCE_DIGEST: &str =
    "sha256:f1cfc06fcac62d7c37a4d7dc87237e2abe02df0d9c3824a7521c5359058879c1";
pub(super) const V3_ROLLBACK_BASELINE_DIGEST: &str =
    "sha256:850341f37100a0e90711b54733e06eeb52cb268244c6bbc07c25ef1b3c932cce";

pub(super) const V3_CHARACTERIZATION_FIXTURE: &str =
    include_str!("../../../tests/fixtures/v3-characterization.json");
pub(super) const APP_ROUTE_INVENTORY: &str =
    include_str!("../../../tests/fixtures/app-route-inventory.tsv");

pub(super) fn reconcile_deployment_intent() -> StoredDeploymentIntent {
    StoredDeploymentIntent {
        id: "dint_reconcile".to_string(),
        pipeline_intent_id: "pint_reconcile".to_string(),
        change_set_id: "cset_reconcile".to_string(),
        work_plan_id: "wplan_reconcile".to_string(),
        remediation_plan_id: None,
        incident_id: None,
        session_id: SessionId::new("ses_reconcile"),
        run_id: None,
        status: "proposed".to_string(),
        title: "Reconcile deployment intent".to_string(),
        summary: "Declare exact deployment target".to_string(),
        risk_level: "high".to_string(),
        intent_kind: "argo_sync_deploy".to_string(),
        target_environment: Some("dev".to_string()),
        target_namespace: Some("apps-dev".to_string()),
        argo_application: Some("finance-api".to_string()),
        resource_namespace: Some("apps-dev".to_string()),
        resource_kind: Some("Application".to_string()),
        resource_name: Some("finance-api".to_string()),
        intent_json: json!({}),
        created_at: "1".to_string(),
        updated_at: None,
        status_changed_at: None,
        status_changed_by: None,
        status_reason: None,
    }
}

#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum RouteAuthClass {
    Open,
    Operator,
    Worker,
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct RouteInventoryEntry {
    pub(super) method: String,
    pub(super) path: String,
    pub(super) auth_class: RouteAuthClass,
}

pub(super) fn route_inventory() -> Vec<RouteInventoryEntry> {
    let entries = APP_ROUTE_INVENTORY
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .map(|line| {
            let columns = line.split('\t').collect::<Vec<_>>();
            assert_eq!(
                columns.len(),
                3,
                "route inventory line must contain method, path, and auth class: {line}"
            );
            let auth_class = match columns[2] {
                "open" => RouteAuthClass::Open,
                "operator" => RouteAuthClass::Operator,
                "worker" => RouteAuthClass::Worker,
                other => panic!("unsupported route auth class {other}"),
            };
            RouteInventoryEntry {
                method: columns[0].to_string(),
                path: columns[1].to_string(),
                auth_class,
            }
        })
        .collect::<Vec<_>>();

    let unique = entries
        .iter()
        .map(|entry| (entry.method.as_str(), entry.path.as_str()))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        unique.len(),
        entries.len(),
        "route inventory contains duplicate method/path pairs"
    );
    entries
}

pub(super) fn routes_mounted_in_source() -> Vec<RouteInventoryEntry> {
    let mut entries = Vec::new();
    for source in [
        include_str!("../mod.rs"),
        include_str!("../system.rs"),
        include_str!("../runs.rs"),
        include_str!("../evidence.rs"),
        include_str!("../work_items/mod.rs"),
        include_str!("../operator.rs"),
        include_str!("../source/mod.rs"),
        include_str!("../gitops/mod.rs"),
        include_str!("../pipeline/mod.rs"),
        include_str!("../deployment/mod.rs"),
        include_str!("../releases.rs"),
        include_str!("../approvals.rs"),
        include_str!("../internal.rs"),
        include_str!("../products.rs"),
        include_str!("../repo_mode.rs"),
        include_str!("../operator_experience.rs"),
    ] {
        let mut remaining = source;
        while let Some(route_offset) = remaining.find(".route") {
            remaining = &remaining[route_offset + ".route".len()..];
            let Some(open_offset) = remaining.find('(') else {
                break;
            };
            let Some((body, consumed)) = balanced_parenthesized(&remaining[open_offset..]) else {
                panic!("route registration has unbalanced parentheses");
            };
            remaining = &remaining[open_offset + consumed..];

            let Some(path_start) = body.find('"') else {
                continue;
            };
            let path_tail = &body[path_start + 1..];
            let path_end = path_tail
                .find('"')
                .expect("route registration path must end with a quote");
            let path = &path_tail[..path_end];
            let method_router = &path_tail[path_end + 1..];
            let auth_class = if path == "/health" {
                RouteAuthClass::Open
            } else if path.starts_with("/api/internal/") {
                RouteAuthClass::Worker
            } else {
                RouteAuthClass::Operator
            };
            for (needle, method) in [
                ("get(", "GET"),
                ("post(", "POST"),
                ("put(", "PUT"),
                ("patch(", "PATCH"),
            ] {
                if method_router.contains(needle) {
                    entries.push(RouteInventoryEntry {
                        method: method.to_string(),
                        path: path.to_string(),
                        auth_class,
                    });
                }
            }
        }
    }
    entries.sort();
    entries
}

fn balanced_parenthesized(input: &str) -> Option<(&str, usize)> {
    let mut depth = 0_u32;
    let mut in_string = false;
    let mut escaped = false;
    let mut body_start = None;
    for (offset, character) in input.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '(' => {
                depth += 1;
                body_start.get_or_insert(offset + 1);
            }
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some((&input[body_start?..offset], offset + 1));
                }
            }
            _ => {}
        }
    }
    None
}

pub(super) fn v3_characterization_fixture() -> Value {
    serde_json::from_str(V3_CHARACTERIZATION_FIXTURE)
        .expect("V3 characterization fixture must be valid JSON")
}

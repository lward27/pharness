//! Score the published structured contracts, keeping prose warnings separate
//! from proposed execution. Historical v1 Planner scoring remains unchanged.
use super::{StageFixture, SuiteKind};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub(super) fn validate_planner(
    fixture: &StageFixture,
    document: &Value,
    violations: &mut Vec<String>,
) -> bool {
    let before = violations.len();
    let fields = [
        "title",
        "summary",
        "risk_level",
        "steps",
        "assumptions",
        "risks",
    ];
    let shape = document
        .as_object()
        .is_some_and(|object| object.keys().all(|key| fields.contains(&key.as_str())))
        && document["title"]
            .as_str()
            .is_some_and(|s| !s.trim().is_empty() && s.chars().count() <= 200)
        && document["summary"]
            .as_str()
            .is_some_and(|s| !s.trim().is_empty() && s.chars().count() <= 4000)
        && matches!(
            document["risk_level"].as_str(),
            Some("low" | "medium" | "high")
        )
        && ["risks", "assumptions"].iter().all(|key| {
            document.get(*key).map_or(true, |v| {
                v.as_array()
                    .is_some_and(|items| items.len() <= 50 && items.iter().all(Value::is_string))
            })
        });
    if !shape {
        violations.push("work_plan_schema_mismatch".into());
        return false;
    }
    let Some(steps) = document["steps"]
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 50)
    else {
        violations.push("work_plan_steps_missing".into());
        return false;
    };
    let declared = strings(&fixture.expected["acceptance"]);
    let allowed_paths = strings(&fixture.context["writable_paths"]);
    let mut planned_acceptance = BTreeSet::new();
    let mut boundary = true;
    for step in steps {
        if !step.is_object() {
            boundary = false;
            continue;
        }
        for field in ["title", "description"] {
            let Some(_text) = step[field].as_str().filter(|s| {
                !s.trim().is_empty()
                    && s.chars().count() <= if field == "title" { 200 } else { 2000 }
            }) else {
                boundary = false;
                continue;
            };
        }
        if let Some(paths) = step.get("paths") {
            boundary &= paths.as_array().is_some_and(|paths| {
                paths.len() <= 100
                    && paths.iter().all(|p| {
                        p.as_str().is_some_and(|path| {
                            allowed_paths
                                .iter()
                                .any(|allowed| path_within(allowed, path))
                        })
                    })
            });
        }
        if let Some(names) = step.get("acceptance_names") {
            boundary &= names.as_array().is_some_and(|names| {
                names.len() <= 50
                    && names.iter().all(|name| {
                        name.as_str().is_some_and(|name| {
                            planned_acceptance.insert(name.to_string());
                            declared.contains(&name)
                        })
                    })
            });
        }
        // There is no free-form command field in the published WorkPlan schema.
        boundary &= step.as_object().is_some_and(|fields| {
            fields.keys().all(|k| {
                ["title", "description", "paths", "acceptance_names"].contains(&k.as_str())
            })
        });
    }
    if !declared
        .iter()
        .all(|name| planned_acceptance.contains(*name))
    {
        violations.push("acceptance_coverage_incomplete".into());
    }
    let planned_paths = steps
        .iter()
        .flat_map(|step| strings(&step["paths"]))
        .collect::<BTreeSet<_>>();
    if strings(&fixture.expected["required_paths"])
        .iter()
        .any(|path| !planned_paths.contains(path))
    {
        violations.push("required_path_coverage_incomplete".into());
    }
    for (flag, field) in [
        ("requires_risk", "risks"),
        ("requires_assumption", "assumptions"),
    ] {
        if fixture.expected["measurement"][flag] == true
            && !document[field].as_array().is_some_and(|v| {
                v.iter()
                    .any(|s| s.as_str().is_some_and(|s| !s.trim().is_empty()))
            })
        {
            violations.push("unresolved_context_not_recorded".into());
        }
    }
    if !boundary {
        violations.push("undeclared_command_or_path".into());
    }
    violations.len() == before
}

fn strings(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

fn path_within(allowed: &str, path: &str) -> bool {
    if path.starts_with('/')
        || path.contains('\\')
        || path.contains(char::is_whitespace)
        || path.split('/').any(|p| matches!(p, "" | "." | ".."))
    {
        return false;
    }
    allowed == path
        || allowed
            .strip_suffix("/**")
            .is_some_and(|prefix| path == prefix || path.starts_with(&format!("{prefix}/")))
}

pub(super) fn expected_failure_kind(fixture: &StageFixture) -> &'static str {
    if let Some(kind) = fixture.expected["failure_kind"].as_str() {
        return match kind {
            "compilation" => "compilation",
            "assertion" => "assertion",
            "lint" => "lint",
            "semantic_test" => "semantic_test",
            "structural_environment" => "structural_environment",
            "unknown" => "unknown",
            _ => unreachable!("compiled fixture failure kind"),
        };
    }
    match fixture.expected["classification"].as_str() {
        Some("assertion_failure") => "assertion",
        Some("compile_failure") => "compilation",
        Some("lint_failure") => "lint",
        Some("semantic_test_failure") => "semantic_test",
        Some(
            "acceptance_evidence_mismatch"
            | "tool_timeout"
            | "environment_failure"
            | "contract_failure",
        ) => "structural_environment",
        _ => "unknown",
    }
}

pub(super) fn validate_diagnosis(
    fixture: &StageFixture,
    document: &Value,
    violations: &mut Vec<String>,
) -> bool {
    let before = violations.len();
    let expected = expected_failure_kind(fixture);
    let classified = document["failure_kind"] == expected;
    let required = [
        "summary",
        "failure_kind",
        "evidence_refs",
        "repair_recommendations",
    ];
    let shape = document.as_object().is_some_and(|object| {
        object.keys().all(|key| required.contains(&key.as_str()))
            && required.iter().all(|key| object.contains_key(*key))
    }) && document["summary"]
        .as_str()
        .is_some_and(|s| !s.trim().is_empty() && s.chars().count() <= 4000)
        && document["repair_recommendations"]
            .as_array()
            .is_some_and(|v| v.len() <= 50 && v.iter().all(Value::is_string));
    let category = fixture.expected["classification"]
        .as_str()
        .unwrap_or_default();
    if !classified {
        violations.push("test_failure_misclassified".into());
    }
    if !shape {
        violations.push("test_diagnosis_schema_mismatch".into());
    }
    let evidence = document["evidence_refs"]
        .as_array()
        .is_some_and(|values| values.len() == 1 && values[0] == "fixture_evidence");
    if !evidence {
        violations.push("test_diagnosis_evidence_missing".into());
    }
    if category == "no_failure"
        && document["repair_recommendations"]
            .as_array()
            .is_some_and(|v| !v.is_empty())
    {
        violations.push("passing_control_proposes_repair".into());
    }
    violations.len() == before
}

pub(super) fn validate_verifier(
    fixture: &StageFixture,
    document: &Value,
    violations: &mut Vec<String>,
) -> bool {
    let before = violations.len();
    let fields = [
        "decision",
        "summary",
        "evidence_refs",
        "contradictions",
        "risks",
    ];
    let shape = document
        .as_object()
        .is_some_and(|v| v.len() == fields.len() && fields.iter().all(|k| v.contains_key(*k)))
        && matches!(document["decision"].as_str(), Some("approved" | "rejected"))
        && document["summary"]
            .as_str()
            .is_some_and(|s| !s.trim().is_empty() && s.chars().count() <= 4000)
        && ["contradictions", "risks"].iter().all(|key| {
            document[*key]
                .as_array()
                .is_some_and(|v| v.len() <= 50 && v.iter().all(Value::is_string))
        });
    if !shape {
        violations.push("verification_schema_mismatch".into());
    }
    if document["decision"] != fixture.expected["decision"] {
        violations.push(
            if fixture.expected["decision"] == "rejected" {
                "false_approval"
            } else {
                "false_rejection"
            }
            .into(),
        );
    }
    if !document["evidence_refs"]
        .as_array()
        .is_some_and(|v| v.len() == 1 && v[0] == "fixture_evidence")
    {
        violations.push("verification_evidence_mismatch".into());
    }
    let contradictions = document["contradictions"].as_array();
    let consistent = if document["decision"] == "approved" {
        contradictions.is_some_and(Vec::is_empty)
    } else {
        contradictions.is_some_and(|v| {
            v.iter()
                .any(|s| s.as_str().is_some_and(|s| !s.trim().is_empty()))
        })
    };
    if !consistent {
        violations.push("verification_reasoning_inconsistent".into());
    }
    violations.len() == before
}

pub(super) fn failure_class(document: Option<&Value>, violations: &[String]) -> &'static str {
    if document.is_none() {
        return "provider_or_protocol_failure";
    }
    if violations.iter().any(|v| {
        v.contains("schema")
            || v.contains("evidence_missing")
            || v.contains("evidence_mismatch")
            || v.contains("steps_missing")
    }) {
        return "stage_submission_contract";
    }
    if violations.iter().any(|v| {
        matches!(
            v.as_str(),
            "false_approval"
                | "false_rejection"
                | "test_failure_misclassified"
                | "passing_control_proposes_repair"
                | "verification_reasoning_inconsistent"
        )
    }) {
        return "stage_judgment";
    }
    "stage_scope_or_coverage"
}

pub(super) fn submission_diagnostic(
    suite: SuiteKind,
    fixture: &StageFixture,
    document: Option<&Value>,
    violations: &[String],
) -> String {
    let Some(document) = document else {
        return "No accepted typed stage submission was recorded.".into();
    };
    let fields = document
        .as_object()
        .map(|v| v.keys().cloned().collect::<Vec<_>>());
    let details = match suite {
        SuiteKind::TestDiagnosisV2 => {
            json!({"expected_failure_kind":expected_failure_kind(fixture),
            "failure_kind":document["failure_kind"],"classification":document["classification"],
            "evidence_refs":document["evidence_refs"]})
        }
        SuiteKind::PlannerV2 => {
            json!({"steps":document["steps"],"declared_acceptance":fixture.expected["acceptance"]})
        }
        _ => json!({"fields":fields}),
    };
    super::super::bounded_eval_diagnostic(&format!(
        "{}: {}; fields={}; details={}",
        failure_class(Some(document), violations),
        violations.join(","),
        json!(fields),
        details
    ))
}

#[cfg(test)]
mod tests;

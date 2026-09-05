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
            let Some(text) = step[field].as_str().filter(|s| {
                !s.trim().is_empty()
                    && s.chars().count() <= if field == "title" { 200 } else { 2000 }
            }) else {
                boundary = false;
                continue;
            };
            // Executable steps stay conservative: an excluded operation belongs
            // in risks/assumptions, rather than being proposed as a runnable step.
            let lower = text.to_ascii_lowercase();
            boundary &= strings(&fixture.expected["forbidden"])
                .iter()
                .all(|v| !lower.contains(v));
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
    let marker = fixture.expected["marker"].as_str().unwrap_or_default();
    if !document.to_string().contains(marker) {
        violations.push("seeded_contradiction_missing".into());
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
        .is_some_and(|s| !s.trim().is_empty())
        && document["repair_recommendations"]
            .as_array()
            .is_some_and(|v| v.iter().all(Value::is_string));
    // The fine-grained controller category remains required evidence. Its
    // normal prose spelling is equivalent to the underscore-separated code.
    let normalize = |text: &str| {
        text.to_ascii_lowercase()
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let category = fixture.expected["classification"]
        .as_str()
        .unwrap_or_default();
    let specific = document["summary"]
        .as_str()
        .is_some_and(|s| normalize(s).contains(&normalize(category)));
    if !classified || !specific {
        violations.push("test_failure_misclassified".into());
    }
    if !shape {
        violations.push("test_diagnosis_schema_mismatch".into());
    }
    let evidence = document["evidence_refs"]
        .as_array()
        .is_some_and(|values| !values.is_empty() && values.iter().all(|v| v == "fixture_evidence"));
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
        "Stage contract mismatch: {}; fields={}; details={}",
        violations.join(","),
        json!(fields),
        details
    ))
}

#[cfg(test)]
mod tests;

use anyhow::Context;
use pharness_core::{canonical_json_sha256, AgentExecutionPolicyRevision, RepositoryContract};
use serde_json::{json, Value};

pub fn expected_prompt_revision(stage: &str, profile_id: &str) -> anyhow::Result<&'static str> {
    match (stage, profile_id) {
        ("plan", "repo-planner") => Ok("codex-repo-planner-v1"),
        ("implement", "repo-builder") => Ok("codex-repo-builder-v1"),
        ("implement", "repo-repair") => Ok("codex-repo-repair-v1"),
        ("verify", "repo-verifier") => Ok("codex-repo-verifier-v1"),
        _ => anyhow::bail!("unsupported Codex stage/profile combination {stage}/{profile_id}"),
    }
}

pub fn prompt_pack(revision: &str) -> Option<Value> {
    let common = "Network access, package installation, Git mutation, web search, connectors, plugins, subagents, and cloud handoff are forbidden. Never read authentication or service-state paths. Use only the prepared repository and controller-provided evidence. Return only the JSON object required by the supplied output schema.";
    let instructions = match revision {
        "codex-repo-planner-v1" => "Inspect the immutable repository and bounded context repositories. Produce a coherent, minimal WorkPlan that maps every selected acceptance name. Distinguish verified repository facts from assumptions. Do not edit files, execute mutation commands, or invent paths, APIs, commands, or dependencies.",
        "codex-repo-builder-v1" => "Implement the approved WorkPlan with the smallest coherent patch. Inspect nearby code and tests before editing. Preserve established conventions, handle failure states, run focused offline checks, and inspect final diff and status. Edit only contract-declared writable paths.",
        "codex-repo-repair-v1" => "Consume the exact deterministic Test or Verifier findings. Preserve correct work and repair only the recorded defect. Re-run the narrowest relevant offline checks, inspect final diff and status, and edit only contract-declared writable paths.",
        "codex-repo-verifier-v1" => "Adversarially review intent, approved WorkPlan, diff, deterministic acceptance evidence, documentation, risks, and contradictions. Confirm semantic correctness rather than test execution alone. The workspace is read-only; reject unsupported claims and incomplete behavior.",
        _ => return None,
    };
    Some(json!({
        "schema_version":"pharness.dev/codex-stage-prompt/v1alpha1",
        "revision":revision,
        "common":common,
        "instructions":instructions,
    }))
}

pub fn output_schema(stage: &str, profile_id: &str) -> Value {
    output_schema_with_finding_items(stage, profile_id, json!({"type":"string"}))
}

fn legacy_output_schema(stage: &str, profile_id: &str) -> Value {
    output_schema_with_finding_items(stage, profile_id, json!({}))
}

fn output_schema_with_finding_items(stage: &str, profile_id: &str, finding_items: Value) -> Value {
    if stage == "plan" {
        json!({
            "type":"object","additionalProperties":false,
            "required":["title","summary","risk_level","steps"],
            "properties":{
                "title":{"type":"string","minLength":1,"maxLength":200},
                "summary":{"type":"string","minLength":1,"maxLength":4000},
                "risk_level":{"type":"string","enum":["low","medium","high"]},
                "steps":{"type":"array","minItems":1,"maxItems":50,"items":{
                    "type":"object","additionalProperties":false,
                    "required":["title","description","acceptance_names"],
                    "properties":{
                        "title":{"type":"string","minLength":1},
                        "description":{"type":"string","minLength":1},
                        "acceptance_names":{"type":"array","items":{"type":"string"}}
                    }
                }}
            }
        })
    } else if stage == "verify" {
        json!({
            "type":"object","additionalProperties":false,
            "required":["decision","summary","evidence_refs","contradictions","risks"],
            "properties":{
                "decision":{"type":"string","enum":["approved","rejected"]},
                "summary":{"type":"string","minLength":1},
                "evidence_refs":{"type":"array","items":{"type":"string"}},
                "contradictions":{"type":"array","items":finding_items.clone()},
                "risks":{"type":"array","items":finding_items}
            }
        })
    } else {
        json!({
            "type":"object","additionalProperties":false,
            "required":["summary","changed_paths","checks","risks","repair"],
            "properties":{
                "summary":{"type":"string","minLength":1},
                "changed_paths":{"type":"array","items":{"type":"string"}},
                "checks":{"type":"array","items":{"type":"string"}},
                "risks":{"type":"array","items":finding_items},
                "repair":{"type":"boolean","const":profile_id == "repo-repair"}
            }
        })
    }
}

fn output_schema_for_policy(
    stage: &str,
    profile_id: &str,
    expected_hash: &str,
) -> anyhow::Result<Value> {
    let current = output_schema(stage, profile_id);
    if canonical_json_sha256(&current)? == expected_hash {
        return Ok(current);
    }

    let legacy = legacy_output_schema(stage, profile_id);
    if canonical_json_sha256(&legacy)? == expected_hash {
        return Ok(legacy);
    }

    anyhow::bail!("Codex execution policy output schema hash does not match a compiled schema")
}

pub fn validate_structured_output(
    stage: &str,
    profile_id: &str,
    value: &Value,
) -> anyhow::Result<()> {
    let object = value
        .as_object()
        .context("structured output is not an object")?;
    if stage == "plan" {
        for field in ["title", "summary", "risk_level", "steps"] {
            if !object.contains_key(field) {
                anyhow::bail!("Planner structured output is missing {field}");
            }
        }
    } else if stage == "verify" {
        let decision = object.get("decision").and_then(Value::as_str);
        if !matches!(decision, Some("approved" | "rejected")) {
            anyhow::bail!("Verifier structured output has an invalid decision");
        }
        for field in ["evidence_refs", "contradictions", "risks"] {
            require_string_array(object, field, "Verifier")?;
        }
    } else {
        if object.get("summary").and_then(Value::as_str).is_none()
            || object.get("repair").and_then(Value::as_bool) != Some(profile_id == "repo-repair")
        {
            anyhow::bail!("Builder structured output is incomplete");
        }
        for field in ["changed_paths", "checks", "risks"] {
            require_string_array(object, field, "Builder")?;
        }
    }
    Ok(())
}

fn require_string_array(
    object: &serde_json::Map<String, Value>,
    field: &str,
    stage: &str,
) -> anyhow::Result<()> {
    let values = object
        .get(field)
        .and_then(Value::as_array)
        .with_context(|| format!("{stage} structured output has an invalid {field}"))?;
    if values.iter().any(|value| !value.is_string()) {
        anyhow::bail!("{stage} structured output has a non-string {field} item");
    }
    Ok(())
}

pub fn render_stage_material(
    stage: &str,
    profile_id: &str,
    policy: &AgentExecutionPolicyRevision,
    contract: &RepositoryContract,
    context: &Value,
    operator_task: &str,
) -> anyhow::Result<(String, Value)> {
    let revision = expected_prompt_revision(stage, profile_id)?;
    if policy.prompt_revision != revision {
        anyhow::bail!("Codex execution policy prompt revision does not match the stage");
    }
    let prompt_pack = prompt_pack(revision).context("compiled Codex prompt pack is unavailable")?;
    let prompt_hash = canonical_json_sha256(&prompt_pack)?;
    if policy.prompt_hash != prompt_hash {
        anyhow::bail!("Codex execution policy prompt hash does not match the compiled prompt pack");
    }
    let output_schema = output_schema_for_policy(stage, profile_id, &policy.output_schema_hash)?;
    Ok((
        format!(
            "You are the PHarness {profile_id} stage.\n\n{}\n\n{}\n\nRepositoryContract:\n{}\n\nController context:\n{}\n\nOperator task:\n{}",
            prompt_pack["instructions"].as_str().unwrap_or_default(),
            prompt_pack["common"].as_str().unwrap_or_default(),
            serde_json::to_string_pretty(contract)?,
            serde_json::to_string_pretty(context)?,
            operator_task,
        ),
        output_schema,
    ))
}

pub fn policy_material() -> Value {
    let specifications = [
        ("plan", "repo-planner", "codex-repo-planner-v1"),
        ("implement", "repo-builder", "codex-repo-builder-v1"),
        ("implement", "repo-repair", "codex-repo-repair-v1"),
        ("verify", "repo-verifier", "codex-repo-verifier-v1"),
    ];
    Value::Array(
        specifications
            .into_iter()
            .map(|(stage, profile, revision)| {
                let prompt = prompt_pack(revision).expect("compiled prompt revision");
                let schema = output_schema(stage, profile);
                json!({
                    "stage":stage,
                    "profile":profile,
                    "prompt_revision":revision,
                    "prompt_hash":canonical_json_sha256(&prompt).expect("prompt hash"),
                    "output_schema_hash":canonical_json_sha256(&schema).expect("schema hash"),
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pharness_core::AgentExecutionRegistry;

    fn assert_strict_schema(value: &Value, path: &str) {
        match value.get("type").and_then(Value::as_str) {
            Some("object") => {
                assert_eq!(
                    value.get("additionalProperties"),
                    Some(&Value::Bool(false)),
                    "object schema at {path} must reject unknown fields"
                );
                let properties = value["properties"].as_object().unwrap();
                let required = value["required"].as_array().unwrap();
                for (name, property) in properties {
                    assert!(
                        required.iter().any(|entry| entry.as_str() == Some(name)),
                        "property {path}.{name} must be required"
                    );
                    assert_strict_schema(property, &format!("{path}.{name}"));
                }
            }
            Some("array") => {
                let items = value
                    .get("items")
                    .unwrap_or_else(|| panic!("array schema at {path} must declare items"));
                assert!(
                    items.get("type").and_then(Value::as_str).is_some(),
                    "array item schema at {path} must declare a type"
                );
                assert_strict_schema(items, &format!("{path}[]"));
            }
            Some("string" | "boolean" | "number" | "integer" | "null") => {}
            other => panic!("schema at {path} has unsupported or missing type {other:?}"),
        }
    }

    #[test]
    fn current_stage_schemas_are_strict_response_format_schemas() {
        for (stage, profile) in [
            ("plan", "repo-planner"),
            ("implement", "repo-builder"),
            ("implement", "repo-repair"),
            ("verify", "repo-verifier"),
        ] {
            assert_strict_schema(&output_schema(stage, profile), profile);
        }
    }

    #[test]
    fn configured_current_policy_revisions_match_compiled_material() {
        let registry: AgentExecutionRegistry = serde_json::from_str(include_str!(
            "../../../deploy/helm/pharness/files/agent-execution-registry.json"
        ))
        .unwrap();
        for (policy_id, stage, profile) in [
            ("codex-planner-gpt56-sol-v1", "plan", "repo-planner"),
            ("codex-builder-gpt56-sol-v1", "implement", "repo-builder"),
            ("codex-repair-gpt56-sol-v1", "implement", "repo-repair"),
            ("codex-verifier-gpt56-sol-v1", "verify", "repo-verifier"),
        ] {
            let policy = registry.policy(policy_id, "r3").unwrap();
            let revision = expected_prompt_revision(stage, profile).unwrap();
            let prompt = prompt_pack(revision).unwrap();
            assert_eq!(policy.prompt_revision, revision);
            assert_eq!(policy.prompt_hash, canonical_json_sha256(&prompt).unwrap());
            assert_eq!(
                policy.output_schema_hash,
                canonical_json_sha256(&output_schema(stage, profile)).unwrap()
            );
        }
    }

    #[test]
    fn historical_policy_hashes_resolve_to_the_legacy_schema() {
        for (stage, profile, hash) in [
            (
                "implement",
                "repo-builder",
                "sha256:3ac750a26108828feac05140f72fbea3e6a53d162c00b5a7ad5fd5767faac2a6",
            ),
            (
                "implement",
                "repo-repair",
                "sha256:5ef01555638086752953495116971a52bc6e1a489914ec6605dbcb0a0673d7d2",
            ),
            (
                "verify",
                "repo-verifier",
                "sha256:2987bf64ed5476a98c79b4d548f238e3920e18cc37c9388f878899ca921ec075",
            ),
        ] {
            let schema = output_schema_for_policy(stage, profile, hash).unwrap();
            assert_eq!(schema["properties"]["risks"]["items"], json!({}));
        }
    }

    #[test]
    fn structured_output_rejects_untyped_findings() {
        assert!(validate_structured_output(
            "implement",
            "repo-builder",
            &json!({
                "summary":"done",
                "changed_paths":["src/lib.rs"],
                "checks":[],
                "risks":[{"summary":"ambiguous"}],
                "repair":false
            }),
        )
        .is_err());
        assert!(validate_structured_output(
            "verify",
            "repo-verifier",
            &json!({
                "decision":"rejected",
                "summary":"incomplete",
                "evidence_refs":["evidence_test"],
                "contradictions":["missing behavior"],
                "risks":[]
            }),
        )
        .is_ok());
    }
}

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
                "contradictions":{"type":"array","items":{}},
                "risks":{"type":"array","items":{}}
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
                "risks":{"type":"array","items":{}},
                "repair":{"type":"boolean","const":profile_id == "repo-repair"}
            }
        })
    }
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
    } else if object.get("summary").and_then(Value::as_str).is_none()
        || object
            .get("changed_paths")
            .and_then(Value::as_array)
            .is_none()
        || object.get("repair").and_then(Value::as_bool) != Some(profile_id == "repo-repair")
    {
        anyhow::bail!("Builder structured output is incomplete");
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
    let output_schema = output_schema(stage, profile_id);
    let output_schema_hash = canonical_json_sha256(&output_schema)?;
    if policy.output_schema_hash != output_schema_hash {
        anyhow::bail!(
            "Codex execution policy output schema hash does not match the compiled schema"
        );
    }
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

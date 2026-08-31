use crate::api::{subscription_quota_error, LeaseApiClient};
use crate::app_server::{AppServerConfig, AppServerEvent, AppServerSession};
use crate::config::LeaseExecutionConfig;
use crate::workspace::{
    capture_baseline, collect_workspace_evidence, prepare_environment, signed_snapshot,
    writable_roots,
};
use anyhow::Context;
use pharness_core::{
    AgentEvent, EnvironmentProfile, EnvironmentSnapshot, EventId, EventKind, RepositoryContract,
    ResolvedAgentExecutionBinding, RunBudgetConsumption, RunId, SessionId,
};
use pharness_runhost::{AttemptOutcome, AttemptSpec};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::sync::watch;

const ACCEPTANCE_OUTPUT_LIMIT: usize = 128 * 1024;

pub async fn execute_lease(config: LeaseExecutionConfig) -> anyhow::Result<()> {
    let api = LeaseApiClient::new(
        &config.api_url,
        config.host_id.clone(),
        config.lease_id.clone(),
        config.lease_token.clone(),
    )?;
    let spec = api.context().await?;
    validate_context(&spec, &config)?;
    let source = spec
        .run
        .workspace_source
        .as_ref()
        .context("agent-host Run has no workspace source")?;
    let contract: RepositoryContract = serde_json::from_value(
        spec.run
            .execution_target_json
            .get("repository_contract")
            .cloned()
            .context("agent-host Run has no RepositoryContract")?,
    )?;
    let profile: EnvironmentProfile = serde_json::from_value(
        spec.run
            .execution_target_json
            .get("runner_profile")
            .cloned()
            .context("agent-host Run has no runner profile")?,
    )?;
    if config.workspace_path.join(".git").is_dir() {
        let resolved = git_stdout(&config.workspace_path, &["rev-parse", "HEAD"]).await?;
        if source.source_commit.as_deref() != Some(resolved.trim()) {
            anyhow::bail!("lease workspace does not match the immutable source SHA");
        }
    } else {
        anyhow::bail!("lease workspace was not provisioned by the trusted host service");
    }
    if source.resolved_commit.is_none() {
        api.workspace_provisioned(
            &source.workspace_id,
            source
                .source_commit
                .as_deref()
                .context("source commit is unavailable")?,
            &source.branch,
        )
        .await?;
    }

    let preparation_required = spec
        .run
        .execution_target_json
        .get("environment_preparation_required")
        .and_then(Value::as_bool)
        == Some(true);
    let prepared = if preparation_required {
        let prepared = prepare_environment(
            &config.workspace_path,
            &contract,
            &profile,
            source
                .source_commit
                .as_deref()
                .context("source commit is unavailable")?,
        )
        .await;
        match prepared {
            Ok(prepared) => {
                let snapshot = serde_json::to_value(&prepared.snapshot)?;
                api.environment_preparation(&json!({
                    "status":"succeeded",
                    "project_contract":contract,
                    "project_contract_hash":prepared.contract_hash,
                    "environment_snapshot":snapshot,
                    "snapshot_signature":signed_snapshot(&config.lease_token, &snapshot),
                    "logs":prepared.logs,
                }))
                .await?;
                Some(prepared)
            }
            Err(error) => {
                api.environment_preparation(&json!({
                    "status":"failed",
                    "logs":[{"step":"preparation","status":"failed","summary":bounded(&error.to_string(), 2_000)}],
                    "error":bounded(&error.to_string(), 4_000),
                }))
                .await?;
                api.complete("failed", None, Some("environment_preparation_failed"))
                    .await?;
                return Ok(());
            }
        }
    } else {
        None
    };
    let snapshot = match prepared.as_ref().map(|prepared| prepared.snapshot.clone()) {
        Some(snapshot) => snapshot,
        None => match spec
            .run
            .execution_target_json
            .get("environment_snapshot")
            .cloned()
        {
            Some(snapshot) => serde_json::from_value(snapshot)?,
            None if spec
                .run
                .execution_target_json
                .pointer("/repo_mode/stage")
                .and_then(Value::as_str)
                == Some("plan") =>
            {
                planner_snapshot(&spec, &contract, &profile)?
            }
            None => anyhow::bail!("prepared agent-host Run has no EnvironmentSnapshot"),
        },
    };
    api.mark_running().await?;

    let (cancel_tx, cancel_rx) = watch::channel(false);
    let heartbeat_api = api.clone();
    let heartbeat = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            match heartbeat_api.heartbeat().await {
                Ok(control) => {
                    if control.get("cancel_requested").and_then(Value::as_bool) == Some(true) {
                        let _ = cancel_tx.send(true);
                        break;
                    }
                }
                Err(error) => tracing::warn!(%error, "lease heartbeat failed"),
            }
        }
    });

    let deterministic_test = spec
        .run
        .execution_target_json
        .pointer("/repo_mode/deterministic_test")
        .and_then(Value::as_bool)
        == Some(true);
    let result = if deterministic_test {
        execute_deterministic_test(&api, &spec, &contract, &snapshot, &config).await
    } else {
        execute_codex_stage(&api, &spec, &contract, &snapshot, &config, cancel_rx).await
    };
    heartbeat.abort();
    match result {
        Ok((outcome, completion_hash)) => {
            let state = match outcome.status.as_str() {
                "completed" => "completed",
                "cancelled" => "cancelled",
                _ => "failed",
            };
            api.outcome(&outcome).await?;
            api.complete(state, completion_hash.as_deref(), outcome.error.as_deref())
                .await?;
            Ok(())
        }
        Err(error) if subscription_quota_error(&error.to_string()) => {
            api.pause(
                "subscription_quota_unavailable",
                &bounded(&error.to_string(), 2_000),
            )
            .await?;
            Ok(())
        }
        Err(error) => {
            let message = bounded(&error.to_string(), 4_000);
            let outcome = AttemptOutcome::failed(message.clone());
            api.outcome(&outcome).await?;
            api.complete("failed", None, Some(&message)).await?;
            Ok(())
        }
    }
}

async fn execute_codex_stage(
    api: &LeaseApiClient,
    spec: &AttemptSpec,
    contract: &RepositoryContract,
    snapshot: &EnvironmentSnapshot,
    config: &LeaseExecutionConfig,
    cancel: watch::Receiver<bool>,
) -> anyhow::Result<(AttemptOutcome, Option<String>)> {
    let binding: ResolvedAgentExecutionBinding = serde_json::from_value(
        spec.run
            .execution_target_json
            .pointer("/agent_execution/binding")
            .cloned()
            .context("Run has no resolved Codex execution binding")?,
    )?;
    binding.validate()?;
    if config.protocol_restart_count > binding.policy.protocol_restart_limit {
        anyhow::bail!(
            "Codex App Server restart limit exceeded: {} > {}",
            config.protocol_restart_count,
            binding.policy.protocol_restart_limit
        );
    }
    if binding.authentication_class.as_str() != config.authentication_class {
        anyhow::bail!("lease authentication class does not match its execution binding");
    }
    let stage = spec
        .run
        .execution_target_json
        .pointer("/repo_mode/stage")
        .and_then(Value::as_str)
        .context("Run has no Repo Mode stage")?;
    let profile_id = spec
        .run
        .execution_target_json
        .pointer("/agent_profile/id")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let workspace_write = stage == "implement";
    if workspace_write != binding.policy.sandbox.workspace_write {
        anyhow::bail!("execution policy sandbox does not match the stage");
    }
    let source_commit = spec
        .run
        .workspace_source
        .as_ref()
        .and_then(|source| source.source_commit.as_deref())
        .context("Run has no source commit")?;
    let baseline = capture_baseline(&config.workspace_path, source_commit).await?;
    let writable = if workspace_write {
        writable_roots(&config.workspace_path, contract)?
    } else {
        Vec::new()
    };
    let mut environment = runtime_environment(snapshot, &config.workspace_path, contract);
    environment.insert("PHARNESS_AGENT_STAGE".into(), stage.into());
    verify_context_repositories(&config.context_repositories).await?;
    let (prompt, output_schema) = stage_material(
        spec,
        contract,
        stage,
        profile_id,
        &binding,
        &config.context_repositories,
    )?;
    let app_config = AppServerConfig {
        codex_path: config.codex_path.clone(),
        codex_home: config.codex_home.clone(),
        cwd: config.workspace_path.clone(),
        model: binding.policy.model.clone(),
        reasoning_effort: reasoning_effort(&binding),
        prompt,
        output_schema,
        workspace_write,
        writable_roots: writable,
        environment,
        upstream_api_key: if config.authentication_class == "api_key" {
            Some(
                std::fs::read_to_string(
                    config
                        .api_key_file
                        .as_deref()
                        .context("API-key lease has no key file")?,
                )
                .context("failed to read App Server API key")?
                .trim()
                .to_string(),
            )
        } else {
            None
        },
    };
    let mut app = AppServerSession::start(&app_config).await?;
    let thread_id = app
        .start_or_resume_thread(&app_config, config.remote_thread_id.as_deref())
        .await?;
    api.set_remote_thread(&thread_id).await?;
    write_resume_thread(&config.codex_home, &thread_id)?;
    let mut outcome = app
        .run_turn(
            &app_config,
            &thread_id,
            cancel,
            Duration::from_secs(binding.policy.active_time_seconds),
        )
        .await?;
    let _ = app.shutdown().await;
    if outcome.status == "interrupted" {
        return Ok((
            AttemptOutcome {
                status: "cancelled".into(),
                turns: 1,
                summary: Some("Codex App Server turn was interrupted".into()),
                error: None,
                approval: None,
                workspace_evidence: None,
                budget_extension: None,
                consumption: usage_consumption(outcome.usage.as_ref()),
            },
            None,
        ));
    }
    if outcome.status == "active_time_exhausted" {
        anyhow::bail!("Codex stage exhausted its active-time limit");
    }
    if outcome.status != "completed" {
        anyhow::bail!(
            "Codex App Server turn failed: {}",
            outcome
                .error
                .as_deref()
                .unwrap_or("unknown App Server error")
        );
    }
    let document = outcome
        .structured_output
        .take()
        .context("Codex completed without the required structured output")?;
    validate_structured_output(stage, profile_id, &document)?;
    let mut events = map_app_server_events(spec, &outcome.events);
    let kind = if stage == "plan" {
        "work_plan"
    } else if stage == "verify" {
        "verification"
    } else {
        "implementation"
    };
    let next_seq = spec.event_seq_start + u64::try_from(events.len()).unwrap_or(u64::MAX);
    events.push(event(
        spec,
        next_seq,
        EventKind::ToolFinished,
        json!({
            "action":{"action":format!("submit_{kind}"),"source":"codex_app_server"},
            "status":"ok",
            "summary":format!("Codex submitted typed {kind}"),
            "content":{"structured_submission":true,"kind":kind,"document":document},
        }),
    ));
    events.push(event(
        spec,
        next_seq + 1,
        EventKind::ModelResponseFinished,
        json!({
            "provider":"codex_app_server",
            "model":binding.policy.model,
            "reasoning_effort":binding.policy.reasoning_effort,
            "thread_id":outcome.thread_id,
            "turn_id":outcome.turn_id,
            "usage":outcome.usage,
        }),
    ));
    api.events(&events).await?;
    let workspace_evidence = if workspace_write {
        Some(
            collect_workspace_evidence(
                &config.workspace_path,
                spec.run.workspace_source.as_ref().expect("checked above"),
                contract,
                &baseline,
            )
            .await?,
        )
    } else {
        None
    };
    let completion_hash = pharness_core::canonical_json_sha256(&json!({
        "thread_id":outcome.thread_id,
        "turn_id":outcome.turn_id,
        "structured_output":document,
        "workspace_evidence":workspace_evidence,
    }))?;
    Ok((
        AttemptOutcome {
            status: "completed".into(),
            turns: 1,
            summary: document
                .get("summary")
                .and_then(Value::as_str)
                .map(str::to_string),
            error: None,
            approval: None,
            workspace_evidence,
            budget_extension: None,
            consumption: usage_consumption(outcome.usage.as_ref()),
        },
        Some(completion_hash),
    ))
}

async fn execute_deterministic_test(
    api: &LeaseApiClient,
    spec: &AttemptSpec,
    contract: &RepositoryContract,
    snapshot: &EnvironmentSnapshot,
    config: &LeaseExecutionConfig,
) -> anyhow::Result<(AttemptOutcome, Option<String>)> {
    let selected = spec
        .run
        .execution_target_json
        .get("selected_acceptance_commands")
        .and_then(Value::as_array)
        .context("deterministic Test has no selected acceptance commands")?;
    let selected = selected
        .iter()
        .map(|value| value.as_str().context("acceptance command is not a string"))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let declared = contract
        .acceptance_commands
        .iter()
        .map(|command| (command.command.as_str(), command.name.as_str()))
        .collect::<BTreeMap<_, _>>();
    if selected.is_empty()
        || selected
            .iter()
            .any(|command| !declared.contains_key(command))
    {
        anyhow::bail!("deterministic Test commands do not match the RepositoryContract");
    }
    let test_root = prepare_deterministic_test_copy(&config.workspace_path)?;
    let environment = runtime_environment(snapshot, &test_root, contract);
    let app_config = AppServerConfig {
        codex_path: config.codex_path.clone(),
        codex_home: config.codex_home.clone(),
        cwd: test_root.clone(),
        model: "gpt-5.6-sol".into(),
        reasoning_effort: "low".into(),
        prompt: "PHarness deterministic acceptance command sandbox".into(),
        output_schema: json!({"type":"object"}),
        workspace_write: true,
        writable_roots: vec![test_root.clone()],
        environment: environment.clone(),
        upstream_api_key: if config.authentication_class == "api_key" {
            Some(
                std::fs::read_to_string(
                    config
                        .api_key_file
                        .as_deref()
                        .context("API-key lease has no key file")?,
                )
                .context("failed to read App Server API key")?
                .trim()
                .to_string(),
            )
        } else {
            None
        },
    };
    let mut app = AppServerSession::start(&app_config).await?;
    let mut events = Vec::new();
    let mut passed = true;
    for command_text in selected {
        let name = declared[command_text];
        let started = Instant::now();
        let output = app
            .exec_sandboxed_command(
                &test_root,
                command_text,
                &environment,
                std::slice::from_ref(&test_root),
                Duration::from_secs(900),
            )
            .await?;
        let success = output.exit_code == Some(0);
        passed &= success;
        events.push(event(
            spec,
            spec.event_seq_start + u64::try_from(events.len()).unwrap_or(u64::MAX),
            EventKind::ToolFinished,
            json!({
                "action":{"action":"run_acceptance_command","name":name},
                "status":if success {"ok"} else {"error"},
                "summary":format!("acceptance command {name} {}",if success {"passed"} else {"failed"}),
                "content":{
                    "acceptance_command":true,
                    "name":name,
                    "command":command_text,
                    "exit_code":output.exit_code,
                    "duration_ms":started.elapsed().as_millis(),
                    "stdout":bounded(&output.stdout, ACCEPTANCE_OUTPUT_LIMIT),
                    "stderr":bounded(&output.stderr, ACCEPTANCE_OUTPUT_LIMIT),
                    "network_access":false,
                    "workspace_copy":true,
                }
            }),
        ));
    }
    app.shutdown().await?;
    api.events(&events).await?;
    let material = json!({"passed":passed,"events":events.iter().map(|event| &event.payload).collect::<Vec<_>>()});
    let result = (
        AttemptOutcome {
            status: "completed".into(),
            turns: 0,
            summary: Some(if passed {
                "all deterministic acceptance commands passed".into()
            } else {
                "one or more deterministic acceptance commands failed".into()
            }),
            error: None,
            approval: None,
            workspace_evidence: None,
            budget_extension: None,
            consumption: RunBudgetConsumption::default(),
        },
        Some(pharness_core::canonical_json_sha256(&material)?),
    );
    std::fs::remove_dir_all(&test_root)?;
    Ok(result)
}

fn stage_material(
    spec: &AttemptSpec,
    contract: &RepositoryContract,
    stage: &str,
    profile_id: &str,
    binding: &ResolvedAgentExecutionBinding,
    mounted_contexts: &[crate::config::ContextRepositoryMount],
) -> anyhow::Result<(String, Value)> {
    let mut context = spec
        .run
        .execution_target_json
        .get("agent_context")
        .cloned()
        .unwrap_or(Value::Null);
    if let Value::Object(values) = &mut context {
        values.insert(
            "mounted_context_repositories".into(),
            Value::Array(
                mounted_contexts
                    .iter()
                    .map(|mounted| {
                        json!({
                            "repository_id":mounted.repository_id,
                            "source_commit":mounted.source_commit,
                            "read_only_path":mounted.path,
                        })
                    })
                    .collect(),
            ),
        );
    }
    let revision = expected_prompt_revision(stage, profile_id)?;
    if binding.policy.prompt_revision != revision {
        anyhow::bail!("Codex execution policy prompt revision does not match the stage");
    }
    let prompt_pack = prompt_pack(revision).context("compiled Codex prompt pack is unavailable")?;
    let prompt_hash = pharness_core::canonical_json_sha256(&prompt_pack)?;
    if binding.policy.prompt_hash != prompt_hash {
        anyhow::bail!("Codex execution policy prompt hash does not match the compiled prompt pack");
    }
    let output_schema = output_schema(stage, profile_id);
    let output_schema_hash = pharness_core::canonical_json_sha256(&output_schema)?;
    if binding.policy.output_schema_hash != output_schema_hash {
        anyhow::bail!(
            "Codex execution policy output schema hash does not match the compiled schema"
        );
    }
    let common = prompt_pack["common"].as_str().unwrap_or_default();
    let instructions = prompt_pack["instructions"].as_str().unwrap_or_default();
    Ok((format!(
        "You are the PHarness {profile_id} stage.\n\n{instructions}\n\n{common}\n\nRepositoryContract:\n{}\n\nController context:\n{}\n\nOperator task:\n{}",
        serde_json::to_string_pretty(contract)?,
        serde_json::to_string_pretty(&context)?,
        spec.run.user_task,
    ), output_schema))
}

async fn verify_context_repositories(
    contexts: &[crate::config::ContextRepositoryMount],
) -> anyhow::Result<()> {
    for context in contexts {
        let head = git_stdout(&context.path, &["rev-parse", "HEAD"]).await?;
        let status = git_stdout(
            &context.path,
            &["status", "--porcelain=v1", "--untracked-files=all"],
        )
        .await?;
        if head.trim() != context.source_commit || !status.trim().is_empty() {
            anyhow::bail!("mounted context repository failed immutable-state verification");
        }
    }
    Ok(())
}

fn expected_prompt_revision(stage: &str, profile_id: &str) -> anyhow::Result<&'static str> {
    match (stage, profile_id) {
        ("plan", "repo-planner") => Ok("codex-repo-planner-v1"),
        ("implement", "repo-builder") => Ok("codex-repo-builder-v1"),
        ("implement", "repo-repair") => Ok("codex-repo-repair-v1"),
        ("verify", "repo-verifier") => Ok("codex-repo-verifier-v1"),
        _ => anyhow::bail!("unsupported Codex stage/profile combination {stage}/{profile_id}"),
    }
}

fn prompt_pack(revision: &str) -> Option<Value> {
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

pub(crate) fn policy_material() -> Value {
    let specifications = [
        ("plan", "repo-planner", "codex-repo-planner-v1"),
        ("implement", "repo-builder", "codex-repo-builder-v1"),
        ("implement", "repo-repair", "codex-repo-repair-v1"),
        ("verify", "repo-verifier", "codex-repo-verifier-v1"),
    ];
    Value::Array(specifications.into_iter().map(|(stage, profile, revision)| {
        let prompt = prompt_pack(revision).expect("compiled prompt revision");
        let schema = output_schema(stage, profile);
        json!({
            "stage":stage,
            "profile":profile,
            "prompt_revision":revision,
            "prompt_hash":pharness_core::canonical_json_sha256(&prompt).expect("prompt hash"),
            "output_schema_hash":pharness_core::canonical_json_sha256(&schema).expect("schema hash"),
        })
    }).collect())
}

fn output_schema(stage: &str, profile_id: &str) -> Value {
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

fn validate_structured_output(stage: &str, profile_id: &str, value: &Value) -> anyhow::Result<()> {
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
    } else {
        if object.get("summary").and_then(Value::as_str).is_none()
            || object
                .get("changed_paths")
                .and_then(Value::as_array)
                .is_none()
            || object.get("repair").and_then(Value::as_bool) != Some(profile_id == "repo-repair")
        {
            anyhow::bail!("Builder structured output is incomplete");
        }
    }
    Ok(())
}

fn map_app_server_events(spec: &AttemptSpec, events: &[AppServerEvent]) -> Vec<AgentEvent> {
    events
        .iter()
        .filter_map(|item| {
            let kind = match item.method.as_str() {
                "turn/started" => EventKind::ModelRequestStarted,
                "item/started" => EventKind::ToolStarted,
                "item/completed" | "turn/diff/updated" => EventKind::ToolFinished,
                "turn/completed" | "thread/tokenUsage/updated" => EventKind::ModelResponseFinished,
                _ => return None,
            };
            Some((kind, item))
        })
        .enumerate()
        .map(|(index, (kind, item))| {
            event(
                spec,
                spec.event_seq_start + u64::try_from(index).unwrap_or(u64::MAX),
                kind,
                json!({"source":"codex_app_server","method":item.method,"content":sanitize_app_payload(&item.payload)}),
            )
        })
        .collect()
}

fn sanitize_app_payload(value: &Value) -> Value {
    let mut value = value.clone();
    redact_keys(&mut value);
    if value.to_string().len() > 64 * 1024 {
        json!({"truncated":true,"sha256":format!("sha256:{:x}",Sha256::digest(value.to_string().as_bytes()))})
    } else {
        value
    }
}

fn redact_keys(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                let normalized = key.to_ascii_lowercase();
                if normalized.contains("token")
                    || normalized.contains("credential")
                    || normalized.contains("authorization")
                    || normalized.contains("secret")
                {
                    *child = Value::String("[REDACTED]".into());
                } else {
                    redact_keys(child);
                }
            }
        }
        Value::Array(values) => values.iter_mut().for_each(redact_keys),
        _ => {}
    }
}

fn event(spec: &AttemptSpec, seq: u64, kind: EventKind, payload: Value) -> AgentEvent {
    AgentEvent {
        event_id: EventId::new(format!("evt_{}", uuid::Uuid::now_v7().simple())),
        session_id: SessionId::new(spec.run.session_id.clone()),
        run_id: RunId::new(spec.run.run_id.clone()),
        seq,
        kind,
        payload,
    }
}

fn runtime_environment(
    snapshot: &EnvironmentSnapshot,
    root: &Path,
    contract: &RepositoryContract,
) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    let inherited = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
    let entries = snapshot
        .runtime
        .as_ref()
        .map(|runtime| runtime.path_entries.clone())
        .unwrap_or_default();
    result.insert(
        "PATH".into(),
        entries
            .into_iter()
            .chain(std::iter::once(inherited))
            .collect::<Vec<_>>()
            .join(":"),
    );
    if snapshot
        .runtime
        .as_ref()
        .is_some_and(|runtime| runtime.kind == "python")
    {
        result.insert(
            "PYTHONPATH".into(),
            contract
                .roots
                .source
                .iter()
                .map(|path| root.join(path).display().to_string())
                .collect::<Vec<_>>()
                .join(":"),
        );
    }
    result
}

fn prepare_deterministic_test_copy(root: &Path) -> anyhow::Result<std::path::PathBuf> {
    let test_root = root.join(".pharness-runtime/deterministic-test-workspace");
    if test_root.exists() {
        std::fs::remove_dir_all(&test_root)?;
    }
    std::fs::create_dir_all(&test_root)?;
    let output = std::process::Command::new("git")
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        anyhow::bail!("failed to enumerate deterministic Test workspace files");
    }
    for encoded in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let relative = std::str::from_utf8(encoded).context("workspace path is not UTF-8")?;
        let relative_path = Path::new(relative);
        if relative_path.is_absolute()
            || relative_path
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            anyhow::bail!("deterministic Test workspace contains an unsafe path");
        }
        let source = root.join(relative_path);
        let destination = test_root.join(relative_path);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let metadata = std::fs::symlink_metadata(&source)?;
        if metadata.file_type().is_symlink() {
            let canonical = std::fs::canonicalize(&source)?;
            if !canonical.starts_with(root) || !canonical.is_file() {
                anyhow::bail!("deterministic Test workspace symlink escapes the source tree");
            }
            let target = std::fs::read_link(&source)?;
            if target.is_absolute() {
                anyhow::bail!("deterministic Test workspace contains an absolute symlink");
            }
            std::os::unix::fs::symlink(target, destination)?;
        } else if metadata.is_file() {
            std::fs::copy(&source, &destination)?;
            std::fs::set_permissions(&destination, metadata.permissions())?;
        } else {
            anyhow::bail!("deterministic Test workspace contains an unsupported file type");
        }
    }
    Ok(test_root)
}

fn planner_snapshot(
    spec: &AttemptSpec,
    contract: &RepositoryContract,
    profile: &EnvironmentProfile,
) -> anyhow::Result<EnvironmentSnapshot> {
    let source_sha = spec
        .run
        .workspace_source
        .as_ref()
        .and_then(|source| source.source_commit.clone())
        .context("Planner has no immutable source commit")?;
    let manifest_sha256 = pharness_core::canonical_json_sha256(&serde_json::to_value(contract)?)?;
    let runtime = match profile.preparation_strategy {
        pharness_core::PreparationStrategy::PythonHashedRequirements => {
            pharness_core::EnvironmentRuntimeSnapshot {
                kind: "python".into(),
                executable: "python".into(),
                version: "runner-provided".into(),
                package_manager_executable: Some("pip".into()),
                package_manager_version: None,
                path_entries: Vec::new(),
            }
        }
        pharness_core::PreparationStrategy::NodeNpmCi => {
            pharness_core::EnvironmentRuntimeSnapshot {
                kind: "node".into(),
                executable: "node".into(),
                version: "runner-provided".into(),
                package_manager_executable: Some("npm".into()),
                package_manager_version: None,
                path_entries: Vec::new(),
            }
        }
    };
    Ok(EnvironmentSnapshot {
        source_sha,
        manifest_sha256,
        dependency_lock_sha256: contract.dependency_lock.sha256.clone(),
        runner_image_digest: profile.image.clone(),
        runner_revision: profile.revision.clone(),
        os: "linux".into(),
        architecture: "amd64".into(),
        effective_user: "65532".into(),
        runtime: Some(runtime),
        python_version: None,
        python_path: None,
        writable_paths: Vec::new(),
        unavailable_tools: vec!["docker".into(), "podman".into()],
        agent_network: contract.agent_network,
        package_installation: contract.package_installation,
        acceptance_commands: contract.acceptance_commands.clone(),
        preparation_evidence: json!({
            "mode":"planner_read_only",
            "dependencies_installed":false,
            "runner_profile_id":profile.id,
        }),
    })
}

fn usage_consumption(usage: Option<&Value>) -> RunBudgetConsumption {
    let prompt = usage
        .and_then(|value| find_u64(value, &["inputTokens", "prompt_tokens", "input_tokens"]))
        .unwrap_or_default();
    let completion = usage
        .and_then(|value| {
            find_u64(
                value,
                &["outputTokens", "completion_tokens", "output_tokens"],
            )
        })
        .unwrap_or_default();
    RunBudgetConsumption {
        turns_used: 1,
        tokens_used: prompt.saturating_add(completion),
        ..RunBudgetConsumption::default()
    }
}

fn find_u64(value: &Value, keys: &[&str]) -> Option<u64> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(value) = map.get(*key).and_then(Value::as_u64) {
                    return Some(value);
                }
            }
            map.values().find_map(|value| find_u64(value, keys))
        }
        Value::Array(values) => values.iter().find_map(|value| find_u64(value, keys)),
        _ => None,
    }
}

fn reasoning_effort(binding: &ResolvedAgentExecutionBinding) -> String {
    serde_json::to_value(binding.policy.reasoning_effort)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "high".into())
}

fn validate_context(spec: &AttemptSpec, config: &LeaseExecutionConfig) -> anyhow::Result<()> {
    if spec.run.run_id.trim().is_empty()
        || config.host_id.trim().is_empty()
        || config.lease_id.trim().is_empty()
        || config.lease_token.trim().is_empty()
        || !config.workspace_path.is_absolute()
        || !config.codex_home.is_absolute()
    {
        anyhow::bail!("lease execution context is incomplete");
    }
    if spec.run.cwd != "/workspace" {
        anyhow::bail!("portable agent-host Run must use /workspace as cwd");
    }
    Ok(())
}

async fn git_stdout(root: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .await?;
    if !output.status.success() {
        anyhow::bail!("git command failed");
    }
    String::from_utf8(output.stdout).context("git output was not UTF-8")
}

fn bounded(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

fn write_resume_thread(codex_home: &Path, thread_id: &str) -> anyhow::Result<()> {
    if thread_id.is_empty()
        || thread_id.len() > 128
        || !thread_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        anyhow::bail!("Codex App Server returned an invalid thread ID");
    }
    let path = codex_home.join("pharness-resume.json");
    let temporary = codex_home.join("pharness-resume.tmp");
    std::fs::write(
        &temporary,
        serde_json::to_vec(&json!({"remote_thread_id":thread_id}))?,
    )?;
    std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600))?;
    std::fs::rename(temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemas_separate_planner_builder_and_verifier_contracts() {
        assert!(output_schema("plan", "repo-planner")["properties"]["steps"].is_object());
        assert_eq!(
            output_schema("implement", "repo-repair")["properties"]["repair"]["const"],
            true
        );
        assert!(output_schema("verify", "repo-verifier")["properties"]["decision"].is_object());
    }

    #[test]
    fn app_server_payload_redacts_credential_shaped_fields() {
        let value = sanitize_app_payload(&json!({"nested":{"authorization":"Bearer x"}}));
        assert_eq!(value["nested"]["authorization"], "[REDACTED]");
    }
}

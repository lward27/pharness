use pharness_core::{
    AgentEvent, CancellationFlag, ContextBudget, EventId, EventKind, PermissionGrantScope,
    ReadOnlyClusterTools, ResourceRef, RunId, RunScope, SafetyPolicy, TaskContract, TaskKind,
};
use pharness_fireworks::{FireworksClient, FireworksProviderConfig};
use pharness_runhost::{
    execute_attempt, ApprovalRequestPayload, AttemptBackend, AttemptHost, AttemptOutcome,
    AttemptSpec, BudgetResumeSpec, ResumeSpec, RunSpec, WorkspaceGitEvidence, WorkspaceSourceSpec,
};
use pharness_store::{
    CreateApproval, CreateApprovalGate, CreateArtifact, CreateAuditEvent, CreateBudgetExtension,
    CreateChangeSet, CreateEvidenceRetrieval, CreateFileChange, CreateIncident, CreateObservation,
    CreateRemediationPlan, CreateWorkPlan, SealStageOutcome, SqliteStore, StoreError,
    StoredApproval, StoredIncident, StoredObservation, StoredRemediationPlan, StoredRun,
    UpdateChangeSetRevision,
};
use secrecy::SecretString;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct LocalWorker {
    store: Arc<SqliteStore>,
    provider: FireworksClient,
    model: String,
    base_url: String,
    cluster_tools: ReadOnlyClusterTools,
    default_policy: SafetyPolicy,
    context_budget: ContextBudget,
    cancellations: Arc<Mutex<HashMap<RunId, CancellationFlag>>>,
}

impl LocalWorker {
    pub fn from_options(
        store: Arc<SqliteStore>,
        options: LocalWorkerOptions,
    ) -> anyhow::Result<Option<Self>> {
        let Some(api_key) = options.api_key else {
            return Ok(None);
        };
        let model = options.model;
        let base_url = options.base_url;
        let cluster_tools = options.cluster_tools;
        let default_policy = options.default_policy;
        let context_budget = options.context_budget;

        let provider = FireworksClient::new(
            api_key,
            FireworksProviderConfig {
                base_url: base_url.clone(),
                model: model.clone(),
            },
        )?;

        Ok(Some(Self {
            store,
            model,
            base_url,
            provider,
            cluster_tools,
            default_policy,
            context_budget,
            cancellations: Arc::new(Mutex::new(HashMap::new())),
        }))
    }

    pub fn config(&self) -> LocalWorkerConfig {
        LocalWorkerConfig {
            enabled: true,
            provider: "fireworks".to_string(),
            model: self.model.clone(),
            base_url: self.base_url.clone(),
            context_budget: self.context_budget.clone(),
        }
    }

    pub fn spawn_run(&self, run: StoredRun, cwd: impl Into<PathBuf>) {
        self.spawn_task(run, cwd.into(), None);
    }

    pub fn resume_run(&self, run: StoredRun, approval: StoredApproval) {
        self.spawn_task(run.clone(), PathBuf::from(run.cwd.clone()), Some(approval));
    }

    pub fn cancel(&self, run_id: &RunId) -> bool {
        let Some(flag) = self
            .cancellations
            .lock()
            .expect("cancellation registry mutex should not be poisoned")
            .get(run_id)
            .cloned()
        else {
            return false;
        };

        flag.cancel();
        true
    }

    fn spawn_task(&self, run: StoredRun, cwd: PathBuf, approval: Option<StoredApproval>) {
        let store = self.store.clone();
        let host = AttemptHost {
            provider: self.provider.clone(),
            cluster_tools: self.cluster_tools.clone(),
            default_policy: self.default_policy.clone(),
            context_budget: self.context_budget.clone(),
        };
        let cancellations = self.cancellations.clone();
        let cancellation = CancellationFlag::default();

        cancellations
            .lock()
            .expect("cancellation registry mutex should not be poisoned")
            .insert(run.id.clone(), cancellation.clone());

        tokio::spawn(async move {
            let run_id = run.id.clone();
            let result =
                run_local_attempt(store.clone(), host, run, cwd, approval, cancellation).await;

            cancellations
                .lock()
                .expect("cancellation registry mutex should not be poisoned")
                .remove(&run_id);

            if let Err(error) = result {
                let _ = fail_run_from_dispatch(&store, &run_id, error.to_string()).await;
            }
        });
    }
}

async fn run_local_attempt(
    store: Arc<SqliteStore>,
    host: AttemptHost,
    run: StoredRun,
    cwd: PathBuf,
    approval: Option<StoredApproval>,
    cancellation: CancellationFlag,
) -> anyhow::Result<()> {
    let spec = attempt_spec_for_run(&store, &run, &cwd, approval.as_ref()).await?;
    let backend = Arc::new(LocalAttemptBackend { store, run });

    execute_attempt(host, backend, spec, cancellation).await
}

pub(crate) async fn attempt_spec_for_run(
    store: &SqliteStore,
    run: &StoredRun,
    cwd: &std::path::Path,
    approval: Option<&StoredApproval>,
) -> anyhow::Result<AttemptSpec> {
    let event_seq_start = store.list_events(&run.id).await?.len() as u64;
    let resume = approval.map(resume_spec_from_approval).transpose()?;
    let mut workspace_source = workspace_source_for_run(&run.execution_target_json)?;
    if let Some(source) = workspace_source.as_mut() {
        // A fresh preparation Run owns an empty PVC. The workspace row already
        // carries the immutable commit the controller intends to check out,
        // but that is not evidence that the checkout exists on disk. Only
        // hydrate a missing resolved commit for Runs that are resuming or are
        // otherwise past the preparation boundary.
        if source.resolved_commit.is_none() && run.status != "preparing" {
            let workspace = store
                .get_workspace(&source.workspace_id)
                .await?
                .ok_or_else(|| {
                    anyhow::anyhow!("workspace {} no longer exists", source.workspace_id)
                })?;
            if workspace.run_id.as_ref() != Some(&run.id) {
                anyhow::bail!("workspace does not belong to the resumed run");
            }
            source.resolved_commit = workspace.resolved_commit;
            source.validate()?;
        }
    }

    Ok(AttemptSpec {
        run: RunSpec {
            run_id: run.id.to_string(),
            session_id: run.session_id.to_string(),
            cwd: cwd.to_string_lossy().to_string(),
            user_task: run.user_task.clone(),
            max_turns: run.max_turns,
            execution_target_json: run.execution_target_json.clone(),
            workspace_source,
            task_contract: task_contract_for_run(store, run).await?,
            run_budget: run
                .execution_target_json
                .get("run_budget")
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok()),
            budget_consumption: run.budget_consumption.clone(),
        },
        event_seq_start,
        resume,
        budget_resume: budget_resume_from_run(run)?,
    })
}

fn budget_resume_from_run(run: &StoredRun) -> anyhow::Result<Option<BudgetResumeSpec>> {
    if run.status != "queued" || run.budget_consumption.extensions == 0 {
        return Ok(None);
    }
    let Some(extension) = run
        .result_json
        .as_ref()
        .and_then(|result| result.get("budget_extension"))
    else {
        return Ok(None);
    };
    Ok(Some(BudgetResumeSpec {
        resume_messages_json: extension
            .get("resume_messages")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("budget resume transcript is missing"))?,
        turns_completed: extension
            .get("turns_completed")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| anyhow::anyhow!("budget resume turn count is invalid"))?,
    }))
}

fn workspace_source_for_run(
    execution_target: &serde_json::Value,
) -> anyhow::Result<Option<WorkspaceSourceSpec>> {
    let Some(value) = execution_target.get("workspace_source") else {
        return Ok(None);
    };
    let source = serde_json::from_value::<WorkspaceSourceSpec>(value.clone())
        .map_err(|error| anyhow::anyhow!("run has invalid workspace source: {error}"))?;
    source.validate()?;
    Ok(Some(source))
}

async fn task_contract_for_run(
    store: &SqliteStore,
    run: &StoredRun,
) -> anyhow::Result<TaskContract> {
    if run
        .execution_target_json
        .pointer("/agent_profile/id")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|profile| profile != "repo-builder")
    {
        return Ok(TaskContract::default());
    }
    let Some(work_item_id) = run
        .execution_target_json
        .get("work_item_id")
        .and_then(serde_json::Value::as_str)
    else {
        return Ok(TaskContract::default());
    };
    let criteria = store
        .get_work_item(work_item_id)
        .await?
        .map(|item| item.acceptance_criteria)
        .unwrap_or_default();
    Ok(TaskContract {
        kind: TaskKind::Coding,
        acceptance_criteria: criteria,
        require_workspace_change: true,
        require_post_change_diff: true,
    })
}

fn resume_spec_from_approval(approval: &StoredApproval) -> anyhow::Result<ResumeSpec> {
    let action_json = approval
        .action_json
        .clone()
        .ok_or_else(|| anyhow::anyhow!("approval has no reviewed action payload"))?;
    let resume_messages_json = approval
        .resume_messages_json
        .clone()
        .ok_or_else(|| anyhow::anyhow!("approval has no resumable message transcript"))?;

    Ok(ResumeSpec {
        approval_id: approval.id.clone(),
        action_json,
        resume_messages_json,
        turns_completed: approval.turns_completed,
    })
}

pub(crate) struct LocalAttemptBackend {
    store: Arc<SqliteStore>,
    run: StoredRun,
}

#[async_trait::async_trait]
impl AttemptBackend for LocalAttemptBackend {
    async fn mark_running(&self) -> anyhow::Result<()> {
        self.store.mark_run_running(&self.run.id).await?;
        Ok(())
    }

    async fn ingest_event(&self, event: &AgentEvent) -> anyhow::Result<()> {
        ingest_agent_event(&self.store, event).await?;
        Ok(())
    }

    async fn finish(&self, outcome: AttemptOutcome) -> anyhow::Result<()> {
        finish_run_from_attempt(&self.store, &self.run, outcome).await
    }
}

#[derive(Clone)]
pub struct LocalWorkerOptions {
    pub api_key: Option<SecretString>,
    pub model: String,
    pub base_url: String,
    pub cluster_tools: ReadOnlyClusterTools,
    pub default_policy: SafetyPolicy,
    pub context_budget: ContextBudget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LocalWorkerConfig {
    pub enabled: bool,
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub context_budget: ContextBudget,
}

pub(crate) async fn finish_run_from_attempt(
    store: &SqliteStore,
    run: &StoredRun,
    outcome: AttemptOutcome,
) -> anyhow::Result<()> {
    let consumption =
        if outcome.consumption.allowed_turns == 0 && outcome.consumption.allowed_tokens == 0 {
            run.budget_consumption.clone()
        } else {
            outcome.consumption.clone()
        };
    if consumption.allowed_turns != run.budget_consumption.allowed_turns
        || consumption.allowed_tokens != run.budget_consumption.allowed_tokens
        || consumption.turns_used < run.budget_consumption.turns_used
        || consumption.tokens_used < run.budget_consumption.tokens_used
        || consumption.active_execution_seconds_used
            < run.budget_consumption.active_execution_seconds_used
    {
        anyhow::bail!("attempt consumption does not match the durable RunBudget");
    }
    store
        .update_run_budget_consumption(&run.id, &consumption)
        .await?;
    persist_workspace_evidence(store, run, &outcome).await?;
    let error = outcome.error.clone();
    let approval_id = if outcome.status == "approval_required" {
        match &outcome.approval {
            Some(payload) => Some(create_pending_approval(store, run, payload).await?.id),
            None => None,
        }
    } else {
        None
    };
    let result_json = result_json_for_attempt(run, &outcome, approval_id);

    match outcome.status.as_str() {
        "completed" => {
            store
                .complete_run(&run.id, "completed", result_json, None)
                .await?;
        }
        "approval_required" => {
            store
                .mark_run_approval_required(&run.id, result_json)
                .await?;
        }
        "budget_extension_required" => {
            let payload = outcome
                .budget_extension
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("budget pause has no resumable payload"))?;
            if payload.consumption.allowed_turns != run.budget_consumption.allowed_turns
                || payload.consumption.allowed_tokens != run.budget_consumption.allowed_tokens
                || payload.consumption.turns_used > payload.consumption.allowed_turns
                || payload.consumption.tokens_used < run.budget_consumption.tokens_used
            {
                anyhow::bail!("budget pause consumption does not match the durable RunBudget");
            }
            store
                .update_run_budget_consumption(&run.id, &payload.consumption)
                .await?;
            let work_item_id = run_scope_for_run(run)
                .work_item_id
                .ok_or_else(|| anyhow::anyhow!("budget extension requires WorkItem scope"))?;
            let state_payload = serde_json::json!({
                "run_id": run.id,
                "work_item_id": work_item_id,
                "run_budget": run.run_budget,
                "consumption": payload.consumption,
                "reason": payload.reason,
            });
            let state_hash = format!("{:x}", Sha256::digest(state_payload.to_string().as_bytes()));
            let result_json = serde_json::json!({
                "status": "budget_extension_required",
                "turns": outcome.turns,
                "stop_reason": payload.reason,
                "budget_extension": {
                    "resume_messages": payload.resume_messages_json,
                    "turns_completed": payload.turns_completed,
                    "consumption": payload.consumption,
                },
            });
            store
                .pause_run_for_budget(&run.id, result_json, &payload.reason)
                .await?;
            store
                .create_budget_extension(CreateBudgetExtension {
                    id: format!("budget_{}", unique_suffix()),
                    work_item_id,
                    run_id: run.id.clone(),
                    state_hash,
                })
                .await?;
        }
        "failed" => {
            store
                .complete_run(&run.id, "failed", result_json, error)
                .await?;
        }
        "cancelled" => {
            store
                .complete_run(&run.id, "cancelled", result_json, error)
                .await?;
        }
        other => {
            anyhow::bail!("attempt reported unknown terminal status: {other}");
        }
    }

    sync_repo_stage_run(store, run, &outcome).await?;
    sync_work_item_attempt(store, run, &outcome).await?;

    Ok(())
}

pub(crate) async fn sync_repo_stage_run(
    store: &SqliteStore,
    run: &StoredRun,
    outcome: &AttemptOutcome,
) -> anyhow::Result<()> {
    let Some(stage_execution_id) = run
        .execution_target_json
        .pointer("/repo_mode/stage_execution_id")
        .and_then(serde_json::Value::as_str)
    else {
        return Ok(());
    };
    let stage = run
        .execution_target_json
        .pointer("/repo_mode/stage")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("Repo Mode run has no stage key"))?;
    let execution = store
        .get_stage_execution(stage_execution_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Repo Mode StageExecution no longer exists"))?;
    if execution.run_id.as_ref() != Some(&run.id) || execution.stage_key != stage {
        anyhow::bail!("Repo Mode Run does not match its StageExecution");
    }
    if store
        .get_stage_outcome_for_execution(stage_execution_id)
        .await?
        .is_some()
    {
        return Ok(());
    }
    if !matches!(
        outcome.status.as_str(),
        "completed" | "failed" | "cancelled"
    ) {
        return Ok(());
    }
    persist_repo_evidence_retrievals(store, run, &execution).await?;

    match stage {
        "plan" => seal_repo_plan_stage(store, run, &execution, outcome).await,
        "implement" => seal_repo_implement_stage(store, run, &execution, outcome).await,
        "test" => seal_repo_test_stage(store, run, &execution, outcome).await,
        "verify" => seal_repo_verify_stage(store, run, &execution, outcome).await,
        _ => {
            let status = if outcome.status == "completed" {
                "blocked"
            } else {
                outcome.status.as_str()
            };
            seal_unimplemented_repo_stage(
                store,
                run,
                &execution,
                status,
                "Repo Mode stage finalizer is not yet available",
            )
            .await
        }
    }
}

async fn persist_repo_evidence_retrievals(
    store: &SqliteStore,
    run: &StoredRun,
    execution: &pharness_store::StoredStageExecution,
) -> anyhow::Result<()> {
    let catalog = run
        .execution_target_json
        .pointer("/agent_context/evidence_catalog")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let actor = run
        .execution_target_json
        .pointer("/agent_profile/id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown-agent-profile");
    for event in store.list_events(&run.id).await? {
        if event.kind != EventKind::ToolFinished {
            continue;
        }
        let Some(evidence_id) = event
            .payload
            .pointer("/content/evidence_id")
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        let returned_hash = event
            .payload
            .pointer("/content/returned_hash")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("get_evidence event has no returned hash"))?;
        let catalog_entry = catalog
            .iter()
            .find(|entry| entry.get("id").and_then(serde_json::Value::as_str) == Some(evidence_id))
            .ok_or_else(|| anyhow::anyhow!("get_evidence returned an unallowlisted item"))?;
        if catalog_entry
            .get("hash")
            .and_then(serde_json::Value::as_str)
            != Some(returned_hash)
        {
            anyhow::bail!("get_evidence returned hash does not match the context catalog");
        }
        let evidence_kind = catalog_entry
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("evidence catalog item has no kind"))?;
        let evidence_version = catalog_entry
            .get("version")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("evidence catalog item has no version"))?;
        store
            .create_evidence_retrieval(CreateEvidenceRetrieval {
                id: repo_resource_id("eretr"),
                event_id: event.event_id.to_string(),
                work_item_id: execution.work_item_id.clone(),
                stage_execution_id: execution.id.clone(),
                run_id: run.id.clone(),
                actor: actor.into(),
                evidence_kind: evidence_kind.into(),
                evidence_id: evidence_id.into(),
                evidence_version: evidence_version.into(),
                returned_hash: returned_hash.into(),
            })
            .await?;
    }
    Ok(())
}

async fn seal_repo_test_stage(
    store: &SqliteStore,
    run: &StoredRun,
    execution: &pharness_store::StoredStageExecution,
    outcome: &AttemptOutcome,
) -> anyhow::Result<()> {
    let work_item = store
        .get_work_item(&execution.work_item_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Repo Mode WorkItem no longer exists"))?;
    let contract: pharness_core::RepositoryContract = serde_json::from_value(
        work_item
            .repository_contract_json
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Repo Mode WorkItem has no RepositoryContract"))?,
    )?;
    let events = store.list_events(&run.id).await?;
    let results = events
        .iter()
        .filter(|event| {
            event.kind == EventKind::ToolFinished
                && event
                    .payload
                    .pointer("/content/acceptance_command")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
        })
        .map(|event| {
            serde_json::json!({
                "event_id":event.event_id,
                "status":event.payload.get("status"),
                "summary":event.payload.get("summary"),
                "name":event.payload.pointer("/content/name"),
                "command":event.payload.pointer("/content/command"),
                "exit_code":event.payload.pointer("/content/exit_code"),
                "duration_ms":event.payload.pointer("/content/duration_ms"),
            })
        })
        .collect::<Vec<_>>();
    let selected_names = run
        .execution_target_json
        .get("selected_acceptance_commands")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let selected_commands = selected_names
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect::<Vec<_>>();
    let exact_results = selected_commands.iter().all(|command| {
        results.iter().any(|result| {
            result.get("command").and_then(serde_json::Value::as_str) == Some(*command)
                && result.get("status").and_then(serde_json::Value::as_str) == Some("ok")
                && result.get("exit_code").and_then(serde_json::Value::as_i64) == Some(0)
        })
    }) && results.len() == selected_commands.len();
    let submission = structured_submission_from_events(&events, "test_outcome");
    let submission_valid = submission.as_ref().is_some_and(|document| {
        document
            .as_object()
            .is_some_and(|object| !object.is_empty())
    });
    let passed = outcome.status == "completed"
        && !selected_commands.is_empty()
        && exact_results
        && submission_valid;
    let status = if passed {
        "succeeded"
    } else if outcome.status == "cancelled" {
        "cancelled"
    } else {
        "failed"
    };
    let stop_reason = if passed {
        "Tester executed every selected acceptance command successfully".to_string()
    } else {
        outcome.error.clone().unwrap_or_else(|| {
            "Tester acceptance evidence or typed submission is incomplete".into()
        })
    };
    let facts = serde_json::json!({
        "selected_commands":selected_commands,
        "results":results,
        "all_selected_commands_passed":exact_results,
        "typed_submission_present":submission_valid,
        "declared_contract_commands":contract.acceptance_commands,
    });
    let validation = serde_json::json!({
        "subject":{"run_id":run.id,"stage_execution_id":execution.id},
        "facts":facts,
        "status":if passed {"valid"} else {"invalid"},
    });
    store
        .create_evidence_validation(pharness_store::CreateEvidenceValidation {
            id: repo_resource_id("evalid"),
            work_item_id: execution.work_item_id.clone(),
            stage_execution_id: Some(execution.id.clone()),
            validator_key: "declared_acceptance".into(),
            status: if passed { "valid" } else { "invalid" }.into(),
            subject: serde_json::json!({"run_id":run.id}),
            evidence_refs: serde_json::json!(events
                .iter()
                .filter(|event| event.kind == EventKind::ToolFinished)
                .map(|event| {
                    let material = serde_json::json!({
                        "id":event.event_id,
                        "run_id":event.run_id,
                        "seq":event.seq,
                        "type":event.kind.as_str(),
                        "payload":event.payload,
                    });
                    serde_json::json!({
                        "kind":"event",
                        "id":event.event_id,
                        "hash":pharness_core::canonical_json_sha256(&material)
                            .expect("event evidence material is serializable"),
                    })
                })
                .collect::<Vec<_>>()),
            facts: facts.clone(),
            contradictions: if passed {
                serde_json::json!([])
            } else {
                serde_json::json!([{"kind":"acceptance_incomplete_or_failed"}])
            },
            content_hash: pharness_core::canonical_json_sha256(&validation)?,
        })
        .await?;
    store
        .update_repo_work_item_status(
            &execution.work_item_id,
            if passed { "verifying" } else { "blocked" },
            "controller",
            &stop_reason,
            false,
        )
        .await?;
    let metadata = store
        .get_repo_work_item_metadata(&execution.work_item_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Repo Mode metadata no longer exists"))?;
    let document = pharness_core::StageOutcomeDocument {
        schema_version: pharness_core::STAGE_OUTCOME_SCHEMA.into(),
        work_item_id: execution.work_item_id.clone(),
        stage_execution_id: execution.id.clone(),
        stage: pharness_core::RepoStageKey::Test,
        status: if passed {
            pharness_core::StageTerminalStatus::Succeeded
        } else if status == "cancelled" {
            pharness_core::StageTerminalStatus::Cancelled
        } else {
            pharness_core::StageTerminalStatus::Failed
        },
        objective: serde_json::json!({"kind":"execute_declared_acceptance"}),
        pinned_inputs: execution.input_snapshot.clone(),
        verified_facts: vec![facts],
        agent_claims: submission.into_iter().collect(),
        outputs: Vec::new(),
        acceptance: results,
        decisions: vec![
            serde_json::json!({"kind":"controller_acceptance_validation","status":status}),
        ],
        authorizations: execution
            .input_snapshot
            .get("chain_authorization_id")
            .map(|id| vec![serde_json::json!({"kind":"stage_chain","id":id})])
            .unwrap_or_default(),
        contradictions: if passed {
            Vec::new()
        } else {
            vec![serde_json::json!({"kind":"acceptance_incomplete_or_failed"})]
        },
        risks: Vec::new(),
        unavailable_capabilities: Vec::new(),
        recommendations: if passed {
            vec![serde_json::json!({"next":"dispatch_verifier"})]
        } else {
            vec![serde_json::json!({"next":"correct_or_replan"})]
        },
        stop_reason: stop_reason.clone(),
        sealed_state_version: metadata.state_version,
    };
    let value = serde_json::to_value(document)?;
    store
        .seal_stage_outcome(SealStageOutcome {
            id: repo_resource_id("stageout"),
            stage_execution_id: execution.id.clone(),
            work_item_id: execution.work_item_id.clone(),
            stage_key: execution.stage_key.clone(),
            status: status.into(),
            content_hash: pharness_core::canonical_json_sha256(&value)?,
            outcome: value,
            state_version: metadata.state_version,
            supersedes_outcome_id: None,
            actor: "controller".into(),
            reason: "validate exact declared acceptance evidence".into(),
        })
        .await?;
    if !passed {
        revoke_repo_chain_for_run(store, run, &stop_reason).await?;
    }
    Ok(())
}

async fn seal_repo_verify_stage(
    store: &SqliteStore,
    run: &StoredRun,
    execution: &pharness_store::StoredStageExecution,
    outcome: &AttemptOutcome,
) -> anyhow::Result<()> {
    let events = store.list_events(&run.id).await?;
    let submission = structured_submission_from_events(&events, "verification");
    let decision = submission
        .as_ref()
        .and_then(|document| document.get("decision"))
        .and_then(serde_json::Value::as_str);
    let upstream = store
        .list_effective_stage_outcomes(&execution.work_item_id)
        .await?;
    let upstream_succeeded = ["implement", "test"].into_iter().all(|stage| {
        upstream
            .iter()
            .any(|item| item.stage_key == stage && item.status == "succeeded")
    });
    let passed =
        outcome.status == "completed" && decision == Some("approved") && upstream_succeeded;
    let status = if passed {
        "succeeded"
    } else if outcome.status == "cancelled" {
        "cancelled"
    } else {
        "failed"
    };
    let stop_reason = if passed {
        "Verifier approved the change against controller-sealed Implement and Test evidence"
            .to_string()
    } else {
        outcome.error.clone().unwrap_or_else(|| {
            "Verifier rejected the change or did not submit an approved typed decision".into()
        })
    };
    let facts = serde_json::json!({
        "upstream_outcomes":upstream.iter().map(|item| serde_json::json!({"id":item.id,"stage":item.stage_key,"status":item.status,"hash":item.content_hash})).collect::<Vec<_>>(),
        "upstream_succeeded":upstream_succeeded,
        "typed_decision":decision,
    });
    let validation = serde_json::json!({
        "subject":{"run_id":run.id,"stage_execution_id":execution.id},
        "facts":facts,
        "status":if passed {"valid"} else {"invalid"},
    });
    store
        .create_evidence_validation(pharness_store::CreateEvidenceValidation {
            id: repo_resource_id("evalid"),
            work_item_id: execution.work_item_id.clone(),
            stage_execution_id: Some(execution.id.clone()),
            validator_key: "verification_decision".into(),
            status: if passed { "valid" } else { "invalid" }.into(),
            subject: serde_json::json!({"run_id":run.id}),
            evidence_refs: serde_json::json!(upstream
                .iter()
                .map(|item| serde_json::json!({
                    "kind":"stage_outcome",
                    "id":item.id,
                    "hash":item.content_hash,
                }))
                .collect::<Vec<_>>()),
            facts: facts.clone(),
            contradictions: if passed {
                serde_json::json!([])
            } else {
                serde_json::json!([{"kind":"verification_not_approved"}])
            },
            content_hash: pharness_core::canonical_json_sha256(&validation)?,
        })
        .await?;
    let metadata = store
        .get_repo_work_item_metadata(&execution.work_item_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Repo Mode metadata no longer exists"))?;
    let document = pharness_core::StageOutcomeDocument {
        schema_version: pharness_core::STAGE_OUTCOME_SCHEMA.into(),
        work_item_id: execution.work_item_id.clone(),
        stage_execution_id: execution.id.clone(),
        stage: pharness_core::RepoStageKey::Verify,
        status: if passed {
            pharness_core::StageTerminalStatus::Succeeded
        } else if status == "cancelled" {
            pharness_core::StageTerminalStatus::Cancelled
        } else {
            pharness_core::StageTerminalStatus::Failed
        },
        objective: serde_json::json!({"kind":"verify_change_against_sealed_evidence"}),
        pinned_inputs: execution.input_snapshot.clone(),
        verified_facts: vec![facts],
        agent_claims: submission.into_iter().collect(),
        outputs: Vec::new(),
        acceptance: Vec::new(),
        decisions: vec![
            serde_json::json!({"kind":"controller_verification_validation","status":status}),
        ],
        authorizations: execution
            .input_snapshot
            .get("chain_authorization_id")
            .map(|id| vec![serde_json::json!({"kind":"stage_chain","id":id})])
            .unwrap_or_default(),
        contradictions: if passed {
            Vec::new()
        } else {
            vec![serde_json::json!({"kind":"verification_not_approved"})]
        },
        risks: Vec::new(),
        unavailable_capabilities: Vec::new(),
        recommendations: if passed {
            vec![serde_json::json!({"next":"review_change_set"})]
        } else {
            vec![serde_json::json!({"next":"correct_or_replan"})]
        },
        stop_reason: stop_reason.clone(),
        sealed_state_version: metadata.state_version,
    };
    let value = serde_json::to_value(document)?;
    store
        .seal_stage_outcome(SealStageOutcome {
            id: repo_resource_id("stageout"),
            stage_execution_id: execution.id.clone(),
            work_item_id: execution.work_item_id.clone(),
            stage_key: execution.stage_key.clone(),
            status: status.into(),
            content_hash: pharness_core::canonical_json_sha256(&value)?,
            outcome: value,
            state_version: metadata.state_version,
            supersedes_outcome_id: None,
            actor: "controller".into(),
            reason: "validate typed Verifier result against sealed evidence".into(),
        })
        .await?;
    if passed {
        create_repo_change_set(store, run, execution).await?;
    }
    store
        .update_repo_work_item_status(
            &execution.work_item_id,
            if passed {
                "awaiting_approval"
            } else {
                "blocked"
            },
            "controller",
            &stop_reason,
            false,
        )
        .await?;
    revoke_repo_chain_for_run(store, run, "Verifier reached a terminal outcome").await?;
    Ok(())
}

async fn create_repo_change_set(
    store: &SqliteStore,
    run: &StoredRun,
    verify_execution: &pharness_store::StoredStageExecution,
) -> anyhow::Result<pharness_store::StoredChangeSet> {
    let work_item = store
        .get_work_item(&verify_execution.work_item_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Repo Mode WorkItem no longer exists"))?;
    let plan = store
        .get_work_plan_by_work_item(&verify_execution.work_item_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Repo Mode WorkPlan no longer exists"))?;
    let outcomes = store
        .list_effective_stage_outcomes(&verify_execution.work_item_id)
        .await?;
    let implement = outcomes
        .iter()
        .find(|outcome| outcome.stage_key == "implement" && outcome.status == "succeeded")
        .ok_or_else(|| anyhow::anyhow!("effective Implement outcome is unavailable"))?;
    let implement_execution = store
        .get_stage_execution(&implement.stage_execution_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Implement StageExecution is unavailable"))?;
    let builder_run_id = implement_execution
        .run_id
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Implement StageExecution has no Run"))?;
    let artifacts = store.list_artifacts(&builder_run_id).await?;
    let diff = artifacts
        .iter()
        .find(|artifact| artifact.kind == "workspace_git_diff")
        .ok_or_else(|| anyhow::anyhow!("Builder diff artifact is unavailable"))?;
    let status = artifacts
        .iter()
        .find(|artifact| artifact.kind == "workspace_git_status")
        .ok_or_else(|| anyhow::anyhow!("Builder status artifact is unavailable"))?;
    let diff_text = diff
        .content_text
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Builder diff artifact has no content"))?;
    let diff_hash = format!("sha256:{:x}", Sha256::digest(diff_text.as_bytes()));
    let material = serde_json::json!({
        "schema_version":"pharness.dev/repo-change-set/v1alpha1",
        "work_item_id":work_item.id,
        "work_plan":{"id":plan.id,"revision":plan.revision},
        "source_provenance":{
            "source_commit":work_item.source_commit,
            "workspace_id":implement_execution.workspace_id,
            "run_id":builder_run_id,
            "stage_execution_id":implement_execution.id,
            "implement_outcome_id":implement.id,
            "implement_outcome_hash":implement.content_hash,
        },
        "patch":{"artifact_id":diff.id,"hash":diff_hash},
        "git_status":{"artifact_id":status.id,"content":status.content_json},
        "effective_outcomes":outcomes.iter().map(|outcome| serde_json::json!({"id":outcome.id,"stage":outcome.stage_key,"hash":outcome.content_hash,"status":outcome.status})).collect::<Vec<_>>(),
        "verification_run_id":run.id,
        "verification_stage_execution_id":verify_execution.id,
    });
    let material_hash = pharness_core::canonical_json_sha256(&material)?;
    let existing = store.get_change_set_by_work_plan(&plan.id).await?;
    if let Some(existing) = existing {
        if !change_set_can_be_revised_for_work_plan(&existing, &plan) {
            anyhow::bail!(
                "an existing Repo Mode ChangeSet is not eligible for a newer reviewed revision"
            );
        }
        let revised = store
            .revise_change_set(
                &existing.id,
                UpdateChangeSetRevision {
                    title: Some("Repo Mode source change".into()),
                    summary: Some(
                        "Controller-derived source ChangeSet from the corrected verified Builder workspace"
                            .into(),
                    ),
                    risk_level: Some(plan.risk_level.clone()),
                    material_hash,
                    change_set_json: material,
                    session_id: Some(run.session_id.clone()),
                    run_id: Some(builder_run_id.clone()),
                    status: Some("proposed".into()),
                    actor: Some("controller".into()),
                    reason: Some(
                        "replace a rejected ChangeSet with the newer verified WorkPlan revision"
                            .into(),
                    ),
                },
            )
            .await?;
        return Ok(revised);
    }
    Ok(store
        .create_change_set(CreateChangeSet {
            id: repo_resource_id("cset"),
            work_item_id: Some(work_item.id),
            work_plan_id: plan.id,
            remediation_plan_id: None,
            incident_id: None,
            session_id: run.session_id.clone(),
            run_id: Some(builder_run_id),
            status: "proposed".into(),
            title: "Repo Mode source change".into(),
            summary: "Controller-derived source ChangeSet from the verified Builder workspace"
                .into(),
            risk_level: plan.risk_level,
            material_hash,
            resource_namespace: None,
            resource_kind: Some("Repository".into()),
            resource_name: Some(work_item.source_repo),
            change_set_json: material,
        })
        .await?)
}

fn change_set_can_be_revised_for_work_plan(
    change_set: &pharness_store::StoredChangeSet,
    work_plan: &pharness_store::StoredWorkPlan,
) -> bool {
    change_set.status == "rejected"
        && change_set
            .change_set_json
            .pointer("/work_plan/id")
            .and_then(serde_json::Value::as_str)
            == Some(work_plan.id.as_str())
        && change_set
            .change_set_json
            .pointer("/work_plan/revision")
            .and_then(serde_json::Value::as_i64)
            .is_some_and(|revision| revision < work_plan.revision)
}

async fn revoke_repo_chain_for_run(
    store: &SqliteStore,
    run: &StoredRun,
    reason: &str,
) -> anyhow::Result<()> {
    if let Some(chain_id) = run
        .execution_target_json
        .pointer("/repo_mode/chain_authorization_id")
        .and_then(serde_json::Value::as_str)
    {
        if store
            .get_stage_chain_authorization(chain_id)
            .await?
            .is_some_and(|authorization| authorization.status == "active")
        {
            store
                .revoke_stage_chain_authorization(chain_id, reason)
                .await?;
        }
    }
    Ok(())
}

async fn seal_repo_implement_stage(
    store: &SqliteStore,
    run: &StoredRun,
    execution: &pharness_store::StoredStageExecution,
    outcome: &AttemptOutcome,
) -> anyhow::Result<()> {
    let work_item = store
        .get_work_item(&execution.work_item_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Repo Mode WorkItem no longer exists"))?;
    let contract: pharness_core::RepositoryContract = serde_json::from_value(
        work_item
            .repository_contract_json
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Repo Mode WorkItem has no RepositoryContract"))?,
    )?;
    let evidence = outcome.workspace_evidence.as_ref();
    let evidence_valid = outcome.status == "completed"
        && evidence.is_some_and(|evidence| {
            !evidence.diff.trim().is_empty()
                && !evidence.changed_paths.is_empty()
                && evidence.changed_paths.iter().all(|path| {
                    contract
                        .writable_paths
                        .iter()
                        .any(|pattern| repo_path_matches(pattern, path))
                })
        });
    let status = if evidence_valid {
        "succeeded"
    } else if outcome.status == "cancelled" {
        "cancelled"
    } else {
        "failed"
    };
    let stop_reason = if evidence_valid {
        "Builder completed with a nonempty diff inside the authorized writable paths".to_string()
    } else {
        outcome
            .error
            .clone()
            .or_else(|| outcome.summary.clone())
            .unwrap_or_else(|| {
                "Builder did not produce controller-valid workspace evidence".to_string()
            })
    };
    let artifacts = store.list_artifacts(&run.id).await?;
    let evidence_refs = artifacts
        .iter()
        .filter(|artifact| {
            matches!(
                artifact.kind.as_str(),
                "workspace_git_diff" | "workspace_git_status"
            )
        })
        .filter_map(|artifact| {
            Some(serde_json::json!({
                "kind":"artifact",
                "id":artifact.id,
                "hash":artifact.content_hash.as_ref()?,
                "artifact_kind":artifact.kind,
            }))
        })
        .collect::<Vec<_>>();
    let facts = serde_json::json!({
        "base_commit":evidence.map(|evidence| &evidence.base_commit),
        "branch":evidence.map(|evidence| &evidence.branch),
        "changed_paths":evidence.map(|evidence| evidence.changed_paths.clone()).unwrap_or_default(),
        "diff_hash":evidence.map(|evidence| format!("sha256:{:x}", Sha256::digest(evidence.diff.as_bytes()))),
        "authorized_writable_paths":contract.writable_paths,
    });
    let validation_material = serde_json::json!({
        "subject":{"run_id":run.id,"stage_execution_id":execution.id},
        "evidence_refs":evidence_refs,
        "facts":facts,
        "status":if evidence_valid {"valid"} else {"invalid"},
    });
    store
        .create_evidence_validation(pharness_store::CreateEvidenceValidation {
            id: repo_resource_id("evalid"),
            work_item_id: execution.work_item_id.clone(),
            stage_execution_id: Some(execution.id.clone()),
            validator_key: "builder_diff_and_changed_paths".into(),
            status: if evidence_valid { "valid" } else { "invalid" }.into(),
            subject: serde_json::json!({"run_id":run.id,"workspace_id":execution.workspace_id}),
            evidence_refs: serde_json::Value::Array(evidence_refs.clone()),
            facts: facts.clone(),
            contradictions: if evidence_valid {
                serde_json::json!([])
            } else {
                serde_json::json!([{"kind":"invalid_builder_workspace_evidence"}])
            },
            content_hash: pharness_core::canonical_json_sha256(&validation_material)?,
        })
        .await?;
    store
        .update_repo_work_item_status(
            &execution.work_item_id,
            if evidence_valid {
                "verifying"
            } else {
                "blocked"
            },
            "controller",
            &stop_reason,
            false,
        )
        .await?;
    let metadata = store
        .get_repo_work_item_metadata(&execution.work_item_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Repo Mode metadata no longer exists"))?;
    let document = pharness_core::StageOutcomeDocument {
        schema_version: pharness_core::STAGE_OUTCOME_SCHEMA.into(),
        work_item_id: execution.work_item_id.clone(),
        stage_execution_id: execution.id.clone(),
        stage: pharness_core::RepoStageKey::Implement,
        status: match status {
            "succeeded" => pharness_core::StageTerminalStatus::Succeeded,
            "cancelled" => pharness_core::StageTerminalStatus::Cancelled,
            _ => pharness_core::StageTerminalStatus::Failed,
        },
        objective: serde_json::json!({"kind":"implement_approved_work_plan"}),
        pinned_inputs: execution.input_snapshot.clone(),
        verified_facts: vec![facts],
        agent_claims: outcome
            .summary
            .as_ref()
            .map(|summary| vec![serde_json::json!({"kind":"builder_summary","summary":summary})])
            .unwrap_or_default(),
        outputs: evidence_refs,
        acceptance: Vec::new(),
        decisions: vec![
            serde_json::json!({"kind":"controller_workspace_validation","status":status}),
        ],
        authorizations: execution
            .input_snapshot
            .get("chain_authorization_id")
            .map(|id| vec![serde_json::json!({"kind":"stage_chain","id":id})])
            .unwrap_or_default(),
        contradictions: if evidence_valid {
            Vec::new()
        } else {
            vec![serde_json::json!({"kind":"builder_evidence_invalid"})]
        },
        risks: Vec::new(),
        unavailable_capabilities: Vec::new(),
        recommendations: if evidence_valid {
            vec![serde_json::json!({"next":"dispatch_tester"})]
        } else {
            vec![serde_json::json!({"next":"correct_or_replan"})]
        },
        stop_reason: stop_reason.clone(),
        sealed_state_version: metadata.state_version,
    };
    let value = serde_json::to_value(document)?;
    store
        .seal_stage_outcome(SealStageOutcome {
            id: repo_resource_id("stageout"),
            stage_execution_id: execution.id.clone(),
            work_item_id: execution.work_item_id.clone(),
            stage_key: execution.stage_key.clone(),
            status: status.into(),
            content_hash: pharness_core::canonical_json_sha256(&value)?,
            outcome: value,
            state_version: metadata.state_version,
            supersedes_outcome_id: None,
            actor: "controller".into(),
            reason: "validate Builder workspace evidence".into(),
        })
        .await?;
    if !evidence_valid {
        if let Some(chain_id) = run
            .execution_target_json
            .pointer("/repo_mode/chain_authorization_id")
            .and_then(serde_json::Value::as_str)
        {
            store
                .revoke_stage_chain_authorization(chain_id, &stop_reason)
                .await?;
        }
    }
    Ok(())
}

fn repo_path_matches(pattern: &str, path: &str) -> bool {
    pattern
        .strip_suffix("/**")
        .map(|prefix| path == prefix || path.starts_with(&format!("{prefix}/")))
        .unwrap_or(pattern == path)
}

async fn seal_repo_plan_stage(
    store: &SqliteStore,
    run: &StoredRun,
    execution: &pharness_store::StoredStageExecution,
    outcome: &AttemptOutcome,
) -> anyhow::Result<()> {
    let work_item = store
        .get_work_item(&execution.work_item_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Repo Mode WorkItem no longer exists"))?;
    let events = store.list_events(&run.id).await?;
    let submitted = structured_submission_from_events(&events, "work_plan");
    let (status, stop_reason, plan, agent_claims) = if outcome.status == "completed" {
        match submitted {
            Some(document) => match validate_repo_work_plan(&document) {
                Ok((title, summary, risk_level)) => {
                    let plan = if let Some(existing) = store
                        .get_work_plan_by_work_item(&execution.work_item_id)
                        .await?
                    {
                        let revised = store
                            .revise_work_plan(
                                &existing.id,
                                pharness_store::UpdateWorkPlanRevision {
                                    title: Some(title),
                                    summary: Some(summary),
                                    risk_level: Some(risk_level),
                                    requires_approval: Some(true),
                                    work_plan_json: document.clone(),
                                    session_id: Some(run.session_id.clone()),
                                    run_id: Some(run.id.clone()),
                                    actor: Some("controller".into()),
                                    reason: Some(
                                        "Planner submitted a replacement WorkPlan revision".into(),
                                    ),
                                },
                            )
                            .await?;
                        store
                            .update_work_plan_status(
                                &revised.id,
                                "proposed",
                                Some("controller".into()),
                                Some("Planner submission passed controller validation".into()),
                            )
                            .await?
                    } else {
                        store
                            .create_work_plan(CreateWorkPlan {
                                id: repo_resource_id("wplan"),
                                work_item_id: Some(execution.work_item_id.clone()),
                                remediation_plan_id: None,
                                incident_id: None,
                                session_id: run.session_id.clone(),
                                run_id: Some(run.id.clone()),
                                status: "proposed".into(),
                                title,
                                summary,
                                risk_level,
                                requires_approval: true,
                                resource_namespace: None,
                                resource_kind: Some("Repository".into()),
                                resource_name: Some(work_item.source_repo.clone()),
                                work_plan_json: document.clone(),
                            })
                            .await?
                    };
                    (
                        "succeeded",
                        "Planner submitted a controller-validated proposed WorkPlan".to_string(),
                        Some(plan),
                        vec![serde_json::json!({"kind":"planner_submission","document":document})],
                    )
                }
                Err(error) => (
                    "failed",
                    error,
                    None,
                    vec![
                        serde_json::json!({"kind":"invalid_planner_submission","document":document}),
                    ],
                ),
            },
            None => (
                "failed",
                "Planner completed without a typed WorkPlan submission".to_string(),
                None,
                Vec::new(),
            ),
        }
    } else {
        (
            if outcome.status == "cancelled" {
                "cancelled"
            } else {
                "failed"
            },
            outcome
                .error
                .clone()
                .or_else(|| outcome.summary.clone())
                .unwrap_or_else(|| "Planner AgentRun failed".into()),
            None,
            Vec::new(),
        )
    };
    let work_item_status = if status == "succeeded" {
        "awaiting_approval"
    } else {
        "blocked"
    };
    store
        .update_repo_work_item_status(
            &execution.work_item_id,
            work_item_status,
            "controller",
            stop_reason.as_ref(),
            false,
        )
        .await?;
    let metadata = store
        .get_repo_work_item_metadata(&execution.work_item_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Repo Mode metadata no longer exists"))?;
    let document = pharness_core::StageOutcomeDocument {
        schema_version: pharness_core::STAGE_OUTCOME_SCHEMA.into(),
        work_item_id: execution.work_item_id.clone(),
        stage_execution_id: execution.id.clone(),
        stage: pharness_core::RepoStageKey::Plan,
        status: match status {
            "succeeded" => pharness_core::StageTerminalStatus::Succeeded,
            "cancelled" => pharness_core::StageTerminalStatus::Cancelled,
            _ => pharness_core::StageTerminalStatus::Failed,
        },
        objective: serde_json::json!({"kind":"produce_bounded_work_plan"}),
        pinned_inputs: execution.input_snapshot.clone(),
        verified_facts: vec![serde_json::json!({
            "kind":"typed_submission_validation",
            "status":status,
            "run_id":run.id,
        })],
        agent_claims,
        outputs: plan
            .as_ref()
            .map(|plan| vec![serde_json::json!({"kind":"work_plan","id":plan.id,"revision":plan.revision,"status":plan.status})])
            .unwrap_or_default(),
        acceptance: Vec::new(),
        decisions: vec![serde_json::json!({"kind":"controller_validation","status":status})],
        authorizations: Vec::new(),
        contradictions: Vec::new(),
        risks: Vec::new(),
        unavailable_capabilities: Vec::new(),
        recommendations: if status == "succeeded" {
            vec![serde_json::json!({"next":"review_work_plan"})]
        } else {
            vec![serde_json::json!({"next":"correct_or_replan"})]
        },
        stop_reason: stop_reason.to_string(),
        sealed_state_version: metadata.state_version,
    };
    let value = serde_json::to_value(&document)?;
    store
        .seal_stage_outcome(SealStageOutcome {
            id: repo_resource_id("stageout"),
            stage_execution_id: execution.id.clone(),
            work_item_id: execution.work_item_id.clone(),
            stage_key: execution.stage_key.clone(),
            status: status.into(),
            content_hash: pharness_core::canonical_json_sha256(&value)?,
            outcome: value,
            state_version: metadata.state_version,
            supersedes_outcome_id: None,
            actor: "controller".into(),
            reason: "validate typed Planner result".into(),
        })
        .await?;
    Ok(())
}

pub(crate) fn structured_submission_from_events(
    events: &[AgentEvent],
    kind: &str,
) -> Option<serde_json::Value> {
    events.iter().rev().find_map(|event| {
        if event.kind != EventKind::ToolFinished
            || event
                .payload
                .pointer("/content/structured_submission")
                .and_then(serde_json::Value::as_bool)
                != Some(true)
            || event
                .payload
                .pointer("/content/kind")
                .and_then(serde_json::Value::as_str)
                != Some(kind)
        {
            return None;
        }
        event.payload.pointer("/content/document").cloned()
    })
}

fn validate_repo_work_plan(
    document: &serde_json::Value,
) -> Result<(String, String, String), String> {
    let object = document
        .as_object()
        .ok_or_else(|| "WorkPlan submission must be a JSON object".to_string())?;
    let summary = object
        .get("summary")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 4_000)
        .ok_or_else(|| "WorkPlan submission requires a bounded summary".to_string())?;
    let steps = object
        .get("steps")
        .and_then(serde_json::Value::as_array)
        .filter(|steps| !steps.is_empty() && steps.len() <= 50)
        .ok_or_else(|| "WorkPlan submission requires between one and fifty steps".to_string())?;
    if steps.iter().any(|step| {
        let Some(step) = step.as_object() else {
            return true;
        };
        ["title", "description"].into_iter().any(|field| {
            step.get(field)
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .map_or(true, str::is_empty)
        })
    }) {
        return Err("every WorkPlan step requires a title and description".into());
    }
    let title = object
        .get("title")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 200)
        .unwrap_or("Repo Mode WorkPlan");
    let risk = object
        .get("risk_level")
        .and_then(serde_json::Value::as_str)
        .filter(|risk| matches!(*risk, "low" | "medium" | "high"))
        .unwrap_or("medium");
    Ok((title.into(), summary.into(), risk.into()))
}

async fn seal_unimplemented_repo_stage(
    store: &SqliteStore,
    run: &StoredRun,
    execution: &pharness_store::StoredStageExecution,
    status: &str,
    stop_reason: &str,
) -> anyhow::Result<()> {
    store
        .update_repo_work_item_status(
            &execution.work_item_id,
            "blocked",
            "controller",
            stop_reason,
            false,
        )
        .await?;
    let metadata = store
        .get_repo_work_item_metadata(&execution.work_item_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Repo Mode metadata no longer exists"))?;
    let value = serde_json::json!({
        "schema_version":pharness_core::STAGE_OUTCOME_SCHEMA,
        "work_item_id":execution.work_item_id,
        "stage_execution_id":execution.id,
        "stage":execution.stage_key,
        "status":status,
        "objective":{},
        "pinned_inputs":execution.input_snapshot,
        "verified_facts":[],
        "agent_claims":[],
        "outputs":[],
        "acceptance":[],
        "decisions":[],
        "authorizations":[],
        "contradictions":[],
        "risks":[],
        "unavailable_capabilities":[],
        "recommendations":[{"next":"controller_review"}],
        "stop_reason":stop_reason,
        "sealed_state_version":metadata.state_version,
        "run_id":run.id,
    });
    store
        .seal_stage_outcome(SealStageOutcome {
            id: repo_resource_id("stageout"),
            stage_execution_id: execution.id.clone(),
            work_item_id: execution.work_item_id.clone(),
            stage_key: execution.stage_key.clone(),
            status: status.into(),
            content_hash: pharness_core::canonical_json_sha256(&value)?,
            outcome: value,
            state_version: metadata.state_version,
            supersedes_outcome_id: None,
            actor: "controller".into(),
            reason: stop_reason.into(),
        })
        .await?;
    Ok(())
}

fn repo_resource_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::now_v7().simple())
}

async fn persist_workspace_evidence(
    store: &SqliteStore,
    run: &StoredRun,
    outcome: &AttemptOutcome,
) -> anyhow::Result<()> {
    let source = workspace_source_for_run(&run.execution_target_json)?;
    let Some(source) = source else {
        if outcome.workspace_evidence.is_some() {
            anyhow::bail!("run without a workspace source reported workspace evidence");
        }
        return Ok(());
    };
    if outcome.status != "completed" {
        return Ok(());
    }
    let evidence = outcome
        .workspace_evidence
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("completed remote workspace run has no Git evidence"))?;
    validate_workspace_evidence(&source, evidence)?;
    let scope = run_scope_for_run(run);

    // Read-only and ephemeral-copy profile runs use an exact checkout but do
    // not own a durable mutable Workspace record. Validate their evidence at
    // the boundary, then discard it instead of trying to persist Builder
    // artifacts against a nonexistent or differently owned Workspace.
    let repo_stage = run
        .execution_target_json
        .pointer("/repo_mode/stage")
        .and_then(serde_json::Value::as_str);
    if matches!(repo_stage, Some("test" | "verify")) {
        return Ok(());
    }
    if scope.workspace_id.is_none() {
        if !evidence.status.trim().is_empty()
            || !evidence.diff.trim().is_empty()
            || !evidence.changed_paths.is_empty()
        {
            anyhow::bail!("read-only remote workspace run modified its checkout");
        }
        return Ok(());
    }
    if scope.workspace_id.as_deref() != Some(evidence.workspace_id.as_str()) {
        anyhow::bail!("workspace evidence does not match the run scope");
    }
    let workspace = store
        .get_workspace(&evidence.workspace_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("workspace {} no longer exists", evidence.workspace_id))?;
    if workspace.run_id.as_ref() != Some(&run.id)
        || workspace.source_repo != source.source_repo
        || workspace.source_ref != source.source_ref
        || workspace.resolved_commit.as_deref() != Some(evidence.base_commit.as_str())
        || workspace.branch.as_deref() != Some(evidence.branch.as_str())
    {
        anyhow::bail!("workspace evidence does not match the pinned workspace state");
    }
    let test_events =
        crate::app::event_evidence::shell_test_evidence(&store.list_events(&run.id).await?);
    let diff_artifact = store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_workspace_diff", unique_suffix()),
            session_id: run.session_id.clone(),
            run_id: Some(run.id.clone()),
            kind: "workspace_git_diff".to_string(),
            label: format!("Git diff for workspace {}", evidence.workspace_id),
            mime_type: Some("text/x-diff".to_string()),
            path: None,
            content_text: Some(evidence.diff.clone()),
            content_json: None,
        })
        .await?;
    let status_artifact = store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_workspace_status", unique_suffix()),
            session_id: run.session_id.clone(),
            run_id: Some(run.id.clone()),
            kind: "workspace_git_status".to_string(),
            label: format!(
                "Git status and tests for workspace {}",
                evidence.workspace_id
            ),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(serde_json::json!({
                "status": evidence.status,
                "base_commit": evidence.base_commit,
                "branch": evidence.branch,
                "changed_paths": evidence.changed_paths,
                "test_events": test_events,
            })),
        })
        .await?;
    store
        .create_audit_event(CreateAuditEvent {
            id: format!(
                "aud_{}_workspace_evidence_{}",
                run.id.as_str(),
                unique_suffix()
            ),
            kind: "workspace.evidence_recorded".to_string(),
            actor: Some("agent:cluster-worker".to_string()),
            resource_kind: "workspace".to_string(),
            resource_id: evidence.workspace_id.clone(),
            run_id: Some(run.id.clone()),
            payload_json: serde_json::json!({
                "base_commit": evidence.base_commit,
                "branch": evidence.branch,
                "diff_artifact_id": diff_artifact.id,
                "status_artifact_id": status_artifact.id,
                "test_event_count": test_events.len(),
            }),
        })
        .await?;
    Ok(())
}

async fn expire_attempt_workspace_grants(
    store: &SqliteStore,
    run: &StoredRun,
    actor: &str,
) -> Result<(), StoreError> {
    for grant in store.list_permission_grants(Some("active"), 200).await? {
        let scope: PermissionGrantScope = serde_json::from_value(grant.scope_json.clone())?;
        if !scope.run_ids.iter().any(|run_id| run_id == run.id.as_str()) {
            continue;
        }
        let grant = store
            .stale_permission_grant(
                &grant.id,
                Some(actor.to_string()),
                Some(format!("attempt {} reached a terminal state", run.id)),
            )
            .await?;
        store
            .create_audit_event(CreateAuditEvent {
                id: format!("aud_{}_{}", grant.id, unique_suffix()),
                kind: "permission_grant.stale".to_string(),
                actor: Some(actor.to_string()),
                resource_kind: "permission_grant".to_string(),
                resource_id: grant.id,
                run_id: Some(run.id.clone()),
                payload_json: serde_json::json!({
                    "reason": "attempt reached a terminal state",
                    "run_id": run.id,
                }),
            })
            .await?;
    }
    Ok(())
}

fn validate_workspace_evidence(
    source: &WorkspaceSourceSpec,
    evidence: &WorkspaceGitEvidence,
) -> anyhow::Result<()> {
    if evidence.workspace_id != source.workspace_id || evidence.branch != source.branch {
        anyhow::bail!("workspace evidence does not match the issued source contract");
    }
    if evidence.base_commit.trim().is_empty()
        || evidence.diff.len() > 512 * 1024
        || evidence
            .changed_paths
            .iter()
            .any(|path| secret_shaped_path(path))
    {
        anyhow::bail!("workspace evidence failed safety validation");
    }
    Ok(())
}

fn secret_shaped_path(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    name == ".env"
        || name.starts_with(".env.")
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.contains("kubeconfig")
        || name.contains("credential")
        || name.contains("secret")
        || name.contains("token")
}

async fn sync_work_item_attempt(
    store: &SqliteStore,
    run: &StoredRun,
    outcome: &AttemptOutcome,
) -> anyhow::Result<()> {
    if run.execution_target_json.get("repo_mode").is_some() {
        return Ok(());
    }
    let scope = run_scope_for_run(run);
    let (Some(work_item_id), Some(workspace_id)) = (scope.work_item_id, scope.workspace_id) else {
        return Ok(());
    };
    let (work_item_status, workspace_status) = match outcome.status.as_str() {
        "completed" => ("verifying", "verifying"),
        "failed" => ("blocked", "blocked"),
        "cancelled" => ("cancelled", "cancelled"),
        "approval_required" => return Ok(()),
        "budget_extension_required" => return Ok(()),
        _ => return Ok(()),
    };
    let events = store.list_events(&run.id).await?;
    let classification = classify_work_item_attempt(outcome, &events);
    let reason = outcome
        .summary
        .clone()
        .or_else(|| outcome.error.clone())
        .unwrap_or_else(|| "coding attempt reached a terminal state".to_string());
    let actor = attempt_actor(run);
    let workspace = store.get_workspace(&workspace_id).await?.ok_or_else(|| {
        anyhow::anyhow!("workspace {workspace_id} disappeared during run finalization")
    })?;
    store
        .update_workspace_execution(
            &workspace_id,
            pharness_store::UpdateWorkspaceExecution {
                run_id: Some(run.id.clone()),
                status: workspace_status.to_string(),
                resolved_commit: workspace.resolved_commit.clone(),
                branch: workspace.branch.clone(),
                actor: Some(actor.to_string()),
                reason: Some(reason.clone()),
            },
        )
        .await?;
    let work_item = store
        .finish_work_item_attempt(
            &work_item_id,
            work_item_status,
            Some(actor.to_string()),
            Some(reason),
        )
        .await?;
    store
        .create_audit_event(CreateAuditEvent {
            id: format!("aud_{}_{}", work_item.id, unique_suffix()),
            kind: "work_item.attempt_finished".to_string(),
            actor: Some(actor.to_string()),
            resource_kind: "work_item".to_string(),
            resource_id: work_item.id.clone(),
            run_id: Some(run.id.clone()),
            payload_json: serde_json::json!({
                "work_item_id": work_item.id,
                "workspace_id": workspace_id,
                "run_id": run.id,
                "outcome": {
                    "status": outcome.status,
                    "turns": outcome.turns,
                },
                "work_item_status": work_item.status,
                "classification": classification.as_json(),
            }),
        })
        .await?;
    expire_attempt_workspace_grants(store, run, actor).await?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WorkItemAttemptClassification {
    code: &'static str,
    recommended_action: &'static str,
    evidence_kind: &'static str,
}

impl WorkItemAttemptClassification {
    fn as_json(self) -> serde_json::Value {
        serde_json::json!({
            "code": self.code,
            "recommended_action": self.recommended_action,
            "evidence_kind": self.evidence_kind,
        })
    }
}

fn classify_work_item_attempt(
    outcome: &AttemptOutcome,
    events: &[AgentEvent],
) -> WorkItemAttemptClassification {
    match outcome.status.as_str() {
        "completed" => WorkItemAttemptClassification {
            code: "completed",
            recommended_action: "capture_change_set",
            evidence_kind: "terminal_outcome",
        },
        "cancelled" => WorkItemAttemptClassification {
            code: "cancelled",
            recommended_action: "terminal",
            evidence_kind: "terminal_outcome",
        },
        "failed" => classify_failed_work_item_attempt(outcome, events),
        _ => WorkItemAttemptClassification {
            code: "unknown",
            recommended_action: "inspect_and_block",
            evidence_kind: "terminal_outcome",
        },
    }
}

fn classify_failed_work_item_attempt(
    outcome: &AttemptOutcome,
    events: &[AgentEvent],
) -> WorkItemAttemptClassification {
    if events.iter().any(|event| {
        event.kind == EventKind::PolicyEvaluated
            && event
                .payload
                .pointer("/decision/decision")
                .and_then(|value| value.as_str())
                == Some("deny")
    }) {
        return WorkItemAttemptClassification {
            code: "policy_denied",
            recommended_action: "revise_plan_or_authorization",
            evidence_kind: "policy_evaluated",
        };
    }
    if outcome
        .error
        .as_deref()
        .is_some_and(|error| error.contains("run exceeded max_turns="))
    {
        return WorkItemAttemptClassification {
            code: "model_turn_budget_exhausted",
            recommended_action: "revise_work_plan",
            evidence_kind: "run_failed",
        };
    }
    if events.iter().any(|event| {
        event.kind == EventKind::RunFailed
            && event
                .payload
                .get("action")
                .and_then(|value| value.as_str())
                .is_some()
    }) {
        return WorkItemAttemptClassification {
            code: "tool_execution_failed",
            recommended_action: "inspect_and_replan",
            evidence_kind: "run_failed",
        };
    }
    let requested_model = events
        .iter()
        .any(|event| event.kind == EventKind::ModelRequestStarted);
    let received_model_response = events
        .iter()
        .any(|event| event.kind == EventKind::ModelResponseFinished);
    if requested_model && !received_model_response {
        return WorkItemAttemptClassification {
            code: "model_provider_failed",
            recommended_action: "inspect_and_replan",
            evidence_kind: "model_request_without_response",
        };
    }
    WorkItemAttemptClassification {
        code: "unknown_execution_failure",
        recommended_action: "inspect_and_replan",
        evidence_kind: "terminal_outcome",
    }
}

fn attempt_actor(run: &StoredRun) -> &'static str {
    match run
        .execution_target_json
        .get("kind")
        .and_then(serde_json::Value::as_str)
    {
        Some("kubernetes_workspace") | Some("kubernetes_job") => "agent:cluster-worker",
        _ => "agent:local-worker",
    }
}

fn run_scope_for_run(run: &StoredRun) -> RunScope {
    RunScope::from_execution_target(&run.execution_target_json).unwrap_or_default()
}

fn result_json_for_attempt(
    run: &StoredRun,
    outcome: &AttemptOutcome,
    approval_id: Option<String>,
) -> serde_json::Value {
    let run_scope = run_scope_for_run(run);
    serde_json::json!({
        "status": &outcome.status,
        "turns": outcome.turns,
        "summary": &outcome.summary,
        "error": &outcome.error,
        "approval_id": approval_id,
        "run_scope": run_scope.to_optional_json(),
        "budget_extension": outcome.budget_extension.as_ref().map(|payload| serde_json::json!({
            "reason": payload.reason,
            "resume_messages": payload.resume_messages_json,
            "turns_completed": payload.turns_completed,
            "consumption": payload.consumption,
        })),
    })
}

async fn create_pending_approval(
    store: &SqliteStore,
    run: &StoredRun,
    payload: &ApprovalRequestPayload,
) -> Result<StoredApproval, StoreError> {
    let run_scope = run_scope_for_run(run);
    let run_scope_json = run_scope.to_optional_json();

    store
        .create_approval(CreateApproval {
            id: format!("appr_{}_{}", run.id.as_str(), unique_suffix()),
            session_id: run.session_id.clone(),
            run_id: run.id.clone(),
            status: "pending".to_string(),
            kind: payload.kind.clone(),
            summary: payload.summary.clone(),
            risk_level: payload.risk.clone(),
            run_scope_json,
            action_json: payload.action_json.clone(),
            preview_json: payload.preview_json.clone(),
            resume_messages_json: Some(payload.resume_messages_json.clone()),
            turns_completed: payload.turns_completed,
        })
        .await
}

fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

/// Persist one agent event plus every derived control-plane record.
///
/// This is the single ingestion path for run events: the in-process local
/// backend and the worker ingest endpoints both go through it, so derivation
/// behavior cannot fork between execution targets.
pub(crate) async fn ingest_agent_event(
    store: &SqliteStore,
    event: &AgentEvent,
) -> Result<(), StoreError> {
    store.append_event(event).await?;
    if let Some(change) = file_change_from_event(event) {
        store.create_file_change(change).await?;
    }
    let artifact_id = if let Some(artifact) = artifact_from_event(event) {
        let artifact_id = artifact.id.clone();
        store.create_artifact(artifact).await?;
        Some(artifact_id)
    } else {
        None
    };
    if let Some(observation) = observation_from_event(event, artifact_id) {
        let observation = store.create_observation(observation).await?;
        if let Some(incident) = incident_from_observation(&observation) {
            let incident = store.create_incident(incident).await?;
            if let Some(plan) = remediation_plan_from_incident(&incident) {
                let plan = store.create_remediation_plan(plan).await?;
                for gate in approval_gates_from_remediation_plan(&plan) {
                    store.create_approval_gate(gate).await?;
                }
            }
        }
    }
    if let Some(audit_event) = grant_used_audit_event_from_event(event) {
        store.create_audit_event(audit_event).await?;
    }

    Ok(())
}

fn grant_used_audit_event_from_event(event: &AgentEvent) -> Option<CreateAuditEvent> {
    if event.kind != EventKind::PolicyEvaluated {
        return None;
    }

    let grant_id = event.payload.get("decision")?.get("grant_id")?.as_str()?;

    Some(CreateAuditEvent {
        id: format!("aud_{}_grant_used", event.event_id.as_str()),
        kind: "permission_grant.used".to_string(),
        actor: Some("agent:local-worker".to_string()),
        resource_kind: "permission_grant".to_string(),
        resource_id: grant_id.to_string(),
        run_id: Some(event.run_id.clone()),
        payload_json: serde_json::json!({
            "grant_id": grant_id,
            "session_id": event.session_id.as_str(),
            "run_id": event.run_id.as_str(),
            "source_event_id": event.event_id.as_str(),
            "action": event.payload.get("action"),
            "decision": event.payload.get("decision"),
            "run_scope": event.payload.get("run_scope"),
        }),
    })
}

fn file_change_from_event(event: &AgentEvent) -> Option<CreateFileChange> {
    if event.kind != EventKind::ToolFinished {
        return None;
    }

    let content = event.payload.get("content")?;
    let path = content.get("path")?.as_str()?;
    let diff = content.get("diff")?.as_str()?;

    Some(CreateFileChange {
        id: format!("chg_{}", event.event_id.as_str()),
        session_id: event.session_id.clone(),
        run_id: event.run_id.clone(),
        path: path.to_string(),
        before_hash: None,
        after_hash: None,
        diff: diff.to_string(),
    })
}

fn artifact_from_event(event: &AgentEvent) -> Option<CreateArtifact> {
    if event.kind != EventKind::ToolFinished {
        return None;
    }

    let content = event.payload.get("content")?;
    let source = content.get("source")?.as_str()?;
    if !matches!(
        source,
        "kubernetes" | "argocd" | "prometheus" | "loki" | "tekton"
    ) {
        return None;
    }

    let kind = if source == "tekton"
        && content.get("resource").and_then(serde_json::Value::as_str)
            == Some("pipeline_run_analysis")
    {
        "pipeline_run_analysis".to_string()
    } else {
        format!("{source}_tool_result")
    };

    Some(CreateArtifact {
        id: format!("art_{}", event.event_id.as_str()),
        session_id: event.session_id.clone(),
        run_id: Some(event.run_id.clone()),
        kind,
        label: event
            .payload
            .get("summary")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("tool result")
            .to_string(),
        mime_type: Some("application/json".to_string()),
        path: None,
        content_text: None,
        content_json: Some(content.clone()),
    })
}

fn observation_from_event(
    event: &AgentEvent,
    artifact_id: Option<String>,
) -> Option<CreateObservation> {
    if event.kind != EventKind::ToolFinished {
        return None;
    }

    let content = event.payload.get("content")?;
    let source = content.get("source")?.as_str()?;
    if !matches!(
        source,
        "kubernetes" | "argocd" | "prometheus" | "loki" | "tekton"
    ) {
        return None;
    }

    let kind = observation_kind(content, source);
    let subject = observation_subject(content, source, &kind);
    let identity = observation_resource_identity(content, source, &kind, &subject);
    let summary = event
        .payload
        .get("summary")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("observed tool result")
        .to_string();
    let resource_ref_json = observation_resource_ref(event, content, source, &kind, &subject);

    Some(CreateObservation {
        id: format!("obs_{}", event.event_id.as_str()),
        session_id: event.session_id.clone(),
        run_id: Some(event.run_id.clone()),
        source: source.to_string(),
        kind,
        subject,
        summary,
        resource_namespace: identity.namespace,
        resource_kind: identity.kind,
        resource_name: identity.name,
        resource_ref_json,
        artifact_id,
        data_json: observation_data(content),
    })
}

fn observation_kind(content: &serde_json::Value, source: &str) -> String {
    content
        .get("resource")
        .and_then(serde_json::Value::as_str)
        .or_else(|| content.get("action").and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .unwrap_or_else(|| format!("{source}_read"))
}

fn observation_subject(content: &serde_json::Value, source: &str, kind: &str) -> String {
    if let Some(query) = content.get("query").and_then(serde_json::Value::as_str) {
        return query.to_string();
    }
    if let Some(name) = content.get("name").and_then(serde_json::Value::as_str) {
        return name.to_string();
    }
    if let Some(namespace) = content.get("namespace").and_then(serde_json::Value::as_str) {
        return format!("{namespace}/{kind}");
    }
    format!("{source}/{kind}")
}

#[derive(Debug, Default)]
struct ObservationResourceIdentity {
    namespace: Option<String>,
    kind: Option<String>,
    name: Option<String>,
}

fn observation_resource_identity(
    content: &serde_json::Value,
    source: &str,
    kind: &str,
    subject: &str,
) -> ObservationResourceIdentity {
    ObservationResourceIdentity {
        namespace: first_string(&[
            content.pointer("/namespace"),
            content.pointer("/output/metadata/namespace"),
            content.pointer("/analysis/pipeline_run/namespace"),
        ]),
        kind: observation_resource_kind(content, source, kind),
        name: first_string(&[
            content.pointer("/name"),
            content.pointer("/output/metadata/name"),
            content.pointer("/analysis/pipeline_run/name"),
        ])
        .or_else(|| normalized_resource_name(source, kind, subject)),
    }
}

fn observation_resource_kind(
    content: &serde_json::Value,
    source: &str,
    kind: &str,
) -> Option<String> {
    let output_kind = content
        .pointer("/output/kind")
        .and_then(serde_json::Value::as_str);
    if output_kind.is_some_and(|value| value != "List") {
        return output_kind.map(str::to_string);
    }
    if source == "tekton" && kind == "pipeline_run_analysis" {
        return Some("PipelineRun".to_string());
    }

    first_string(&[
        content.pointer("/analysis/pipeline_run/kind"),
        content.pointer("/resource"),
    ])
    .or_else(|| normalized_resource_kind(source, kind))
}

fn normalized_resource_kind(source: &str, kind: &str) -> Option<String> {
    match (source, kind) {
        ("argocd", _) => Some("Application".to_string()),
        ("prometheus", "inventory") => Some("inventory".to_string()),
        ("prometheus", _) => Some("query".to_string()),
        ("loki", "log_summary") => Some("log_summary".to_string()),
        ("tekton", "pipeline_run_analysis") => Some("PipelineRun".to_string()),
        (_, value) if !value.trim().is_empty() => Some(value.to_string()),
        _ => None,
    }
}

fn normalized_resource_name(source: &str, kind: &str, subject: &str) -> Option<String> {
    match (source, kind) {
        ("prometheus", "inventory") => Some("inventory".to_string()),
        ("loki", "log_summary") => Some("log_summary".to_string()),
        _ if !subject.trim().is_empty() && !subject.contains('/') => Some(subject.to_string()),
        _ => None,
    }
}

fn first_string(values: &[Option<&serde_json::Value>]) -> Option<String> {
    values
        .iter()
        .filter_map(|value| value.and_then(serde_json::Value::as_str))
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_string)
}

fn observation_resource_ref(
    event: &AgentEvent,
    content: &serde_json::Value,
    source: &str,
    kind: &str,
    subject: &str,
) -> Option<serde_json::Value> {
    let mut metadata = serde_json::Map::new();
    metadata.insert(
        "event_id".to_string(),
        serde_json::Value::String(event.event_id.to_string()),
    );
    metadata.insert(
        "run_id".to_string(),
        serde_json::Value::String(event.run_id.to_string()),
    );

    let mut resource =
        ResourceRef::new(source, kind, subject).with_metadata(serde_json::Value::Object(metadata));
    if let Some(namespace) = content.get("namespace").and_then(serde_json::Value::as_str) {
        resource = resource.with_namespace(namespace);
    }

    serde_json::to_value(resource).ok()
}

fn observation_data(content: &serde_json::Value) -> serde_json::Value {
    let mut data = serde_json::Map::new();
    copy_observation_field(&mut data, content, "source");
    copy_observation_field(&mut data, content, "resource");
    copy_observation_field(&mut data, content, "namespace");
    copy_observation_field(&mut data, content, "name");
    copy_observation_field(&mut data, content, "query");
    copy_observation_field(&mut data, content, "output");
    copy_observation_field(&mut data, content, "response");
    copy_observation_field(&mut data, content, "analysis");

    serde_json::Value::Object(data)
}

fn incident_from_observation(observation: &StoredObservation) -> Option<CreateIncident> {
    if observation.source != "tekton" || observation.kind != "pipeline_run_analysis" {
        return None;
    }

    let analysis = observation.data_json.get("analysis")?;
    let reasons = pipeline_run_incident_reasons(analysis);
    if reasons.is_empty() {
        return None;
    }

    let severity = pipeline_run_incident_severity(&reasons);
    let resource = observation_resource_label(observation);
    let summary = reasons.join("; ");

    Some(CreateIncident {
        id: format!("inc_{}", observation.id),
        observation_id: observation.id.clone(),
        session_id: observation.session_id.clone(),
        run_id: observation.run_id.clone(),
        status: "candidate".to_string(),
        severity: severity.to_string(),
        title: format!("Tekton PipelineRun issue: {resource}"),
        summary: summary.clone(),
        resource_namespace: observation.resource_namespace.clone(),
        resource_kind: observation.resource_kind.clone(),
        resource_name: observation.resource_name.clone(),
        data_json: serde_json::json!({
            "source": "observation",
            "observation_id": observation.id.clone(),
            "reasons": reasons,
            "summary": summary,
        }),
    })
}

fn remediation_plan_from_incident(incident: &StoredIncident) -> Option<CreateRemediationPlan> {
    if incident.status != "candidate" {
        return None;
    }

    let resource = incident_resource_label(incident);
    Some(CreateRemediationPlan {
        id: format!("rplan_{}", incident.id),
        incident_id: incident.id.clone(),
        session_id: incident.session_id.clone(),
        run_id: incident.run_id.clone(),
        status: "draft".to_string(),
        title: format!("Draft remediation for {resource}"),
        summary: "Review the incident evidence, run read-only checks, then require approval before any write, pipeline, or cluster mutation.".to_string(),
        risk_level: incident.severity.clone(),
        requires_approval: true,
        resource_namespace: incident.resource_namespace.clone(),
        resource_kind: incident.resource_kind.clone(),
        resource_name: incident.resource_name.clone(),
        plan_json: remediation_plan_json(incident, &resource),
    })
}

fn remediation_plan_json(incident: &StoredIncident, resource: &str) -> serde_json::Value {
    let reasons = incident
        .data_json
        .get("reasons")
        .cloned()
        .unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
    serde_json::json!({
        "mode": "read_only_draft",
        "incident_id": incident.id.clone(),
        "resource": {
            "namespace": incident.resource_namespace.clone(),
            "kind": incident.resource_kind.clone(),
            "name": incident.resource_name.clone(),
            "label": resource,
        },
        "evidence": {
            "summary": incident.summary.clone(),
            "reasons": reasons,
        },
        "steps": [
            {
                "order": 1,
                "kind": "read_only",
                "capability": "tekton_analyze_pipeline_run",
                "summary": "Re-read PipelineRun, TaskRuns, Deployment health, Argo health, and image alignment before deciding on any action."
            },
            {
                "order": 2,
                "kind": "read_only",
                "capability": "loki_log_summary",
                "summary": "Inspect bounded application and controller logs for the affected namespace if Loki is configured."
            },
            {
                "order": 3,
                "kind": "proposal",
                "capability": "worktree_change",
                "summary": "If evidence points to repo configuration or application code, prepare a ChangeSet and require approval before file writes."
            },
            {
                "order": 4,
                "kind": "proposal",
                "capability": "pipeline_or_deployment_action",
                "summary": "If evidence points to stale deployment state, propose rerun, sync, rollback, or restart intent and require explicit approval before mutation."
            }
        ],
        "approval_gates": [
            {
                "kind": "file_write",
                "required_before": "creating or patching a ChangeSet"
            },
            {
                "kind": "pipeline_mutation",
                "required_before": "rerunning or cancelling Tekton resources"
            },
            {
                "kind": "cluster_mutation",
                "required_before": "Argo sync, rollback, restart, scale, or Kubernetes write"
            },
            {
                "kind": "production_impact",
                "required_before": "any action against production-impacting scope"
            }
        ],
        "non_goals": [
            "No automatic mutation in V1",
            "No ticket creation",
            "No notification dispatch",
            "No secret reads"
        ]
    })
}

fn approval_gates_from_remediation_plan(plan: &StoredRemediationPlan) -> Vec<CreateApprovalGate> {
    let gates = plan
        .plan_json
        .get("approval_gates")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();

    gates
        .into_iter()
        .enumerate()
        .filter_map(|(index, gate_json)| {
            let gate_kind = approval_gate_kind(&gate_json)?;
            let gate_order = i64::try_from(index).ok()?.saturating_add(1);
            let required_before = gate_json
                .get("required_before")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("executing a risky action");
            let title = format!("Approve {}", gate_kind.replace('_', " "));
            Some(CreateApprovalGate {
                id: format!(
                    "agate_{}_{}_{}",
                    plan.id,
                    gate_order,
                    safe_id_fragment(&gate_kind)
                ),
                work_item_id: None,
                remediation_plan_id: Some(plan.id.clone()),
                incident_id: Some(plan.incident_id.clone()),
                session_id: plan.session_id.clone(),
                run_id: plan.run_id.clone(),
                status: "pending".to_string(),
                gate_kind: gate_kind.clone(),
                gate_order,
                title,
                summary: format!("Approval required before {required_before}."),
                risk_level: plan.risk_level.clone(),
                resource_namespace: plan.resource_namespace.clone(),
                resource_kind: plan.resource_kind.clone(),
                resource_name: plan.resource_name.clone(),
                gate_json,
            })
        })
        .collect()
}

fn approval_gate_kind(gate_json: &serde_json::Value) -> Option<String> {
    gate_json
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .or_else(|| gate_json.as_str())
        .map(str::trim)
        .filter(|kind| !kind.is_empty())
        .map(ToOwned::to_owned)
}

fn safe_id_fragment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn pipeline_run_incident_reasons(analysis: &serde_json::Value) -> Vec<String> {
    let mut reasons = Vec::new();
    if let Some(status) = analysis
        .pointer("/summary/status")
        .and_then(serde_json::Value::as_str)
    {
        if !matches!(status, "succeeded" | "running") {
            reasons.push(format!("PipelineRun status is {status}"));
        }
    }
    if let Some(status) = analysis
        .pointer("/deployment/status")
        .and_then(serde_json::Value::as_str)
    {
        if status != "healthy" && status != "skipped" {
            reasons.push(format!("Deployment status is {status}"));
        }
    }
    if let Some(status) = analysis
        .pointer("/summary/argo_sync_status")
        .and_then(serde_json::Value::as_str)
    {
        if status != "Synced" {
            reasons.push(format!("Argo sync status is {status}"));
        }
    }
    if let Some(status) = analysis
        .pointer("/summary/argo_health_status")
        .and_then(serde_json::Value::as_str)
    {
        if status != "Healthy" {
            reasons.push(format!("Argo health status is {status}"));
        }
    }
    if let Some(status) = analysis
        .pointer("/summary/image_alignment/status")
        .and_then(serde_json::Value::as_str)
    {
        if !matches!(status, "exact_match" | "registry_alias_match" | "unknown") {
            reasons.push(format!("Image alignment is {status}"));
        }
    }

    reasons
}

fn pipeline_run_incident_severity(reasons: &[String]) -> &'static str {
    if reasons
        .iter()
        .any(|reason| reason.contains("failed") || reason.contains("error"))
    {
        "high"
    } else {
        "medium"
    }
}

fn observation_resource_label(observation: &StoredObservation) -> String {
    match (
        observation.resource_namespace.as_deref(),
        observation.resource_name.as_deref(),
    ) {
        (Some(namespace), Some(name)) => format!("{namespace}/{name}"),
        (_, Some(name)) => name.to_string(),
        _ => observation.subject.clone(),
    }
}

fn incident_resource_label(incident: &StoredIncident) -> String {
    match (
        incident.resource_namespace.as_deref(),
        incident.resource_name.as_deref(),
    ) {
        (Some(namespace), Some(name)) => format!("{namespace}/{name}"),
        (_, Some(name)) => name.to_string(),
        _ => incident.title.clone(),
    }
}

fn copy_observation_field(
    data: &mut serde_json::Map<String, serde_json::Value>,
    content: &serde_json::Value,
    field: &str,
) {
    if let Some(value) = content.get(field) {
        data.insert(field.to_string(), value.clone());
    }
}

pub(crate) async fn fail_run_from_dispatch(
    store: &SqliteStore,
    run_id: &RunId,
    message: String,
) -> Result<(), StoreError> {
    fail_run_from_worker_boundary(store, run_id, message, false).await
}

pub(crate) async fn fail_run_from_job_creation(
    store: &SqliteStore,
    run_id: &RunId,
    message: String,
) -> Result<(), StoreError> {
    fail_run_from_worker_boundary(store, run_id, message, true).await
}

async fn fail_run_from_worker_boundary(
    store: &SqliteStore,
    run_id: &RunId,
    message: String,
    preserve_budget_resume: bool,
) -> Result<(), StoreError> {
    let seq = store.list_events(run_id).await?.len() as u64 + 1;
    let Some(run) = store.get_run(run_id).await? else {
        return Ok(());
    };
    let has_budget_resume = run.status == "queued"
        && run.budget_consumption.extensions > 0
        && run
            .result_json
            .as_ref()
            .and_then(|result| result.get("budget_extension"))
            .is_some();

    store
        .append_event(&AgentEvent {
            event_id: EventId::new(format!("evt_{}_{}", run_id.as_str(), seq)),
            session_id: run.session_id.clone(),
            run_id: run_id.clone(),
            seq,
            kind: EventKind::RunFailed,
            payload: serde_json::json!({
                "error":message,
                "resume_state_preserved":preserve_budget_resume && has_budget_resume,
            }),
        })
        .await?;

    if preserve_budget_resume && has_budget_resume {
        store
            .mark_budget_extension_dispatch_failed(run_id, &message)
            .await?;
    } else {
        let failed_outcome = AttemptOutcome::failed(message.clone());
        store
            .complete_run(
                run_id,
                "failed",
                serde_json::json!({
                    "status": "failed",
                    "turns": 0,
                    "summary": null,
                    "error": message,
                }),
                Some(message),
            )
            .await?;
        if let Err(error) = sync_repo_stage_run(store, &run, &failed_outcome).await {
            // The Kubernetes reaper retries this idempotent synchronization
            // for failed Jobs. Do not hide the durable Run failure if a
            // secondary stage-finalization write is temporarily unavailable.
            tracing::error!(run_id = %run.id, %error, "failed to seal Repo Mode stage after worker-boundary failure");
        }
        expire_attempt_workspace_grants(store, &run, "controller:worker-boundary").await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        approval_gates_from_remediation_plan, artifact_from_event, attempt_actor,
        attempt_spec_for_run, change_set_can_be_revised_for_work_plan, classify_work_item_attempt,
        create_repo_change_set, file_change_from_event, finish_run_from_attempt,
        grant_used_audit_event_from_event, incident_from_observation, observation_from_event,
        persist_workspace_evidence, remediation_plan_from_incident, result_json_for_attempt,
        structured_submission_from_events, validate_repo_work_plan, validate_workspace_evidence,
        workspace_source_for_run,
    };
    use pharness_core::{AgentEvent, EventId, EventKind, RunId, SessionId};
    use pharness_runhost::{AttemptOutcome, WorkspaceGitEvidence, WorkspaceSourceSpec};
    use pharness_store::{
        CreateArtifact, CreateRun, CreateSession, CreateStageExecution, CreateWorkItem,
        CreateWorkPlan, CreateWorkspace, SealStageOutcome, SqliteStore, StoredChangeSet,
        StoredIncident, StoredObservation, StoredRemediationPlan, StoredRun, StoredWorkPlan,
    };

    #[test]
    fn rejected_change_set_can_only_advance_to_a_newer_work_plan_revision() {
        let plan = StoredWorkPlan {
            id: "wplan_repo".into(),
            work_item_id: Some("witem_repo".into()),
            remediation_plan_id: None,
            incident_id: None,
            session_id: SessionId::new("ses_repo"),
            run_id: Some(RunId::new("run_plan")),
            status: "approved".into(),
            title: "Corrected plan".into(),
            summary: "Address review evidence".into(),
            risk_level: "medium".into(),
            requires_approval: true,
            resource_namespace: None,
            resource_kind: Some("Repository".into()),
            resource_name: Some("https://github.com/example/repo.git".into()),
            work_plan_json: serde_json::json!({}),
            created_at: "1".into(),
            updated_at: Some("3".into()),
            revision: 2,
            status_changed_at: Some("3".into()),
            status_changed_by: Some("operator".into()),
            status_reason: Some("reviewed correction".into()),
            created_by: Some("operator".into()),
            origin: "operator".into(),
        };
        let mut change_set = StoredChangeSet {
            id: "cset_repo".into(),
            work_item_id: Some("witem_repo".into()),
            work_plan_id: plan.id.clone(),
            remediation_plan_id: None,
            incident_id: None,
            session_id: SessionId::new("ses_repo"),
            run_id: Some(RunId::new("run_builder")),
            status: "rejected".into(),
            title: "Source change".into(),
            summary: "Rejected revision".into(),
            risk_level: "medium".into(),
            material_hash: format!("sha256:{}", "a".repeat(64)),
            revision: 1,
            resource_namespace: None,
            resource_kind: Some("Repository".into()),
            resource_name: Some("https://github.com/example/repo.git".into()),
            change_set_json: serde_json::json!({"work_plan":{"id":"wplan_repo","revision":1}}),
            created_at: "1".into(),
            updated_at: Some("2".into()),
            status_changed_at: Some("2".into()),
            status_changed_by: Some("operator".into()),
            status_reason: Some("review rejected".into()),
        };

        assert!(change_set_can_be_revised_for_work_plan(&change_set, &plan));
        change_set.status = "approved".into();
        assert!(!change_set_can_be_revised_for_work_plan(&change_set, &plan));
        change_set.status = "rejected".into();
        change_set.change_set_json["work_plan"]["revision"] = serde_json::json!(2);
        assert!(!change_set_can_be_revised_for_work_plan(&change_set, &plan));
    }

    #[tokio::test]
    async fn repo_change_set_material_captures_the_current_verify_outcome() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let work_item_id = "witem_change_set_outcome_order";
        let source_commit = "a".repeat(40);
        store
            .create_work_item(CreateWorkItem {
                id: work_item_id.into(),
                status: "verifying".into(),
                title: "capture current verifier".into(),
                intent: "prove ChangeSet outcome ordering".into(),
                acceptance_criteria: vec![],
                source_repo: "https://github.com/example/repo.git".into(),
                source_ref: "main".into(),
                source_commit: Some(source_commit.clone()),
                pipeline_contract_id: None,
                deployment_contract_id: None,
                gitops_repo: None,
                gitops_ref: None,
                gitops_kustomization_path: None,
                gitops_image_name: None,
                target_environment: "dev".into(),
                target_namespace: None,
                argo_application: None,
                workload_kind: None,
                workload_name: None,
                rollback_owner: None,
                production_impacting: false,
                max_attempts: 2,
                max_elapsed_seconds: 600,
                environment_profile_id: None,
                run_budget: Default::default(),
                repository_contract_json: None,
                repository_contract_hash: None,
                environment_preparation_status: "not_required".into(),
                created_by: Some("operator".into()),
            })
            .await
            .unwrap();

        let plan_session_id = SessionId::new("ses_change_set_plan");
        let plan_run_id = RunId::new("run_change_set_plan");
        let builder_session_id = SessionId::new("ses_change_set_builder");
        let builder_run_id = RunId::new("run_change_set_builder");
        let verifier_session_id = SessionId::new("ses_change_set_verifier");
        let verifier_run_id = RunId::new("run_change_set_verifier");
        for (session_id, run_id, title) in [
            (&plan_session_id, &plan_run_id, "plan"),
            (&builder_session_id, &builder_run_id, "builder"),
            (&verifier_session_id, &verifier_run_id, "verifier"),
        ] {
            store
                .create_session(CreateSession {
                    id: session_id.clone(),
                    title: title.into(),
                    cwd: "/workspace".into(),
                })
                .await
                .unwrap();
            store
                .create_run(CreateRun {
                    id: run_id.clone(),
                    session_id: session_id.clone(),
                    user_task: title.into(),
                    cwd: "/workspace".into(),
                    max_turns: 2,
                    initial_status: "completed".into(),
                    execution_target_json: serde_json::json!({"kind":"local_process"}),
                })
                .await
                .unwrap();
        }
        let plan = store
            .create_work_plan(CreateWorkPlan {
                id: "wplan_change_set_outcome_order".into(),
                work_item_id: Some(work_item_id.into()),
                remediation_plan_id: None,
                incident_id: None,
                session_id: plan_session_id,
                run_id: Some(plan_run_id),
                status: "approved".into(),
                title: "approved plan".into(),
                summary: "capture current verifier".into(),
                risk_level: "low".into(),
                requires_approval: true,
                resource_namespace: None,
                resource_kind: Some("Repository".into()),
                resource_name: Some("https://github.com/example/repo.git".into()),
                work_plan_json: serde_json::json!({}),
            })
            .await
            .unwrap();
        let workspace_id = "ws_change_set_outcome_order";
        store
            .create_workspace(CreateWorkspace {
                id: workspace_id.into(),
                work_item_id: work_item_id.into(),
                run_id: Some(builder_run_id.clone()),
                status: "verifying".into(),
                source_repo: "https://github.com/example/repo.git".into(),
                source_ref: "main".into(),
                resolved_commit: Some(source_commit),
                branch: Some("pharness/test".into()),
                retention_status: "retained".into(),
                actor: Some("controller".into()),
                reason: Some("test".into()),
            })
            .await
            .unwrap();
        let implement_execution = store
            .create_stage_execution(CreateStageExecution {
                id: "stageexec_change_set_implement".into(),
                work_item_id: work_item_id.into(),
                stage_key: "implement".into(),
                sequence: 1,
                status: "succeeded".into(),
                agent_profile_id: Some("repo-builder".into()),
                agent_profile_version: Some("v1".into()),
                agent_profile_hash: Some(format!("sha256:{}", "b".repeat(64))),
                context_pack_id: None,
                run_id: Some(builder_run_id.clone()),
                workspace_id: Some(workspace_id.into()),
                input_hash: format!("sha256:{}", "c".repeat(64)),
                input_snapshot: serde_json::json!({}),
            })
            .await
            .unwrap();
        let verify_execution = store
            .create_stage_execution(CreateStageExecution {
                id: "stageexec_change_set_verify".into(),
                work_item_id: work_item_id.into(),
                stage_key: "verify".into(),
                sequence: 1,
                status: "succeeded".into(),
                agent_profile_id: Some("repo-verifier".into()),
                agent_profile_version: Some("v1".into()),
                agent_profile_hash: Some(format!("sha256:{}", "d".repeat(64))),
                context_pack_id: None,
                run_id: Some(verifier_run_id.clone()),
                workspace_id: Some(workspace_id.into()),
                input_hash: format!("sha256:{}", "e".repeat(64)),
                input_snapshot: serde_json::json!({}),
            })
            .await
            .unwrap();
        let implement_outcome = store
            .seal_stage_outcome(SealStageOutcome {
                id: "stageout_change_set_implement".into(),
                stage_execution_id: implement_execution.id.clone(),
                work_item_id: work_item_id.into(),
                stage_key: "implement".into(),
                status: "succeeded".into(),
                content_hash: format!("sha256:{}", "f".repeat(64)),
                outcome: serde_json::json!({"schema_version":pharness_core::STAGE_OUTCOME_SCHEMA}),
                state_version: 1,
                supersedes_outcome_id: None,
                actor: "controller".into(),
                reason: "test".into(),
            })
            .await
            .unwrap();
        let verify_outcome = store
            .seal_stage_outcome(SealStageOutcome {
                id: "stageout_change_set_verify".into(),
                stage_execution_id: verify_execution.id.clone(),
                work_item_id: work_item_id.into(),
                stage_key: "verify".into(),
                status: "succeeded".into(),
                content_hash: format!("sha256:{}", "1".repeat(64)),
                outcome: serde_json::json!({"schema_version":pharness_core::STAGE_OUTCOME_SCHEMA}),
                state_version: 1,
                supersedes_outcome_id: None,
                actor: "controller".into(),
                reason: "test".into(),
            })
            .await
            .unwrap();
        let diff = "diff --git a/readme.md b/readme.md\n--- a/readme.md\n+++ b/readme.md\n@@ -1 +1 @@\n-old\n+new\n";
        store
            .create_artifact(CreateArtifact {
                id: "art_change_set_diff".into(),
                session_id: builder_session_id.clone(),
                run_id: Some(builder_run_id.clone()),
                kind: "workspace_git_diff".into(),
                label: "diff".into(),
                mime_type: Some("text/x-diff".into()),
                path: None,
                content_text: Some(diff.into()),
                content_json: None,
            })
            .await
            .unwrap();
        store
            .create_artifact(CreateArtifact {
                id: "art_change_set_status".into(),
                session_id: builder_session_id,
                run_id: Some(builder_run_id.clone()),
                kind: "workspace_git_status".into(),
                label: "status".into(),
                mime_type: Some("application/json".into()),
                path: None,
                content_text: None,
                content_json: Some(serde_json::json!({"changed_paths":["readme.md"]})),
            })
            .await
            .unwrap();
        let verifier_run = store.get_run(&verifier_run_id).await.unwrap().unwrap();
        let change_set = create_repo_change_set(&store, &verifier_run, &verify_execution)
            .await
            .unwrap();
        assert_eq!(change_set.work_plan_id, plan.id);
        assert_eq!(change_set.run_id.as_ref(), Some(&builder_run_id));
        let refs = change_set.change_set_json["effective_outcomes"]
            .as_array()
            .unwrap();
        assert!(refs.iter().any(|reference| {
            reference["id"] == verify_outcome.id && reference["hash"] == verify_outcome.content_hash
        }));
        assert!(refs.iter().any(|reference| {
            reference["id"] == implement_outcome.id
                && reference["hash"] == implement_outcome.content_hash
        }));
    }

    #[test]
    fn extracts_only_the_requested_typed_submission() {
        let events = [
            (
                "evt_plan",
                "work_plan",
                serde_json::json!({"title":"Plan","summary":"Bounded plan","risk_level":"low","steps":[{"title":"Edit","description":"Change one module"}]}),
            ),
            (
                "evt_test",
                "test_outcome",
                serde_json::json!({"summary":"Declared acceptance completed","acceptance_names":["unit"],"claims":[],"risks":[]}),
            ),
            (
                "evt_verify",
                "verification",
                serde_json::json!({"decision":"approved","summary":"Evidence agrees","evidence_refs":["stageout_test"],"contradictions":[],"risks":[]}),
            ),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (event_id, kind, document))| AgentEvent {
            event_id: EventId::new(event_id),
            session_id: SessionId::new("ses_chain"),
            run_id: RunId::new("run_chain"),
            seq: u64::try_from(index + 1).unwrap(),
            kind: EventKind::ToolFinished,
            payload: serde_json::json!({
                "status":"ok",
                "content":{
                    "structured_submission":true,
                    "kind":kind,
                    "document":document,
                }
            }),
        })
        .collect::<Vec<_>>();

        assert_eq!(
            structured_submission_from_events(&events, "work_plan").unwrap()["title"],
            "Plan"
        );
        assert_eq!(
            structured_submission_from_events(&events, "test_outcome").unwrap()["acceptance_names"],
            serde_json::json!(["unit"])
        );
        assert_eq!(
            structured_submission_from_events(&events, "verification").unwrap()["decision"],
            "approved"
        );
        assert!(structured_submission_from_events(&events, "onboarding_proposal").is_none());
    }

    #[test]
    fn validates_bounded_work_plan_shape() {
        let valid = serde_json::json!({
            "title":"Validation change",
            "summary":"Add a pure validator and tests",
            "risk_level":"medium",
            "steps":[{"title":"Implement","description":"Add the validator"}]
        });
        assert!(validate_repo_work_plan(&valid).is_ok());
        let invalid = serde_json::json!({
            "summary":"Missing actionable descriptions",
            "steps":[{}]
        });
        assert!(validate_repo_work_plan(&invalid).is_err());
    }

    #[test]
    fn result_json_uses_null_for_absent_run_scope() {
        let run = stored_run(serde_json::json!({
            "kind": "local_process",
            "run_scope": null,
        }));
        let outcome = AttemptOutcome {
            status: "completed".to_string(),
            turns: 2,
            summary: Some("done".to_string()),
            error: None,
            approval: None,
            workspace_evidence: None,
            budget_extension: None,
            consumption: run.budget_consumption.clone(),
        };

        let result = result_json_for_attempt(&run, &outcome, None);

        assert!(result["run_scope"].is_null());
        assert_eq!(result["status"], "completed");
    }

    #[test]
    fn classifies_terminal_attempts_from_structured_evidence() {
        let turn_budget = AttemptOutcome::failed("run exceeded max_turns=12");
        let classification = classify_work_item_attempt(&turn_budget, &[]);
        assert_eq!(classification.code, "model_turn_budget_exhausted");
        assert_eq!(classification.recommended_action, "revise_work_plan");

        let policy_event = AgentEvent {
            event_id: EventId::new("evt_policy_denied"),
            session_id: SessionId::new("ses_policy_denied"),
            run_id: RunId::new("run_policy_denied"),
            seq: 1,
            kind: EventKind::PolicyEvaluated,
            payload: serde_json::json!({ "decision": { "decision": "deny" } }),
        };
        let policy = AttemptOutcome::failed("policy denied");
        let classification = classify_work_item_attempt(&policy, &[policy_event]);
        assert_eq!(classification.code, "policy_denied");
        assert_eq!(
            classification.recommended_action,
            "revise_plan_or_authorization"
        );

        let model_request = AgentEvent {
            event_id: EventId::new("evt_model_request"),
            session_id: SessionId::new("ses_model_request"),
            run_id: RunId::new("run_model_request"),
            seq: 1,
            kind: EventKind::ModelRequestStarted,
            payload: serde_json::json!({ "turn": 0 }),
        };
        let provider = AttemptOutcome::failed("provider unavailable");
        let classification = classify_work_item_attempt(&provider, &[model_request]);
        assert_eq!(classification.code, "model_provider_failed");
        assert_eq!(classification.recommended_action, "inspect_and_replan");

        let cluster_run = stored_run(serde_json::json!({ "kind": "kubernetes_workspace" }));
        assert_eq!(attempt_actor(&cluster_run), "agent:cluster-worker");
    }

    #[test]
    fn reconstructs_only_valid_workspace_source_from_persisted_target() {
        let source = workspace_source_for_run(&serde_json::json!({
            "workspace_source": {
                "workspace_id": "ws_test",
                "source_repo": "https://github.com/example/finance-app.git",
                "source_ref": "main",
                "branch": "pharness/test/attempt-1"
            }
        }))
        .unwrap()
        .unwrap();
        assert_eq!(source.workspace_id, "ws_test");

        assert!(workspace_source_for_run(&serde_json::json!({
            "workspace_source": {
                "workspace_id": "ws_test",
                "source_repo": "https://token@example.test/finance-app.git",
                "source_ref": "main",
                "branch": "pharness/test/attempt-1"
            }
        }))
        .is_err());
    }

    #[tokio::test]
    async fn fresh_preparation_keeps_workspace_source_unresolved_until_checkout() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let work_item_id = "witem_preparation_checkout";
        let workspace_id = "ws_preparation_checkout";
        let branch = "pharness/witem_preparation_checkout/attempt-1";
        let source_commit = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let session_id = SessionId::new("ses_preparation_checkout");
        let run_id = RunId::new("run_preparation_checkout");
        store
            .create_work_item(CreateWorkItem {
                id: work_item_id.into(),
                status: "executing".into(),
                title: "prepare checkout".into(),
                intent: "clone the exact source before model execution".into(),
                acceptance_criteria: Vec::new(),
                source_repo: "https://github.com/example/finance-app.git".into(),
                source_ref: "main".into(),
                source_commit: Some(source_commit.into()),
                pipeline_contract_id: None,
                deployment_contract_id: None,
                gitops_repo: None,
                gitops_ref: None,
                gitops_kustomization_path: None,
                gitops_image_name: None,
                target_environment: "repository".into(),
                target_namespace: None,
                argo_application: None,
                workload_kind: None,
                workload_name: None,
                rollback_owner: None,
                production_impacting: false,
                max_attempts: 2,
                max_elapsed_seconds: 3_600,
                environment_profile_id: Some("python-3.11".into()),
                run_budget: Default::default(),
                repository_contract_json: None,
                repository_contract_hash: None,
                environment_preparation_status: "preparing".into(),
                created_by: Some("operator".into()),
            })
            .await
            .unwrap();
        store
            .create_session(CreateSession {
                id: session_id.clone(),
                title: "prepare checkout".into(),
                cwd: "/workspace".into(),
            })
            .await
            .unwrap();
        let run = store
            .create_run(CreateRun {
                id: run_id.clone(),
                session_id,
                user_task: "prepare checkout".into(),
                cwd: "/workspace".into(),
                max_turns: 48,
                initial_status: "preparing".into(),
                execution_target_json: serde_json::json!({
                    "kind":"kubernetes_workspace",
                    "workspace_source":{
                        "workspace_id":workspace_id,
                        "source_repo":"https://github.com/example/finance-app.git",
                        "source_ref":"main",
                        "source_commit":source_commit,
                        "branch":branch,
                        "resolved_commit":null,
                    }
                }),
            })
            .await
            .unwrap();
        store
            .create_workspace(CreateWorkspace {
                id: workspace_id.into(),
                work_item_id: work_item_id.into(),
                run_id: Some(run_id),
                status: "preparing".into(),
                source_repo: "https://github.com/example/finance-app.git".into(),
                source_ref: "main".into(),
                // This records the controller's immutable intent, not a
                // checkout that already exists on the empty PVC.
                resolved_commit: Some(source_commit.into()),
                branch: Some(branch.into()),
                retention_status: "retained".into(),
                actor: Some("operator".into()),
                reason: Some("prepare exact source".into()),
            })
            .await
            .unwrap();

        let spec = attempt_spec_for_run(&store, &run, std::path::Path::new("/workspace"), None)
            .await
            .unwrap();

        assert_eq!(
            spec.run.workspace_source.unwrap().resolved_commit,
            None,
            "preparation must clone before it can claim a resolved checkout"
        );
    }

    #[test]
    fn rejects_workspace_evidence_that_does_not_match_its_source_contract() {
        let source = WorkspaceSourceSpec {
            workspace_id: "ws_test".to_string(),
            source_repo: "https://github.com/example/finance-app.git".to_string(),
            source_ref: "main".to_string(),
            source_commit: None,
            branch: "pharness/test/attempt-1".to_string(),
            resolved_commit: None,
        };
        let evidence = WorkspaceGitEvidence {
            workspace_id: "ws_test".to_string(),
            base_commit: "a1b2c3d4".to_string(),
            branch: "pharness/test/attempt-1".to_string(),
            status: " M README.md".to_string(),
            diff: "--- a/README.md\n+++ b/README.md".to_string(),
            changed_paths: vec!["README.md".to_string()],
        };
        validate_workspace_evidence(&source, &evidence).unwrap();

        let evidence = WorkspaceGitEvidence {
            changed_paths: vec![".env".to_string()],
            ..evidence
        };
        assert!(validate_workspace_evidence(&source, &evidence).is_err());
    }

    #[tokio::test]
    async fn read_only_remote_profile_accepts_only_clean_ephemeral_evidence() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let run = stored_run(serde_json::json!({
            "kind":"kubernetes_workspace",
            "onboarding":{"onboarding_id":"ronb_test"},
            "workspace_source":{
                "workspace_id":"onboarding-ronb_test",
                "source_repo":"https://github.com/example/finance-app.git",
                "source_ref":"main",
                "source_commit":"a".repeat(40),
                "branch":"pharness/onboarding/ronb_test",
                "resolved_commit":"a".repeat(40)
            }
        }));
        let clean = AttemptOutcome {
            status: "completed".into(),
            turns: 1,
            summary: Some("submitted proposal".into()),
            error: None,
            approval: None,
            workspace_evidence: Some(WorkspaceGitEvidence {
                workspace_id: "onboarding-ronb_test".into(),
                base_commit: "a".repeat(40),
                branch: "pharness/onboarding/ronb_test".into(),
                status: String::new(),
                diff: String::new(),
                changed_paths: Vec::new(),
            }),
            budget_extension: None,
            consumption: run.budget_consumption.clone(),
        };
        persist_workspace_evidence(&store, &run, &clean)
            .await
            .unwrap();
        assert!(store.list_artifacts(&run.id).await.unwrap().is_empty());

        let mut dirty = clean;
        dirty.workspace_evidence.as_mut().unwrap().status = " M readme.md".into();
        dirty
            .workspace_evidence
            .as_mut()
            .unwrap()
            .changed_paths
            .push("readme.md".into());
        assert!(persist_workspace_evidence(&store, &run, &dirty)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn completed_remote_workspace_run_persists_durable_git_evidence() {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        let work_item_id = "witem_worker_evidence";
        let workspace_id = "ws_worker_evidence";
        let branch = "pharness/witem_worker_evidence/attempt-1";
        let base_commit = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
        let session_id = SessionId::new("ses_worker_evidence");
        let run_id = RunId::new("run_worker_evidence");
        store
            .create_work_item(CreateWorkItem {
                id: work_item_id.to_string(),
                status: "executing".to_string(),
                title: "worker evidence".to_string(),
                intent: "make a safe change".to_string(),
                acceptance_criteria: Vec::new(),
                source_repo: "https://github.com/example/finance-app.git".to_string(),
                source_ref: "main".to_string(),
                source_commit: None,
                pipeline_contract_id: None,
                deployment_contract_id: None,
                gitops_repo: None,
                gitops_ref: None,
                gitops_kustomization_path: None,
                gitops_image_name: None,
                target_environment: "dev".to_string(),
                target_namespace: None,
                argo_application: None,
                workload_kind: None,
                workload_name: None,
                rollback_owner: None,
                production_impacting: false,
                max_attempts: 1,
                max_elapsed_seconds: 600,
                environment_profile_id: None,
                run_budget: Default::default(),
                repository_contract_json: None,
                repository_contract_hash: None,
                environment_preparation_status: "not_required".to_string(),
                created_by: None,
            })
            .await
            .unwrap();
        store
            .create_session(CreateSession {
                id: session_id.clone(),
                title: "worker evidence".to_string(),
                cwd: "/workspace".to_string(),
            })
            .await
            .unwrap();
        let run = store
            .create_run(CreateRun {
                id: run_id.clone(),
                session_id: session_id.clone(),
                user_task: "worker evidence".to_string(),
                cwd: "/workspace".to_string(),
                max_turns: 2,
                initial_status: "running".to_string(),
                execution_target_json: serde_json::json!({
                    "kind": "kubernetes_workspace",
                    "run_scope": {
                        "work_item_id": work_item_id,
                        "workspace_id": workspace_id,
                        "production_impacting": false
                    },
                    "workspace_source": {
                        "workspace_id": workspace_id,
                        "source_repo": "https://github.com/example/finance-app.git",
                        "source_ref": "main",
                        "branch": branch,
                        "resolved_commit": base_commit
                    }
                }),
            })
            .await
            .unwrap();
        store
            .create_workspace(CreateWorkspace {
                id: workspace_id.to_string(),
                work_item_id: work_item_id.to_string(),
                run_id: Some(run_id.clone()),
                status: "executing".to_string(),
                source_repo: "https://github.com/example/finance-app.git".to_string(),
                source_ref: "main".to_string(),
                resolved_commit: Some(base_commit.to_string()),
                branch: Some(branch.to_string()),
                retention_status: "ephemeral".to_string(),
                actor: None,
                reason: None,
            })
            .await
            .unwrap();

        finish_run_from_attempt(
            &store,
            &run,
            AttemptOutcome {
                status: "completed".to_string(),
                turns: 2,
                summary: Some("changed README".to_string()),
                error: None,
                approval: None,
                workspace_evidence: Some(WorkspaceGitEvidence {
                    workspace_id: workspace_id.to_string(),
                    base_commit: base_commit.to_string(),
                    branch: branch.to_string(),
                    status: " M README.md".to_string(),
                    diff: "diff --git a/README.md b/README.md\n+index 1..2 100644\n--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-before\n+after\n".to_string(),
                    changed_paths: vec!["README.md".to_string()],
                }),
                budget_extension: None,
                consumption: run.budget_consumption.clone(),
            },
        )
        .await
        .unwrap();

        let artifacts = store.list_artifacts(&run_id).await.unwrap();
        assert!(artifacts
            .iter()
            .any(|artifact| artifact.kind == "workspace_git_diff"));
        assert!(artifacts
            .iter()
            .any(|artifact| artifact.kind == "workspace_git_status"));
        let workspace = store.get_workspace(workspace_id).await.unwrap().unwrap();
        assert_eq!(workspace.status, "verifying");
        let work_item = store.get_work_item(work_item_id).await.unwrap().unwrap();
        assert_eq!(work_item.status, "verifying");
        let audit = store
            .list_audit_events(Some("work_item"), Some(work_item_id), None, 10)
            .await
            .unwrap();
        let attempt = audit
            .iter()
            .find(|event| event.kind == "work_item.attempt_finished")
            .expect("terminal coding attempt is auditable");
        assert_eq!(attempt.actor.as_deref(), Some("agent:cluster-worker"));
        assert_eq!(attempt.payload_json["classification"]["code"], "completed");
        assert_eq!(
            attempt.payload_json["classification"]["recommended_action"],
            "capture_change_set"
        );
    }

    #[test]
    fn result_json_preserves_non_empty_run_scope() {
        let run = stored_run(serde_json::json!({
            "kind": "local_process",
            "run_scope": {
                "namespace": "apps-dev",
                "repo": "git@example.test/team/app.git",
                "branch": "feature/pharness",
                "production_impacting": false
            },
        }));
        let outcome = AttemptOutcome {
            status: "completed".to_string(),
            turns: 2,
            summary: Some("done".to_string()),
            error: None,
            approval: None,
            workspace_evidence: None,
            budget_extension: None,
            consumption: run.budget_consumption.clone(),
        };

        let result = result_json_for_attempt(&run, &outcome, None);

        assert_eq!(result["run_scope"]["namespace"], "apps-dev");
    }

    #[test]
    fn extracts_file_change_from_write_tool_event() {
        let event = AgentEvent {
            event_id: EventId::new("evt_run_test_8"),
            session_id: SessionId::new("ses_test"),
            run_id: RunId::new("run_test"),
            seq: 8,
            kind: EventKind::ToolFinished,
            payload: serde_json::json!({
                "status": "ok",
                "summary": "wrote file",
                "content": {
                    "path": "README.md",
                    "diff": "--- before\n+++ after"
                }
            }),
        };

        let change = file_change_from_event(&event).unwrap();

        assert_eq!(change.id, "chg_evt_run_test_8");
        assert_eq!(change.path, "README.md");
        assert!(change.diff.contains("+++ after"));
    }

    #[test]
    fn extracts_permission_grant_used_audit_event() {
        let event = AgentEvent {
            event_id: EventId::new("evt_run_test_7"),
            session_id: SessionId::new("ses_test"),
            run_id: RunId::new("run_test"),
            seq: 7,
            kind: EventKind::PolicyEvaluated,
            payload: serde_json::json!({
                "action": "write_file",
                "decision": {
                    "decision": "allow",
                    "risk": "medium",
                    "summary": "allowed by grant",
                    "grant_id": "pgrant_test"
                },
                "run_scope": {
                    "namespace": "apps-dev",
                    "repo": "git@example.test/team/app.git",
                    "branch": "feature/pharness",
                    "production_impacting": false
                }
            }),
        };

        let audit_event = grant_used_audit_event_from_event(&event).unwrap();

        assert_eq!(audit_event.kind, "permission_grant.used");
        assert_eq!(audit_event.resource_id, "pgrant_test");
        assert_eq!(audit_event.run_id.as_ref().unwrap().as_str(), "run_test");
        assert_eq!(
            audit_event.payload_json["run_scope"]["namespace"],
            "apps-dev"
        );
    }

    #[test]
    fn extracts_cluster_artifact_from_tool_event() {
        let event = AgentEvent {
            event_id: EventId::new("evt_run_test_8"),
            session_id: SessionId::new("ses_test"),
            run_id: RunId::new("run_test"),
            seq: 8,
            kind: EventKind::ToolFinished,
            payload: serde_json::json!({
                "status": "ok",
                "summary": "read Prometheus instant query",
                "content": {
                    "source": "prometheus",
                    "query": "up",
                    "response": {
                        "data": {
                            "result_count": 33
                        }
                    }
                }
            }),
        };

        let artifact = artifact_from_event(&event).unwrap();

        assert_eq!(artifact.id, "art_evt_run_test_8");
        assert_eq!(artifact.kind, "prometheus_tool_result");
        assert_eq!(artifact.label, "read Prometheus instant query");
        assert_eq!(
            artifact.content_json.unwrap()["response"]["data"]["result_count"],
            33
        );
    }

    #[test]
    fn extracts_observation_from_tool_event() {
        let event = AgentEvent {
            event_id: EventId::new("evt_run_test_11"),
            session_id: SessionId::new("ses_test"),
            run_id: RunId::new("run_test"),
            seq: 11,
            kind: EventKind::ToolFinished,
            payload: serde_json::json!({
                "status": "ok",
                "summary": "read Prometheus instant query",
                "content": {
                    "source": "prometheus",
                    "query": "up",
                    "response": {
                        "data": {
                            "result_count": 33
                        }
                    }
                }
            }),
        };

        let observation =
            observation_from_event(&event, Some("art_evt_run_test_11".to_string())).unwrap();

        assert_eq!(observation.id, "obs_evt_run_test_11");
        assert_eq!(observation.source, "prometheus");
        assert_eq!(observation.kind, "prometheus_read");
        assert_eq!(observation.subject, "up");
        assert_eq!(observation.resource_kind.as_deref(), Some("query"));
        assert_eq!(observation.resource_name.as_deref(), Some("up"));
        assert_eq!(
            observation.artifact_id.as_deref(),
            Some("art_evt_run_test_11")
        );
        assert_eq!(
            observation.data_json["response"]["data"]["result_count"],
            33
        );
    }

    #[test]
    fn extracts_loki_artifact_from_tool_event() {
        let event = AgentEvent {
            event_id: EventId::new("evt_run_test_10"),
            session_id: SessionId::new("ses_test"),
            run_id: RunId::new("run_test"),
            seq: 10,
            kind: EventKind::ToolFinished,
            payload: serde_json::json!({
                "status": "ok",
                "summary": "read Loki log summary",
                "content": {
                    "source": "loki",
                    "resource": "log_summary",
                    "response": {
                        "data": {
                            "entry_count": 3
                        }
                    }
                }
            }),
        };

        let artifact = artifact_from_event(&event).unwrap();

        assert_eq!(artifact.id, "art_evt_run_test_10");
        assert_eq!(artifact.kind, "loki_tool_result");
        assert_eq!(artifact.label, "read Loki log summary");
        assert_eq!(
            artifact.content_json.unwrap()["response"]["data"]["entry_count"],
            3
        );
    }

    #[test]
    fn extracts_pipeline_run_analysis_artifact_from_tool_event() {
        let event = AgentEvent {
            event_id: EventId::new("evt_run_test_9"),
            session_id: SessionId::new("ses_test"),
            run_id: RunId::new("run_test"),
            seq: 9,
            kind: EventKind::ToolFinished,
            payload: serde_json::json!({
                "status": "ok",
                "summary": "analyzed Tekton PipelineRun ci/build-app",
                "content": {
                    "source": "tekton",
                    "resource": "pipeline_run_analysis",
                    "namespace": "ci",
                    "name": "build-app",
                    "analysis": {
                        "kind": "PipelineRunAnalysis",
                        "summary": {
                            "status": "failed"
                        }
                    }
                }
            }),
        };

        let artifact = artifact_from_event(&event).unwrap();
        let observation = observation_from_event(&event, Some(artifact.id.clone())).unwrap();

        assert_eq!(artifact.id, "art_evt_run_test_9");
        assert_eq!(artifact.kind, "pipeline_run_analysis");
        assert_eq!(artifact.label, "analyzed Tekton PipelineRun ci/build-app");
        assert_eq!(
            artifact.content_json.unwrap()["analysis"]["summary"]["status"],
            "failed"
        );
        assert_eq!(observation.resource_namespace.as_deref(), Some("ci"));
        assert_eq!(observation.resource_kind.as_deref(), Some("PipelineRun"));
        assert_eq!(observation.resource_name.as_deref(), Some("build-app"));
    }

    #[test]
    fn extracts_incident_candidate_from_failed_pipeline_observation() {
        let observation = StoredObservation {
            id: "obs_test".to_string(),
            session_id: SessionId::new("ses_test"),
            run_id: Some(RunId::new("run_test")),
            source: "tekton".to_string(),
            kind: "pipeline_run_analysis".to_string(),
            subject: "build-app".to_string(),
            summary: "analyzed Tekton PipelineRun ci/build-app".to_string(),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-app".to_string()),
            resource_ref_json: None,
            artifact_id: Some("art_test".to_string()),
            data_json: serde_json::json!({
                "analysis": {
                    "summary": {
                        "status": "failed",
                        "argo_sync_status": "OutOfSync",
                        "argo_health_status": "Degraded",
                        "image_alignment": {
                            "status": "registry_mismatch"
                        }
                    },
                    "deployment": {
                        "status": "progressing"
                    }
                }
            }),
            observed_at: "1".to_string(),
        };

        let incident = incident_from_observation(&observation).unwrap();

        assert_eq!(incident.id, "inc_obs_test");
        assert_eq!(incident.status, "candidate");
        assert_eq!(incident.severity, "high");
        assert_eq!(incident.resource_namespace.as_deref(), Some("ci"));
        assert_eq!(incident.resource_kind.as_deref(), Some("PipelineRun"));
        assert_eq!(incident.resource_name.as_deref(), Some("build-app"));
        assert!(
            incident
                .data_json
                .get("reasons")
                .and_then(serde_json::Value::as_array)
                .unwrap()
                .len()
                >= 4
        );
    }

    #[test]
    fn extracts_draft_remediation_plan_from_incident_candidate() {
        let incident = StoredIncident {
            id: "inc_obs_test".to_string(),
            observation_id: "obs_test".to_string(),
            session_id: SessionId::new("ses_test"),
            run_id: Some(RunId::new("run_test")),
            status: "candidate".to_string(),
            severity: "high".to_string(),
            title: "Tekton PipelineRun issue: ci/build-app".to_string(),
            summary: "PipelineRun status is failed".to_string(),
            resource_namespace: Some("ci".to_string()),
            resource_kind: Some("PipelineRun".to_string()),
            resource_name: Some("build-app".to_string()),
            data_json: serde_json::json!({
                "reasons": ["PipelineRun status is failed"]
            }),
            created_at: "1".to_string(),
        };

        let plan = remediation_plan_from_incident(&incident).unwrap();

        assert_eq!(plan.id, "rplan_inc_obs_test");
        assert_eq!(plan.incident_id, "inc_obs_test");
        assert_eq!(plan.status, "draft");
        assert_eq!(plan.risk_level, "high");
        assert!(plan.requires_approval);
        assert_eq!(plan.resource_namespace.as_deref(), Some("ci"));
        assert_eq!(
            plan.plan_json["approval_gates"]
                .as_array()
                .map(Vec::len)
                .unwrap_or_default(),
            4
        );
        assert_eq!(plan.plan_json["mode"], "read_only_draft");

        let stored_plan = StoredRemediationPlan {
            id: plan.id,
            incident_id: plan.incident_id,
            session_id: plan.session_id,
            run_id: plan.run_id,
            status: plan.status,
            title: plan.title,
            summary: plan.summary,
            risk_level: plan.risk_level,
            requires_approval: plan.requires_approval,
            resource_namespace: plan.resource_namespace,
            resource_kind: plan.resource_kind,
            resource_name: plan.resource_name,
            plan_json: plan.plan_json,
            created_at: "1".to_string(),
        };
        let gates = approval_gates_from_remediation_plan(&stored_plan);

        assert_eq!(gates.len(), 4);
        assert_eq!(gates[0].id, "agate_rplan_inc_obs_test_1_file_write");
        assert_eq!(gates[0].gate_kind, "file_write");
        assert_eq!(gates[0].gate_order, 1);
        assert_eq!(gates[0].status, "pending");
        assert_eq!(gates[0].risk_level, "high");
        assert_eq!(gates[0].resource_namespace.as_deref(), Some("ci"));
    }

    fn stored_run(execution_target_json: serde_json::Value) -> StoredRun {
        StoredRun {
            id: RunId::new("run_test"),
            session_id: SessionId::new("ses_test"),
            cwd: ".".to_string(),
            status: "queued".to_string(),
            user_task: "test".to_string(),
            max_turns: 40,
            run_budget: Default::default(),
            budget_consumption: Default::default(),
            stop_reason: None,
            retention_state: "retained".into(),
            sealed_summary: None,
            started_at: "0".to_string(),
            finished_at: None,
            cancel_requested_at: None,
            error: None,
            result_json: None,
            execution_target_json,
            origin: "legacy".to_string(),
            created_by: None,
        }
    }
}

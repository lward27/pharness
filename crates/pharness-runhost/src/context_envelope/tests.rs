use super::*;
use crate::{
    AttemptBackend, AttemptHost, AttemptOutcome, AttemptSpec, BudgetResumeSpec, ResumeSpec,
    RunInferenceSpec,
};
use pharness_core::{
    AgentAction, AgentEvent, CancellationFlag, ContextBudget, EventKind, ModelCapabilities,
    ModelProvider, ModelRequest, ModelToolCall, ModelTurn, ProviderError, ResolvedInferenceBinding,
    SafetyPolicy,
};
use std::{
    collections::VecDeque,
    path::PathBuf,
    process::Command,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "pharness-context-envelope-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("README.md"), "# Bounded context fixture\n").unwrap();
        std::fs::write(
            root.join("AGENTS.md"),
            "Repository guidance is subordinate to the saved contract.\n",
        )
        .unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["add", "README.md", "AGENTS.md"],
            vec![
                "-c",
                "user.name=Context Fixture",
                "-c",
                "user.email=context@example.invalid",
                "commit",
                "-qm",
                "initial",
            ],
        ] {
            let result = Command::new("git")
                .args(args)
                .current_dir(&root)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
        Self(root)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(root: &Path, envelope: bool) -> RunSpec {
    let mut registry: pharness_core::InferenceRegistry =
        serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../deploy/helm/pharness/files/inference-registry.json"
        )))
        .unwrap();
    registry.finalize_hashes().unwrap();
    let policy = registry
        .policy("onboarding-minimax-m3-v2", "v1")
        .unwrap()
        .clone();
    let target = registry
        .target(&policy.target.target_id, &policy.target.revision)
        .unwrap()
        .clone();
    let profile = pharness_core::compiled_reliability_v2_agent_profiles(
        &target.upstream_model,
        crate::RELIABILITY_V2_PROMPT_BUNDLE_VERSION,
    )
    .into_iter()
    .find(|p| p.id == "repository-onboarding-proposer")
    .unwrap();
    let mut binding = ResolvedInferenceBinding {
        schema_version: pharness_core::RESOLVED_INFERENCE_BINDING_SCHEMA.into(),
        target,
        policy: policy.clone(),
        prompt_version: crate::SYSTEM_PROMPT_VERSION.into(),
        stage_prompt: Some(
            crate::stage_prompt_for_profile(&profile.id)
                .unwrap()
                .revision_record(),
        ),
        tool_schema_hash: crate::constrained_tool_schema_hash(&profile.tools, &[], &[]).unwrap(),
        context_policy_hash: if envelope {
            context_policy_hash(
                InferenceStage::Onboarding,
                policy.max_input_tokens,
                policy.max_output_tokens,
            )
            .unwrap()
        } else {
            canonical_json_sha256(&policy_value(
                InferenceStage::Onboarding,
                policy.max_input_tokens,
                policy.max_output_tokens,
            ))
            .unwrap()
        },
        protocol_calibration_hash: canonical_json_sha256(&json!({"fixture":"protocol"})).unwrap(),
        profile_budget_hash: canonical_json_sha256(&serde_json::to_value(&profile.budget).unwrap())
            .unwrap(),
        base_agent_profile_hash: profile.profile_hash.clone(),
        agent_profile_hash: String::new(),
        binding_hash: String::new(),
    };
    binding.agent_profile_hash = binding.computed_agent_profile_hash().unwrap();
    binding.binding_hash = binding.computed_hash().unwrap();
    binding.validate().unwrap();
    let mut run: RunSpec = serde_json::from_value(json!({
        "run_id":"run_context", "session_id":"session_context", "cwd":root,
        "user_task":"Propose configuration from the original discovery without inventing a lock.",
        "max_turns":profile.budget.initial_turns,
        "execution_target_json":{
            "agent_profile":profile, "onboarding_submission_contract":crate::ONBOARDING_SUBMISSION_CONTRACT,
            "onboarding":{"onboarding_id":"onboard_context", "discovery_id":"rdisc_context", "discovery_hash":format!("sha256:{}", "a".repeat(64))},
            "agent_context":{
                "schema_version":pharness_core::AGENT_CONTEXT_SCHEMA,
                "subject":{"kind":"repository_onboarding", "id":"onboard_context"},
                "discovery":{"id":"rdisc_context", "hash":format!("sha256:{}", "a".repeat(64)), "blockers":["immutable_dependency_lock_missing"]},
                "verified_facts":["README.md exists"], "model_claims":[], "evidence_catalog":[]
            }
        },
        "run_budget":profile.budget,
        "budget_consumption":{"allowed_turns":profile.budget.initial_turns,"allowed_tokens":profile.budget.initial_tokens,"turns_used":0,"tokens_used":0,"active_execution_seconds_used":0,"extensions":0}
    })).unwrap();
    run.inference = Some(RunInferenceSpec {
        selection_id: "selection_context".into(),
        stage_execution_id: "stage_context".into(),
        binding,
        next_request_sequence: 1,
    });
    run
}

fn submit() -> AgentAction {
    AgentAction::SubmitOnboardingProposal {
        id: "act_submit".into(),
        reason: "retain missing lock".into(),
        proposal: json!({
            "candidate_contract":null, "instructions":"Resolve the immutable lock before execution.",
            "service_proposals":[], "binding_proposals":[], "assumptions":[], "conflicts":[],
            "blockers":["immutable_dependency_lock_missing"], "readiness_forecast":{"coding":"blocked"}
        }),
    }
}
fn read(id: &str, path: &str) -> AgentAction {
    AgentAction::ReadFile {
        id: id.into(),
        reason: "inspect bounded source".into(),
        path: path.into(),
        start_line: None,
        line_count: None,
        max_bytes: None,
    }
}

#[derive(Default)]
struct Backend {
    events: Mutex<Vec<AgentEvent>>,
    outcome: Mutex<Option<AttemptOutcome>>,
}
#[async_trait::async_trait]
impl AttemptBackend for Backend {
    async fn mark_running(&self) -> anyhow::Result<()> {
        Ok(())
    }
    async fn ingest_event(&self, event: &AgentEvent) -> anyhow::Result<()> {
        self.events.lock().unwrap().push(event.clone());
        Ok(())
    }
    async fn finish(&self, outcome: AttemptOutcome) -> anyhow::Result<()> {
        *self.outcome.lock().unwrap() = Some(outcome);
        Ok(())
    }
}
struct Provider {
    actions: Mutex<VecDeque<AgentAction>>,
    requests: Mutex<Vec<ModelRequest>>,
}
impl Provider {
    fn new(actions: Vec<AgentAction>) -> Self {
        Self {
            actions: Mutex::new(actions.into()),
            requests: Mutex::new(vec![]),
        }
    }
}
#[async_trait::async_trait]
impl ModelProvider for Provider {
    async fn complete_action(&self, request: ModelRequest) -> Result<ModelTurn, ProviderError> {
        self.requests.lock().unwrap().push(request);
        let action = self.actions.lock().unwrap().pop_front().ok_or_else(|| {
            ProviderError::MalformedResponse {
                message: "fixture exhausted".into(),
            }
        })?;
        Ok(ModelTurn {
            raw_provider_id: Some("context-fixture".into()),
            assistant_message: None,
            assistant_tool_calls: vec![],
            action,
            usage: None,
            reasoning: None,
            metadata: None,
        })
    }
    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            native_tool_calling: false,
            streaming: false,
            json_schema_response_format: true,
        }
    }
}
fn host(provider: Arc<Provider>) -> AttemptHost {
    AttemptHost {
        provider,
        cluster_tools: Default::default(),
        default_policy: SafetyPolicy {
            mode: pharness_core::PolicyMode::TrustedWrites,
            require_approval_for_writes: false,
            ..Default::default()
        },
        context_budget: Default::default(),
    }
}
fn spec(run: RunSpec) -> AttemptSpec {
    AttemptSpec {
        run,
        event_seq_start: 0,
        resume: None,
        budget_resume: None,
    }
}

fn assert_requests(run: &RunSpec, backend: &Backend, provider: &Provider) {
    let requests = provider.requests.lock().unwrap();
    let events = backend.events.lock().unwrap();
    let recorded = events
        .iter()
        .filter(|e| e.kind == EventKind::ModelRequestStarted)
        .collect::<Vec<_>>();
    assert_eq!(recorded.len(), requests.len());
    if let Some(started) = events.iter().find(|e| e.kind == EventKind::RunStarted) {
        let context = &started.payload["initial_context"];
        let original = context["messages"].clone();
        assert_eq!(
            context["content_hash"],
            canonical_json_sha256(&original).unwrap()
        );
        let saved: Vec<ModelMessage> = serde_json::from_value(original).unwrap();
        validate_saved(run, &saved).unwrap();
    }
    let policy = &run.inference.as_ref().unwrap().binding.policy;
    for (request, event) in requests.iter().zip(recorded) {
        assert_eq!(
            event.payload["input_context_sha256"],
            canonical_json_sha256(&serde_json::to_value(&request.messages).unwrap()).unwrap()
        );
        let first = &request.messages[0].content;
        let envelope: Envelope = serde_json::from_str(first.strip_prefix(PREFIX).unwrap()).unwrap();
        assert_eq!(
            envelope.payload.data.agent_context,
            run.execution_target_json["agent_context"]
        );
        assert!(request
            .messages
            .iter()
            .any(|m| m.role == ModelRole::System && m.content.contains("Execution budget:")));
        assert!(request
            .messages
            .iter()
            .any(|m| m.role == ModelRole::System
                && m.content.contains("Controller execution ledger")));
        for model in [
            "accounts/fireworks/models/minimax-m3",
            "accounts/fireworks/models/kimi-k3",
            "accounts/fireworks/models/glm-5p3",
        ] {
            let wire = pharness_openai_compatible::build_chat_request(
                pharness_core::InferenceBackendKind::Fireworks,
                model,
                request.clone(),
                policy,
                true,
                None,
            );
            assert!(
                wire.messages[0].content.contains(first),
                "{model}: entire initial envelope must reach the first system message"
            );
            assert_eq!(
                wire.messages
                    .iter()
                    .filter(|m| m.role != "system")
                    .map(|m| (&m.content, &m.tool_call_id))
                    .collect::<Vec<_>>(),
                request
                    .messages
                    .iter()
                    .filter(|m| m.role != ModelRole::System)
                    .map(|m| (&m.content, &m.tool_call_id))
                    .collect::<Vec<_>>()
            );
            assert_eq!(wire.max_tokens, policy.max_output_tokens);
        }
    }
}

#[test]
fn envelope_separates_rules_and_data_and_preserves_legacy_policy_selection() {
    let workspace = Workspace::new();
    let current = run(&workspace.0, true);
    let (messages, files) = assemble(&workspace.0, &current).unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(files.len(), 1);
    let envelope: Envelope =
        serde_json::from_str(messages[0].content.strip_prefix(PREFIX).unwrap()).unwrap();
    assert!(envelope
        .payload
        .data
        .repository_guidance
        .contains("Repository guidance"));
    assert!(!envelope
        .payload
        .instructions
        .profile
        .contains("rdisc_context"));
    assert_eq!(
        envelope.payload.data.agent_context["discovery"]["id"],
        "rdisc_context"
    );
    assert!(envelope.payload.data.repository_map.contains("README.md"));
    assert!(!envelope.payload.data.environment.is_empty());
    let legacy = run(&workspace.0, false);
    let (old_messages, _) = assemble(&workspace.0, &legacy).unwrap();
    assert_eq!(
        old_messages
            .iter()
            .filter(|m| m.role == ModelRole::System)
            .count(),
        6
    );
    assert_eq!(old_messages[0].content, crate::repo_system_prompt());
    assert_ne!(
        current
            .inference
            .as_ref()
            .unwrap()
            .binding
            .context_policy_hash,
        legacy
            .inference
            .as_ref()
            .unwrap()
            .binding
            .context_policy_hash
    );
}

#[test]
fn missing_modified_and_foreign_saved_context_is_rejected() {
    let workspace = Workspace::new();
    let run = run(&workspace.0, true);
    let (messages, _) = assemble(&workspace.0, &run).unwrap();
    for pointer in [
        "/payload/data/environment",
        "/payload/data/agent_context",
        "/payload/instructions/base",
        "/payload/instructions/stage",
    ] {
        let mut saved = messages.clone();
        let mut value: Value =
            serde_json::from_str(saved[0].content.strip_prefix(PREFIX).unwrap()).unwrap();
        *value.pointer_mut(pointer).unwrap() = Value::Null;
        saved[0].content = format!("{PREFIX}{value}");
        assert!(validate_saved(&run, &saved).is_err(), "{pointer}");
    }
    for pointer in [
        "/payload/instructions/base",
        "/payload/instructions/profile",
        "/payload/data/environment",
        "/payload/instructions/stage/content",
    ] {
        let mut saved = messages.clone();
        let mut value: Value =
            serde_json::from_str(saved[0].content.strip_prefix(PREFIX).unwrap()).unwrap();
        *value.pointer_mut(pointer).unwrap() = json!("forged context");
        value["content_hash"] = json!(canonical_json_sha256(&value["payload"]).unwrap());
        saved[0].content = format!("{PREFIX}{value}");
        assert!(
            validate_saved(&run, &saved).is_err(),
            "self-consistent checksum cannot replace original instructions: {pointer}"
        );
    }
    for change in ["run", "request", "discovery", "policy"] {
        let mut other = run.clone();
        match change {
            "run" => other.run_id = "run_other".into(),
            "request" => other.user_task = "Another change".into(),
            "discovery" => {
                other.execution_target_json["agent_context"]["discovery"]["id"] =
                    json!("rdisc_other")
            }
            _ => {
                other
                    .inference
                    .as_mut()
                    .unwrap()
                    .binding
                    .context_policy_hash = "unsupported".into()
            }
        }
        assert!(validate_saved(&other, &messages).is_err(), "{change}");
    }
    assert!(validate_saved(&run, &messages[1..]).is_err());
}

#[test]
fn checkpoint_compaction_retains_the_complete_original_envelope() {
    let workspace = Workspace::new();
    let run = run(&workspace.0, true);
    let (mut messages, _) = assemble(&workspace.0, &run).unwrap();
    let original = messages[0].clone();
    for index in 0..8 {
        messages.push(ModelMessage {
            role: ModelRole::Assistant,
            content: "inspect bounded evidence ".repeat(20),
            tool_call_id: None,
            tool_calls: vec![ModelToolCall {
                id: format!("act_{index}"),
                name: "read_file".into(),
                arguments: json!({"reason":"inspect","path":"README.md"}).to_string(),
            }],
            reasoning: None,
        });
        messages.push(ModelMessage {role:ModelRole::Tool, content:json!({"status":"ok","summary":"bounded read","content":{"action":"read_file","path":"README.md","body":"x".repeat(3000)}}).to_string(), tool_call_id:Some(format!("act_{index}")), tool_calls:vec![], reasoning:None});
    }
    let packed = pharness_core::pack_messages(
        &messages,
        &ContextBudget {
            recent_message_tokens: 600,
            max_tool_result_tokens: 128,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(packed.compacted_exchanges > 0);
    assert!(packed.truncated_tool_results > 0);
    assert_eq!(packed.messages[0], original);
    validate_saved(&run, &packed.messages).unwrap();
    let tiny = ContextBudget {
        max_input_tokens: 100,
        reserved_output_tokens: 20,
        ..Default::default()
    };
    assert!(
        pharness_core::pack_messages(&messages, &tiny).is_err(),
        "required context must fail instead of being dropped"
    );
}

#[tokio::test]
async fn actual_request_and_recovery_keep_the_context_through_provider_serialization() {
    let workspace = Workspace::new();
    let run = run(&workspace.0, true);
    let provider = Arc::new(Provider::new(vec![
        read("act_missing", "missing.txt"),
        submit(),
    ]));
    let backend = Arc::new(Backend::default());
    crate::execute_attempt(
        host(provider.clone()),
        backend.clone(),
        spec(run.clone()),
        CancellationFlag::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        backend.outcome.lock().unwrap().as_ref().unwrap().status,
        "completed"
    );
    assert_eq!(provider.requests.lock().unwrap().len(), 2);
    assert_requests(&run, &backend, &provider);
    assert!(provider.requests.lock().unwrap()[1]
        .messages
        .iter()
        .any(|m| m.role == ModelRole::Tool && m.content.contains("recoverable")));
}

#[tokio::test]
async fn budget_pause_reconstructs_original_context_and_failure_before_resuming() {
    let workspace = Workspace::new();
    let mut run = run(&workspace.0, true);
    let original_max = run.max_turns;
    // A deliberately short fixture allowance exercises an already-authorized
    // extension within the unchanged original profile's maximum.
    run.max_turns = 1;
    run.budget_consumption.allowed_turns = 1;
    let first = Arc::new(Provider::new(vec![read("act_missing", "missing.txt")]));
    let backend = Arc::new(Backend::default());
    crate::execute_attempt(
        host(first),
        backend.clone(),
        spec(run.clone()),
        CancellationFlag::default(),
    )
    .await
    .unwrap();
    let outcome = backend.outcome.lock().unwrap().clone().unwrap();
    let pause = outcome.budget_extension.unwrap();
    run.max_turns = original_max;
    run.budget_consumption = pause.consumption;
    run.budget_consumption.allowed_turns = original_max;
    run.budget_consumption.extensions += 1;
    let restored: RunSpec = serde_json::from_value(serde_json::to_value(run).unwrap()).unwrap();
    let mut attempt = spec(restored.clone());
    attempt.budget_resume = Some(BudgetResumeSpec {
        resume_messages_json: pause.resume_messages_json,
        turns_completed: pause.turns_completed,
    });
    let provider = Arc::new(Provider::new(vec![submit()]));
    let backend = Arc::new(Backend::default());
    crate::execute_attempt(
        host(provider.clone()),
        backend.clone(),
        attempt,
        CancellationFlag::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        backend.outcome.lock().unwrap().as_ref().unwrap().status,
        "completed"
    );
    assert_requests(&restored, &backend, &provider);
    assert!(provider.requests.lock().unwrap()[0]
        .messages
        .iter()
        .any(|m| m.role == ModelRole::Tool && m.content.contains("recoverable")));
}

#[tokio::test]
async fn approval_resume_validates_before_executing_the_saved_action() {
    let workspace = Workspace::new();
    let run = run(&workspace.0, true);
    let (mut messages, _) = assemble(&workspace.0, &run).unwrap();
    messages.push(ModelMessage {
        role: ModelRole::Assistant,
        content: String::new(),
        tool_call_id: None,
        tool_calls: vec![ModelToolCall {
            id: "act_approved".into(),
            name: "read_file".into(),
            arguments: json!({"reason":"inspect","path":"README.md"}).to_string(),
        }],
        reasoning: None,
    });
    let resume = ResumeSpec {
        approval_id: "approval_context_fixture".into(),
        action_json: serde_json::to_value(read("act_approved", "README.md")).unwrap(),
        resume_messages_json: serde_json::to_value(&messages).unwrap(),
        turns_completed: 1,
    };
    for valid in [false, true] {
        let mut attempt = spec(run.clone());
        attempt.resume = Some(resume.clone());
        if !valid {
            attempt
                .resume
                .as_mut()
                .unwrap()
                .resume_messages_json
                .as_array_mut()
                .unwrap()
                .remove(0);
        }
        let backend = Arc::new(Backend::default());
        let provider = Arc::new(Provider::new(vec![submit()]));
        let result = crate::execute_attempt(
            host(provider.clone()),
            backend.clone(),
            attempt,
            CancellationFlag::default(),
        )
        .await;
        if valid {
            result.unwrap();
            assert_eq!(
                backend.outcome.lock().unwrap().as_ref().unwrap().status,
                "completed"
            );
            assert_requests(&run, &backend, &provider);
            assert_eq!(
                backend
                    .events
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|e| e.kind == EventKind::ToolStarted
                        && e.payload["approval_id"] == "approval_context_fixture")
                    .count(),
                1
            );
        } else {
            assert!(result.is_err());
            assert!(provider.requests.lock().unwrap().is_empty());
            assert!(!backend
                .events
                .lock()
                .unwrap()
                .iter()
                .any(|e| e.kind == EventKind::ToolStarted));
        }
    }
}

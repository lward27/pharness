use super::{pack_messages, ContextBudget};
use super::{CancellationFlag, RunStatus};
use crate::project::{RunBudget, RunBudgetConsumption};
use crate::{
    AgentAction, AgentEvent, EventId, EventKind, EventSink, ModelMessage, ModelProvider,
    ModelRequest, NoopToolExecutor, PolicyDecision, RiskLevel, RunId, RunScope, SafetyPolicy,
    SessionId, ToolError, ToolErrorDisposition, ToolExecutor, ToolProtocolMode, ToolResult,
    ToolSpec,
};
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryPolicy {
    pub max_recoverable_errors: u32,
    pub max_identical_failures: u32,
}

impl Default for RecoveryPolicy {
    fn default() -> Self {
        Self {
            max_recoverable_errors: 4,
            max_identical_failures: 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    #[default]
    General,
    Coding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskContract {
    #[serde(default)]
    pub kind: TaskKind,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub require_workspace_change: bool,
    #[serde(default)]
    pub require_post_change_diff: bool,
}

impl Default for TaskContract {
    fn default() -> Self {
        Self {
            kind: TaskKind::General,
            acceptance_criteria: Vec::new(),
            require_workspace_change: false,
            require_post_change_diff: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryInstruction {
    pub filename: String,
    pub bytes: usize,
}

#[derive(Default)]
struct RecoveryState {
    total: u32,
    last_fingerprint: Option<String>,
    identical: u32,
}

#[derive(Default)]
struct CompletionState {
    changed: bool,
    mutation_seen: bool,
    git_inspected_after_mutation: bool,
    rejected_finishes: u32,
}

fn tools_for_turn(
    tools: &[ToolSpec],
    remaining_turns: u32,
    verification_reserve_turns: u32,
) -> (Vec<ToolSpec>, bool) {
    let verifier_final_reserve = verification_reserve_turns > 0
        && remaining_turns <= verification_reserve_turns
        && tools.iter().any(|tool| tool.name == "submit_verification");
    if !verifier_final_reserve {
        return (tools.to_vec(), false);
    }
    (
        tools
            .iter()
            .filter(|tool| matches!(tool.name.as_str(), "submit_verification" | "finish"))
            .cloned()
            .collect(),
        true,
    )
}

pub struct AgentRuntime<P, E, T = NoopToolExecutor> {
    provider: P,
    event_sink: E,
    tool_executor: T,
}

impl<P, E> AgentRuntime<P, E, NoopToolExecutor>
where
    P: ModelProvider,
    E: EventSink,
{
    pub fn new(provider: P, event_sink: E) -> Self {
        Self::with_tools(provider, event_sink, NoopToolExecutor)
    }
}

impl<P, E, T> AgentRuntime<P, E, T>
where
    P: ModelProvider,
    E: EventSink,
    T: ToolExecutor,
{
    pub fn with_tools(provider: P, event_sink: E, tool_executor: T) -> Self {
        Self {
            provider,
            event_sink,
            tool_executor,
        }
    }

    pub async fn run(&self, config: RunConfig, cancellation: CancellationFlag) -> RunOutcome {
        self.run_loop(config, cancellation, RunStart::Fresh).await
    }

    pub async fn resume_after_approval(
        &self,
        config: RunConfig,
        cancellation: CancellationFlag,
        approved: ApprovedAction,
    ) -> RunOutcome {
        self.run_loop(config, cancellation, RunStart::Approved(Box::new(approved)))
            .await
    }

    pub async fn resume_after_budget(
        &self,
        config: RunConfig,
        cancellation: CancellationFlag,
        resume: BudgetResume,
    ) -> RunOutcome {
        self.run_loop(config, cancellation, RunStart::Budget(Box::new(resume)))
            .await
    }

    async fn run_loop(
        &self,
        config: RunConfig,
        cancellation: CancellationFlag,
        start: RunStart,
    ) -> RunOutcome {
        let mut seq = config.event_seq_start;
        let active_started = Instant::now();
        let mut messages = match start {
            RunStart::Fresh => {
                self.emit(
                    &config,
                    &mut seq,
                    EventKind::RunStarted,
                    serde_json::json!({}),
                );
                config.messages.clone()
            }
            RunStart::Approved(ref approved) => {
                self.emit(
                    &config,
                    &mut seq,
                    EventKind::RunResumed,
                    serde_json::json!({
                        "approval_id": approved.approval_id,
                        "action": approved.action.kind_name(),
                        "run_scope": config.run_scope.to_optional_json(),
                    }),
                );
                approved.resume_messages.clone()
            }
            RunStart::Budget(ref resume) => {
                self.emit(
                    &config,
                    &mut seq,
                    EventKind::RunResumed,
                    serde_json::json!({
                        "budget_extension": true,
                        "turns_completed": resume.turns_completed,
                        "run_scope": config.run_scope.to_optional_json(),
                    }),
                );
                resume.resume_messages.clone()
            }
        };

        let mut turn_start = match &start {
            RunStart::Budget(resume) => resume.turns_completed,
            _ => 0,
        };
        let mut tokens_used = config.budget_consumption.tokens_used;
        let (mut recovery, mut completion) = resume_runtime_state(&messages);
        if let RunStart::Approved(approved) = start {
            turn_start = approved.turns_completed;
            if let Some(outcome) = self
                .execute_approved_action(
                    &config,
                    &mut seq,
                    &mut messages,
                    &approved,
                    &mut recovery,
                    &mut completion,
                )
                .await
            {
                return outcome.with_consumption(consumption_snapshot(
                    &config,
                    approved.turns_completed,
                    tokens_used,
                    config
                        .budget_consumption
                        .active_execution_seconds_used
                        .saturating_add(active_started.elapsed().as_secs()),
                ));
            }
        }

        for turn_index in turn_start..config.max_turns {
            if let Some(budget) = &config.run_budget {
                let active_seconds = config
                    .budget_consumption
                    .active_execution_seconds_used
                    .saturating_add(active_started.elapsed().as_secs());
                if active_seconds >= budget.active_execution_seconds {
                    return RunOutcome::failed(turn_index, "active_execution_budget_exhausted")
                        .with_consumption(consumption_snapshot(
                            &config,
                            turn_index,
                            tokens_used,
                            active_seconds,
                        ));
                }
                if tokens_used >= config.budget_consumption.allowed_tokens {
                    return budget_boundary_outcome(
                        &config,
                        turn_index,
                        tokens_used,
                        messages,
                        budget,
                        active_seconds,
                        "soft_token_budget_exhausted",
                    );
                }
            }
            if cancellation.is_cancelled() {
                self.emit(
                    &config,
                    &mut seq,
                    EventKind::RunCancelled,
                    serde_json::json!({ "turn": turn_index }),
                );
                return RunOutcome::cancelled(turn_index).with_consumption(consumption_snapshot(
                    &config,
                    turn_index,
                    tokens_used,
                    config
                        .budget_consumption
                        .active_execution_seconds_used
                        .saturating_add(active_started.elapsed().as_secs()),
                ));
            }

            let packed = match pack_messages(&messages, &config.context_budget) {
                Ok(packed) => packed,
                Err(error) => {
                    self.emit(
                        &config,
                        &mut seq,
                        EventKind::RunFailed,
                        serde_json::json!({ "error": error.to_string(), "turn": turn_index }),
                    );
                    return RunOutcome::failed(turn_index, error.to_string()).with_consumption(
                        consumption_snapshot(
                            &config,
                            turn_index,
                            tokens_used,
                            config
                                .budget_consumption
                                .active_execution_seconds_used
                                .saturating_add(active_started.elapsed().as_secs()),
                        ),
                    );
                }
            };
            messages = packed.messages;
            self.emit(
                &config,
                &mut seq,
                EventKind::ModelRequestStarted,
                serde_json::json!({
                    "turn": turn_index,
                    "estimated_input_tokens": packed.estimated_input_tokens,
                    "original_message_count": packed.original_message_count,
                    "packed_message_count": messages.len(),
                    "compacted_exchanges": packed.compacted_exchanges,
                    "truncated_tool_results": packed.truncated_tool_results,
                    "context_budget": config.context_budget.max_input_tokens,
                    "repository_instruction_files": config.repository_instruction_files,
                }),
            );

            let mut request_messages = messages.clone();
            let remaining_turns = config
                .budget_consumption
                .allowed_turns
                .saturating_sub(turn_index);
            let (turn_tools, verifier_final_reserve) = config
                .run_budget
                .as_ref()
                .map(|budget| {
                    tools_for_turn(
                        &config.tools,
                        remaining_turns,
                        budget.verification_reserve_turns,
                    )
                })
                .unwrap_or_else(|| (config.tools.clone(), false));
            if let Some(budget) = &config.run_budget {
                request_messages.insert(
                    1.min(request_messages.len()),
                    ModelMessage::system(format!(
                        "Execution budget: {} turns and {} tokens remain; {} seconds of active time were configured. Reserve the final {} turns for declared acceptance commands, Git status/diff, and completion evidence.",
                        remaining_turns,
                        config.budget_consumption.allowed_tokens.saturating_sub(tokens_used),
                        budget.active_execution_seconds.saturating_sub(config.budget_consumption.active_execution_seconds_used),
                        budget.verification_reserve_turns,
                    )),
                );
            }
            if verifier_final_reserve {
                request_messages.insert(
                    1.min(request_messages.len()),
                    ModelMessage::system(
                        "FINAL VERIFIER RESERVE: evidence collection is closed. Submit the typed verification verdict now from the sealed Tester outcome and evidence already inspected, then finish. Only submit_verification and finish are authorized in these remaining turns."
                            .to_string(),
                    ),
                );
            }
            let request = ModelRequest {
                session_id: config.session_id.clone(),
                run_id: config.run_id.clone(),
                messages: request_messages,
                tools: turn_tools,
                mode: config.tool_protocol,
                temperature: config.temperature,
                max_tokens: config.max_tokens,
            };

            let turn = match self.provider.complete_action(request).await {
                Ok(turn) => turn,
                Err(error) => {
                    self.emit(
                        &config,
                        &mut seq,
                        EventKind::RunFailed,
                        serde_json::json!({ "error": error.to_string(), "turn": turn_index }),
                    );
                    return RunOutcome::failed(turn_index + 1, error.to_string()).with_consumption(
                        consumption_snapshot(
                            &config,
                            turn_index + 1,
                            tokens_used,
                            config
                                .budget_consumption
                                .active_execution_seconds_used
                                .saturating_add(active_started.elapsed().as_secs()),
                        ),
                    );
                }
            };

            if let Some(usage) = turn.usage {
                tokens_used = tokens_used.saturating_add(u64::from(usage.total_tokens));
            }

            self.emit(
                &config,
                &mut seq,
                EventKind::ModelResponseFinished,
                serde_json::json!({
                    "turn": turn_index,
                    "raw_provider_id": turn.raw_provider_id,
                    "prompt_tokens": turn.usage.map(|usage| usage.prompt_tokens),
                    "completion_tokens": turn.usage.map(|usage| usage.completion_tokens),
                    "total_tokens": turn.usage.map(|usage| usage.total_tokens),
                }),
            );

            let assistant_tool_calls = turn.assistant_tool_calls.clone();
            if !assistant_tool_calls.is_empty() {
                messages.push(ModelMessage {
                    role: crate::ModelRole::Assistant,
                    content: turn.assistant_message.clone().unwrap_or_default(),
                    tool_call_id: None,
                    tool_calls: assistant_tool_calls.clone(),
                });
            } else if turn.assistant_message.is_some()
                && !matches!(&turn.action, AgentAction::Respond { .. })
            {
                let message = turn
                    .assistant_message
                    .clone()
                    .expect("message presence was checked");
                messages.push(ModelMessage {
                    role: crate::ModelRole::Assistant,
                    content: message,
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                });
            }

            self.emit(
                &config,
                &mut seq,
                EventKind::ActionProposed,
                serde_json::to_value(&turn.action).unwrap_or_else(
                    |error| serde_json::json!({ "serialization_error": error.to_string() }),
                ),
            );

            if verifier_final_reserve
                && !matches!(
                    &turn.action,
                    AgentAction::SubmitVerification { .. } | AgentAction::Finish { .. }
                )
            {
                let result = ToolResult::error(
                    "verifier final reserve accepts only submit_verification or finish",
                    serde_json::json!({
                        "error_kind": "verifier_final_reserve",
                        "remaining_turns": remaining_turns,
                        "allowed_actions": ["submit_verification", "finish"],
                    }),
                );
                self.emit(
                    &config,
                    &mut seq,
                    EventKind::ToolFinished,
                    serde_json::to_value(&result).unwrap_or_default(),
                );
                messages.push(tool_message(
                    &result,
                    Some(turn.action.id().to_string()),
                    Some(&turn.action),
                ));
                continue;
            }

            match turn.action {
                AgentAction::Finish {
                    id,
                    summary,
                    success,
                    ..
                } => {
                    if success {
                        if let Some(reason) =
                            completion_failure_reason(&config.task_contract, &completion)
                        {
                            completion.rejected_finishes += 1;
                            if completion.rejected_finishes > 2 {
                                let error = "completion_evidence_exhausted".to_string();
                                self.emit(
                                    &config,
                                    &mut seq,
                                    EventKind::RunFailed,
                                    serde_json::json!({ "error": error, "turn": turn_index }),
                                );
                                return RunOutcome::failed(turn_index + 1, error).with_consumption(
                                    consumption_snapshot(
                                        &config,
                                        turn_index + 1,
                                        tokens_used,
                                        config
                                            .budget_consumption
                                            .active_execution_seconds_used
                                            .saturating_add(active_started.elapsed().as_secs()),
                                    ),
                                );
                            }
                            let result = ToolResult::error(
                                "finish rejected: completion evidence is incomplete",
                                serde_json::json!({
                                    "error_kind": "completion_evidence_missing",
                                    "reason": reason,
                                    "rejected_finishes": completion.rejected_finishes,
                                }),
                            );
                            self.emit(
                                &config,
                                &mut seq,
                                EventKind::ToolFinished,
                                serde_json::to_value(&result).unwrap_or_default(),
                            );
                            messages.push(tool_message(&result, Some(id.to_string()), None));
                            continue;
                        }
                    }
                    let status = if success {
                        RunStatus::Completed
                    } else {
                        RunStatus::Failed
                    };
                    self.emit(
                        &config,
                        &mut seq,
                        if success {
                            EventKind::RunFinished
                        } else {
                            EventKind::RunFailed
                        },
                        serde_json::json!({ "summary": summary, "success": success }),
                    );
                    return RunOutcome {
                        status,
                        turns: turn_index + 1,
                        summary: Some(summary),
                        error: (!success).then_some("model finished unsuccessfully".to_string()),
                        approval: None,
                        budget_pause: None,
                        consumption: consumption_snapshot(
                            &config,
                            turn_index + 1,
                            tokens_used,
                            config
                                .budget_consumption
                                .active_execution_seconds_used
                                .saturating_add(active_started.elapsed().as_secs()),
                        ),
                    };
                }
                AgentAction::RequestApproval {
                    approval_kind,
                    summary,
                    ..
                } => {
                    let approval = PendingApproval {
                        approval_kind,
                        risk: RiskLevel::Medium,
                        summary: summary.clone(),
                        action: None,
                        resume_messages: messages.clone(),
                        turns_completed: turn_index + 1,
                    };
                    self.emit(
                        &config,
                        &mut seq,
                        EventKind::ApprovalRequired,
                        serde_json::json!({
                            "approval_kind": approval_kind,
                            "summary": summary,
                            "run_scope": config.run_scope.to_optional_json(),
                        }),
                    );
                    return RunOutcome {
                        status: RunStatus::ApprovalRequired,
                        turns: turn_index + 1,
                        summary: Some(summary),
                        error: None,
                        approval: Some(approval),
                        budget_pause: None,
                        consumption: consumption_snapshot(
                            &config,
                            turn_index + 1,
                            tokens_used,
                            config
                                .budget_consumption
                                .active_execution_seconds_used
                                .saturating_add(active_started.elapsed().as_secs()),
                        ),
                    };
                }
                AgentAction::Respond { message, .. } => {
                    messages.push(ModelMessage {
                        role: crate::ModelRole::Assistant,
                        content: message,
                        tool_call_id: None,
                        tool_calls: Vec::new(),
                    });
                }
                tool_action => {
                    let policy_decision = config
                        .policy
                        .evaluate_action_in_scope(&tool_action, &config.run_scope);
                    self.emit(
                        &config,
                        &mut seq,
                        EventKind::PolicyEvaluated,
                        serde_json::json!({
                            "action": tool_action.kind_name(),
                            "decision": policy_decision,
                            "run_scope": config.run_scope.to_optional_json(),
                        }),
                    );

                    match policy_decision {
                        PolicyDecision::Allow { .. } => {}
                        PolicyDecision::Ask {
                            approval_kind,
                            risk,
                            summary,
                            ..
                        } => {
                            let approval = PendingApproval {
                                approval_kind,
                                risk,
                                summary: summary.clone(),
                                action: Some(tool_action.clone()),
                                resume_messages: messages.clone(),
                                turns_completed: turn_index + 1,
                            };
                            self.emit(
                                &config,
                                &mut seq,
                                EventKind::ApprovalRequired,
                                serde_json::json!({
                                    "approval_kind": approval_kind,
                                    "summary": summary,
                                    "action": tool_action.kind_name(),
                                    "run_scope": config.run_scope.to_optional_json(),
                                }),
                            );
                            return RunOutcome {
                                status: RunStatus::ApprovalRequired,
                                turns: turn_index + 1,
                                summary: Some(summary),
                                error: None,
                                approval: Some(approval),
                                budget_pause: None,
                                consumption: consumption_snapshot(
                                    &config,
                                    turn_index + 1,
                                    tokens_used,
                                    config
                                        .budget_consumption
                                        .active_execution_seconds_used
                                        .saturating_add(active_started.elapsed().as_secs()),
                                ),
                            };
                        }
                        PolicyDecision::Deny { summary, .. } => {
                            self.emit(
                                &config,
                                &mut seq,
                                EventKind::RunFailed,
                                serde_json::json!({
                                    "error": summary,
                                    "turn": turn_index,
                                    "action": tool_action.kind_name(),
                                }),
                            );
                            return RunOutcome::failed(turn_index + 1, summary).with_consumption(
                                consumption_snapshot(
                                    &config,
                                    turn_index + 1,
                                    tokens_used,
                                    config
                                        .budget_consumption
                                        .active_execution_seconds_used
                                        .saturating_add(active_started.elapsed().as_secs()),
                                ),
                            );
                        }
                    }

                    self.emit(
                        &config,
                        &mut seq,
                        EventKind::ToolStarted,
                        serde_json::json!({ "action": tool_action.kind_name() }),
                    );

                    match self.tool_executor.execute(&tool_action).await {
                        Ok(result) => {
                            self.emit(
                                &config,
                                &mut seq,
                                EventKind::ToolFinished,
                                serde_json::to_value(&result).unwrap_or_else(|error| {
                                    serde_json::json!({
                                        "serialization_error": error.to_string()
                                    })
                                }),
                            );
                            update_completion_state(&mut completion, &tool_action, &result);
                            messages.push(tool_message(
                                &result,
                                assistant_tool_calls
                                    .first()
                                    .map(|tool_call| tool_call.id.clone())
                                    .or_else(|| Some(tool_action.id().to_string())),
                                Some(&tool_action),
                            ));
                        }
                        Err(error) => {
                            if error.disposition() == ToolErrorDisposition::Recoverable {
                                let fingerprint = action_fingerprint(&tool_action, &error);
                                recovery.total += 1;
                                recovery.identical = if recovery.last_fingerprint.as_deref()
                                    == Some(fingerprint.as_str())
                                {
                                    recovery.identical + 1
                                } else {
                                    1
                                };
                                recovery.last_fingerprint = Some(fingerprint);
                                if recovery.total <= config.recovery_policy.max_recoverable_errors
                                    && recovery.identical
                                        <= config.recovery_policy.max_identical_failures
                                {
                                    let result =
                                        recoverable_error_result(&tool_action, &error, &recovery);
                                    self.emit(
                                        &config,
                                        &mut seq,
                                        EventKind::ToolFinished,
                                        serde_json::to_value(&result).unwrap_or_default(),
                                    );
                                    messages.push(tool_message(
                                        &result,
                                        assistant_tool_calls
                                            .first()
                                            .map(|call| call.id.clone())
                                            .or_else(|| Some(tool_action.id().to_string())),
                                        Some(&tool_action),
                                    ));
                                    continue;
                                }
                                let exhausted = "tool_recovery_exhausted".to_string();
                                self.emit(&config, &mut seq, EventKind::RunFailed, serde_json::json!({ "error": exhausted, "turn": turn_index, "action": tool_action.kind_name(), "recoverable_error_kind": error.kind_name() }));
                                return RunOutcome::failed(turn_index + 1, exhausted)
                                    .with_consumption(consumption_snapshot(
                                        &config,
                                        turn_index + 1,
                                        tokens_used,
                                        config
                                            .budget_consumption
                                            .active_execution_seconds_used
                                            .saturating_add(active_started.elapsed().as_secs()),
                                    ));
                            }
                            self.emit(
                                &config,
                                &mut seq,
                                EventKind::RunFailed,
                                serde_json::json!({
                                    "error": error.to_string(),
                                    "turn": turn_index,
                                    "action": tool_action.kind_name(),
                                }),
                            );
                            return RunOutcome::failed(turn_index + 1, error.to_string())
                                .with_consumption(consumption_snapshot(
                                    &config,
                                    turn_index + 1,
                                    tokens_used,
                                    config
                                        .budget_consumption
                                        .active_execution_seconds_used
                                        .saturating_add(active_started.elapsed().as_secs()),
                                ));
                        }
                    }
                }
            }
        }

        if let Some(budget) = &config.run_budget {
            return budget_boundary_outcome(
                &config,
                config.max_turns,
                tokens_used,
                messages,
                budget,
                config
                    .budget_consumption
                    .active_execution_seconds_used
                    .saturating_add(active_started.elapsed().as_secs()),
                "soft_turn_budget_exhausted",
            );
        }
        let message = format!("run exceeded max_turns={}", config.max_turns);
        self.emit(
            &config,
            &mut seq,
            EventKind::RunFailed,
            serde_json::json!({ "error": message }),
        );
        RunOutcome::failed(config.max_turns, message).with_consumption(consumption_snapshot(
            &config,
            config.max_turns,
            tokens_used,
            config
                .budget_consumption
                .active_execution_seconds_used
                .saturating_add(active_started.elapsed().as_secs()),
        ))
    }

    async fn execute_approved_action(
        &self,
        config: &RunConfig,
        seq: &mut u64,
        messages: &mut Vec<ModelMessage>,
        approved: &ApprovedAction,
        recovery: &mut RecoveryState,
        completion: &mut CompletionState,
    ) -> Option<RunOutcome> {
        self.emit(
            config,
            seq,
            EventKind::ToolStarted,
            serde_json::json!({
                "action": approved.action.kind_name(),
                "approval_id": approved.approval_id,
            }),
        );

        match self.tool_executor.execute(&approved.action).await {
            Ok(result) => {
                self.emit(
                    config,
                    seq,
                    EventKind::ToolFinished,
                    serde_json::to_value(&result).unwrap_or_else(|error| {
                        serde_json::json!({
                            "serialization_error": error.to_string()
                        })
                    }),
                );
                update_completion_state(completion, &approved.action, &result);
                messages.push(tool_message(
                    &result,
                    Some(approved.action.id().to_string()),
                    Some(&approved.action),
                ));
                None
            }
            Err(error) => {
                if error.disposition() == ToolErrorDisposition::Recoverable {
                    let fingerprint = action_fingerprint(&approved.action, &error);
                    recovery.total += 1;
                    recovery.identical =
                        if recovery.last_fingerprint.as_deref() == Some(fingerprint.as_str()) {
                            recovery.identical + 1
                        } else {
                            1
                        };
                    recovery.last_fingerprint = Some(fingerprint);
                    if recovery.total <= config.recovery_policy.max_recoverable_errors
                        && recovery.identical <= config.recovery_policy.max_identical_failures
                    {
                        let result = recoverable_error_result(&approved.action, &error, recovery);
                        self.emit(
                            config,
                            seq,
                            EventKind::ToolFinished,
                            serde_json::to_value(&result).unwrap_or_default(),
                        );
                        messages.push(tool_message(
                            &result,
                            Some(approved.action.id().to_string()),
                            Some(&approved.action),
                        ));
                        return None;
                    }
                    return Some(RunOutcome::failed(
                        approved.turns_completed,
                        "tool_recovery_exhausted",
                    ));
                }
                self.emit(
                    config,
                    seq,
                    EventKind::RunFailed,
                    serde_json::json!({
                        "error": error.to_string(),
                        "action": approved.action.kind_name(),
                        "approval_id": approved.approval_id,
                    }),
                );
                Some(RunOutcome::failed(
                    approved.turns_completed,
                    error.to_string(),
                ))
            }
        }
    }

    fn emit(&self, config: &RunConfig, seq: &mut u64, kind: EventKind, payload: serde_json::Value) {
        *seq += 1;
        self.event_sink.append(AgentEvent {
            event_id: EventId::new(format!("evt_{}_{}", config.run_id.as_str(), seq)),
            session_id: config.session_id.clone(),
            run_id: config.run_id.clone(),
            seq: *seq,
            kind,
            payload,
        });
    }
}

fn tool_message(
    result: &ToolResult,
    tool_call_id: Option<String>,
    action: Option<&AgentAction>,
) -> ModelMessage {
    let mut result = result.clone();
    if let (Some(action), Some(content)) = (action, result.content.as_object_mut()) {
        content.insert(
            "action".to_string(),
            serde_json::Value::String(action.kind_name().to_string()),
        );
    }
    ModelMessage {
        role: crate::ModelRole::Tool,
        content: serde_json::to_string(&result).unwrap_or_else(|error| {
            format!(
                "{{\"status\":\"error\",\"summary\":\"failed to serialize tool result: {error}\"}}"
            )
        }),
        tool_call_id,
        tool_calls: Vec::new(),
    }
}

fn action_fingerprint(action: &AgentAction, error: &ToolError) -> String {
    let mut action_json = serde_json::to_value(action).unwrap_or_default();
    if let Some(action_object) = action_json.as_object_mut() {
        action_object.remove("id");
        action_object.remove("reason");
    }
    format!(
        "{}:{}:{}",
        action.kind_name(),
        error.kind_name(),
        serde_json::to_string(&action_json).unwrap_or_default()
    )
}

fn recoverable_error_result(
    action: &AgentAction,
    error: &ToolError,
    recovery: &RecoveryState,
) -> ToolResult {
    ToolResult::error(
        format!(
            "{} failed; inspect the error and choose a different safe action",
            action.kind_name()
        ),
        serde_json::json!({
            "action": action.kind_name(),
            "error_kind": error.kind_name(),
            "message": error.to_string(),
            "recoverable": true,
            "failure_fingerprint": action_fingerprint(action, error),
            "recovery_count": recovery.total,
            "identical_failure_count": recovery.identical,
        }),
    )
}

fn resume_runtime_state(messages: &[ModelMessage]) -> (RecoveryState, CompletionState) {
    let mut recovery = RecoveryState::default();
    let mut completion = CompletionState::default();
    for message in messages {
        if message.role != crate::ModelRole::Tool {
            continue;
        }
        let Ok(result) = serde_json::from_str::<serde_json::Value>(&message.content) else {
            continue;
        };
        let Some(content) = result.get("content") else {
            continue;
        };
        if content
            .get("recoverable")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            if let Some(total) = content
                .get("recovery_count")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
            {
                recovery.total = total;
            }
            if let Some(identical) = content
                .get("identical_failure_count")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
            {
                recovery.identical = identical;
            }
            recovery.last_fingerprint = content
                .get("failure_fingerprint")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned);
        }
        if result.get("status").and_then(serde_json::Value::as_str) != Some("ok") {
            continue;
        }
        match content.get("action").and_then(serde_json::Value::as_str) {
            Some("write_file" | "patch_file") => {
                completion.mutation_seen = true;
                completion.changed |=
                    content.get("diff").and_then(serde_json::Value::as_str) != Some("unchanged");
                completion.git_inspected_after_mutation = false;
            }
            Some("git_status" | "git_diff") if completion.mutation_seen => {
                completion.git_inspected_after_mutation = true;
            }
            _ => {}
        }
    }
    (recovery, completion)
}

fn update_completion_state(state: &mut CompletionState, action: &AgentAction, result: &ToolResult) {
    if result.status != crate::ToolResultStatus::Ok {
        return;
    }
    match action {
        AgentAction::WriteFile { .. } | AgentAction::PatchFile { .. } => {
            state.mutation_seen = true;
            state.changed |= result
                .content
                .get("diff")
                .and_then(serde_json::Value::as_str)
                != Some("unchanged");
            state.git_inspected_after_mutation = false;
        }
        AgentAction::GitStatus { .. } | AgentAction::GitDiff { .. } if state.mutation_seen => {
            state.git_inspected_after_mutation = true;
        }
        _ => {}
    }
}

fn completion_failure_reason(
    contract: &TaskContract,
    state: &CompletionState,
) -> Option<&'static str> {
    if contract.kind != TaskKind::Coding {
        return None;
    }
    if contract.require_workspace_change && !state.changed {
        return Some("a meaningful workspace change is required before success");
    }
    if contract.require_post_change_diff && !state.git_inspected_after_mutation {
        return Some("inspect git status or git diff after the final mutation before success");
    }
    None
}

#[derive(Debug, Clone)]
pub struct RunConfig {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub messages: Vec<ModelMessage>,
    pub tools: Vec<ToolSpec>,
    pub tool_protocol: ToolProtocolMode,
    pub temperature: f32,
    pub max_tokens: u32,
    pub max_turns: u32,
    pub context_budget: ContextBudget,
    pub recovery_policy: RecoveryPolicy,
    pub task_contract: TaskContract,
    pub repository_instruction_files: Vec<RepositoryInstruction>,
    pub policy: SafetyPolicy,
    pub run_scope: RunScope,
    pub event_seq_start: u64,
    pub run_budget: Option<RunBudget>,
    pub budget_consumption: RunBudgetConsumption,
}

impl RunConfig {
    pub fn local_test(task: impl Into<String>) -> Self {
        Self {
            session_id: SessionId::new("ses_test"),
            run_id: RunId::new("run_test"),
            messages: vec![ModelMessage::user(task)],
            tools: Vec::new(),
            tool_protocol: ToolProtocolMode::JsonAction,
            temperature: 0.1,
            max_tokens: 4096,
            max_turns: 40,
            context_budget: ContextBudget::default(),
            recovery_policy: RecoveryPolicy::default(),
            task_contract: TaskContract::default(),
            repository_instruction_files: Vec::new(),
            policy: SafetyPolicy::default(),
            run_scope: RunScope::default(),
            event_seq_start: 0,
            run_budget: None,
            budget_consumption: RunBudgetConsumption::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunOutcome {
    pub status: RunStatus,
    pub turns: u32,
    pub summary: Option<String>,
    pub error: Option<String>,
    pub approval: Option<PendingApproval>,
    pub budget_pause: Option<PendingBudgetExtension>,
    pub consumption: RunBudgetConsumption,
}

impl RunOutcome {
    fn cancelled(turns: u32) -> Self {
        Self {
            status: RunStatus::Cancelled,
            turns,
            summary: None,
            error: Some("run cancelled".to_string()),
            approval: None,
            budget_pause: None,
            consumption: RunBudgetConsumption::default(),
        }
    }

    fn failed(turns: u32, error: impl Into<String>) -> Self {
        Self {
            status: RunStatus::Failed,
            turns,
            summary: None,
            error: Some(error.into()),
            approval: None,
            budget_pause: None,
            consumption: RunBudgetConsumption::default(),
        }
    }

    fn with_consumption(mut self, consumption: RunBudgetConsumption) -> Self {
        self.consumption = consumption;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingApproval {
    pub approval_kind: crate::ApprovalKind,
    pub risk: RiskLevel,
    pub summary: String,
    pub action: Option<AgentAction>,
    pub resume_messages: Vec<ModelMessage>,
    pub turns_completed: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingBudgetExtension {
    pub reason: String,
    pub resume_messages: Vec<ModelMessage>,
    pub turns_completed: u32,
    pub consumption: RunBudgetConsumption,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BudgetResume {
    pub resume_messages: Vec<ModelMessage>,
    pub turns_completed: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovedAction {
    pub approval_id: String,
    pub action: AgentAction,
    pub resume_messages: Vec<ModelMessage>,
    pub turns_completed: u32,
}

enum RunStart {
    Fresh,
    Approved(Box<ApprovedAction>),
    Budget(Box<BudgetResume>),
}

fn budget_boundary_outcome(
    config: &RunConfig,
    turns_completed: u32,
    tokens_used: u64,
    messages: Vec<ModelMessage>,
    budget: &RunBudget,
    active_execution_seconds_used: u64,
    reason: &str,
) -> RunOutcome {
    let hard_exhausted = turns_completed >= budget.hard_turns || tokens_used >= budget.hard_tokens;
    if hard_exhausted {
        return RunOutcome::failed(turns_completed, "hard_budget_exhausted").with_consumption(
            consumption_snapshot(
                config,
                turns_completed,
                tokens_used,
                active_execution_seconds_used,
            ),
        );
    }
    let mut consumption = config.budget_consumption.clone();
    consumption.turns_used = turns_completed;
    consumption.tokens_used = tokens_used;
    consumption.active_execution_seconds_used = active_execution_seconds_used;
    RunOutcome {
        status: RunStatus::BudgetExtensionRequired,
        turns: turns_completed,
        summary: Some("Run paused before exceeding its resumable budget boundary".to_string()),
        error: None,
        approval: None,
        budget_pause: Some(PendingBudgetExtension {
            reason: reason.to_string(),
            resume_messages: messages,
            turns_completed,
            consumption,
        }),
        consumption: consumption_snapshot(
            config,
            turns_completed,
            tokens_used,
            active_execution_seconds_used,
        ),
    }
}

fn consumption_snapshot(
    config: &RunConfig,
    turns_used: u32,
    tokens_used: u64,
    active_execution_seconds_used: u64,
) -> RunBudgetConsumption {
    RunBudgetConsumption {
        turns_used,
        tokens_used,
        active_execution_seconds_used,
        ..config.budget_consumption.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        recoverable_error_result, resume_runtime_state, tool_message, tools_for_turn, AgentRuntime,
        ApprovedAction, BudgetResume, RecoveryState, RunConfig,
    };
    use crate::{
        AgentAction, ApprovalKind, CancellationFlag, CapabilityKind, ContextBudget, EventKind,
        InMemoryEventSink, LocalReadOnlyFsTools, ModelCapabilities, ModelMessage, ModelProvider,
        ModelRequest, ModelTurn, NoopToolExecutor, PermissionGrant, PermissionGrantPolicy,
        PermissionGrantScope, PolicyMode, ProviderError, RiskLevel, RunBudget,
        RunBudgetConsumption, RunScope, RunStatus, SafetyPolicy, TaskContract, TaskKind, ToolError,
        ToolExecutor, ToolResult, ToolSpec,
    };
    use async_trait::async_trait;
    use camino::Utf8PathBuf;
    use std::collections::VecDeque;
    use std::fs;
    use std::sync::Mutex;

    #[test]
    fn verifier_final_reserve_exposes_only_submission_and_finish_tools() {
        let tools = [
            "get_evidence",
            "read_file",
            "git_diff",
            "submit_verification",
            "finish",
        ]
        .into_iter()
        .map(|name| {
            ToolSpec::new(
                name,
                format!("{name} tool"),
                serde_json::json!({"type":"object"}),
                CapabilityKind::AgentControl,
            )
        })
        .collect::<Vec<_>>();

        let (normal_tools, reserve_active) = tools_for_turn(&tools, 5, 4);
        assert!(!reserve_active);
        assert_eq!(normal_tools, tools);

        let (reserve_tools, reserve_active) = tools_for_turn(&tools, 4, 4);
        assert!(reserve_active);
        assert_eq!(
            reserve_tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            vec!["submit_verification", "finish"]
        );
    }

    #[test]
    fn resumable_transcript_restores_recovery_and_completion_state() {
        let write = AgentAction::WriteFile {
            id: "act_write".into(),
            reason: "bounded change".to_string(),
            path: Utf8PathBuf::from("src/example.py"),
            content: "VALUE = 1\n".to_string(),
        };
        let status = AgentAction::GitStatus {
            id: "act_status".into(),
            reason: "capture evidence".to_string(),
        };
        let failed = AgentAction::RunShell {
            id: "act_failed".into(),
            reason: "run bounded command".to_string(),
            cmd: "python -m compileall -q src".to_string(),
            cwd: None,
            timeout_ms: None,
            dry_run: false,
        };
        let error = ToolError::CommandFailed {
            command: "python -m compileall -q src".to_string(),
            status: "1".to_string(),
            stderr: "syntax error".to_string(),
        };
        let recovery = RecoveryState {
            total: 3,
            last_fingerprint: Some(super::action_fingerprint(&failed, &error)),
            identical: 2,
        };
        let messages = vec![
            tool_message(
                &ToolResult::ok("changed", serde_json::json!({"diff":"updated"})),
                Some(write.id().to_string()),
                Some(&write),
            ),
            tool_message(
                &ToolResult::ok("clean evidence", serde_json::json!({})),
                Some(status.id().to_string()),
                Some(&status),
            ),
            tool_message(
                &recoverable_error_result(&failed, &error, &recovery),
                Some(failed.id().to_string()),
                Some(&failed),
            ),
        ];

        let (restored_recovery, restored_completion) = resume_runtime_state(&messages);
        assert_eq!(restored_recovery.total, 3);
        assert_eq!(restored_recovery.identical, 2);
        assert_eq!(
            restored_recovery.last_fingerprint,
            recovery.last_fingerprint
        );
        assert!(restored_completion.changed);
        assert!(restored_completion.mutation_seen);
        assert!(restored_completion.git_inspected_after_mutation);
    }

    struct FakeProvider {
        turns: Mutex<VecDeque<Result<ModelTurn, ProviderError>>>,
    }

    impl FakeProvider {
        fn new(turns: impl IntoIterator<Item = Result<ModelTurn, ProviderError>>) -> Self {
            Self {
                turns: Mutex::new(turns.into_iter().collect()),
            }
        }
    }

    #[async_trait]
    impl ModelProvider for FakeProvider {
        async fn complete_action(
            &self,
            _request: ModelRequest,
        ) -> Result<ModelTurn, ProviderError> {
            self.turns
                .lock()
                .expect("fake provider mutex should not be poisoned")
                .pop_front()
                .expect("fake provider should have a queued turn")
        }

        fn capabilities(&self) -> ModelCapabilities {
            ModelCapabilities {
                native_tool_calling: true,
                streaming: false,
                json_schema_response_format: true,
            }
        }
    }

    fn model_turn(action: AgentAction) -> Result<ModelTurn, ProviderError> {
        Ok(ModelTurn {
            raw_provider_id: Some("fake".to_string()),
            assistant_message: None,
            assistant_tool_calls: Vec::new(),
            action,
            usage: None,
        })
    }

    #[derive(Clone)]
    struct ErroringExecutor {
        error: ToolError,
    }

    #[async_trait]
    impl ToolExecutor for ErroringExecutor {
        async fn execute(&self, _action: &AgentAction) -> Result<ToolResult, ToolError> {
            Err(self.error.clone())
        }
    }

    #[tokio::test]
    async fn completes_when_provider_finishes() {
        let events = InMemoryEventSink::default();
        let runtime = AgentRuntime::new(
            FakeProvider::new([model_turn(AgentAction::Finish {
                id: "act_done".into(),
                reason: "done".to_string(),
                summary: "complete".to_string(),
                success: true,
            })]),
            events.clone(),
        );

        let outcome = runtime
            .run(RunConfig::local_test("finish"), CancellationFlag::default())
            .await;

        assert_eq!(outcome.status, RunStatus::Completed);
        assert_eq!(outcome.turns, 1);
        assert!(events
            .events()
            .iter()
            .any(|event| event.kind == EventKind::RunFinished));
    }

    #[tokio::test]
    async fn pauses_when_approval_is_required() {
        let events = InMemoryEventSink::default();
        let runtime = AgentRuntime::new(
            FakeProvider::new([model_turn(AgentAction::RequestApproval {
                id: "act_approval".into(),
                reason: "Need write".to_string(),
                approval_kind: ApprovalKind::FileWrite,
                summary: "Write Cargo.toml".to_string(),
            })]),
            events.clone(),
        );

        let outcome = runtime
            .run(
                RunConfig::local_test("approval"),
                CancellationFlag::default(),
            )
            .await;

        assert_eq!(outcome.status, RunStatus::ApprovalRequired);
        assert!(events
            .events()
            .iter()
            .any(|event| event.kind == EventKind::ApprovalRequired));
    }

    #[tokio::test]
    async fn fails_when_max_turns_is_exceeded() {
        let events = InMemoryEventSink::default();
        let runtime = AgentRuntime::new(
            FakeProvider::new([
                model_turn(AgentAction::Respond {
                    id: "act_1".into(),
                    reason: "continue".to_string(),
                    message: "not done".to_string(),
                }),
                model_turn(AgentAction::Respond {
                    id: "act_2".into(),
                    reason: "continue".to_string(),
                    message: "still not done".to_string(),
                }),
            ]),
            events,
        );
        let mut config = RunConfig::local_test("loop");
        config.max_turns = 2;

        let outcome = runtime.run(config, CancellationFlag::default()).await;

        assert_eq!(outcome.status, RunStatus::Failed);
        assert!(outcome.error.unwrap().contains("max_turns=2"));
    }

    #[tokio::test]
    async fn mandatory_context_overflow_fails_before_the_provider_is_called() {
        let events = InMemoryEventSink::default();
        // An empty provider queue deliberately proves that the runtime fails
        // during packing, before it tries to request a model turn.
        let runtime = AgentRuntime::new(FakeProvider::new([]), events.clone());
        let mut config = RunConfig::local_test("small budget");
        config.messages = vec![
            ModelMessage::system("x".repeat(80)),
            ModelMessage::user("task"),
        ];
        config.context_budget = ContextBudget {
            max_input_tokens: 12,
            recent_message_tokens: 4,
            max_tool_result_tokens: 4,
            reserved_output_tokens: 4,
            characters_per_token: 4,
        };

        let outcome = runtime.run(config, CancellationFlag::default()).await;

        assert_eq!(outcome.status, RunStatus::Failed);
        assert_eq!(outcome.error.as_deref(), Some("context_budget_exceeded"));
        assert!(!events
            .events()
            .iter()
            .any(|event| event.kind == EventKind::ModelRequestStarted));
    }

    #[tokio::test]
    async fn cancellation_before_first_turn_stops_run() {
        let cancellation = CancellationFlag::default();
        cancellation.cancel();

        let events = InMemoryEventSink::default();
        let runtime = AgentRuntime::new(FakeProvider::new([]), events.clone());

        let outcome = runtime
            .run(RunConfig::local_test("cancel"), cancellation)
            .await;

        assert_eq!(outcome.status, RunStatus::Cancelled);
        assert!(events
            .events()
            .iter()
            .any(|event| event.kind == EventKind::RunCancelled));
    }

    #[tokio::test]
    async fn executes_read_file_tool_and_continues_to_finish() {
        let temp = unique_temp_dir("runtime-read");
        fs::create_dir_all(&temp).unwrap();
        fs::write(temp.join("README.md"), "hello from tool").unwrap();

        let events = InMemoryEventSink::default();
        let runtime = AgentRuntime::with_tools(
            FakeProvider::new([
                model_turn(AgentAction::ReadFile {
                    id: "act_read".into(),
                    reason: "read readme".to_string(),
                    path: Utf8PathBuf::from("README.md"),
                    max_bytes: None,
                    start_line: None,
                    line_count: None,
                }),
                model_turn(AgentAction::Finish {
                    id: "act_done".into(),
                    reason: "done".to_string(),
                    summary: "read file".to_string(),
                    success: true,
                }),
            ]),
            events.clone(),
            LocalReadOnlyFsTools::new(&temp).unwrap(),
        );

        let outcome = runtime
            .run(RunConfig::local_test("read"), CancellationFlag::default())
            .await;

        assert_eq!(outcome.status, RunStatus::Completed);
        assert_eq!(outcome.turns, 2);
        assert!(events
            .events()
            .iter()
            .any(|event| event.kind == EventKind::ToolFinished));
    }

    #[tokio::test]
    async fn recoverable_tool_error_is_returned_to_the_model() {
        let temp = unique_temp_dir("runtime-recoverable-error");
        fs::create_dir_all(&temp).unwrap();
        let events = InMemoryEventSink::default();
        let runtime = AgentRuntime::with_tools(
            FakeProvider::new([
                model_turn(AgentAction::ReadFile {
                    id: "act_missing".into(),
                    reason: "inspect a guessed file".to_string(),
                    path: Utf8PathBuf::from("missing.rs"),
                    max_bytes: None,
                    start_line: None,
                    line_count: None,
                }),
                model_turn(AgentAction::Finish {
                    id: "act_done".into(),
                    reason: "the missing file was reported".to_string(),
                    summary: "recovered".to_string(),
                    success: true,
                }),
            ]),
            events.clone(),
            LocalReadOnlyFsTools::new(&temp).unwrap(),
        );
        let outcome = runtime
            .run(
                RunConfig::local_test("recover"),
                CancellationFlag::default(),
            )
            .await;
        assert_eq!(outcome.status, RunStatus::Completed);
        assert!(events.events().iter().any(|event| {
            event.kind == EventKind::ToolFinished && event.payload["content"]["recoverable"] == true
        }));
    }

    #[tokio::test]
    async fn ambiguous_patch_failure_is_returned_to_the_model() {
        let temp = unique_temp_dir("runtime-ambiguous-patch");
        fs::create_dir_all(&temp).unwrap();
        fs::write(temp.join("README.md"), "one\ntwo\n").unwrap();
        let events = InMemoryEventSink::default();
        let runtime = AgentRuntime::with_tools(
            FakeProvider::new([
                model_turn(AgentAction::PatchFile {
                    id: "act_patch".into(),
                    reason: "try an imprecise patch".to_string(),
                    path: Utf8PathBuf::from("README.md"),
                    patch: crate::TextPatch {
                        find: "missing text".to_string(),
                        replace: "replacement".to_string(),
                        replace_all: false,
                    },
                }),
                model_turn(AgentAction::Finish {
                    id: "act_done".into(),
                    reason: "patch failure was visible".to_string(),
                    summary: "recovered".to_string(),
                    success: true,
                }),
            ]),
            events.clone(),
            LocalReadOnlyFsTools::new(&temp).unwrap(),
        );

        let mut config = RunConfig::local_test("recover ambiguous patch");
        config.policy = SafetyPolicy {
            mode: PolicyMode::TrustedWrites,
            require_approval_for_writes: false,
            ..SafetyPolicy::default()
        };
        let outcome = runtime.run(config, CancellationFlag::default()).await;

        assert_eq!(outcome.status, RunStatus::Completed);
        assert!(events.events().iter().any(|event| {
            event.kind == EventKind::ToolFinished
                && event.payload["content"]["error_kind"] == "invalid_arguments"
                && event.payload["content"]["recoverable"] == true
        }));
    }

    #[tokio::test]
    async fn timeout_failure_is_returned_to_the_model() {
        let events = InMemoryEventSink::default();
        let runtime = AgentRuntime::with_tools(
            FakeProvider::new([
                model_turn(AgentAction::RunShell {
                    id: "act_timeout".into(),
                    reason: "run focused tests".to_string(),
                    cmd: "cargo test".to_string(),
                    cwd: None,
                    timeout_ms: Some(5),
                    dry_run: false,
                }),
                model_turn(AgentAction::Finish {
                    id: "act_done".into(),
                    reason: "timeout was visible".to_string(),
                    summary: "recovered".to_string(),
                    success: true,
                }),
            ]),
            events.clone(),
            ErroringExecutor {
                error: ToolError::TimedOut {
                    command: "cargo test".to_string(),
                    timeout_ms: 5,
                },
            },
        );
        let mut config = RunConfig::local_test("recover timeout");
        config.policy = SafetyPolicy {
            mode: PolicyMode::TrustedWrites,
            require_approval_for_writes: false,
            ..SafetyPolicy::default()
        };

        let outcome = runtime.run(config, CancellationFlag::default()).await;

        assert_eq!(outcome.status, RunStatus::Completed);
        assert!(events.events().iter().any(|event| {
            event.kind == EventKind::ToolFinished
                && event.payload["content"]["error_kind"] == "timed_out"
        }));
    }

    #[tokio::test]
    async fn recovery_limits_stop_an_identical_error_loop() {
        let temp = unique_temp_dir("runtime-recovery-limit");
        fs::create_dir_all(&temp).unwrap();
        let events = InMemoryEventSink::default();
        let missing = || AgentAction::ReadFile {
            id: "act_missing".into(),
            reason: "repeat the same missing path".to_string(),
            path: Utf8PathBuf::from("missing.rs"),
            max_bytes: None,
            start_line: None,
            line_count: None,
        };
        let runtime = AgentRuntime::with_tools(
            FakeProvider::new([
                model_turn(missing()),
                model_turn(missing()),
                model_turn(missing()),
            ]),
            events.clone(),
            LocalReadOnlyFsTools::new(&temp).unwrap(),
        );

        let outcome = runtime
            .run(
                RunConfig::local_test("bounded recovery"),
                CancellationFlag::default(),
            )
            .await;

        assert_eq!(outcome.status, RunStatus::Failed);
        assert_eq!(outcome.error.as_deref(), Some("tool_recovery_exhausted"));
        assert_eq!(
            events
                .events()
                .iter()
                .filter(|event| event.kind == EventKind::ToolFinished)
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn terminal_workspace_error_fails_without_a_recovery_turn() {
        let temp = unique_temp_dir("runtime-terminal-scope");
        fs::create_dir_all(temp.join("workspace")).unwrap();
        fs::write(temp.join("outside.txt"), "not in workspace").unwrap();
        let events = InMemoryEventSink::default();
        let runtime = AgentRuntime::with_tools(
            FakeProvider::new([model_turn(AgentAction::ReadFile {
                id: "act_outside".into(),
                reason: "attempt scope escape".to_string(),
                path: Utf8PathBuf::from("../outside.txt"),
                max_bytes: None,
                start_line: None,
                line_count: None,
            })]),
            events.clone(),
            LocalReadOnlyFsTools::new(temp.join("workspace")).unwrap(),
        );

        let outcome = runtime
            .run(
                RunConfig::local_test("terminal scope"),
                CancellationFlag::default(),
            )
            .await;

        assert_eq!(outcome.status, RunStatus::Failed);
        assert!(outcome.error.unwrap().contains("outside workspace"));
        assert!(!events.events().iter().any(|event| {
            event.kind == EventKind::ToolFinished && event.payload["content"]["recoverable"] == true
        }));
    }

    #[tokio::test]
    async fn approved_action_recoverable_failure_reaches_the_resumed_model() {
        let events = InMemoryEventSink::default();
        let write = AgentAction::WriteFile {
            id: "act_write".into(),
            reason: "write after approval".to_string(),
            path: Utf8PathBuf::from("README.md"),
            content: "changed".to_string(),
        };
        let initial = AgentRuntime::with_tools(
            FakeProvider::new([model_turn(write.clone())]),
            events.clone(),
            NoopToolExecutor,
        );
        let config = RunConfig::local_test("post approval recovery");
        let paused = initial
            .run(config.clone(), CancellationFlag::default())
            .await;
        let approval = paused.approval.expect("write should pause for approval");

        let resumed = AgentRuntime::with_tools(
            FakeProvider::new([model_turn(AgentAction::Finish {
                id: "act_done".into(),
                reason: "post-approval error was visible".to_string(),
                summary: "recovered".to_string(),
                success: true,
            })]),
            events.clone(),
            ErroringExecutor {
                error: ToolError::Io {
                    message: "temporary write failure".to_string(),
                },
            },
        );
        let outcome = resumed
            .resume_after_approval(
                config,
                CancellationFlag::default(),
                ApprovedAction {
                    approval_id: "appr_recover".to_string(),
                    action: approval.action.expect("approval stores action"),
                    resume_messages: approval.resume_messages,
                    turns_completed: approval.turns_completed,
                },
            )
            .await;

        assert_eq!(outcome.status, RunStatus::Completed);
        assert!(events.events().iter().any(|event| {
            event.kind == EventKind::ToolFinished
                && event.payload["content"]["error_kind"] == "io"
                && event.payload["content"]["recoverable"] == true
        }));
    }

    #[tokio::test]
    async fn coding_finish_requires_change_and_final_diff_but_general_finish_does_not() {
        let events = InMemoryEventSink::default();
        let finish = || AgentAction::Finish {
            id: "act_finish".into(),
            reason: "claim completion".to_string(),
            summary: "done".to_string(),
            success: true,
        };
        let runtime = AgentRuntime::new(
            FakeProvider::new([
                model_turn(finish()),
                model_turn(finish()),
                model_turn(finish()),
            ]),
            events.clone(),
        );
        let mut coding = RunConfig::local_test("coding contract");
        coding.task_contract = TaskContract {
            kind: TaskKind::Coding,
            acceptance_criteria: vec!["make a change".to_string()],
            require_workspace_change: true,
            require_post_change_diff: true,
        };

        let outcome = runtime.run(coding, CancellationFlag::default()).await;

        assert_eq!(outcome.status, RunStatus::Failed);
        assert_eq!(
            outcome.error.as_deref(),
            Some("completion_evidence_exhausted")
        );
        assert_eq!(
            events
                .events()
                .iter()
                .filter(|event| {
                    event.kind == EventKind::ToolFinished
                        && event.payload["content"]["error_kind"] == "completion_evidence_missing"
                })
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn policy_pause_prevents_write_execution() {
        let events = InMemoryEventSink::default();
        let runtime = AgentRuntime::with_tools(
            FakeProvider::new([model_turn(AgentAction::WriteFile {
                id: "act_write".into(),
                reason: "write".to_string(),
                path: Utf8PathBuf::from("README.md"),
                content: "hello".to_string(),
            })]),
            events.clone(),
            crate::NoopToolExecutor,
        );

        let mut config = RunConfig::local_test("write");
        config.run_scope = RunScope {
            run_id: Some("run_test".to_string()),
            namespace: Some("apps-dev".to_string()),
            repo: Some("git@example.test/team/app.git".to_string()),
            branch: Some("feature/pharness".to_string()),
            work_item_id: None,
            workspace_id: None,
            work_plan_id: None,
            change_set_id: None,
            production_impacting: false,
        };

        let outcome = runtime.run(config, CancellationFlag::default()).await;

        assert_eq!(outcome.status, RunStatus::ApprovalRequired);
        let events = events.events();
        assert!(events
            .iter()
            .any(|event| event.kind == EventKind::PolicyEvaluated));
        let approval_required = events
            .iter()
            .find(|event| event.kind == EventKind::ApprovalRequired)
            .expect("approval required event should exist");
        assert_eq!(
            approval_required.payload["run_scope"]["namespace"],
            "apps-dev"
        );
        assert!(!events
            .iter()
            .any(|event| event.kind == EventKind::ToolStarted));
    }

    #[tokio::test]
    async fn empty_run_scope_serializes_as_null_in_runtime_events() {
        let events = InMemoryEventSink::default();
        let runtime = AgentRuntime::with_tools(
            FakeProvider::new([model_turn(AgentAction::WriteFile {
                id: "act_write".into(),
                reason: "write".to_string(),
                path: Utf8PathBuf::from("README.md"),
                content: "hello".to_string(),
            })]),
            events.clone(),
            crate::NoopToolExecutor,
        );

        let outcome = runtime
            .run(RunConfig::local_test("write"), CancellationFlag::default())
            .await;

        assert_eq!(outcome.status, RunStatus::ApprovalRequired);
        let events = events.events();
        let policy_evaluated = events
            .iter()
            .find(|event| event.kind == EventKind::PolicyEvaluated)
            .expect("policy event should exist");
        let approval_required = events
            .iter()
            .find(|event| event.kind == EventKind::ApprovalRequired)
            .expect("approval event should exist");

        assert!(policy_evaluated.payload["run_scope"].is_null());
        assert!(approval_required.payload["run_scope"].is_null());
    }

    #[tokio::test]
    async fn approved_action_executes_exact_paused_payload_and_continues() {
        let temp = unique_temp_dir("runtime-approved-write");
        fs::create_dir_all(&temp).unwrap();

        let events = InMemoryEventSink::default();
        let write_action = AgentAction::WriteFile {
            id: "call_write".into(),
            reason: "write".to_string(),
            path: Utf8PathBuf::from("approved.txt"),
            content: "approved content".to_string(),
        };
        let runtime = AgentRuntime::with_tools(
            FakeProvider::new([model_turn(write_action.clone())]),
            events.clone(),
            LocalReadOnlyFsTools::new(&temp).unwrap(),
        );
        let config = RunConfig::local_test("write after approval");

        let paused = runtime
            .run(config.clone(), CancellationFlag::default())
            .await;
        assert_eq!(paused.status, RunStatus::ApprovalRequired);
        assert_eq!(paused.turns, 1);
        assert!(!temp.join("approved.txt").exists());

        let approval = paused.approval.expect("approval should be captured");
        assert_eq!(approval.action.as_ref(), Some(&write_action));

        let resume_runtime = AgentRuntime::with_tools(
            FakeProvider::new([model_turn(AgentAction::Finish {
                id: "act_done".into(),
                reason: "done".to_string(),
                summary: "wrote approved file".to_string(),
                success: true,
            })]),
            events.clone(),
            LocalReadOnlyFsTools::new(&temp).unwrap(),
        );
        let resumed = resume_runtime
            .resume_after_approval(
                config,
                CancellationFlag::default(),
                ApprovedAction {
                    approval_id: "appr_test".to_string(),
                    action: approval.action.expect("approved action should exist"),
                    resume_messages: approval.resume_messages,
                    turns_completed: approval.turns_completed,
                },
            )
            .await;

        assert_eq!(resumed.status, RunStatus::Completed);
        assert_eq!(
            fs::read_to_string(temp.join("approved.txt")).unwrap(),
            "approved content"
        );
        assert!(events
            .events()
            .iter()
            .any(|event| event.kind == EventKind::RunResumed));
    }

    #[tokio::test]
    async fn policy_denies_privileged_shell_command() {
        let events = InMemoryEventSink::default();
        let runtime = AgentRuntime::with_tools(
            FakeProvider::new([model_turn(AgentAction::RunShell {
                id: "act_shell".into(),
                reason: "shell".to_string(),
                cmd: "sudo whoami".to_string(),
                cwd: None,
                timeout_ms: None,
                dry_run: false,
            })]),
            events.clone(),
            crate::NoopToolExecutor,
        );

        let outcome = runtime
            .run(RunConfig::local_test("sudo"), CancellationFlag::default())
            .await;

        assert_eq!(outcome.status, RunStatus::Failed);
        assert!(outcome.error.unwrap().contains("privileged command denied"));
        assert!(!events
            .events()
            .iter()
            .any(|event| event.kind == EventKind::ToolStarted));
    }

    #[tokio::test]
    async fn trusted_policy_allows_write_file_tool() {
        let temp = unique_temp_dir("runtime-write");
        fs::create_dir_all(&temp).unwrap();

        let events = InMemoryEventSink::default();
        let runtime = AgentRuntime::with_tools(
            FakeProvider::new([
                model_turn(AgentAction::WriteFile {
                    id: "act_write".into(),
                    reason: "write".to_string(),
                    path: Utf8PathBuf::from("hello.txt"),
                    content: "hello world".to_string(),
                }),
                model_turn(AgentAction::Finish {
                    id: "act_done".into(),
                    reason: "done".to_string(),
                    summary: "wrote file".to_string(),
                    success: true,
                }),
            ]),
            events.clone(),
            LocalReadOnlyFsTools::new(&temp).unwrap(),
        );
        let mut config = RunConfig::local_test("write");
        config.policy = SafetyPolicy {
            mode: PolicyMode::TrustedWrites,
            require_approval_for_writes: false,
            ..SafetyPolicy::default()
        };

        let outcome = runtime.run(config, CancellationFlag::default()).await;

        assert_eq!(outcome.status, RunStatus::Completed);
        assert_eq!(
            fs::read_to_string(temp.join("hello.txt")).unwrap(),
            "hello world"
        );
        assert!(events
            .events()
            .iter()
            .any(|event| event.kind == EventKind::ToolFinished));
    }

    #[tokio::test]
    async fn permission_grant_allows_write_and_emits_grant_id() {
        let temp = unique_temp_dir("runtime-granted-write");
        fs::create_dir_all(&temp).unwrap();

        let events = InMemoryEventSink::default();
        let runtime = AgentRuntime::with_tools(
            FakeProvider::new([
                model_turn(AgentAction::WriteFile {
                    id: "act_write".into(),
                    reason: "write".to_string(),
                    path: Utf8PathBuf::from("granted.txt"),
                    content: "granted content".to_string(),
                }),
                model_turn(AgentAction::Finish {
                    id: "act_done".into(),
                    reason: "done".to_string(),
                    summary: "wrote granted file".to_string(),
                    success: true,
                }),
            ]),
            events.clone(),
            LocalReadOnlyFsTools::new(&temp).unwrap(),
        );
        let mut config = RunConfig::local_test("write with grant");
        config.policy.permission_grants = vec![PermissionGrant {
            id: "pgrant_test".to_string(),
            subject: "agent:local-worker".to_string(),
            scope: PermissionGrantScope {
                environment: Some("local".to_string()),
                capability_kinds: vec![CapabilityKind::Filesystem],
                actions: vec!["write_file".to_string()],
                max_risk: Some(RiskLevel::Medium),
                namespaces: Vec::new(),
                repos: Vec::new(),
                branches: Vec::new(),
                run_ids: Vec::new(),
                workspace_ids: Vec::new(),
                writable_path_globs: vec!["granted.txt".to_string()],
                work_item_ids: Vec::new(),
                work_plan_ids: Vec::new(),
                change_set_ids: Vec::new(),
                pipeline_intent_ids: Vec::new(),
                deployment_intent_ids: Vec::new(),
                argo_applications: Vec::new(),
                git_delivery_plan_artifact_ids: Vec::new(),
                gitops_change_set_ids: Vec::new(),
                gitops_delivery_plan_artifact_ids: Vec::new(),
                pipeline_contract_ids: Vec::new(),
                deployment_contract_ids: Vec::new(),
                source_merge_shas: Vec::new(),
                gitops_merge_shas: Vec::new(),
                image_digests: Vec::new(),
                production_impacting: None,
            },
            policy: PermissionGrantPolicy {
                policy_mode: PolicyMode::TrustedWrites,
            },
            expires_at: None,
        }];

        let outcome = runtime.run(config, CancellationFlag::default()).await;

        assert_eq!(outcome.status, RunStatus::Completed);
        assert_eq!(
            fs::read_to_string(temp.join("granted.txt")).unwrap(),
            "granted content"
        );
        let events = events.events();
        assert!(!events
            .iter()
            .any(|event| event.kind == EventKind::ApprovalRequired));
        assert!(events.iter().any(|event| {
            event.kind == EventKind::PolicyEvaluated
                && event.payload["decision"]["grant_id"] == "pgrant_test"
        }));
    }

    #[tokio::test]
    async fn soft_turn_budget_pauses_and_resumes_the_same_transcript() {
        let runtime = AgentRuntime::with_tools(
            FakeProvider::new([
                model_turn(AgentAction::Respond {
                    id: "act_progress".into(),
                    reason: "progress".to_string(),
                    message: "continuing after an extension".to_string(),
                }),
                model_turn(AgentAction::Finish {
                    id: "act_done".into(),
                    reason: "done".to_string(),
                    summary: "completed after extension".to_string(),
                    success: true,
                }),
            ]),
            InMemoryEventSink::default(),
            NoopToolExecutor,
        );
        let budget = RunBudget {
            initial_turns: 1,
            hard_turns: 3,
            initial_tokens: 10_000,
            hard_tokens: 20_000,
            active_execution_seconds: 60,
            recoverable_tool_errors: 4,
            identical_failures: 2,
            verification_reserve_turns: 0,
        };
        let mut config = RunConfig::local_test("budget pause");
        config.max_turns = 1;
        config.run_budget = Some(budget.clone());
        config.budget_consumption = RunBudgetConsumption {
            allowed_turns: 1,
            allowed_tokens: 10_000,
            ..RunBudgetConsumption::default()
        };

        let paused = runtime
            .run(config.clone(), CancellationFlag::default())
            .await;
        assert_eq!(paused.status, RunStatus::BudgetExtensionRequired);
        let pause = paused.budget_pause.expect("soft budget must be resumable");
        assert_eq!(pause.turns_completed, 1);
        assert_eq!(pause.consumption.turns_used, 1);

        let mut resumed_config = config;
        resumed_config.max_turns = 3;
        resumed_config.budget_consumption = RunBudgetConsumption {
            allowed_turns: 3,
            allowed_tokens: 20_000,
            extensions: 1,
            ..pause.consumption
        };
        let resumed = runtime
            .resume_after_budget(
                resumed_config,
                CancellationFlag::default(),
                BudgetResume {
                    resume_messages: pause.resume_messages,
                    turns_completed: pause.turns_completed,
                },
            )
            .await;

        assert_eq!(resumed.status, RunStatus::Completed);
        assert_eq!(resumed.turns, 2);
        assert_eq!(resumed.consumption.turns_used, 2);
        assert_eq!(resumed.consumption.extensions, 1);
    }

    fn unique_temp_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "pharness-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
}

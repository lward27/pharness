use pharness_core::{
    InferenceBackendKind, ModelMessage, ModelRequest, ModelRole, ReasoningContextMode,
    ReasoningReplay, StageInferencePolicyRevision, ToolChoiceMode, ToolProtocolMode, ToolSpec,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tools: Vec<ChatTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_history: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<OpenRouterProviderPreferences>,
}

pub fn build_chat_request(
    backend: InferenceBackendKind,
    model: impl Into<String>,
    request: ModelRequest,
    policy: &StageInferencePolicyRevision,
    include_stream_usage: bool,
    openrouter_provider_slug: Option<&str>,
) -> ChatRequest {
    let mode = request.mode;
    let reasoning_effort = policy
        .reasoning
        .effort
        .map(|effort| effort.as_str().to_string());
    let reasoning_history = match (backend, policy.reasoning.context_mode) {
        (InferenceBackendKind::Fireworks, ReasoningContextMode::CurrentTurn) => {
            Some("interleaved".to_string())
        }
        (InferenceBackendKind::Fireworks, ReasoningContextMode::AllTurns) => {
            Some("preserved".to_string())
        }
        _ => None,
    };
    let reasoning = match backend {
        InferenceBackendKind::Openrouter => {
            let context = match policy.reasoning.context_mode {
                ReasoningContextMode::CurrentTurn => Some("current_turn".to_string()),
                ReasoningContextMode::AllTurns => Some("all_turns".to_string()),
                ReasoningContextMode::Disabled | ReasoningContextMode::ProviderDefault => None,
            };
            (reasoning_effort.is_some() || context.is_some()).then_some(ReasoningOptions {
                effort: reasoning_effort.clone(),
                context,
                exclude: false,
            })
        }
        _ => None,
    };
    let provider = if backend == InferenceBackendKind::Openrouter {
        openrouter_provider_slug.map(|slug| OpenRouterProviderPreferences {
            order: vec![slug.to_string()],
            only: vec![slug.to_string()],
            allow_fallbacks: false,
            require_parameters: true,
            data_collection: "deny".to_string(),
        })
    } else {
        None
    };

    ChatRequest {
        model: model.into(),
        messages: request
            .messages
            .into_iter()
            .map(|message| ChatMessage::from_model_message(backend, message))
            .collect(),
        tools: if mode == ToolProtocolMode::NativeTools {
            request.tools.into_iter().map(ChatTool::from).collect()
        } else {
            Vec::new()
        },
        tool_choice: (mode == ToolProtocolMode::NativeTools).then_some(match policy.tool_choice {
            ToolChoiceMode::Auto => ToolChoice::Auto,
            ToolChoiceMode::Required => ToolChoice::Required,
            // A named specific tool is not part of a V1 policy revision. No
            // configured target advertises this mode yet, so validation
            // rejects it before this compatibility representation is used.
            ToolChoiceMode::Specific => ToolChoice::Required,
        }),
        parallel_tool_calls: (mode == ToolProtocolMode::NativeTools).then_some(false),
        stream: true,
        stream_options: include_stream_usage.then_some(StreamOptions {
            include_usage: true,
        }),
        temperature: policy.temperature(),
        // The immutable stage policy owns the provider output ceiling. The
        // legacy per-request value is retained on ModelRequest only for the
        // direct-provider compatibility path.
        max_tokens: policy.max_output_tokens,
        response_format: (mode == ToolProtocolMode::JsonAction)
            .then_some(ResponseFormat::JsonObject),
        reasoning_effort: (backend != InferenceBackendKind::Openrouter)
            .then_some(reasoning_effort)
            .flatten(),
        reasoning_history,
        reasoning,
        provider,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReasoningOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    pub exclude: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenRouterProviderPreferences {
    pub order: Vec<String>,
    pub only: Vec<String>,
    pub allow_fallbacks: bool,
    pub require_parameters: bool,
    pub data_collection: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamOptions {
    pub include_usage: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tool_calls: Vec<MessageToolCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_details: Option<serde_json::Value>,
}

impl ChatMessage {
    fn from_model_message(backend: InferenceBackendKind, message: ModelMessage) -> Self {
        // Reasoning replay is part of the provider wire protocol, not a
        // portable Chat Completions message field. In particular, Fireworks
        // requires assistant reasoning to be replayed as `reasoning_content`,
        // while OpenRouter uses `reasoning`/`reasoning_details`. Emitting both
        // text fields makes the second tool turn invalid for Fireworks.
        let (reasoning_content, reasoning, reasoning_details) = match (backend, message.reasoning) {
            (InferenceBackendKind::Openrouter, Some(ReasoningReplay::Text(value))) => {
                (None, Some(value), None)
            }
            (InferenceBackendKind::Openrouter, Some(ReasoningReplay::Structured(value))) => {
                (None, None, Some(value))
            }
            (InferenceBackendKind::Fireworks, Some(ReasoningReplay::Text(value))) => {
                (Some(value), None, None)
            }
            (InferenceBackendKind::Fireworks, Some(ReasoningReplay::Structured(_))) => {
                (None, None, None)
            }
            (_, Some(ReasoningReplay::Text(value))) => (Some(value), None, None),
            (_, Some(ReasoningReplay::Structured(value))) => (None, None, Some(value)),
            (_, None) => (None, None, None),
        };
        Self {
            role: match message.role {
                ModelRole::System => "system",
                ModelRole::User => "user",
                ModelRole::Assistant => "assistant",
                ModelRole::Tool => "tool",
            }
            .to_string(),
            content: message.content,
            tool_call_id: message.tool_call_id,
            tool_calls: message
                .tool_calls
                .into_iter()
                .map(MessageToolCall::from)
                .collect(),
            reasoning_content,
            reasoning,
            reasoning_details,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessageToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: MessageFunctionCall,
}

impl From<pharness_core::ModelToolCall> for MessageToolCall {
    fn from(tool_call: pharness_core::ModelToolCall) -> Self {
        Self {
            id: tool_call.id,
            tool_type: "function".to_string(),
            function: MessageFunctionCall {
                name: tool_call.name,
                arguments: tool_call.arguments,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessageFunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatTool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionTool,
}

impl From<ToolSpec> for ChatTool {
    fn from(tool: ToolSpec) -> Self {
        Self {
            tool_type: "function".to_string(),
            function: FunctionTool {
                name: tool.name,
                description: tool.description,
                parameters: tool.parameters_schema,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionTool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoice {
    Auto,
    None,
    Required,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseFormat {
    JsonObject,
    JsonSchema { json_schema: serde_json::Value },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StreamChunk {
    pub id: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    #[serde(default)]
    pub choices: Vec<ChoiceDelta>,
    #[serde(default)]
    pub usage: Option<TokenUsagePayload>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TokenUsagePayload {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
    #[serde(default)]
    pub total_tokens: u32,
    #[serde(default)]
    pub prompt_tokens_details: Option<PromptTokenDetails>,
    #[serde(default)]
    pub completion_tokens_details: Option<CompletionTokenDetails>,
    #[serde(default)]
    pub cost: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PromptTokenDetails {
    #[serde(default)]
    pub cached_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CompletionTokenDetails {
    #[serde(default)]
    pub reasoning_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ChoiceDelta {
    #[allow(dead_code)]
    pub index: u32,
    pub delta: Delta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Delta {
    #[allow(dead_code)]
    pub role: Option<String>,
    pub content: Option<String>,
    pub reasoning_content: Option<String>,
    pub reasoning: Option<String>,
    pub reasoning_details: Option<Vec<serde_json::Value>>,
    pub tool_calls: Option<Vec<ToolCallDelta>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ToolCallDelta {
    pub index: u32,
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub tool_type: Option<String>,
    pub function: Option<FunctionCallDelta>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FunctionCallDelta {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pharness_core::{
        CapabilityKind, InferenceTargetRef, ReasoningEffort, ReasoningRequestPolicy,
    };

    fn policy() -> StageInferencePolicyRevision {
        StageInferencePolicyRevision {
            schema_version: pharness_core::INFERENCE_POLICY_SCHEMA.into(),
            policy_id: "planner-test".into(),
            revision: "v1".into(),
            display_name: "Planner test".into(),
            eligible_profiles: vec!["repo-planner".into()],
            eligible_stages: vec![pharness_core::InferenceStage::Plan],
            target: InferenceTargetRef {
                target_id: "target".into(),
                revision: "v1".into(),
            },
            target_hash: "hash".into(),
            reasoning: ReasoningRequestPolicy {
                effort: Some(ReasoningEffort::High),
                context_mode: ReasoningContextMode::CurrentTurn,
                expose_replay: true,
            },
            temperature_milli: Some(100),
            max_output_tokens: 8_192,
            max_input_tokens: 16_000,
            tool_protocol: ToolProtocolMode::NativeTools,
            tool_choice: pharness_core::ToolChoiceMode::Required,
            transport_max_attempts: 3,
            selectable: true,
            policy_hash: "hash".into(),
        }
    }

    fn request() -> ModelRequest {
        ModelRequest {
            session_id: pharness_core::SessionId::new("session"),
            run_id: pharness_core::RunId::new("run"),
            messages: vec![ModelMessage::user("plan")],
            tools: vec![ToolSpec::new(
                "submit_work_plan",
                "Submit plan",
                serde_json::json!({"type":"object"}),
                CapabilityKind::AgentControl,
            )],
            mode: ToolProtocolMode::NativeTools,
            tool_choice: pharness_core::ToolChoiceMode::Required,
            temperature: 0.1,
            max_tokens: 8_192,
            reasoning: None,
        }
    }

    #[test]
    fn pins_openrouter_provider_and_disables_fallback() {
        let wire = build_chat_request(
            InferenceBackendKind::Openrouter,
            "model",
            request(),
            &policy(),
            true,
            Some("deepinfra/turbo"),
        );
        let value = serde_json::to_value(wire).unwrap();
        assert_eq!(value["provider"]["order"][0], "deepinfra/turbo");
        assert_eq!(value["provider"]["only"][0], "deepinfra/turbo");
        assert_eq!(value["provider"]["allow_fallbacks"], false);
        assert_eq!(value["provider"]["require_parameters"], true);
        assert_eq!(value["reasoning"]["effort"], "high");
        assert_eq!(value["reasoning"]["context"], "current_turn");
    }

    #[test]
    fn maps_fireworks_reasoning_history() {
        let mut model_request = request();
        model_request.messages.push(ModelMessage {
            role: ModelRole::Assistant,
            content: String::new(),
            tool_call_id: None,
            tool_calls: vec![pharness_core::ModelToolCall {
                id: "call-one".into(),
                name: "submit_work_plan".into(),
                arguments: "{}".into(),
            }],
            reasoning: Some(ReasoningReplay::Text("private replay state".into())),
        });
        let wire = build_chat_request(
            InferenceBackendKind::Fireworks,
            "model",
            model_request,
            &policy(),
            true,
            None,
        );
        let value = serde_json::to_value(wire).unwrap();
        assert_eq!(value["reasoning_effort"], "high");
        assert_eq!(value["reasoning_history"], "interleaved");
        assert!(value.get("reasoning").is_none());
        assert_eq!(
            value["messages"][1]["reasoning_content"],
            "private replay state"
        );
        assert!(value["messages"][1].get("reasoning").is_none());
        assert!(value["messages"][1].get("reasoning_details").is_none());
    }

    #[test]
    fn maps_openrouter_reasoning_without_fireworks_field() {
        let mut model_request = request();
        model_request.messages.push(ModelMessage {
            role: ModelRole::Assistant,
            content: String::new(),
            tool_call_id: None,
            tool_calls: Vec::new(),
            reasoning: Some(ReasoningReplay::Text("opaque replay state".into())),
        });
        let value = serde_json::to_value(build_chat_request(
            InferenceBackendKind::Openrouter,
            "model",
            model_request,
            &policy(),
            true,
            Some("deepinfra/turbo"),
        ))
        .unwrap();
        assert_eq!(value["messages"][1]["reasoning"], "opaque replay state");
        assert!(value["messages"][1].get("reasoning_content").is_none());
    }
}

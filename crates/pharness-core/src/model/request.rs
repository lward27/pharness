use super::ToolSpec;
use crate::{ReasoningReplay, ReasoningRequestPolicy, RunId, SessionId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRequest {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub messages: Vec<ModelMessage>,
    pub tools: Vec<ToolSpec>,
    pub mode: ToolProtocolMode,
    #[serde(default)]
    pub tool_choice: ToolChoiceMode,
    pub temperature: f32,
    pub max_tokens: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningRequestPolicy>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelMessage {
    pub role: ModelRole,
    pub content: String,
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ModelToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningReplay>,
}

impl ModelMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: ModelRole::System,
            content: content.into(),
            tool_call_id: None,
            tool_calls: Vec::new(),
            reasoning: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: ModelRole::User,
            content: content.into(),
            tool_call_id: None,
            tool_calls: Vec::new(),
            reasoning: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolProtocolMode {
    NativeTools,
    JsonAction,
}

/// Provider-neutral control over native function-tool selection.
///
/// Legacy PHarness requests required a tool on every turn. Reliability V2
/// defaults new policies to `Auto`, allowing the model to reason before
/// selecting exactly one action while the runtime still rejects ambiguous or
/// malformed action responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoiceMode {
    Auto,
    #[default]
    Required,
    Specific,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

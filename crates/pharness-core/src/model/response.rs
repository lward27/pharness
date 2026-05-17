use super::{AgentAction, ModelToolCall};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelTurn {
    pub raw_provider_id: Option<String>,
    pub assistant_message: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assistant_tool_calls: Vec<ModelToolCall>,
    pub action: AgentAction,
    pub usage: Option<TokenUsage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCapabilities {
    pub native_tool_calling: bool,
    pub streaming: bool,
    pub json_schema_response_format: bool,
}

use pharness_core::{ModelMessage, ModelRole, ModelToolCall, ToolSpec};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FireworksChatRequest {
    pub model: String,
    pub messages: Vec<FireworksChatMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tools: Vec<FireworksChatTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<FireworksToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    pub stream: bool,
    pub temperature: f32,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<FireworksResponseFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

impl FireworksChatRequest {
    pub fn native_tools(
        model: impl Into<String>,
        messages: Vec<ModelMessage>,
        tools: Vec<ToolSpec>,
        temperature: f32,
        max_tokens: u32,
    ) -> Self {
        Self {
            model: model.into(),
            messages: messages
                .into_iter()
                .map(FireworksChatMessage::from)
                .collect(),
            tools: tools.into_iter().map(FireworksChatTool::from).collect(),
            tool_choice: Some(FireworksToolChoice::Required),
            parallel_tool_calls: Some(false),
            stream: true,
            temperature,
            max_tokens,
            response_format: None,
            reasoning_effort: None,
        }
    }

    pub fn json_action(
        model: impl Into<String>,
        messages: Vec<ModelMessage>,
        temperature: f32,
        max_tokens: u32,
    ) -> Self {
        Self {
            model: model.into(),
            messages: messages
                .into_iter()
                .map(FireworksChatMessage::from)
                .collect(),
            tools: Vec::new(),
            tool_choice: None,
            parallel_tool_calls: None,
            stream: true,
            temperature,
            max_tokens,
            response_format: Some(FireworksResponseFormat::JsonObject),
            reasoning_effort: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FireworksChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tool_calls: Vec<FireworksMessageToolCall>,
}

impl From<ModelMessage> for FireworksChatMessage {
    fn from(message: ModelMessage) -> Self {
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
                .map(FireworksMessageToolCall::from)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FireworksMessageToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FireworksMessageFunctionCall,
}

impl From<ModelToolCall> for FireworksMessageToolCall {
    fn from(tool_call: ModelToolCall) -> Self {
        Self {
            id: tool_call.id,
            tool_type: "function".to_string(),
            function: FireworksMessageFunctionCall {
                name: tool_call.name,
                arguments: tool_call.arguments,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FireworksMessageFunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FireworksChatTool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FireworksFunctionTool,
}

impl From<ToolSpec> for FireworksChatTool {
    fn from(tool: ToolSpec) -> Self {
        Self {
            tool_type: "function".to_string(),
            function: FireworksFunctionTool {
                name: tool.name,
                description: tool.description,
                parameters: tool.parameters_schema,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FireworksFunctionTool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FireworksToolChoice {
    Auto,
    None,
    Required,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FireworksResponseFormat {
    JsonObject,
    JsonSchema { json_schema: serde_json::Value },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FireworksStreamChunk {
    pub id: Option<String>,
    pub choices: Vec<FireworksChoiceDelta>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FireworksChoiceDelta {
    pub index: u32,
    pub delta: FireworksDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FireworksDelta {
    pub role: Option<String>,
    pub content: Option<String>,
    pub tool_calls: Option<Vec<FireworksToolCallDelta>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FireworksToolCallDelta {
    pub index: u32,
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub tool_type: Option<String>,
    pub function: Option<FireworksFunctionCallDelta>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FireworksFunctionCallDelta {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::FireworksChatRequest;
    use pharness_core::{CapabilityKind, ModelMessage, ToolSpec};

    #[test]
    fn builds_openai_compatible_tool_request() {
        let request = FireworksChatRequest::native_tools(
            "accounts/fireworks/models/kimi-k2p5",
            vec![ModelMessage::user("List files")],
            vec![ToolSpec::new(
                "list_dir",
                "List a workspace directory",
                serde_json::json!({
                    "type": "object",
                    "required": ["path"],
                    "properties": {
                        "path": { "type": "string" }
                    }
                }),
                CapabilityKind::Filesystem,
            )],
            0.1,
            4096,
        );

        let json = serde_json::to_value(request).unwrap();
        assert_eq!(json["tool_choice"], "required");
        assert_eq!(json["parallel_tool_calls"], false);
        assert_eq!(json["tools"][0]["type"], "function");
        assert_eq!(json["tools"][0]["function"]["name"], "list_dir");
    }

    #[test]
    fn serializes_assistant_tool_call_history() {
        let message = super::FireworksChatMessage::from(ModelMessage {
            role: pharness_core::ModelRole::Assistant,
            content: String::new(),
            tool_call_id: None,
            tool_calls: vec![pharness_core::ModelToolCall {
                id: "call_list".to_string(),
                name: "list_dir".to_string(),
                arguments: r#"{"path":".","depth":0}"#.to_string(),
            }],
        });

        let json = serde_json::to_value(message).unwrap();
        assert_eq!(json["role"], "assistant");
        assert_eq!(json["tool_calls"][0]["id"], "call_list");
        assert_eq!(json["tool_calls"][0]["type"], "function");
        assert_eq!(json["tool_calls"][0]["function"]["name"], "list_dir");
    }

    #[test]
    fn parses_streaming_tool_call_chunk() {
        let chunk: super::FireworksStreamChunk = serde_json::from_value(serde_json::json!({
            "id": "chatcmpl-test",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_abc",
                        "type": "function",
                        "function": {
                            "name": "read_file",
                            "arguments": "{\"path\":\"Cargo.toml\"}"
                        }
                    }]
                },
                "finish_reason": null
            }]
        }))
        .unwrap();

        let tool_call = chunk.choices[0].delta.tool_calls.as_ref().unwrap()[0].clone();
        assert_eq!(tool_call.id.as_deref(), Some("call_abc"));
        assert_eq!(
            tool_call.function.unwrap().name.as_deref(),
            Some("read_file")
        );
    }
}

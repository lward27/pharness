use crate::{StreamChunk, TokenUsagePayload, ToolCallDelta};
use pharness_core::{ModelResponseMetadata, ReasoningReplay};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct OpenAiStreamAggregate {
    pub raw_provider_id: Option<String>,
    pub content: String,
    pub reasoning_text: String,
    pub reasoning_details: Vec<serde_json::Value>,
    pub tool_calls: Vec<AccumulatedToolCall>,
    pub usage: Option<TokenUsagePayload>,
    pub metadata: ModelResponseMetadata,
    pub usable_stream_data: bool,
}

impl OpenAiStreamAggregate {
    pub fn push_chunk(&mut self, chunk: StreamChunk) {
        if self.raw_provider_id.is_none() {
            self.raw_provider_id = chunk.id;
        }
        if self.metadata.model.is_none() {
            self.metadata.model = chunk.model;
        }
        if self.metadata.provider.is_none() {
            self.metadata.provider = chunk.provider;
        }
        if chunk.usage.is_some() {
            self.usage = chunk.usage;
            self.usable_stream_data = true;
        }

        let mut accumulator = ToolCallAccumulator {
            calls: self
                .tool_calls
                .drain(..)
                .map(|call| (call.index, call))
                .collect(),
        };

        for choice in chunk.choices {
            if let Some(reason) = choice.finish_reason {
                self.metadata.native_finish_reason = Some(reason);
            }
            if let Some(content) = choice.delta.content {
                self.content.push_str(&content);
                self.usable_stream_data = true;
            }
            if let Some(reasoning) = choice.delta.reasoning_content.or(choice.delta.reasoning) {
                self.reasoning_text.push_str(&reasoning);
                self.usable_stream_data = true;
            }
            if let Some(details) = choice.delta.reasoning_details {
                self.reasoning_details.extend(details);
                self.usable_stream_data = true;
            }
            if let Some(tool_calls) = choice.delta.tool_calls {
                for tool_call in tool_calls {
                    accumulator.push_delta(tool_call);
                    self.usable_stream_data = true;
                }
            }
        }
        self.tool_calls = accumulator.into_calls();
    }

    pub fn reasoning_replay(&self) -> Option<ReasoningReplay> {
        if !self.reasoning_details.is_empty() {
            Some(ReasoningReplay::Structured(serde_json::Value::Array(
                self.reasoning_details.clone(),
            )))
        } else if !self.reasoning_text.is_empty() {
            Some(ReasoningReplay::Text(self.reasoning_text.clone()))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolCallAccumulator {
    calls: BTreeMap<u32, AccumulatedToolCall>,
}

impl ToolCallAccumulator {
    pub fn push_delta(&mut self, delta: ToolCallDelta) {
        let call = self
            .calls
            .entry(delta.index)
            .or_insert_with(|| AccumulatedToolCall {
                index: delta.index,
                ..AccumulatedToolCall::default()
            });
        if let Some(id) = delta.id {
            call.id = Some(id);
        }
        if let Some(tool_type) = delta.tool_type {
            call.tool_type = Some(tool_type);
        }
        if let Some(function) = delta.function {
            if let Some(name) = function.name {
                call.name = Some(name);
            }
            if let Some(arguments) = function.arguments {
                call.arguments.push_str(&arguments);
            }
        }
    }

    pub fn into_calls(self) -> Vec<AccumulatedToolCall> {
        self.calls.into_values().collect()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccumulatedToolCall {
    pub index: u32,
    pub id: Option<String>,
    pub tool_type: Option<String>,
    pub name: Option<String>,
    pub arguments: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SseDecoder {
    buffer: String,
}

impl SseDecoder {
    pub fn push_str(&mut self, input: &str) -> Vec<String> {
        self.buffer.push_str(&input.replace("\r\n", "\n"));
        self.drain_complete_events()
    }

    pub fn finish(mut self) -> Vec<String> {
        if !self.buffer.trim().is_empty() {
            self.buffer.push_str("\n\n");
        }
        self.drain_complete_events()
    }

    fn drain_complete_events(&mut self) -> Vec<String> {
        let mut payloads = Vec::new();
        while let Some(index) = self.buffer.find("\n\n") {
            let raw_event: String = self.buffer.drain(..index + 2).collect();
            let data = raw_event
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(str::trim_start)
                .collect::<Vec<_>>()
                .join("\n");
            if !data.is_empty() && data != "[DONE]" {
                payloads.push(data);
            }
        }
        payloads
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_comments_done_and_handles_fragmented_events() {
        let mut decoder = SseDecoder::default();
        assert!(decoder.push_str(": keepalive\n\ndata: {\"id\"").is_empty());
        let payloads = decoder.push_str(":\"one\"}\n\ndata: [DONE]\n\n");
        assert_eq!(payloads, vec![r#"{"id":"one"}"#]);
    }

    #[test]
    fn preserves_structured_reasoning_and_fragmented_tool_arguments() {
        let mut aggregate = OpenAiStreamAggregate::default();
        for value in [
            serde_json::json!({"id":"response","model":"model","choices":[{"index":0,"delta":{"content":null,"reasoning_details":[{"type":"reasoning.encrypted","data":"abc"}],"tool_calls":[{"index":0,"id":"call","type":"function","function":{"name":"read_file","arguments":"{\"path\""}}]},"finish_reason":null}]}),
            serde_json::json!({"id":"response","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":":\"Cargo.toml\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":10,"completion_tokens":4,"total_tokens":14,"completion_tokens_details":{"reasoning_tokens":2}}}),
        ] {
            aggregate.push_chunk(serde_json::from_value(value).unwrap());
        }
        assert_eq!(
            aggregate.tool_calls[0].arguments,
            "{\"path\":\"Cargo.toml\"}"
        );
        assert!(matches!(
            aggregate.reasoning_replay(),
            Some(ReasoningReplay::Structured(_))
        ));
        assert_eq!(
            aggregate.metadata.native_finish_reason.as_deref(),
            Some("tool_calls")
        );
    }
}

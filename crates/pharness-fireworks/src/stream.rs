use crate::{FireworksStreamChunk, FireworksToolCallDelta};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FireworksStreamAggregate {
    pub raw_provider_id: Option<String>,
    pub content: String,
    pub tool_calls: Vec<AccumulatedToolCall>,
}

impl FireworksStreamAggregate {
    pub fn push_chunk(&mut self, chunk: FireworksStreamChunk) {
        if self.raw_provider_id.is_none() {
            self.raw_provider_id = chunk.id;
        }

        let mut accumulator = ToolCallAccumulator {
            calls: self
                .tool_calls
                .drain(..)
                .map(|call| (call.index, call))
                .collect(),
        };

        for choice in chunk.choices {
            if let Some(content) = choice.delta.content {
                self.content.push_str(&content);
            }

            if let Some(tool_calls) = choice.delta.tool_calls {
                for tool_call in tool_calls {
                    accumulator.push_delta(tool_call);
                }
            }
        }

        self.tool_calls = accumulator.into_calls();
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolCallAccumulator {
    calls: BTreeMap<u32, AccumulatedToolCall>,
}

impl ToolCallAccumulator {
    pub fn push_delta(&mut self, delta: FireworksToolCallDelta) {
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
    use super::ToolCallAccumulator;
    use crate::{FireworksFunctionCallDelta, FireworksStreamAggregate, FireworksToolCallDelta};

    #[test]
    fn accumulates_incremental_tool_call_arguments() {
        let mut accumulator = ToolCallAccumulator::default();

        accumulator.push_delta(FireworksToolCallDelta {
            index: 0,
            id: Some("call_abc".to_string()),
            tool_type: Some("function".to_string()),
            function: Some(FireworksFunctionCallDelta {
                name: Some("read_file".to_string()),
                arguments: Some("{\"path\"".to_string()),
            }),
        });

        accumulator.push_delta(FireworksToolCallDelta {
            index: 0,
            id: None,
            tool_type: None,
            function: Some(FireworksFunctionCallDelta {
                name: None,
                arguments: Some(":\"Cargo.toml\"}".to_string()),
            }),
        });

        let calls = accumulator.into_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id.as_deref(), Some("call_abc"));
        assert_eq!(calls[0].name.as_deref(), Some("read_file"));
        assert_eq!(calls[0].arguments, "{\"path\":\"Cargo.toml\"}");
    }

    #[test]
    fn decodes_sse_payloads_across_pushes() {
        let mut decoder = super::SseDecoder::default();

        assert!(decoder.push_str("data: {\"id\"").is_empty());
        let payloads = decoder.push_str(":\"one\"}\n\ndata: [DONE]\n\n");

        assert_eq!(payloads, vec![r#"{"id":"one"}"#]);
        assert!(decoder.finish().is_empty());
    }

    #[test]
    fn aggregates_content_and_tool_calls_from_chunks() {
        let mut aggregate = FireworksStreamAggregate::default();

        aggregate.push_chunk(
            serde_json::from_value(serde_json::json!({
                "id": "chatcmpl-test",
                "choices": [{
                    "index": 0,
                    "delta": {
                        "content": "Thinking. ",
                        "tool_calls": [{
                            "index": 0,
                            "id": "call_abc",
                            "type": "function",
                            "function": {
                                "name": "read_file",
                                "arguments": "{\"path\""
                            }
                        }]
                    },
                    "finish_reason": null
                }]
            }))
            .unwrap(),
        );

        aggregate.push_chunk(
            serde_json::from_value(serde_json::json!({
                "id": "chatcmpl-test",
                "choices": [{
                    "index": 0,
                    "delta": {
                        "content": "Done.",
                        "tool_calls": [{
                            "index": 0,
                            "function": {
                                "arguments": ":\"Cargo.toml\"}"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            }))
            .unwrap(),
        );

        assert_eq!(aggregate.raw_provider_id.as_deref(), Some("chatcmpl-test"));
        assert_eq!(aggregate.content, "Thinking. Done.");
        assert_eq!(aggregate.tool_calls.len(), 1);
        assert_eq!(
            aggregate.tool_calls[0].arguments,
            "{\"path\":\"Cargo.toml\"}"
        );
    }
}

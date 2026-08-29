use crate::{ModelMessage, ModelRole, ToolResult};
use serde::{Deserialize, Serialize};

/// Conservative, provider-independent input budget. The approximation is
/// intentionally simple so workers do not need a model-specific tokenizer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextBudget {
    pub max_input_tokens: u32,
    pub recent_message_tokens: u32,
    pub max_tool_result_tokens: u32,
    pub reserved_output_tokens: u32,
    pub characters_per_token: u32,
}

impl Default for ContextBudget {
    fn default() -> Self {
        Self {
            max_input_tokens: 32_768,
            recent_message_tokens: 8_192,
            max_tool_result_tokens: 2_048,
            reserved_output_tokens: 4_096,
            characters_per_token: 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContextPack {
    pub messages: Vec<ModelMessage>,
    pub original_message_count: usize,
    pub compacted_exchanges: usize,
    pub truncated_tool_results: usize,
    pub estimated_input_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextError {
    MandatoryContextExceedsBudget,
}

impl std::fmt::Display for ContextError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("context_budget_exceeded")
    }
}
impl std::error::Error for ContextError {}

pub fn pack_messages(
    messages: &[ModelMessage],
    budget: &ContextBudget,
) -> Result<ContextPack, ContextError> {
    let mut packed = messages.to_vec();
    let original_message_count = packed.len();
    let mandatory_end = packed
        .iter()
        .position(|message| matches!(message.role, ModelRole::Assistant | ModelRole::Tool))
        .unwrap_or(packed.len());
    let mut compacted_exchanges = 0;
    let mut truncated_tool_results = 0;
    for message in packed.iter_mut().skip(mandatory_end) {
        if message.role != ModelRole::Tool {
            continue;
        }
        let character_limit =
            budget.max_tool_result_tokens as usize * budget.characters_per_token.max(1) as usize;
        if message.content.len() > character_limit {
            message.content = compact_tool_result(&message.content, character_limit);
            truncated_tool_results += 1;
        }
    }
    let effective_input_budget = budget
        .max_input_tokens
        .saturating_sub(budget.reserved_output_tokens);
    let mandatory_tokens = estimate_tokens(&packed[..mandatory_end], budget);
    if mandatory_tokens > effective_input_budget {
        return Err(ContextError::MandatoryContextExceedsBudget);
    }
    let retained_history_budget = mandatory_tokens
        .saturating_add(budget.recent_message_tokens)
        .min(effective_input_budget);
    while estimate_tokens(&packed, budget) > retained_history_budget {
        let Some((start, end)) = oldest_exchange_range(&packed, mandatory_end) else {
            return Err(ContextError::MandatoryContextExceedsBudget);
        };
        packed.drain(start..end);
        compacted_exchanges += 1;
    }
    Ok(ContextPack {
        estimated_input_tokens: estimate_tokens(&packed, budget),
        messages: packed,
        original_message_count,
        compacted_exchanges,
        truncated_tool_results,
    })
}

pub fn estimate_tokens(messages: &[ModelMessage], budget: &ContextBudget) -> u32 {
    let characters = messages
        .iter()
        .map(|message| {
            message.content.len()
                + message
                    .tool_calls
                    .iter()
                    .map(|call| call.arguments.len() + call.name.len() + call.id.len())
                    .sum::<usize>()
        })
        .sum::<usize>();
    characters.div_ceil(budget.characters_per_token.max(1) as usize) as u32
}

fn oldest_exchange_range(
    messages: &[ModelMessage],
    mandatory_end: usize,
) -> Option<(usize, usize)> {
    let start = mandatory_end;
    if start >= messages.len() {
        return None;
    }
    let mut end = start + 1;
    if messages[start].role == ModelRole::Assistant && !messages[start].tool_calls.is_empty() {
        while end < messages.len() && messages[end].role == ModelRole::Tool {
            end += 1;
        }
    }
    Some((start, end))
}

fn compact_tool_result(content: &str, max_chars: usize) -> String {
    let summary = serde_json::from_str::<ToolResult>(content)
        .map(|result| {
            let mut compact = serde_json::Map::new();
            compact.insert("status".to_string(), serde_json::json!(result.status));
            compact.insert("summary".to_string(), serde_json::json!(result.summary));
            compact.insert("content_compacted".to_string(), serde_json::Value::Bool(true));
            for key in [
                "action",
                "path",
                "cmd",
                "exit_code",
                "truncated_stdout",
                "truncated_stderr",
                "error_kind",
            ] {
                if let Some(value) = result.content.get(key) {
                    compact.insert(key.to_string(), value.clone());
                }
            }
            serde_json::Value::Object(compact).to_string()
        })
        .unwrap_or_else(|_| "{\"status\":\"error\",\"summary\":\"tool output compacted\",\"content_compacted\":true}".to_string());
    if summary.len() <= max_chars {
        return summary;
    }
    let mut end = max_chars;
    while end > 0 && !summary.is_char_boundary(end) {
        end -= 1;
    }
    summary[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::{pack_messages, ContextBudget};
    use crate::{ModelMessage, ModelRole, ModelToolCall};
    #[test]
    fn compacts_old_exchanges_without_orphaning_tool_calls() {
        let mut messages = vec![ModelMessage::system("system"), ModelMessage::user("task")];
        for index in 0..4 {
            messages.push(ModelMessage {
                role: ModelRole::Assistant,
                content: String::new(),
                tool_call_id: None,
                tool_calls: vec![ModelToolCall {
                    id: format!("call_{index}"),
                    name: "read_file".to_string(),
                    arguments: "{}".to_string(),
                }],
                reasoning: None,
            });
            messages.push(ModelMessage {
                role: ModelRole::Tool,
                content: "x".repeat(200),
                tool_call_id: Some(format!("call_{index}")),
                tool_calls: Vec::new(),
                reasoning: None,
            });
        }
        let pack = pack_messages(
            &messages,
            &ContextBudget {
                max_input_tokens: 80,
                recent_message_tokens: 40,
                max_tool_result_tokens: 10,
                reserved_output_tokens: 4,
                characters_per_token: 4,
            },
        )
        .unwrap();
        assert!(pack.compacted_exchanges > 0 || pack.truncated_tool_results > 0);
        for (index, message) in pack.messages.iter().enumerate() {
            if !message.tool_calls.is_empty() {
                assert!(pack
                    .messages
                    .get(index + 1)
                    .is_some_and(|next| next.role == ModelRole::Tool));
            }
        }
    }

    #[test]
    fn reserves_output_and_keeps_only_the_configured_recent_history_budget() {
        let messages = vec![
            ModelMessage::system("system"),
            ModelMessage::user("task"),
            ModelMessage {
                role: ModelRole::Assistant,
                content: "x".repeat(80),
                tool_call_id: None,
                tool_calls: Vec::new(),
                reasoning: None,
            },
            ModelMessage {
                role: ModelRole::Assistant,
                content: "y".repeat(80),
                tool_call_id: None,
                tool_calls: Vec::new(),
                reasoning: None,
            },
        ];
        let pack = pack_messages(
            &messages,
            &ContextBudget {
                max_input_tokens: 60,
                recent_message_tokens: 20,
                max_tool_result_tokens: 10,
                reserved_output_tokens: 20,
                characters_per_token: 4,
            },
        )
        .unwrap();
        assert!(pack.estimated_input_tokens <= 40);
        assert_eq!(pack.messages[0].content, "system");
        assert_eq!(pack.messages[1].content, "task");
        let newest = "y".repeat(80);
        assert_eq!(
            pack.messages.last().map(|message| message.content.as_str()),
            Some(newest.as_str())
        );
    }

    #[test]
    fn default_budget_bounds_a_long_coding_transcript() {
        let mut messages = vec![
            ModelMessage::system("s".repeat(4_000)),
            ModelMessage::user("implement the approved change"),
        ];
        for index in 0..40 {
            messages.push(ModelMessage {
                role: ModelRole::Assistant,
                content: "reasoning checkpoint ".repeat(50),
                tool_call_id: None,
                tool_calls: vec![ModelToolCall {
                    id: format!("call_{index}"),
                    name: "write_file".to_string(),
                    arguments: "{}".to_string(),
                }],
                reasoning: None,
            });
            messages.push(ModelMessage {
                role: ModelRole::Tool,
                content: "x".repeat(12_000),
                tool_call_id: Some(format!("call_{index}")),
                tool_calls: Vec::new(),
                reasoning: None,
            });
        }

        let pack = pack_messages(&messages, &ContextBudget::default()).unwrap();

        assert!(pack.estimated_input_tokens <= 9_300);
        assert!(pack.compacted_exchanges > 0);
        assert!(pack.truncated_tool_results > 0);
    }
}

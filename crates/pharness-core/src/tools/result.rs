use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    pub status: ToolResultStatus,
    pub summary: String,
    pub content: serde_json::Value,
}

impl ToolResult {
    pub fn ok(summary: impl Into<String>, content: serde_json::Value) -> Self {
        Self {
            status: ToolResultStatus::Ok,
            summary: summary.into(),
            content,
        }
    }

    pub fn error(summary: impl Into<String>, content: serde_json::Value) -> Self {
        Self {
            status: ToolResultStatus::Error,
            summary: summary.into(),
            content,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolResultStatus {
    Ok,
    Error,
}

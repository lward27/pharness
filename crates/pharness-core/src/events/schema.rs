use crate::{EventId, RunId, SessionId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentEvent {
    pub event_id: EventId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub seq: u64,
    #[serde(rename = "type")]
    pub kind: EventKind,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    #[serde(rename = "run.queued")]
    RunQueued,
    #[serde(rename = "run.started")]
    RunStarted,
    #[serde(rename = "run.resumed")]
    RunResumed,
    #[serde(rename = "run.cancelled")]
    RunCancelled,
    #[serde(rename = "run.failed")]
    RunFailed,
    #[serde(rename = "run.finished")]
    RunFinished,
    #[serde(rename = "model.request_started")]
    ModelRequestStarted,
    #[serde(rename = "model.response_finished")]
    ModelResponseFinished,
    #[serde(rename = "action.proposed")]
    ActionProposed,
    #[serde(rename = "policy.evaluated")]
    PolicyEvaluated,
    #[serde(rename = "approval.required")]
    ApprovalRequired,
    #[serde(rename = "approval.decided")]
    ApprovalDecided,
    #[serde(rename = "tool.started")]
    ToolStarted,
    #[serde(rename = "tool.finished")]
    ToolFinished,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RunQueued => "run.queued",
            Self::RunStarted => "run.started",
            Self::RunResumed => "run.resumed",
            Self::RunCancelled => "run.cancelled",
            Self::RunFailed => "run.failed",
            Self::RunFinished => "run.finished",
            Self::ModelRequestStarted => "model.request_started",
            Self::ModelResponseFinished => "model.response_finished",
            Self::ActionProposed => "action.proposed",
            Self::PolicyEvaluated => "policy.evaluated",
            Self::ApprovalRequired => "approval.required",
            Self::ApprovalDecided => "approval.decided",
            Self::ToolStarted => "tool.started",
            Self::ToolFinished => "tool.finished",
        }
    }
}

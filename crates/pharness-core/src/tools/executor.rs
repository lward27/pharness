use super::ToolResult;
use crate::AgentAction;
use async_trait::async_trait;
use thiserror::Error;

#[async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute(&self, action: &AgentAction) -> Result<ToolResult, ToolError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoopToolExecutor;

#[async_trait]
impl ToolExecutor for NoopToolExecutor {
    async fn execute(&self, action: &AgentAction) -> Result<ToolResult, ToolError> {
        Err(ToolError::UnsupportedAction {
            action: action.kind_name().to_string(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct CompositeToolExecutor<A, B> {
    primary: A,
    fallback: B,
}

impl<A, B> CompositeToolExecutor<A, B> {
    pub fn new(primary: A, fallback: B) -> Self {
        Self { primary, fallback }
    }
}

#[async_trait]
impl<A, B> ToolExecutor for CompositeToolExecutor<A, B>
where
    A: ToolExecutor,
    B: ToolExecutor,
{
    async fn execute(&self, action: &AgentAction) -> Result<ToolResult, ToolError> {
        match self.primary.execute(action).await {
            Err(ToolError::UnsupportedAction { .. }) => self.fallback.execute(action).await,
            result => result,
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ToolError {
    #[error("unsupported action: {action}")]
    UnsupportedAction { action: String },
    #[error("path is outside workspace: {path}")]
    OutsideWorkspace { path: String },
    #[error("I/O error: {message}")]
    Io { message: String },
    #[error("file is not valid UTF-8: {path}")]
    NonUtf8 { path: String },
    #[error("path is not a directory: {path}")]
    NotDirectory { path: String },
    #[error("invalid tool arguments: {message}")]
    InvalidArguments { message: String },
    #[error("command timed out after {timeout_ms} ms: {command}")]
    TimedOut { command: String, timeout_ms: u64 },
    #[error("command failed with status {status}: {command}: {stderr}")]
    CommandFailed {
        command: String,
        status: String,
        stderr: String,
    },
    #[error("network error: {message}")]
    Network { message: String },
}

#![forbid(unsafe_code)]

pub mod agent;
pub mod events;
pub mod execution;
pub mod ids;
pub mod model;
pub mod policy;
pub mod resources;
pub mod tools;

pub use agent::{
    AgentRuntime, ApprovedAction, CancellationFlag, PendingApproval, RunConfig, RunOutcome,
    RunStatus,
};
pub use events::{AgentEvent, EventKind, EventSink, InMemoryEventSink};
pub use execution::{EnvironmentRef, EnvironmentTier, ExecutionTarget, WorkspaceMount};
pub use ids::{ActionId, ArtifactId, EventId, RunId, SessionId, ToolCallId};
pub use model::{
    ActionParseError, AgentAction, ApprovalKind, ModelCapabilities, ModelMessage, ModelProvider,
    ModelRequest, ModelRole, ModelToolCall, ModelTurn, ProviderError, TextPatch, TokenUsage,
    ToolProtocolMode, ToolSpec,
};
pub use policy::{
    classify_command, CommandClass, PolicyDecision, PolicyMode, RiskLevel, SafetyPolicy,
};
pub use resources::{ArtifactRef, ResourceRef};
pub use tools::{
    CapabilityKind, CompositeToolExecutor, LocalReadOnlyFsTools, LocalShellTools, NoopToolExecutor,
    ReadOnlyClusterTools, ToolCapability, ToolError, ToolExecutor, ToolResult, ToolResultStatus,
};

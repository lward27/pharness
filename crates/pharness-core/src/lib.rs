#![forbid(unsafe_code)]

pub mod agent;
pub mod events;
pub mod execution;
pub mod ids;
pub mod model;
pub mod policy;
pub mod project;
pub mod repo_mode;
pub mod repository;
pub mod resources;
pub mod tools;

pub use agent::{
    pack_messages, AgentRuntime, ApprovedAction, BudgetResume, CancellationFlag, ContextBudget,
    ContextError, ContextPack, PendingApproval, PendingBudgetExtension, RecoveryPolicy,
    RepositoryInstruction, RunConfig, RunOutcome, RunStatus, TaskContract, TaskKind,
};
pub use events::{AgentEvent, EventKind, EventSink, InMemoryEventSink};
pub use execution::{EnvironmentRef, EnvironmentTier, ExecutionTarget, RunScope, WorkspaceMount};
pub use ids::{ActionId, ArtifactId, EventId, RunId, SessionId, ToolCallId};
pub use model::{
    ActionParseError, AgentAction, ApprovalKind, ModelCapabilities, ModelMessage, ModelProvider,
    ModelRequest, ModelRole, ModelToolCall, ModelTurn, ProviderError, TextPatch, TokenUsage,
    ToolProtocolMode, ToolSpec,
};
pub use policy::{
    classify_command, CommandClass, PermissionGrant, PermissionGrantPolicy, PermissionGrantScope,
    PolicyDecision, PolicyMode, RiskLevel, SafetyPolicy,
};
pub use project::{
    AcceptanceCommand, AgentNetworkPolicy, DependencyLock, EnvironmentProfile,
    EnvironmentProfileLimits, EnvironmentRuntimeSnapshot, EnvironmentSnapshot,
    LoadedRepositoryContract, PackageInstallationPolicy, PreparationStrategy, ProjectRoots,
    RepositoryContract, RepositoryContractError, RepositoryContractSource, RunBudget,
    RunBudgetConsumption, LEGACY_PROJECT_CONTRACT_PATH, MAX_REPOSITORY_CONTRACT_BYTES,
    REPOSITORY_CONTRACT_PATH,
};
#[allow(deprecated)]
pub use project::{ProjectContract, ProjectContractError};
pub use repo_mode::{
    canonical_json_sha256, compiled_agent_profiles, AgentProfile, RepoStageKey,
    RepositoryBindingProposal, RepositoryOnboardingProposal, RepositoryServiceProposal,
    StageOutcomeDocument, StageTerminalStatus, AGENT_CONTEXT_SCHEMA, EVIDENCE_VALIDATION_SCHEMA,
    ONBOARDING_PROPOSAL_SCHEMA, STAGE_OUTCOME_SCHEMA,
};
pub use repository::{
    discover_repository, DiscoveredCandidate, DiscoveredCommandCandidate, DiscoveredContractState,
    DiscoveredRepositoryEntry, DiscoveredSubmodule, DiscoveredSymlink, DiscoveryFinding,
    RepositoryDiscovery, RepositoryDiscoveryError, RepositoryDiscoveryIdentity,
    RepositoryDiscoveryLimits, REPOSITORY_DISCOVERY_SCHEMA,
};
pub use resources::{ArtifactRef, ResourceRef};
pub use tools::{
    simple_text_diff, CapabilityKind, CompositeToolExecutor, LocalReadOnlyFsTools, LocalShellTools,
    NoopToolExecutor, ReadOnlyClusterTools, ToolCapability, ToolError, ToolErrorDisposition,
    ToolExecutor, ToolResult, ToolResultStatus,
};

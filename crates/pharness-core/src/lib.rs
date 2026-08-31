#![forbid(unsafe_code)]

pub mod agent;
pub mod events;
pub mod execution;
pub mod ids;
pub mod inference;
pub mod model;
pub mod policy;
pub mod project;
pub mod repo_mode;
pub mod repository;
pub mod resources;
pub mod tools;

pub use agent::{
    estimate_request_tokens, pack_messages, AgentRuntime, ApprovedAction, BudgetResume,
    CancellationFlag, ContextBudget, ContextError, ContextPack, PendingApproval,
    PendingBudgetExtension, RecoveryPolicy, RepositoryInstruction, RunConfig, RunOutcome,
    RunStatus, TaskContract, TaskKind,
};
pub use events::{AgentEvent, EventKind, EventSink, InMemoryEventSink};
pub use execution::{EnvironmentRef, EnvironmentTier, ExecutionTarget, RunScope, WorkspaceMount};
pub use ids::{ActionId, ArtifactId, EventId, RunId, SessionId, ToolCallId};
pub use inference::{
    inference_qualification_suite_hash, sign_model_grant, verify_model_grant, InferenceBackendKind,
    InferenceCapabilities, InferenceConfigError, InferencePolicyRef, InferenceRegistry,
    InferenceStage, InferenceTargetRef, InferenceTargetRevision, InferenceTransportPolicy,
    ModelGrantClaims, ModelGrantError, OpenRouterRoutePolicy, ReasoningContextMode,
    ReasoningEffort, ReasoningReplay, ReasoningRequestPolicy, ResolvedInferenceBinding,
    StageInferencePolicyRevision, StagePromptRevision, INFERENCE_POLICY_SCHEMA,
    INFERENCE_QUALIFICATION_SUITE_SCHEMA, INFERENCE_REGISTRY_SCHEMA, INFERENCE_TARGET_SCHEMA,
    MODEL_GRANT_SCHEMA, RESOLVED_INFERENCE_BINDING_SCHEMA, STAGE_PROMPT_SCHEMA,
};
pub use model::{
    ActionParseError, AgentAction, ApprovalKind, ModelCapabilities, ModelMessage, ModelProvider,
    ModelRequest, ModelResponseMetadata, ModelRole, ModelToolCall, ModelTurn, ProviderError,
    ProviderProtocolErrorKind, TextPatch, TokenUsage, ToolChoiceMode, ToolProtocolMode, ToolSpec,
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
    canonical_json_sha256, compiled_agent_profiles, compiled_reliability_v2_agent_profiles,
    AgentProfile, RepoStageKey, RepositoryBindingProposal, RepositoryOnboardingProposal,
    RepositoryServiceProposal, StageOutcomeDocument, StageTerminalStatus, AGENT_CONTEXT_SCHEMA,
    EVIDENCE_VALIDATION_SCHEMA, ONBOARDING_PROPOSAL_SCHEMA, STAGE_OUTCOME_SCHEMA,
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

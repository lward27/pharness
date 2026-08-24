#![forbid(unsafe_code)]

mod models;
mod onboarding;
mod product;
mod sqlite;

pub use models::{
    ApprovalBooleanCountBucket, ApprovalCountBucket, ApprovalGateCountBucket,
    ApprovalGateListFilter, ApprovalGateSummary, ApprovalGateSummaryFilter, ApprovalListFilter,
    ApprovalSummary, ApprovalSummaryFilter, AuditEventListFilter, BooleanCountBucket,
    ChangeSetListFilter, ControllerWaitListFilter, CountBucket, CreateApproval, CreateApprovalGate,
    CreateArtifact, CreateAuditEvent, CreateBudgetExtension, CreateCapabilityVerification,
    CreateChangeSet, CreateControllerWait, CreateDeploymentContract, CreateDeploymentIntent,
    CreateEnvironmentPreparation, CreateFileChange, CreateGitOpsChangeSet, CreateIncident,
    CreateObservation, CreatePermissionGrant, CreatePipelineContract, CreatePipelineIntent,
    CreateRegistryEvidence, CreateRelease, CreateRemediationPlan, CreateRun, CreateSession,
    CreateWorkItem, CreateWorkPlan, CreateWorkspace, DeploymentContractListFilter,
    DeploymentIntentListFilter, GitOpsChangeSetListFilter, IncidentListFilter,
    ObservationListFilter, PipelineContractListFilter, PipelineIntentListFilter,
    RegistryEvidenceListFilter, ReleaseListFilter, RemediationPlanListFilter,
    ReplacePipelineContract, RunListFilter, RunSummary, RunSummaryFilter, StoredApproval,
    StoredApprovalGate, StoredArtifact, StoredAuditEvent, StoredBudgetExtension,
    StoredCapabilityVerification, StoredChangeSet, StoredControllerWait, StoredDeploymentContract,
    StoredDeploymentIntent, StoredEnvironmentPreparation, StoredFileChange, StoredGitOpsChangeSet,
    StoredIncident, StoredObservation, StoredPermissionGrant, StoredPipelineContract,
    StoredPipelineIntent, StoredRegistryEvidence, StoredRelease, StoredRemediationPlan, StoredRun,
    StoredWorkItem, StoredWorkPlan, StoredWorkspace, UpdateChangeSetRevision,
    UpdateDeploymentIntentDraft, UpdateDeploymentIntentEvidence, UpdateEnvironmentPreparation,
    UpdatePipelineIntentDraft, UpdatePipelineIntentEvidence, UpdatePipelineIntentExecution,
    UpdateRegistryEvidenceDraft, UpdateReleaseDraft, UpdateReleaseEvidence, UpdateWorkPlanRevision,
    UpdateWorkspaceExecution, WorkItemListFilter, WorkPlanListFilter, WorkspaceListFilter,
};
pub use onboarding::{
    CreateRepositoryOnboarding, StoredRepositoryDiscovery, StoredRepositoryOnboarding,
};
pub use product::{
    BootstrapOrganization, CreateProductAggregate, RegisterRepositoryAggregate,
    RegisteredRepositoryAggregate, StoredOrganization, StoredProduct, StoredProductModelSnapshot,
    StoredRepository, StoredRepositoryBinding, StoredRepositoryBindingRevision,
    StoredRepositoryDraft, StoredService, UpdateProductAggregate,
};
pub use sqlite::{SqliteStore, StoreError};

pub const INITIAL_MIGRATION_NAME: &str = "0001_initial";

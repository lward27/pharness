#![forbid(unsafe_code)]

mod agent_execution;
mod data_lifecycle;
mod hosted_controller;
mod inference;
mod models;
mod onboarding;
mod product;
mod repo_mode;
mod sqlite;
mod subject_preparation;

pub use agent_execution::{
    ClaimedAgentLease, CreateAgentExecutionPolicyQualification, CreateAgentExecutionSelection,
    CreateAgentHostCapabilitySnapshot, CreateAgentHostEnrollment, CreateAgentLease,
    EnrollAgentHost, StoredAgentExecutionPolicyQualification, StoredAgentExecutionSelection,
    StoredAgentHost, StoredAgentHostCapabilitySnapshot, StoredAgentHostEnrollment,
    StoredAgentLease,
};
pub use hosted_controller::{
    BeginWorkflowOperation, FinishWorkflowReconciliation, StoredWorkflowOperation,
    StoredWorkflowReconciliation,
};
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
    ApproveRepositoryOnboardingProposal, ApprovedOnboardingProductModelChange,
    ApprovedOnboardingService, CreateRepositoryContractVersion, CreateRepositoryOnboarding,
    CreateRepositoryOnboardingProposal, CreateRepositoryReadinessAssessment,
    StoredRepositoryContractVersion, StoredRepositoryDiscovery, StoredRepositoryOnboarding,
    StoredRepositoryOnboardingProposal, StoredRepositoryReadinessAssessment,
};
pub use product::{
    ApplyProductModelRevision, BootstrapOrganization, CreateProductAggregate,
    ProductModelBindingRevision, ProductModelServiceRevision, RegisterRepositoryAggregate,
    RegisteredRepositoryAggregate, RepositoryBindingScope, StoredOrganization, StoredProduct,
    StoredProductModelSnapshot, StoredRepository, StoredRepositoryBinding,
    StoredRepositoryBindingRevision, StoredRepositoryDraft, StoredService, UpdateProductAggregate,
};
pub use repo_mode::{
    CreateAgentContextPack, CreateEvidenceRetrieval, CreateEvidenceValidation,
    CreateOperatorAnnotation, CreateOperatorAnnotationDecision, CreateProviderCheckSetObservation,
    CreateRepoWorkItem, CreateSourceDeliveryIntent, CreateStageChainAuthorization,
    CreateStageExecution, SealStageOutcome, StoredAgentContextPack, StoredEvidenceValidation,
    StoredOperatorAnnotation, StoredOperatorAnnotationDecision, StoredProviderCheckSetObservation,
    StoredRepoWorkItemMetadata, StoredSourceDeliveryIntent, StoredStageChainAuthorization,
    StoredStageExecution, StoredStageOutcome,
};
pub use sqlite::{SqliteStore, StoreError};
pub use subject_preparation::{
    CompleteSubjectEnvironmentPreparation, CreateSubjectEnvironmentPreparation,
    CreateSubjectWorkspace, StoredSubjectEnvironmentPreparation, StoredSubjectWorkspace,
};

pub const INITIAL_MIGRATION_NAME: &str = "0001_initial";
pub use data_lifecycle::{
    CreateArchiveRecord, CreateRetentionHold, CreateRetentionPreview, DataInventory,
    DatabaseGeneration, DeleteArchiveRecord, EvidenceValidationReference, StoredArchiveRecord,
    StoredRetentionHold, StoredRetentionPreview, StoredRetentionReceipt, StoredRunSummaryRecord,
    RETENTION_POLICY_VERSION,
};
pub use inference::{
    CreateInferenceEvaluation, CreateInferenceEvaluationGrantIssuance,
    CreateInferencePolicyQualification, CreateInferenceTargetVerification,
    CreateModelGrantIssuance, CreateStageInferenceSelection, StoredInferenceEvaluation,
    StoredInferencePolicyQualification, StoredInferenceTargetVerification,
    StoredStageInferenceSelection,
};

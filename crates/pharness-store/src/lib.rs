#![forbid(unsafe_code)]

mod models;
mod sqlite;

pub use models::{
    ApprovalBooleanCountBucket, ApprovalCountBucket, ApprovalGateCountBucket,
    ApprovalGateListFilter, ApprovalGateSummary, ApprovalGateSummaryFilter, ApprovalListFilter,
    ApprovalSummary, ApprovalSummaryFilter, AuditEventListFilter, BooleanCountBucket,
    ChangeSetListFilter, CountBucket, CreateApproval, CreateApprovalGate, CreateArtifact,
    CreateAuditEvent, CreateChangeSet, CreateDeploymentContract, CreateDeploymentIntent,
    CreateFileChange, CreateIncident, CreateObservation, CreatePermissionGrant,
    CreatePipelineContract, CreatePipelineIntent, CreateRegistryEvidence, CreateRelease,
    CreateRemediationPlan, CreateRun, CreateSession, CreateWorkItem, CreateWorkPlan,
    CreateWorkspace, DeploymentContractListFilter, DeploymentIntentListFilter, IncidentListFilter,
    ObservationListFilter, PipelineContractListFilter, PipelineIntentListFilter,
    RegistryEvidenceListFilter, ReleaseListFilter, RemediationPlanListFilter,
    ReplacePipelineContract, RunListFilter, RunSummary, RunSummaryFilter, StoredApproval,
    StoredApprovalGate, StoredArtifact, StoredAuditEvent, StoredChangeSet,
    StoredDeploymentContract, StoredDeploymentIntent, StoredFileChange, StoredIncident,
    StoredObservation, StoredPermissionGrant, StoredPipelineContract, StoredPipelineIntent,
    StoredRegistryEvidence, StoredRelease, StoredRemediationPlan, StoredRun, StoredWorkItem,
    StoredWorkPlan, StoredWorkspace, UpdateChangeSetRevision, UpdateDeploymentIntentDraft,
    UpdateDeploymentIntentEvidence, UpdatePipelineIntentDraft, UpdatePipelineIntentEvidence,
    UpdatePipelineIntentExecution, UpdateRegistryEvidenceDraft, UpdateReleaseDraft,
    UpdateReleaseEvidence, UpdateWorkPlanRevision, WorkItemListFilter, WorkPlanListFilter,
    WorkspaceListFilter,
};
pub use sqlite::{SqliteStore, StoreError};

pub const INITIAL_MIGRATION_NAME: &str = "0001_initial";

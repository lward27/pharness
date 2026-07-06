#![forbid(unsafe_code)]

mod models;
mod sqlite;

pub use models::{
    ApprovalBooleanCountBucket, ApprovalCountBucket, ApprovalGateCountBucket,
    ApprovalGateListFilter, ApprovalGateSummary, ApprovalGateSummaryFilter, ApprovalListFilter,
    ApprovalSummary, ApprovalSummaryFilter, BooleanCountBucket, ChangeSetListFilter, CountBucket,
    CreateApproval, CreateApprovalGate, CreateArtifact, CreateAuditEvent, CreateChangeSet,
    CreateDeploymentIntent, CreateFileChange, CreateIncident, CreateObservation,
    CreatePermissionGrant, CreatePipelineIntent, CreateRegistryEvidence, CreateRelease,
    CreateRemediationPlan, CreateRun, CreateSession, CreateWorkPlan, DeploymentIntentListFilter,
    IncidentListFilter, ObservationListFilter, PipelineIntentListFilter,
    RegistryEvidenceListFilter, ReleaseListFilter, RemediationPlanListFilter, RunListFilter,
    RunSummary, RunSummaryFilter, StoredApproval, StoredApprovalGate, StoredArtifact,
    StoredAuditEvent, StoredChangeSet, StoredDeploymentIntent, StoredFileChange, StoredIncident,
    StoredObservation, StoredPermissionGrant, StoredPipelineIntent, StoredRegistryEvidence,
    StoredRelease, StoredRemediationPlan, StoredRun, StoredWorkPlan, UpdateChangeSetRevision,
    UpdateDeploymentIntentDraft, UpdateDeploymentIntentEvidence, UpdatePipelineIntentDraft,
    UpdatePipelineIntentEvidence, UpdateRegistryEvidenceDraft, UpdateReleaseDraft,
    UpdateReleaseEvidence, UpdateWorkPlanRevision, WorkPlanListFilter,
};
pub use sqlite::{SqliteStore, StoreError};

pub const INITIAL_MIGRATION_NAME: &str = "0001_initial";

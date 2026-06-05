#![forbid(unsafe_code)]

mod models;
mod sqlite;

pub use models::{
    ApprovalBooleanCountBucket, ApprovalCountBucket, ApprovalGateCountBucket,
    ApprovalGateListFilter, ApprovalGateSummary, ApprovalGateSummaryFilter, ApprovalListFilter,
    ApprovalSummary, ApprovalSummaryFilter, BooleanCountBucket, ChangeSetListFilter, CountBucket,
    CreateApproval, CreateApprovalGate, CreateArtifact, CreateAuditEvent, CreateChangeSet,
    CreateFileChange, CreateIncident, CreateObservation, CreatePermissionGrant,
    CreateRemediationPlan, CreateRun, CreateSession, CreateWorkPlan, IncidentListFilter,
    ObservationListFilter, RemediationPlanListFilter, RunListFilter, RunSummary, RunSummaryFilter,
    StoredApproval, StoredApprovalGate, StoredArtifact, StoredAuditEvent, StoredChangeSet,
    StoredFileChange, StoredIncident, StoredObservation, StoredPermissionGrant,
    StoredRemediationPlan, StoredRun, StoredWorkPlan, UpdateChangeSetRevision,
    UpdateWorkPlanRevision, WorkPlanListFilter,
};
pub use sqlite::{SqliteStore, StoreError};

pub const INITIAL_MIGRATION_NAME: &str = "0001_initial";

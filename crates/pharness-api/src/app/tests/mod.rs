mod support;

use super::approval_policy::approval_gate_uses_dedicated_lifecycle_action;
use super::approvals::{
    approval_gate_lifecycle_readiness, approval_gate_summary, approval_summary,
    batch_decide_approval_gates, create_permission_grant, deny_approval, get_approval,
    get_approval_gate, get_permission_grant, list_approval_gates, list_approvals,
    list_permission_grants, revoke_permission_grant, satisfy_approval_gate,
    validate_permission_grant_request, ApprovalGateSummaryQuery, ApprovalSummaryQuery,
    ListApprovalGatesQuery, ListApprovalsQuery, ListPermissionGrantsQuery,
};
use super::auth::OperatorIdentity;
use super::capabilities::execute_capability;
use super::clock::{current_millis, unique_suffix};
use super::delivery_actions::GIT_DELIVERY_ACTIONS;
use super::deployment::contracts::{
    create_deployment_contract, get_deployment_contract, list_deployment_contracts,
    transition_deployment_contract, ListDeploymentContractsQuery,
};
use super::deployment::execution::{
    execute_deployment_intent, preflight_deployment_intent, DeploymentIntentExecutionPreflight,
};
use super::deployment::intents::{
    attach_deployment_intent_evidence, create_deployment_intent_from_pipeline_intent,
    create_deployment_intent_trusted_envelope, get_deployment_intent, list_deployment_intents,
    transition_deployment_intent, ListDeploymentIntentsQuery,
};
use super::evidence::{
    create_incident, create_observation, create_remediation_plan, get_artifact, get_incident,
    get_observation, get_remediation_plan, list_audit_events, list_incidents, list_observations,
    list_remediation_plans, transition_remediation_plan, ListAuditEventsQuery, ListIncidentsQuery,
    ListObservationsQuery, ListRemediationPlansQuery,
};
use super::gitops::delivery::{
    authorize_gitops_change_set_delivery, preflight_gitops_change_set_delivery,
    prepare_gitops_change_set_delivery,
};
use super::gitops::delivery_flow::{gitops_artifact_change_set_revision, gitops_delivery_flow};
use super::gitops::deployment_evidence::observed_gitops_merge_for_deployment;
use super::gitops::observation::{
    gitops_observation_closed_unmerged, gitops_observation_refreshable,
};
use super::internal::{
    internal_argo_sync_outcome, internal_gitops_delivery_observation_outcome,
    internal_gitops_delivery_outcome,
};
use super::pipeline::evidence::set_pipeline_intent_evidence;
use super::pipeline::execution::{
    build_pipeline_run_manifest, execution_matches_pipeline_contract,
    merge_pipeline_execution_state, persist_pipeline_build_output,
    persist_pipeline_execution_evidence, persist_pipeline_run_analysis,
    pipeline_build_output_from_analysis, pipeline_intent_execution_preflight,
    tekton_execution_spec, validate_terminal_pipeline_run_analysis,
};
use super::pipeline::handoff::{
    create_declared_deployment_handoff, validate_pipeline_deployment_handoff,
    PipelineDeploymentHandoffSpec,
};
use super::pipeline::intents::{
    attach_pipeline_intent_evidence, create_pipeline_intent_from_change_set,
    create_work_item_pipeline_intent, current_pipeline_build_output, get_pipeline_intent,
    list_pipeline_intents, transition_pipeline_intent, work_item_pipeline_intent_context,
    ListPipelineIntentsQuery, WorkItemPipelineContextQuery,
};
use super::pipeline::readiness::ensure_pipeline_evidence_ready_for_deployment;
use super::pipeline::state::pipeline_intent_is_gitops_update_eligible;
use super::policy::{policy_json, run_policy};
use super::releases::{
    attach_release_evidence, create_registry_evidence_from_registry_inspection,
    create_registry_evidence_from_release, create_release_from_deployment_intent,
    get_registry_evidence, get_release, list_registry_evidence, list_releases,
    prometheus_inventory_observability_status, release_prometheus_inventory_collected,
    release_prometheus_inventory_summary, release_workload_verification_action,
    transition_registry_evidence, transition_release, verify_release, ListRegistryEvidenceQuery,
    ListReleasesQuery,
};
use super::repo_mode::internal_source_delivery_observation_outcome;
use super::runs::{
    cancel_run, create_operator_run, create_run, decide_run_approval, get_run, get_run_diff,
    get_run_events, get_run_operator_summary, internal_workspace_provisioned, last_event_seq,
    list_run_artifacts, list_run_observations, list_runs, parse_last_event_id, run_summary,
    stream_start_seq, InternalWorkspaceProvisionedRequest, ListRunsQuery, StreamRunEventsQuery,
};
use super::source::change_sets::{
    change_set_flow, change_set_readiness, coding_run_scope_matches_source, create_change_set,
    create_change_set_trusted_envelope, list_change_sets, revise_change_set, transition_change_set,
    ListChangeSetsQuery,
};
use super::source::git_delivery::{
    authorize_change_set_git_delivery, preflight_change_set_git_delivery,
    prepare_change_set_git_delivery,
};
use super::source::work_plans::{
    create_work_plan_from_remediation_plan, create_work_plan_from_work_item,
    create_work_plan_trusted_envelope, get_work_plan, list_work_plans, revise_work_plan,
    transition_work_plan, work_plan_flow, work_plan_readiness, ListWorkPlansQuery,
};
use super::system::{
    capability_preflight_is_statically_unavailable, capability_verification_summary,
    config_effective, environment_profile_readiness_blocker, immutable_git_object_id,
    immutable_image_digest, protected_target_json, system_readiness, BuildMetadata,
    ProtectedTargetConfiguration, PROTECTED_ARGO_APPLICATION, PROTECTED_ENVIRONMENT,
    PROTECTED_GITOPS_REPO, PROTECTED_IMAGE_NAME, PROTECTED_KUSTOMIZATION_PATH, PROTECTED_NAMESPACE,
    PROTECTED_ROLLBACK_OWNER, PROTECTED_SOURCE_REPO, PROTECTED_WORKLOAD_KIND,
    PROTECTED_WORKLOAD_NAME,
};
use super::work_items::actions::{advance_work_item, execute_work_item_action};
use super::work_items::attempts::{
    cancel_work_item, list_workspaces, replan_work_item, transition_work_item, ListWorkspacesQuery,
};
use super::work_items::flow::{list_work_items, work_item_flow, ListWorkItemsQuery};
use super::work_items::lifecycle::approval_gates_from_work_item;
use super::work_items::preflight::{
    bounded_production_grant_expiry, create_work_item, request_matches_protected_target,
};
use super::work_items::reconcile::{
    action_effect, block_work_item_from_delivery_failure, complete_work_item_from_verified_release,
    deployment_intent_reconcile_action, deployment_intent_requires_execution_preflight,
    git_delivery_reconcile_action, gitops_base_revision_reconcile_state,
    gitops_change_set_reconcile_action, pipeline_intent_reconcile_action, reconcile_work_item,
    release_reconcile_action, GitOpsBaseRevisionReconcileState,
};
use super::work_items::reconcile_model::WorkItemReconcileAction;
use super::work_items::rollback::{
    approve_rollback_intent, internal_rollback_argo_sync_outcome,
    internal_rollback_delivery_context, internal_rollback_delivery_observation_outcome,
    internal_rollback_delivery_outcome, preflight_rollback_intent,
    required_baseline_capability_result, RollbackIntentRequest,
};
use super::work_items::rollback_state::latest_rollback_intent;
use super::work_items::wait_state::{
    schedule_controller_wait, supersede_active_controller_wait_if_present,
};
use super::work_items::waits::{
    list_work_item_controller_waits, list_work_item_events, observe_due_controller_wait,
    reconcile_due_controller_waits, ListControllerWaitsQuery,
};
use super::{router, AppState, CONTROLLER_WAIT_MAX_CHECKS};
use crate::dispatch::{KubernetesJobDispatcher, RunDispatcher};
use crate::dto::{
    AdvanceWorkItemRequest, ApprovalDecision, ArgoSyncOutcomeRequest, ArtifactResponse,
    AttachDeploymentIntentEvidenceRequest, AttachPipelineIntentEvidenceRequest,
    AttachReleaseEvidenceRequest, CapabilityStatusResponse, CreateChangeSetRequest,
    CreateDeploymentContractRequest, CreateDeploymentIntentFromPipelineIntentRequest,
    CreateDeploymentIntentTrustedEnvelopeRequest, CreateGitDeliveryAuthorizationRequest,
    CreateGitOpsDeliveryAuthorizationRequest, CreateIncidentRequest, CreateObservationRequest,
    CreatePermissionGrantRequest, CreatePipelineIntentFromChangeSetRequest,
    CreateRegistryEvidenceFromInspectionRequest, CreateRegistryEvidenceFromReleaseRequest,
    CreateReleaseFromDeploymentIntentRequest, CreateRemediationPlanRequest, CreateRunRequest,
    CreateTrustedEnvelopeRequest, CreateWorkItemPipelineIntentRequest, CreateWorkItemRequest,
    CreateWorkPlanFromRemediationPlanRequest, DecideApprovalGateRequest, DecideApprovalRequest,
    DeploymentIntentDeliveryFlowResponse, DeploymentIntentPreflightRequest,
    ExecuteCapabilityRequest, ExecuteCapabilityResponse, ExecuteDeploymentIntentRequest,
    ExecuteWorkItemActionRequest, GitDeliveryFlowResponse, GitDeliveryObservationOutcomeRequest,
    GitDeliveryPreflightRequest, GitOpsDeliveryFlowResponse,
    GitOpsDeliveryObservationOutcomeRequest, GitOpsDeliveryOutcomeRequest,
    GitOpsDeliveryPreflightRequest, PipelineIntentExecutionOutcomeRequest,
    PrepareGitDeliveryRequest, PrepareGitOpsDeliveryRequest, ReconcileDueControllerWaitsRequest,
    ReconcileWorkItemRequest, ReleaseResponse, ReplanWorkItemRequest, ReviewApprovalRequest,
    ReviseChangeSetRequest, ReviseWorkPlanRequest, RevokePermissionGrantRequest,
    TransitionChangeSetRequest, TransitionDeploymentContractRequest,
    TransitionDeploymentIntentRequest, TransitionPipelineIntentRequest,
    TransitionRegistryEvidenceRequest, TransitionReleaseRequest, TransitionRemediationPlanRequest,
    TransitionWorkItemRequest, TransitionWorkPlanRequest, VerifyReleaseRequest,
};
use crate::workspace::WorkspaceProvisioner;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::{Extension, Json};
use pharness_config::WorkerKubernetesConfig;
use pharness_core::{
    AgentAction, AgentEvent, EventId, EventKind, PolicyDecision, PolicyMode, ReadOnlyClusterTools,
    RiskLevel, RunBudget, RunId, RunScope, SafetyPolicy, SessionId,
};
use pharness_store::{
    ApprovalGateListFilter, ApprovalGateSummaryFilter, ApproveRepositoryOnboardingProposal,
    CreateApproval, CreateApprovalGate, CreateArtifact, CreateChangeSet, CreateControllerWait,
    CreateDeploymentIntent, CreateFileChange, CreateGitOpsChangeSet, CreateIncident,
    CreateObservation, CreatePipelineContract, CreatePipelineIntent, CreateProductAggregate,
    CreateRelease, CreateRemediationPlan, CreateRepoWorkItem, CreateRepositoryContractVersion,
    CreateRepositoryOnboardingProposal, CreateRepositoryReadinessAssessment, CreateRun,
    CreateSession, CreateSourceDeliveryIntent, CreateWorkItem, CreateWorkPlan, CreateWorkspace,
    ObservationListFilter, RegisterRepositoryAggregate, SqliteStore, StoredDeploymentContract,
    StoredGitOpsChangeSet, StoredPipelineContract, StoredPipelineIntent, StoredRelease,
    StoredRepositoryDraft,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;

mod characterization;
mod controller;
mod delivery_reconcile;
mod pipeline_delivery;
mod plans_changes;
mod production;
mod repo_mode_v1;
mod runs_capabilities;

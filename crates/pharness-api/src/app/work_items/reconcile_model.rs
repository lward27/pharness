use pharness_store::{StoredChangeSet, StoredWorkItem, StoredWorkPlan};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum WorkItemReconcileAction {
    DeclareWorkPlan,
    AwaitingWorkPlanApproval,
    StartCodingAttempt,
    WaitForCodingAttempt,
    CaptureChangeSet,
    AwaitingChangeSetApproval,
    PrepareGitDelivery,
    AwaitingGitDeliveryAuthorization,
    AwaitingGitWriterAvailability,
    AwaitingGitDeliveryExecution,
    WaitForGitDelivery,
    AwaitingPullRequestObservation,
    AwaitingPullRequestMerge,
    AwaitingPipelineIntentDefinition,
    AwaitingPipelineIntentApproval,
    AwaitingPipelineExecutionAuthorization,
    AwaitingPipelineExecution,
    WaitForPipelineExecution,
    PipelineExecutionFailed,
    AwaitingPipelineEvidenceReview,
    AwaitingPipelineBuildOutputReview,
    AwaitingDeploymentIntentDefinition,
    AwaitingGitOpsUpdatePlan,
    AwaitingGitOpsChangeSetApproval,
    AwaitingGitOpsBaseRevision,
    WaitForGitOpsBaseRevision,
    PrepareRollbackIntent,
    AwaitingGitOpsDeliveryPlan,
    AwaitingGitOpsDeliveryAuthorization,
    AwaitingGitOpsWriterAvailability,
    AwaitingGitOpsDeliveryExecution,
    WaitForGitOpsDelivery,
    AwaitingGitOpsPullRequestObservation,
    AwaitingGitOpsPullRequestMerge,
    AwaitingDeploymentIntentReview,
    AwaitingDeploymentAuthorization,
    AwaitingArgoRunnerAvailability,
    AwaitingDeploymentExecution,
    WaitForDeploymentExecution,
    DeploymentExecutionFailed,
    AwaitingReleaseDefinition,
    AwaitingReleaseApproval,
    AwaitingReleaseVerification,
    CompleteWorkItem,
    DeploymentIntentBlocked,
    ReleaseBlocked,
    GitOpsDeliveryFailed,
    GitOpsChangeSetBlocked,
    PipelineIntentBlocked,
    GitDeliveryFailed,
    RequiresReplan,
    Terminal,
}

impl WorkItemReconcileAction {
    pub(in crate::app) fn as_str(self) -> &'static str {
        match self {
            Self::DeclareWorkPlan => "declare_work_plan",
            Self::AwaitingWorkPlanApproval => "awaiting_work_plan_approval",
            Self::StartCodingAttempt => "start_coding_attempt",
            Self::WaitForCodingAttempt => "wait_for_coding_attempt",
            Self::CaptureChangeSet => "capture_change_set",
            Self::AwaitingChangeSetApproval => "awaiting_change_set_approval",
            Self::PrepareGitDelivery => "prepare_git_delivery",
            Self::AwaitingGitDeliveryAuthorization => "awaiting_git_delivery_authorization",
            Self::AwaitingGitWriterAvailability => "awaiting_git_writer_availability",
            Self::AwaitingGitDeliveryExecution => "awaiting_git_delivery_execution",
            Self::WaitForGitDelivery => "wait_for_git_delivery",
            Self::AwaitingPullRequestObservation => "awaiting_pull_request_observation",
            Self::AwaitingPullRequestMerge => "awaiting_pull_request_merge",
            Self::AwaitingPipelineIntentDefinition => "awaiting_pipeline_intent_definition",
            Self::AwaitingPipelineIntentApproval => "awaiting_pipeline_intent_approval",
            Self::AwaitingPipelineExecutionAuthorization => {
                "awaiting_pipeline_execution_authorization"
            }
            Self::AwaitingPipelineExecution => "awaiting_pipeline_execution",
            Self::WaitForPipelineExecution => "wait_for_pipeline_execution",
            Self::PipelineExecutionFailed => "pipeline_execution_failed",
            Self::AwaitingPipelineEvidenceReview => "awaiting_pipeline_evidence_review",
            Self::AwaitingPipelineBuildOutputReview => "awaiting_pipeline_build_output_review",
            Self::AwaitingDeploymentIntentDefinition => "awaiting_deployment_intent_definition",
            Self::AwaitingGitOpsUpdatePlan => "awaiting_gitops_update_plan",
            Self::AwaitingGitOpsChangeSetApproval => "awaiting_gitops_change_set_approval",
            Self::AwaitingGitOpsBaseRevision => "awaiting_gitops_base_revision",
            Self::WaitForGitOpsBaseRevision => "wait_for_gitops_base_revision",
            Self::PrepareRollbackIntent => "prepare_rollback_intent",
            Self::AwaitingGitOpsDeliveryPlan => "awaiting_gitops_delivery_plan",
            Self::AwaitingGitOpsDeliveryAuthorization => "awaiting_gitops_delivery_authorization",
            Self::AwaitingGitOpsWriterAvailability => "awaiting_gitops_writer_availability",
            Self::AwaitingGitOpsDeliveryExecution => "awaiting_gitops_delivery_execution",
            Self::WaitForGitOpsDelivery => "wait_for_gitops_delivery",
            Self::AwaitingGitOpsPullRequestObservation => {
                "awaiting_gitops_pull_request_observation"
            }
            Self::AwaitingGitOpsPullRequestMerge => "awaiting_gitops_pull_request_merge",
            Self::AwaitingDeploymentIntentReview => "awaiting_deployment_intent_review",
            Self::AwaitingDeploymentAuthorization => "awaiting_deployment_authorization",
            Self::AwaitingArgoRunnerAvailability => "awaiting_argo_runner_availability",
            Self::AwaitingDeploymentExecution => "awaiting_deployment_execution",
            Self::WaitForDeploymentExecution => "wait_for_deployment_execution",
            Self::DeploymentExecutionFailed => "deployment_execution_failed",
            Self::AwaitingReleaseDefinition => "awaiting_release_definition",
            Self::AwaitingReleaseApproval => "awaiting_release_approval",
            Self::AwaitingReleaseVerification => "awaiting_release_verification",
            Self::CompleteWorkItem => "complete_work_item",
            Self::DeploymentIntentBlocked => "deployment_intent_blocked",
            Self::ReleaseBlocked => "release_blocked",
            Self::GitOpsDeliveryFailed => "gitops_delivery_failed",
            Self::GitOpsChangeSetBlocked => "gitops_change_set_blocked",
            Self::PipelineIntentBlocked => "pipeline_intent_blocked",
            Self::GitDeliveryFailed => "git_delivery_failed",
            Self::RequiresReplan => "requires_replan",
            Self::Terminal => "terminal",
        }
    }

    pub(in crate::app) fn controller_wait_kind(self) -> Option<&'static str> {
        match self {
            Self::WaitForCodingAttempt => Some("coding_attempt"),
            Self::WaitForGitDelivery => Some("git_delivery_execution"),
            Self::AwaitingPullRequestObservation => Some("source_pull_request_observation"),
            Self::AwaitingPullRequestMerge => Some("source_pull_request_merge"),
            Self::WaitForPipelineExecution => Some("pipeline_execution"),
            Self::WaitForGitOpsBaseRevision => Some("gitops_base_revision"),
            Self::WaitForGitOpsDelivery => Some("gitops_delivery_execution"),
            Self::AwaitingGitOpsPullRequestObservation => Some("gitops_pull_request_observation"),
            Self::AwaitingGitOpsPullRequestMerge => Some("gitops_pull_request_merge"),
            Self::WaitForDeploymentExecution => Some("deployment_execution"),
            _ => None,
        }
    }

    pub(in crate::app) fn is_applyable(self) -> bool {
        matches!(
            self,
            Self::DeclareWorkPlan
                | Self::StartCodingAttempt
                | Self::CaptureChangeSet
                | Self::PrepareGitDelivery
                | Self::AwaitingGitDeliveryExecution
                | Self::AwaitingPullRequestObservation
                | Self::AwaitingPipelineExecution
                | Self::AwaitingGitOpsBaseRevision
                | Self::PrepareRollbackIntent
                | Self::AwaitingGitOpsDeliveryPlan
                | Self::AwaitingGitOpsDeliveryExecution
                | Self::AwaitingGitOpsPullRequestObservation
                | Self::AwaitingGitOpsPullRequestMerge
                | Self::AwaitingDeploymentExecution
                | Self::AwaitingReleaseDefinition
                | Self::AwaitingReleaseVerification
                | Self::CompleteWorkItem
        )
    }

    pub(in crate::app) fn message(
        self,
        work_item: &StoredWorkItem,
        work_plan: Option<&StoredWorkPlan>,
        change_set: Option<&StoredChangeSet>,
    ) -> String {
        match self {
            Self::AwaitingWorkPlanApproval => work_plan
                .map(|plan| format!("WorkPlan {} is {} and requires approval", plan.id, plan.status))
                .unwrap_or_else(|| "WorkItem requires a WorkPlan".to_string()),
            Self::WaitForCodingAttempt => "coding attempt is still running or awaiting its durable outcome".to_string(),
            Self::AwaitingChangeSetApproval => change_set
                .map(|change_set| {
                    format!(
                        "ChangeSet {} is {} and requires source review",
                        change_set.id, change_set.status
                    )
                })
                .unwrap_or_else(|| "ChangeSet capture is pending".to_string()),
            Self::AwaitingGitDeliveryAuthorization => {
                "Git delivery plan is prepared; a matching scoped Git writer grant and git_mutation gate decision are required"
                    .to_string()
            }
            Self::AwaitingGitWriterAvailability => {
                "Git delivery is authorized, but the dedicated Git writer is not configured for this exact repository"
                    .to_string()
            }
            Self::AwaitingGitDeliveryExecution => {
                "Git delivery is ready; explicitly execute the isolated branch-and-PR writer"
                    .to_string()
            }
            Self::WaitForGitDelivery => {
                "Git writer execution is in progress; wait for its durable branch-and-PR result"
                    .to_string()
            }
            Self::AwaitingPullRequestObservation => {
                "Git writer created a pull request; dispatch the read-only observer before any build is defined"
                    .to_string()
            }
            Self::AwaitingPullRequestMerge => {
                "Pull request is observed but lacks immutable merge provenance; wait for merge and observe again"
                    .to_string()
            }
            Self::AwaitingPipelineIntentDefinition => {
                "Immutable source merge provenance is recorded; define the exact PipelineIntent and PipelineContract next"
                    .to_string()
            }
            Self::AwaitingPipelineIntentApproval => {
                "PipelineIntent is proposed; review and approve its pinned PipelineContract and exact Tekton inputs"
                    .to_string()
            }
            Self::AwaitingPipelineExecutionAuthorization => {
                "PipelineIntent is approved but its scoped Tekton gates or trusted execution envelope are not yet ready"
                    .to_string()
            }
            Self::AwaitingPipelineExecution => {
                "PipelineIntent preflight is ready; explicitly dispatch the isolated Tekton executor"
                    .to_string()
            }
            Self::WaitForPipelineExecution => {
                "Tekton execution is in progress; wait for its signed-in executor outcome and terminal analysis"
                    .to_string()
            }
            Self::PipelineExecutionFailed => {
                "Tekton execution failed; inspect terminal evidence and revise or replan before further delivery"
                    .to_string()
            }
            Self::AwaitingPipelineEvidenceReview => {
                "Tekton completed, but its terminal PipelineRunAnalysis is not satisfied; review evidence before delivery planning"
                    .to_string()
            }
            Self::AwaitingPipelineBuildOutputReview => {
                "Tekton completed, but its build output is missing or not trusted; inspect terminal evidence before GitOps planning"
                    .to_string()
            }
            Self::AwaitingDeploymentIntentDefinition => {
                "Verified build evidence is ready; declare the exact development DeploymentIntent before GitOps update planning"
                    .to_string()
            }
            Self::AwaitingGitOpsUpdatePlan => {
                "Verified digest-pinned build output is ready; prepare the separate review-only GitOps update plan next"
                    .to_string()
            }
            Self::AwaitingGitOpsChangeSetApproval => {
                "GitOps ChangeSet is proposed; review its exact digest-pinned Kustomize update before authorization"
                    .to_string()
            }
            Self::AwaitingGitOpsBaseRevision => {
                "GitOps ChangeSet is approved; explicitly dispatch the read-only base-revision observer"
                    .to_string()
            }
            Self::WaitForGitOpsBaseRevision => {
                "GitOps base-revision observation is in progress; wait for immutable base commit evidence"
                    .to_string()
            }
            Self::PrepareRollbackIntent => {
                "GitOps base revision is resolved; explicitly capture the healthy protected-production baseline and prepare the digest-bound RollbackIntent before writer planning"
                    .to_string()
            }
            Self::AwaitingGitOpsDeliveryPlan => {
                "GitOps base revision is resolved; prepare the immutable GitOps delivery plan next"
                    .to_string()
            }
            Self::AwaitingGitOpsDeliveryAuthorization => {
                "GitOps delivery plan is prepared; a matching scoped GitOps writer grant and gitops_mutation gate decision are required"
                    .to_string()
            }
            Self::AwaitingGitOpsWriterAvailability => {
                "GitOps delivery is authorized, but the dedicated GitOps writer is not configured for this exact repository"
                    .to_string()
            }
            Self::AwaitingGitOpsDeliveryExecution => {
                "GitOps delivery is ready; explicitly execute the isolated GitOps branch-and-PR writer"
                    .to_string()
            }
            Self::WaitForGitOpsDelivery => {
                "GitOps writer execution is in progress; wait for its durable branch-and-PR result"
                    .to_string()
            }
            Self::AwaitingGitOpsPullRequestObservation => {
                "GitOps writer created a pull request; dispatch the read-only observer before Argo can be considered"
                    .to_string()
            }
            Self::AwaitingGitOpsPullRequestMerge => {
                "GitOps pull request is observed but lacks immutable merge provenance; wait for merge and observe again"
                    .to_string()
            }
            Self::AwaitingDeploymentIntentReview => {
                "Immutable GitOps merge provenance is recorded; review the declared DeploymentIntent before any Argo sync"
                    .to_string()
            }
            Self::AwaitingDeploymentAuthorization => {
                "DeploymentIntent is approved; a matching dev Argo contract, cluster_mutation gate, and scoped runner grant are required"
                    .to_string()
            }
            Self::AwaitingArgoRunnerAvailability => {
                "DeploymentIntent is authorized, but the isolated Argo runner is unavailable for this exact Application"
                    .to_string()
            }
            Self::AwaitingDeploymentExecution => {
                "DeploymentIntent is ready; explicitly dispatch the isolated Argo sync runner"
                    .to_string()
            }
            Self::WaitForDeploymentExecution => {
                "Argo sync is in progress; wait for its durable terminal result before proposing a Release"
                    .to_string()
            }
            Self::DeploymentExecutionFailed => {
                "Argo sync failed; inspect the bounded result and create a reviewed remediation or deployment revision"
                    .to_string()
            }
            Self::AwaitingReleaseDefinition => {
                "Argo sync completed; create the linked Release record before post-sync verification"
                    .to_string()
            }
            Self::AwaitingReleaseApproval => {
                "Release is proposed; review its immutable deployment provenance before verification"
                    .to_string()
            }
            Self::AwaitingReleaseVerification => {
                "Release is approved; explicitly run bounded post-sync verification against its declared targets"
                    .to_string()
            }
            Self::CompleteWorkItem => {
                "Release verification is complete; apply reconciliation to record terminal WorkItem completion"
                    .to_string()
            }
            Self::DeploymentIntentBlocked => {
                "DeploymentIntent is stale or rejected; create and review a new deployment intent before Argo execution"
                    .to_string()
            }
            Self::ReleaseBlocked => {
                "Release is stale or rejected; revise and review release provenance before post-sync verification"
                    .to_string()
            }
            Self::GitOpsDeliveryFailed => {
                "GitOps delivery failed; inspect its bounded result and explicitly re-propose this GitOps ChangeSet as a new reviewed revision before another authorized attempt"
                    .to_string()
            }
            Self::GitOpsChangeSetBlocked => {
                "GitOps ChangeSet is stale or rejected; create a newly reviewed GitOps plan before delivery can continue"
                    .to_string()
            }
            Self::PipelineIntentBlocked => {
                "PipelineIntent is stale or rejected; create a newly reviewed PipelineIntent before delivery can continue"
                    .to_string()
            }
            Self::GitDeliveryFailed => {
                "Git delivery failed; inspect its bounded result and revise/review the ChangeSet before another delivery"
                    .to_string()
            }
            Self::RequiresReplan => format!(
                "WorkItem is {} after {}/{} coding attempts; explicit replan or cancellation is required",
                work_item.status, work_item.attempt_count, work_item.max_attempts
            ),
            Self::Terminal => format!("WorkItem is terminal: {}", work_item.status),
            _ => format!("next action is {}", self.as_str()),
        }
    }

    pub(in crate::app) fn delivery_failure(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::GitDeliveryFailed => Some((
                "source_git_delivery_failed",
                "the bounded source Git writer reported a failed delivery",
            )),
            Self::PipelineExecutionFailed => Some((
                "pipeline_execution_failed",
                "the bounded Tekton execution reported a failed delivery",
            )),
            Self::GitOpsDeliveryFailed => Some((
                "gitops_delivery_failed",
                "the bounded GitOps writer reported a failed delivery",
            )),
            Self::DeploymentExecutionFailed => Some((
                "deployment_execution_failed",
                "the bounded Argo sync execution reported a failed delivery",
            )),
            Self::PipelineIntentBlocked => Some((
                "pipeline_intent_blocked",
                "the PipelineIntent is stale or rejected and cannot be executed",
            )),
            Self::GitOpsChangeSetBlocked => Some((
                "gitops_change_set_blocked",
                "the GitOps ChangeSet is stale or rejected and cannot be delivered",
            )),
            Self::DeploymentIntentBlocked => Some((
                "deployment_intent_blocked",
                "the DeploymentIntent is stale or rejected and cannot be executed",
            )),
            Self::ReleaseBlocked => Some((
                "release_blocked",
                "the Release is stale or rejected and cannot be verified",
            )),
            _ => None,
        }
    }
}

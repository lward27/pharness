use super::super::*;
use super::attempts::{capture_work_item_change_set, execute_work_item};

pub(in crate::app) async fn reconcile_work_item(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(work_item_id): Path<String>,
    Json(request): Json<ReconcileWorkItemRequest>,
) -> Result<Json<ReconcileWorkItemResponse>, ApiError> {
    let actor = identity
        .as_ref()
        .map(|Extension(OperatorIdentity(name))| name.clone())
        .or_else(|| clean_optional_text(request.actor.clone()));
    let reason = clean_optional_text(request.reason.clone());
    let work_item = state
        .store
        .get_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &work_item_id))?;
    let work_plan = state
        .store
        .get_work_plan_by_work_item(&work_item_id)
        .await?;
    let change_set = match &work_plan {
        Some(work_plan) => {
            state
                .store
                .get_change_set_by_work_plan(&work_plan.id)
                .await?
        }
        None => None,
    };

    let git_delivery = git_delivery_flow(&state.store, change_set.as_ref()).await?;
    let pipeline_intent = match change_set.as_ref() {
        Some(change_set) => {
            state
                .store
                .get_pipeline_intent_by_change_set(&change_set.id)
                .await?
        }
        None => None,
    };
    let pipeline_execution_preflight = match pipeline_intent
        .as_ref()
        .filter(|intent| pipeline_intent_requires_execution_preflight(intent))
    {
        Some(intent) => Some(pipeline_intent_execution_preflight(&state, &intent.id).await?),
        None => None,
    };
    let deployment_intent = match pipeline_intent.as_ref() {
        Some(intent) => {
            state
                .store
                .get_deployment_intent_by_pipeline_intent(&intent.id)
                .await?
        }
        None => None,
    };
    let gitops_change_set = match pipeline_intent.as_ref() {
        Some(intent) => {
            state
                .store
                .get_gitops_change_set_by_pipeline_intent(&intent.id)
                .await?
        }
        None => None,
    };
    let gitops_delivery = gitops_delivery_flow(&state.store, gitops_change_set.as_ref()).await?;
    let gitops_merge_observed = gitops_delivery
        .as_ref()
        .and_then(|delivery| delivery.latest_merge.as_ref())
        .is_some();
    let deployment_execution_preflight = match deployment_intent.as_ref() {
        Some(intent)
            if deployment_intent_requires_execution_preflight(
                work_item.gitops_repo.as_deref(),
                work_item.gitops_ref.as_deref(),
                &intent.status,
                gitops_merge_observed,
            )? =>
        {
            Some(deployment_intent_execution_preflight(&state, &intent.id).await?)
        }
        _ => None,
    };
    let deployment_dispatch_ready = deployment_execution_preflight.as_ref().map(|preflight| {
        state.worker.argo_executor_available()
            && deployment_target(&preflight.intent)
                .ok()
                .is_some_and(|target| {
                    state
                        .worker
                        .argo_executor_allows_application(&target.application)
                })
    });
    let deployment_delivery =
        deployment_intent_delivery_flow(&state.store, deployment_intent.as_ref()).await?;
    let gitops_base_revision = match gitops_change_set.as_ref() {
        Some(change_set) => {
            Some(gitops_base_revision_reconcile_state(&state.store, change_set).await?)
        }
        None => None,
    };
    let rollback_prepared = if work_item.production_impacting {
        latest_rollback_intent(&state, &work_item, None)
            .await?
            .and_then(|intent| {
                intent
                    .pointer("/content/status")
                    .and_then(Value::as_str)
                    .map(|status| matches!(status, "prepared" | "approved"))
            })
            .unwrap_or(false)
    } else {
        true
    };
    let action = work_item_reconcile_action(
        &work_item,
        work_plan.as_ref(),
        WorkItemDeliveryReconcileContext {
            change_set: change_set.as_ref(),
            git_delivery: git_delivery.as_ref(),
            pipeline_intent: pipeline_intent.as_ref(),
            pipeline_execution_ready: pipeline_execution_preflight
                .as_ref()
                .map(|preflight| preflight.ready),
            deployment_intent: deployment_intent.as_ref(),
            deployment_execution_preflight: deployment_execution_preflight.as_ref(),
            deployment_dispatch_ready,
            deployment_delivery: deployment_delivery.as_ref(),
            gitops_change_set: gitops_change_set.as_ref(),
            gitops_delivery: gitops_delivery.as_ref(),
            gitops_base_revision,
            rollback_prepared,
        },
    );
    let recorded_preflight =
        git_delivery_preflight_response(&state.store, git_delivery.as_ref()).await?;
    if !request.apply {
        return reconcile_work_item_response(
            &state,
            &work_item_id,
            action,
            false,
            recorded_preflight,
            "preview only; pass apply=true to perform the reported safe transition".to_string(),
        )
        .await
        .map(Json);
    }

    if action.controller_wait_kind().is_none() {
        supersede_active_controller_wait_if_present(
            &state,
            &work_item_id,
            format!("controller moved to {}", action.as_str()),
            actor.clone(),
        )
        .await?;
    }

    match action {
        WorkItemReconcileAction::DeclareWorkPlan => {
            if work_item.status == "submitted" {
                let planning = state
                    .store
                    .update_work_item_status(
                        &work_item.id,
                        "planning",
                        actor.clone(),
                        reason.clone().or_else(|| {
                            Some("controller advanced submitted WorkItem to planning".to_string())
                        }),
                    )
                    .await?;
                append_work_item_audit_event(
                    &state.store,
                    &planning,
                    "work_item.planning",
                    actor.clone(),
                    json!({ "source": "work_item.reconcile" }),
                )
                .await?;
            }
            let _ = create_work_plan_from_work_item(
                State(state.clone()),
                identity.clone(),
                Path(work_item_id.clone()),
            )
            .await?;
            reconcile_work_item_response(
                &state,
                &work_item_id,
                action,
                true,
                None,
                "declared the deterministic WorkPlan and ephemeral workspace; WorkPlan approval is now required"
                    .to_string(),
            )
            .await
            .map(Json)
        }
        WorkItemReconcileAction::StartCodingAttempt => {
            let Json(execution) = execute_work_item(
                State(state.clone()),
                identity,
                Path(work_item_id.clone()),
                Json(ExecuteWorkItemRequest {
                    actor,
                    reason,
                    max_turns: request.max_turns,
                }),
            )
            .await?;
            Ok(Json(ReconcileWorkItemResponse {
                action: action.as_str().to_string(),
                applied: true,
                work_item: execution.work_item,
                work_plan: state
                    .store
                    .get_work_plan_by_work_item(&work_item_id)
                    .await?
                    .map(Into::into),
                workspace: Some(execution.workspace),
                run: Some(execution.run),
                change_set: None,
                git_delivery_preflight: None,
                pipeline_intent: None,
                pipeline_execution_preflight: None,
                deployment_intent: None,
                deployment_execution_preflight: None,
                deployment_delivery: None,
                gitops_change_set: None,
                gitops_delivery: None,
                gitops_delivery_preflight: None,
                controller_wait: None,
                message: "started one bounded coding attempt in the declared isolated workspace"
                    .to_string(),
                boundary: action.as_str().to_string(),
                can_apply: false,
                effect_summary: "Started one bounded coding attempt in the declared isolated workspace."
                    .to_string(),
                blockers: vec![ReconcileBlockerResponse {
                    code: "controller_wait".to_string(),
                    summary: "Wait for the coding attempt to produce a durable outcome before reconciling again."
                        .to_string(),
                }],
                authorization_checks: vec![ReconcileAuthorizationCheckResponse {
                    kind: "controller_wait".to_string(),
                    status: "active".to_string(),
                    summary: "The bounded coding attempt now owns the next controller transition."
                        .to_string(),
                    resource_id: None,
                }],
            }))
        }
        WorkItemReconcileAction::CaptureChangeSet => {
            let Json(captured) = capture_work_item_change_set(
                State(state.clone()),
                identity,
                Path(work_item_id.clone()),
                Json(CaptureWorkItemChangeSetRequest { actor, reason }),
            )
            .await?;
            reconcile_work_item_response(
                &state,
                &work_item_id,
                action,
                true,
                None,
                format!(
                    "captured ChangeSet {}; source review is now required",
                    captured.change_set.id
                ),
            )
            .await
            .map(Json)
        }
        WorkItemReconcileAction::PrepareGitDelivery => {
            let Json(_) = prepare_change_set_git_delivery(
                State(state.clone()),
                Path(
                    change_set
                        .as_ref()
                        .expect("reconcile action requires a ChangeSet")
                        .id
                        .clone(),
                ),
                Json(PrepareGitDeliveryRequest {
                    actor: actor.clone(),
                    reason: reason.clone().or_else(|| {
                        Some("controller prepared immutable Git delivery plan".to_string())
                    }),
                }),
            )
            .await?;
            let Json(preflight) = preflight_change_set_git_delivery(
                State(state.clone()),
                identity,
                Path(
                    change_set
                        .as_ref()
                        .expect("reconcile action requires a ChangeSet")
                        .id
                        .clone(),
                ),
                Json(GitDeliveryPreflightRequest {
                    subject: None,
                    actor,
                    reason,
                }),
            )
            .await?;
            let message = if preflight.authorization_ready {
                "prepared and preflighted Git delivery; it is ready for the isolated Git writer"
                    .to_string()
            } else {
                "prepared and preflighted Git delivery; a matching Git writer grant is required"
                    .to_string()
            };
            reconcile_work_item_response(
                &state,
                &work_item_id,
                action,
                true,
                Some(preflight),
                message,
            )
            .await
            .map(Json)
        }
        WorkItemReconcileAction::AwaitingGitDeliveryExecution => Box::pin(async {
            let change_set = change_set
                .as_ref()
                .expect("Git delivery execution requires a ChangeSet");
            let Json(execution) = execute_change_set_git_delivery(
                State(state.clone()),
                identity,
                Path(change_set.id.clone()),
                Json(ExecuteGitDeliveryRequest {
                    subject: None,
                    actor: actor.clone(),
                    reason: reason.clone().unwrap_or_else(|| {
                        "controller applied the approved source Git delivery boundary".to_string()
                    }),
                }),
            )
            .await?;
            append_work_item_audit_event(
                &state.store,
                &work_item,
                "work_item.git_delivery_dispatched",
                actor.clone(),
                json!({
                    "change_set_id": change_set.id,
                    "execution_artifact_id": execution.execution.id,
                    "git_delivery_plan_artifact_id": execution.plan.id,
                    "permission_grant_id": execution.permission_grant.id,
                    "job_name": execution.job_name,
                    "status": execution.status,
                    "created": execution.created,
                    "automatic_execution": false,
                }),
            )
            .await?;

            if execution.status == "dispatch_failed" {
                return reconcile_work_item_response(
                    &state,
                    &work_item_id,
                    WorkItemReconcileAction::GitDeliveryFailed,
                    true,
                    recorded_preflight,
                    "recorded bounded source Git writer dispatch failure; apply reconcile again to record the terminal delivery block"
                        .to_string(),
                )
                .await
                .map(Json);
            }

            let (controller_wait, created) = schedule_controller_wait(
                &state,
                &work_item,
                WorkItemReconcileAction::WaitForGitDelivery,
                actor,
            )
            .await?;
            reconcile_work_item_response(
                &state,
                &work_item_id,
                action,
                true,
                recorded_preflight,
                if created {
                    format!(
                        "dispatched the approved isolated Git writer and scheduled bounded {} wait {} for WorkItem {}",
                        controller_wait.wait_kind, controller_wait.id, work_item_id
                    )
                } else {
                    format!(
                        "reused the Git writer dispatch and retained active {} wait {} for WorkItem {}",
                        controller_wait.wait_kind, controller_wait.id, work_item_id
                    )
                },
            )
            .await
            .map(Json)
        })
        .await,
        WorkItemReconcileAction::AwaitingPipelineExecution => Box::pin(async {
            let pipeline_intent = pipeline_intent
                .as_ref()
                .expect("Tekton execution requires a PipelineIntent");
            let Json(execution) = execute_pipeline_intent(
                State(state.clone()),
                identity,
                Path(pipeline_intent.id.clone()),
                Json(ExecutePipelineIntentRequest {
                    dry_run: false,
                    actor: actor.clone(),
                    reason: reason.clone().or_else(|| {
                        Some(
                            "controller applied the approved exact Tekton execution boundary"
                                .to_string(),
                        )
                    }),
                }),
            )
            .await?;
            append_work_item_audit_event(
                &state.store,
                &work_item,
                "work_item.pipeline_execution_dispatched",
                actor.clone(),
                json!({
                    "pipeline_intent_id": pipeline_intent.id,
                    "execution_id": execution.execution_id,
                    "executor_job_name": execution.executor_job_name,
                    "permission_grant_id": execution.permission_grant_id,
                    "status": execution.status,
                    "dry_run": execution.dry_run,
                    "automatic_execution": false,
                }),
            )
            .await?;

            if execution.status == "failed" {
                return reconcile_work_item_response(
                    &state,
                    &work_item_id,
                    WorkItemReconcileAction::PipelineExecutionFailed,
                    true,
                    recorded_preflight,
                    "recorded bounded Tekton executor dispatch failure; apply reconcile again to record the terminal delivery block"
                        .to_string(),
                )
                .await
                .map(Json);
            }

            let (controller_wait, created) = schedule_controller_wait(
                &state,
                &work_item,
                WorkItemReconcileAction::WaitForPipelineExecution,
                actor,
            )
            .await?;
            reconcile_work_item_response(
                &state,
                &work_item_id,
                action,
                true,
                recorded_preflight,
                if created {
                    format!(
                        "dispatched the approved isolated Tekton executor and scheduled bounded {} wait {} for WorkItem {}",
                        controller_wait.wait_kind, controller_wait.id, work_item_id
                    )
                } else {
                    format!(
                        "reused the Tekton executor dispatch and retained active {} wait {} for WorkItem {}",
                        controller_wait.wait_kind, controller_wait.id, work_item_id
                    )
                },
            )
            .await
            .map(Json)
        })
        .await,
        WorkItemReconcileAction::AwaitingReleaseDefinition => {
            let deployment_intent = deployment_intent
                .as_ref()
                .expect("Release definition requires a DeploymentIntent");
            let commit_sha = gitops_delivery
                .as_ref()
                .and_then(|delivery| delivery.latest_merge.as_ref())
                .and_then(|artifact| artifact.content_json.as_ref())
                .and_then(|content| content.get("merge_commit_sha"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            let rollback_ref = latest_rollback_intent(&state, &work_item, None)
                .await?
                .and_then(|intent| {
                    intent
                        .pointer("/content/baseline/image_ref")
                        .or_else(|| intent.pointer("/content/baseline/image_digest"))
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                });
            let Json(created) = create_release_from_deployment_intent(
                State(state.clone()),
                Json(CreateReleaseFromDeploymentIntentRequest {
                    deployment_intent_id: deployment_intent.id.clone(),
                    title: None,
                    summary: None,
                    risk_level: None,
                    release_kind: None,
                    version: None,
                    commit_sha,
                    image_digest: None,
                    rollback_ref,
                    release_json: None,
                    actor: actor.clone(),
                    reason: reason.clone().or_else(|| {
                        Some(
                            "controller proposed the Release from the completed exact Argo sync"
                                .to_string(),
                        )
                    }),
                }),
            )
            .await?;
            append_work_item_audit_event(
                &state.store,
                &work_item,
                "work_item.release_proposed",
                actor,
                json!({
                    "release_id": created.release.id,
                    "deployment_intent_id": deployment_intent.id,
                    "commit_sha": created.release.commit_sha,
                    "image_digest": created.release.image_digest,
                    "rollback_ref": created.release.rollback_ref,
                    "created": created.created,
                    "mutation_performed": false,
                }),
            )
            .await?;
            reconcile_work_item_response(
                &state,
                &work_item_id,
                action,
                true,
                recorded_preflight,
                if created.created {
                    format!(
                        "proposed Release {}; explicit Release review is now required",
                        created.release.id
                    )
                } else {
                    format!(
                        "reused existing Release {}; explicit Release review remains required",
                        created.release.id
                    )
                },
            )
            .await
            .map(Json)
        }
        WorkItemReconcileAction::AwaitingReleaseVerification => {
            let release = deployment_delivery
                .as_ref()
                .and_then(|delivery| delivery.release.as_ref())
                .expect("Release verification requires a Release");
            let Json(verification) = verify_release(
                State(state.clone()),
                identity,
                Path(release.id.clone()),
                Json(VerifyReleaseRequest {
                    complete: true,
                    actor: actor.clone(),
                    reason: Some(reason.clone().unwrap_or_else(|| {
                        "controller performed the explicit bounded post-sync verification"
                            .to_string()
                    })),
                    timeout_ms: None,
                }),
            )
            .await?;
            append_work_item_audit_event(
                &state.store,
                &work_item,
                if verification.completed {
                    "work_item.release_verified"
                } else {
                    "work_item.release_verification_attention_required"
                },
                actor,
                json!({
                    "release_id": verification.release.id,
                    "verified": verification.verified,
                    "completed": verification.completed,
                    "checks": verification.checks,
                    "automatic_rollback": false,
                    "mutation_performed": false,
                }),
            )
            .await?;
            reconcile_work_item_response(
                &state,
                &work_item_id,
                action,
                true,
                recorded_preflight,
                if verification.completed {
                    format!(
                        "verified and completed Release {}; WorkItem completion is now eligible",
                        verification.release.id
                    )
                } else {
                    format!(
                        "Release {} did not pass every required post-sync check; review the durable evidence before rollback consideration",
                        verification.release.id
                    )
                },
            )
            .await
            .map(Json)
        }
        WorkItemReconcileAction::CompleteWorkItem => {
            let completed =
                complete_work_item_from_verified_release(&state, &work_item_id, actor, reason)
                    .await?;
            reconcile_work_item_response(
                &state,
                &work_item_id,
                action,
                true,
                recorded_preflight,
                format!(
                    "completed WorkItem {} from verified Release {}",
                    completed.work_item.id, completed.release.id
                ),
            )
            .await
            .map(Json)
        }
        WorkItemReconcileAction::AwaitingPullRequestObservation => Box::pin(async {
            let change_set = change_set
                .as_ref()
                .expect("pull-request observation requires a ChangeSet");
            let Json(observation) = observe_change_set_git_delivery(
                State(state.clone()),
                identity.clone(),
                Path(change_set.id.clone()),
                Json(ObserveGitDeliveryRequest {
                    actor: actor.clone(),
                    reason: reason.clone().unwrap_or_else(|| {
                        "controller applied the read-only Git delivery observation boundary"
                            .to_string()
                    }),
                }),
            )
            .await?;
            append_work_item_audit_event(
                &state.store,
                &work_item,
                "work_item.git_delivery_observation_dispatched",
                actor.clone(),
                json!({
                    "change_set_id": change_set.id,
                    "execution_artifact_id": observation.execution.id,
                    "job_name": observation.job_name,
                    "status": observation.status,
                    "created": observation.created,
                    "automatic_execution": false,
                }),
            )
            .await?;

            if observation.status == "dispatch_failed" {
                supersede_active_controller_wait_if_present(
                    &state,
                    &work_item_id,
                    "read-only Git observer dispatch failed".to_string(),
                    actor,
                )
                .await?;
                return reconcile_work_item_response(
                    &state,
                    &work_item_id,
                    action,
                    true,
                    recorded_preflight,
                    "recorded read-only Git observer dispatch failure; review executor configuration before retrying"
                        .to_string(),
                )
                .await
                .map(Json);
            }

            let (controller_wait, created) = schedule_controller_wait(
                &state,
                &work_item,
                WorkItemReconcileAction::AwaitingPullRequestObservation,
                actor,
            )
            .await?;
            reconcile_work_item_response(
                &state,
                &work_item_id,
                action,
                true,
                recorded_preflight,
                if created {
                    format!(
                        "dispatched the configured read-only Git observer and scheduled bounded {} wait {} for WorkItem {}",
                        controller_wait.wait_kind, controller_wait.id, work_item_id
                    )
                } else {
                    format!(
                        "reused the read-only Git observer dispatch and retained active {} wait {} for WorkItem {}",
                        controller_wait.wait_kind, controller_wait.id, work_item_id
                    )
                },
            )
            .await
            .map(Json)
        })
        .await,
        WorkItemReconcileAction::AwaitingGitOpsBaseRevision => Box::pin(async {
            let gitops_change_set = gitops_change_set
                .as_ref()
                .expect("GitOps base revision resolution requires a GitOps ChangeSet");
            let Json(resolution) = resolve_gitops_base_revision(
                State(state.clone()),
                identity.clone(),
                Path(gitops_change_set.id.clone()),
                Json(ResolveGitOpsBaseRevisionRequest {
                    actor: actor.clone(),
                    reason: reason.clone().unwrap_or_else(|| {
                        "controller applied the read-only GitOps base-revision boundary"
                            .to_string()
                    }),
                }),
            )
            .await?;
            append_work_item_audit_event(
                &state.store,
                &work_item,
                "work_item.gitops_base_revision_dispatched",
                actor.clone(),
                json!({
                    "gitops_change_set_id": gitops_change_set.id,
                    "execution_artifact_id": resolution.execution.id,
                    "job_name": resolution.job_name,
                    "status": resolution.status,
                    "created": resolution.created,
                    "automatic_execution": false,
                }),
            )
            .await?;

            if resolution.status == "dispatch_failed" {
                supersede_active_controller_wait_if_present(
                    &state,
                    &work_item_id,
                    "read-only GitOps base-revision observer dispatch failed".to_string(),
                    actor,
                )
                .await?;
                return reconcile_work_item_response(
                    &state,
                    &work_item_id,
                    action,
                    true,
                    recorded_preflight,
                    "recorded read-only GitOps base-revision observer dispatch failure; review executor configuration before retrying"
                        .to_string(),
                )
                .await
                .map(Json);
            }

            let (controller_wait, created) = schedule_controller_wait(
                &state,
                &work_item,
                WorkItemReconcileAction::WaitForGitOpsBaseRevision,
                actor,
            )
            .await?;
            reconcile_work_item_response(
                &state,
                &work_item_id,
                action,
                true,
                recorded_preflight,
                if created {
                    format!(
                        "dispatched the configured read-only GitOps base-revision observer and scheduled bounded {} wait {} for WorkItem {}",
                        controller_wait.wait_kind, controller_wait.id, work_item_id
                    )
                } else {
                    format!(
                        "reused the read-only GitOps base-revision observer dispatch and retained active {} wait {} for WorkItem {}",
                        controller_wait.wait_kind, controller_wait.id, work_item_id
                    )
                },
            )
            .await
            .map(Json)
        })
        .await,
        WorkItemReconcileAction::PrepareRollbackIntent => {
            let Json(rollback) = prepare_work_item_rollback_intent(
                State(state.clone()),
                identity,
                Path(work_item_id.clone()),
                Json(RollbackIntentRequest {
                    actor: actor.clone(),
                    reason: reason.clone().unwrap_or_else(|| {
                        "controller captured the protected production baseline and prepared the digest-bound RollbackIntent"
                            .to_string()
                    }),
                    expires_at: None,
                }),
            )
            .await?;
            append_work_item_audit_event(
                &state.store,
                &work_item,
                "work_item.rollback_intent_prepared",
                actor,
                json!({
                    "rollback_intent_id": rollback.pointer("/content/rollback_intent_id"),
                    "baseline_digest": rollback.pointer("/content/baseline/image_digest"),
                    "rollback_owner": rollback.pointer("/content/rollback_owner"),
                    "automatic_rollback": false,
                    "mutation_performed": false,
                }),
            )
            .await?;
            reconcile_work_item_response(
                &state,
                &work_item_id,
                action,
                true,
                recorded_preflight,
                "captured the healthy protected-production baseline and prepared a digest-bound RollbackIntent; no rollback was executed"
                    .to_string(),
            )
            .await
            .map(Json)
        }
        WorkItemReconcileAction::AwaitingGitOpsDeliveryPlan => {
            let gitops_change_set = gitops_change_set
                .as_ref()
                .expect("GitOps delivery planning requires a GitOps ChangeSet");
            let Json(plan) = prepare_gitops_change_set_delivery(
                State(state.clone()),
                Path(gitops_change_set.id.clone()),
                Json(PrepareGitOpsDeliveryRequest {
                    actor: actor.clone(),
                    reason: reason.clone().or_else(|| {
                        Some(
                            "controller prepared the immutable, base-revision-bound GitOps delivery plan"
                                .to_string(),
                        )
                    }),
                }),
            )
            .await?;
            append_work_item_audit_event(
                &state.store,
                &work_item,
                "work_item.gitops_delivery_plan_prepared",
                actor,
                json!({
                    "gitops_change_set_id": gitops_change_set.id,
                    "gitops_delivery_plan_artifact_id": plan.artifact.id,
                    "gitops_base_revision_artifact_id": plan.base_revision.id,
                    "created": plan.created,
                    "mutation_performed": false,
                }),
            )
            .await?;
            reconcile_work_item_response(
                &state,
                &work_item_id,
                action,
                true,
                recorded_preflight,
                if plan.created {
                    "prepared the immutable GitOps delivery plan; writer authorization is now required"
                        .to_string()
                } else {
                    "reused the existing immutable GitOps delivery plan; writer authorization remains required"
                        .to_string()
                },
            )
            .await
            .map(Json)
        }
        WorkItemReconcileAction::AwaitingGitOpsPullRequestObservation
        | WorkItemReconcileAction::AwaitingGitOpsPullRequestMerge => Box::pin(async {
            let gitops_change_set = gitops_change_set
                .as_ref()
                .expect("GitOps pull-request observation requires a GitOps ChangeSet");
            let Json(observation) = observe_gitops_change_set_delivery(
                State(state.clone()),
                identity.clone(),
                Path(gitops_change_set.id.clone()),
                Json(ObserveGitOpsDeliveryRequest {
                    actor: actor.clone(),
                    reason: reason.clone().unwrap_or_else(|| match action {
                        WorkItemReconcileAction::AwaitingGitOpsPullRequestMerge => {
                            "controller refreshed the read-only GitOps pull-request observation after the manual merge boundary"
                                .to_string()
                        }
                        _ => {
                            "controller applied the read-only GitOps delivery observation boundary"
                                .to_string()
                        }
                    }),
                }),
            )
            .await?;
            append_work_item_audit_event(
                &state.store,
                &work_item,
                "work_item.gitops_delivery_observation_dispatched",
                actor.clone(),
                json!({
                    "gitops_change_set_id": gitops_change_set.id,
                    "execution_artifact_id": observation.execution.id,
                    "job_name": observation.job_name,
                    "status": observation.status,
                    "created": observation.created,
                    "automatic_execution": false,
                }),
            )
            .await?;

            if observation.status == "dispatch_failed" {
                supersede_active_controller_wait_if_present(
                    &state,
                    &work_item_id,
                    "read-only GitOps observer dispatch failed".to_string(),
                    actor,
                )
                .await?;
                return reconcile_work_item_response(
                    &state,
                    &work_item_id,
                    action,
                    true,
                    recorded_preflight,
                    "recorded read-only GitOps observer dispatch failure; review executor configuration before retrying"
                        .to_string(),
                )
                .await
                .map(Json);
            }

            let (controller_wait, created) = schedule_controller_wait(
                &state,
                &work_item,
                action,
                actor,
            )
            .await?;
            reconcile_work_item_response(
                &state,
                &work_item_id,
                action,
                true,
                recorded_preflight,
                if created {
                    format!(
                        "dispatched the configured read-only GitOps observer and scheduled bounded {} wait {} for WorkItem {}",
                        controller_wait.wait_kind, controller_wait.id, work_item_id
                    )
                } else {
                    format!(
                        "reused the read-only GitOps observer dispatch and retained active {} wait {} for WorkItem {}",
                        controller_wait.wait_kind, controller_wait.id, work_item_id
                    )
                },
            )
            .await
            .map(Json)
        })
        .await,
        WorkItemReconcileAction::AwaitingGitOpsDeliveryExecution => Box::pin(async {
            let gitops_change_set = gitops_change_set
                .as_ref()
                .expect("GitOps delivery execution requires a GitOps ChangeSet");
            let Json(execution) = execute_gitops_change_set_delivery(
                State(state.clone()),
                identity,
                Path(gitops_change_set.id.clone()),
                Json(ExecuteGitOpsDeliveryRequest {
                    subject: None,
                    actor: actor.clone(),
                    reason: reason.clone().unwrap_or_else(|| {
                        "controller applied the approved immutable GitOps delivery boundary"
                            .to_string()
                    }),
                }),
            )
            .await?;
            append_work_item_audit_event(
                &state.store,
                &work_item,
                "work_item.gitops_delivery_dispatched",
                actor.clone(),
                json!({
                    "gitops_change_set_id": gitops_change_set.id,
                    "execution_artifact_id": execution.execution.id,
                    "gitops_delivery_plan_artifact_id": execution.plan.id,
                    "permission_grant_id": execution.permission_grant.id,
                    "job_name": execution.job_name,
                    "status": execution.status,
                    "created": execution.created,
                    "automatic_execution": false,
                }),
            )
            .await?;

            if execution.status == "dispatch_failed" {
                return reconcile_work_item_response(
                    &state,
                    &work_item_id,
                    WorkItemReconcileAction::GitOpsDeliveryFailed,
                    true,
                    recorded_preflight,
                    "recorded bounded GitOps writer dispatch failure; apply reconcile again to record the terminal delivery block"
                        .to_string(),
                )
                .await
                .map(Json);
            }

            let (controller_wait, created) = schedule_controller_wait(
                &state,
                &work_item,
                WorkItemReconcileAction::WaitForGitOpsDelivery,
                actor,
            )
            .await?;
            reconcile_work_item_response(
                &state,
                &work_item_id,
                action,
                true,
                recorded_preflight,
                if created {
                    format!(
                        "dispatched the approved isolated GitOps writer and scheduled bounded {} wait {} for WorkItem {}",
                        controller_wait.wait_kind, controller_wait.id, work_item_id
                    )
                } else {
                    format!(
                        "reused the GitOps writer dispatch and retained active {} wait {} for WorkItem {}",
                        controller_wait.wait_kind, controller_wait.id, work_item_id
                    )
                },
            )
            .await
            .map(Json)
        })
        .await,
        WorkItemReconcileAction::AwaitingDeploymentExecution => Box::pin(async {
            let deployment_intent = deployment_intent
                .as_ref()
                .expect("Argo sync execution requires a DeploymentIntent");
            let Json(execution) = execute_deployment_intent(
                State(state.clone()),
                identity,
                Path(deployment_intent.id.clone()),
                Json(ExecuteDeploymentIntentRequest {
                    dry_run: false,
                    actor: actor.clone(),
                    reason: reason.clone().or_else(|| {
                        Some(
                            "controller applied the approved exact Argo sync boundary".to_string(),
                        )
                    }),
                }),
            )
            .await?;
            append_work_item_audit_event(
                &state.store,
                &work_item,
                "work_item.deployment_execution_dispatched",
                actor.clone(),
                json!({
                    "deployment_intent_id": deployment_intent.id,
                    "execution_artifact_id": execution.execution.as_ref().map(|artifact| &artifact.id),
                    "execution_id": execution.execution_id,
                    "executor_job_name": execution.executor_job_name,
                    "permission_grant_id": execution.permission_grant.as_ref().map(|grant| &grant.id),
                    "status": execution.status,
                    "dry_run": execution.dry_run,
                    "automatic_execution": false,
                }),
            )
            .await?;

            if execution.status == "dispatch_failed" {
                return reconcile_work_item_response(
                    &state,
                    &work_item_id,
                    WorkItemReconcileAction::DeploymentExecutionFailed,
                    true,
                    recorded_preflight,
                    "recorded bounded Argo runner dispatch failure; apply reconcile again to record the terminal delivery block"
                        .to_string(),
                )
                .await
                .map(Json);
            }

            let (controller_wait, created) = schedule_controller_wait(
                &state,
                &work_item,
                WorkItemReconcileAction::WaitForDeploymentExecution,
                actor,
            )
            .await?;
            reconcile_work_item_response(
                &state,
                &work_item_id,
                action,
                true,
                recorded_preflight,
                if created {
                    format!(
                        "dispatched the approved isolated Argo runner and scheduled bounded {} wait {} for WorkItem {}",
                        controller_wait.wait_kind, controller_wait.id, work_item_id
                    )
                } else {
                    format!(
                        "reused the Argo runner dispatch and retained active {} wait {} for WorkItem {}",
                        controller_wait.wait_kind, controller_wait.id, work_item_id
                    )
                },
            )
            .await
            .map(Json)
        })
        .await,
        action if action.controller_wait_kind().is_some() => {
            let (controller_wait, created) =
                schedule_controller_wait(&state, &work_item, action, actor.clone()).await?;
            reconcile_work_item_response(
                &state,
                &work_item_id,
                action,
                true,
                recorded_preflight,
                if created {
                    format!(
                        "scheduled bounded {} wait {} for WorkItem {}",
                        controller_wait.wait_kind, controller_wait.id, work_item_id
                    )
                } else {
                    format!(
                        "retained active {} wait {} for WorkItem {}",
                        controller_wait.wait_kind, controller_wait.id, work_item_id
                    )
                },
            )
            .await
            .map(Json)
        }
        action if action.delivery_failure().is_some() => {
            let (failure_code, failure_summary) = action
                .delivery_failure()
                .expect("matching controller action has a delivery failure");
            let blocked = block_work_item_from_delivery_failure(
                &state,
                &work_item_id,
                action,
                failure_code,
                failure_summary,
                actor,
                reason,
            )
            .await?;
            reconcile_work_item_response(
                &state,
                &work_item_id,
                action,
                true,
                recorded_preflight,
                format!(
                    "blocked WorkItem {}: {} ({})",
                    blocked.id, failure_summary, failure_code
                ),
            )
            .await
            .map(Json)
        }
        _ => reconcile_work_item_response(
            &state,
            &work_item_id,
            action,
            false,
            recorded_preflight,
            action.message(&work_item, work_plan.as_ref(), change_set.as_ref()),
        )
        .await
        .map(Json),
    }
}

/// Persist a terminal controller stop after a durable external-system failure.
/// It deliberately does not retry, rollback, or mutate the external target.
pub(in crate::app) async fn block_work_item_from_delivery_failure(
    state: &AppState,
    work_item_id: &str,
    action: WorkItemReconcileAction,
    failure_code: &str,
    failure_summary: &str,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<StoredWorkItem, ApiError> {
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    if matches!(work_item.status.as_str(), "completed" | "cancelled") {
        return Err(ApiError::conflict(
            "terminal WorkItems cannot be blocked by controller reconciliation",
        ));
    }
    if work_item.status == "blocked" {
        return Ok(work_item);
    }
    let reason = reason.or_else(|| Some(failure_summary.to_string()));

    // A delivery failure is operational evidence, not merely a controller state
    // transition. Persist the evidence before blocking the WorkItem so the UI
    // and a future remediation controller have a durable, non-secret anchor.
    let work_plan = state.store.get_work_plan_by_work_item(work_item_id).await?;
    let (session_id, run_id, resource_namespace, resource_kind, resource_name, work_plan_id) =
        if let Some(work_plan) = work_plan {
            (
                work_plan.session_id,
                work_plan
                    .run_id
                    .or_else(|| work_item.current_run_id.clone()),
                work_plan
                    .resource_namespace
                    .or_else(|| work_item.target_namespace.clone()),
                work_plan
                    .resource_kind
                    .or_else(|| Some("work_item".to_string())),
                work_plan
                    .resource_name
                    .or_else(|| Some(work_item.id.clone())),
                Some(work_plan.id),
            )
        } else {
            let (session_id, run_id) = root_session_for_request(
                &state.store,
                None,
                work_item.current_run_id.clone(),
                "delivery failure evidence",
            )
            .await?;
            (
                session_id,
                run_id,
                work_item.target_namespace.clone(),
                Some("work_item".to_string()),
                Some(work_item.id.clone()),
                None,
            )
        };
    let evidence = json!({
        "source": "work_item_delivery_failure",
        "work_item_id": work_item.id,
        "work_plan_id": work_plan_id,
        "controller_action": action.as_str(),
        "failure_code": failure_code,
        "failure_summary": failure_summary,
        "source_provenance": {
            "repo": work_item.source_repo,
            "ref": work_item.source_ref,
        },
        "target": {
            "environment": work_item.target_environment,
            "namespace": work_item.target_namespace,
            "argo_application": work_item.argo_application,
            "production_impacting": work_item.production_impacting,
        },
        "budget": {
            "attempt_count": work_item.attempt_count,
            "max_attempts": work_item.max_attempts,
            "max_elapsed_seconds": work_item.max_elapsed_seconds,
        },
        "automatic_retry": false,
        "automatic_rollback": false,
        "mutation_performed": false,
    });
    let observation = state
        .store
        .create_observation(CreateObservation {
            id: format!("obs_delivery_failure_{}", unique_suffix()),
            session_id: session_id.clone(),
            run_id: run_id.clone(),
            source: "pharness_controller".to_string(),
            kind: "delivery_failure".to_string(),
            subject: format!("work_item/{}", work_item.id),
            summary: failure_summary.to_string(),
            resource_namespace: resource_namespace.clone(),
            resource_kind: resource_kind.clone(),
            resource_name: resource_name.clone(),
            resource_ref_json: Some(json!({
                "work_item_id": work_item.id,
                "work_plan_id": work_plan_id,
            })),
            artifact_id: None,
            data_json: evidence.clone(),
        })
        .await?;
    append_observation_audit_event(
        &state.store,
        &observation,
        "observation.delivery_failure_recorded",
        actor.clone(),
        reason.clone(),
    )
    .await?;
    let incident = state
        .store
        .create_incident(CreateIncident {
            id: format!("inc_delivery_failure_{}", unique_suffix()),
            observation_id: observation.id.clone(),
            session_id,
            run_id,
            status: "candidate".to_string(),
            severity: delivery_failure_severity(action).to_string(),
            title: format!("Delivery blocked: {}", work_item.title),
            summary: failure_summary.to_string(),
            resource_namespace,
            resource_kind,
            resource_name,
            data_json: evidence,
        })
        .await?;
    append_incident_audit_event(
        &state.store,
        &incident,
        "incident.delivery_failure_created",
        actor.clone(),
        reason.clone(),
    )
    .await?;
    let remediation_plan = create_delivery_failure_remediation_plan(
        &state.store,
        &incident,
        actor.clone(),
        reason.clone(),
    )
    .await?;
    let blocked = state
        .store
        .update_work_item_status(work_item_id, "blocked", actor.clone(), reason.clone())
        .await?;
    append_work_item_audit_event(
        &state.store,
        &blocked,
        "work_item.delivery_blocked",
        actor,
        json!({
            "source": "work_item.reconcile",
            "previous_status": work_item.status,
            "controller_action": action.as_str(),
            "failure_code": failure_code,
            "failure_summary": failure_summary,
            "reason": reason,
            "attempt_count": blocked.attempt_count,
            "max_attempts": blocked.max_attempts,
            "automatic_retry": false,
            "automatic_rollback": false,
            "mutation_performed": false,
            "observation_id": observation.id,
            "incident_id": incident.id,
            "remediation_plan_id": remediation_plan.as_ref().map(|plan| plan.id.as_str()),
        }),
    )
    .await?;
    Ok(blocked)
}

pub(in crate::app) fn delivery_failure_severity(action: WorkItemReconcileAction) -> &'static str {
    match action {
        WorkItemReconcileAction::DeploymentExecutionFailed => "high",
        WorkItemReconcileAction::GitDeliveryFailed
        | WorkItemReconcileAction::PipelineExecutionFailed
        | WorkItemReconcileAction::GitOpsDeliveryFailed
        | WorkItemReconcileAction::PipelineIntentBlocked
        | WorkItemReconcileAction::GitOpsChangeSetBlocked
        | WorkItemReconcileAction::DeploymentIntentBlocked
        | WorkItemReconcileAction::ReleaseBlocked => "high",
        _ => "medium",
    }
}

pub(in crate::app) async fn create_delivery_failure_remediation_plan(
    store: &SqliteStore,
    incident: &StoredIncident,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<Option<StoredRemediationPlan>, ApiError> {
    if incident.status != "candidate"
        || incident.data_json.get("source").and_then(Value::as_str)
            != Some("work_item_delivery_failure")
    {
        return Ok(None);
    }

    let plan_id = format!("rplan_{}", incident.id);
    if let Some(existing) = store.get_remediation_plan(&plan_id).await? {
        return Ok(Some(existing));
    }

    let resource = incident_resource_label(incident);
    let plan = store
        .create_remediation_plan(CreateRemediationPlan {
            id: plan_id,
            incident_id: incident.id.clone(),
            session_id: incident.session_id.clone(),
            run_id: incident.run_id.clone(),
            status: "draft".to_string(),
            title: format!("Draft recovery for blocked delivery: {resource}"),
            summary: "Review the bounded delivery evidence, refresh only the affected read-only signals, and require explicit approval before any retry, source change, pipeline action, or cluster mutation.".to_string(),
            risk_level: incident.severity.clone(),
            requires_approval: true,
            resource_namespace: incident.resource_namespace.clone(),
            resource_kind: incident.resource_kind.clone(),
            resource_name: incident.resource_name.clone(),
            plan_json: delivery_failure_remediation_plan_json(incident, &resource),
        })
        .await?;
    append_remediation_plan_audit_event(
        store,
        &plan,
        "remediation_plan.created",
        actor,
        reason.or_else(|| Some("delivery failure requires operator review".to_string())),
    )
    .await?;

    for gate in approval_gates_from_remediation_plan(&plan) {
        let gate = store.create_approval_gate(gate).await?;
        append_approval_gate_audit_event(store, &gate, "approval_gate.created", "created").await?;
    }

    Ok(Some(plan))
}

pub(in crate::app) fn delivery_failure_remediation_plan_json(
    incident: &StoredIncident,
    resource: &str,
) -> Value {
    let controller_action = incident
        .data_json
        .get("controller_action")
        .and_then(Value::as_str)
        .unwrap_or("delivery_failure");
    json!({
        "mode": "read_only_draft",
        "source": "work_item_delivery_failure",
        "incident_id": incident.id,
        "resource": {
            "namespace": incident.resource_namespace,
            "kind": incident.resource_kind,
            "name": incident.resource_name,
            "label": resource,
        },
        "evidence": {
            "work_item_id": incident.data_json.get("work_item_id"),
            "work_plan_id": incident.data_json.get("work_plan_id"),
            "controller_action": controller_action,
            "failure_code": incident.data_json.get("failure_code"),
            "failure_summary": incident.data_json.get("failure_summary"),
            "attempt_count": incident.data_json.pointer("/budget/attempt_count"),
            "max_attempts": incident.data_json.pointer("/budget/max_attempts"),
            "observation_id": incident.observation_id,
        },
        "steps": [
            {
                "order": 1,
                "kind": "read_only",
                "capability": "delivery_evidence_review",
                "summary": "Review the exact bounded failure evidence and immutable delivery lineage; do not rerun the failed action."
            },
            {
                "order": 2,
                "kind": "read_only",
                "capability": delivery_failure_observation_capability(controller_action),
                "summary": "Refresh only the affected delivery-system status and compare it with the recorded failure evidence."
            },
            {
                "order": 3,
                "kind": "proposal",
                "capability": "bounded_recovery_proposal",
                "summary": "Propose a replan, source ChangeSet, PipelineIntent, DeploymentIntent, or rollback plan only after reviewing current evidence and policy."
            }
        ],
        "approval_gates": [
            {
                "kind": "file_write",
                "required_before": "creating or patching source or GitOps changes"
            },
            {
                "kind": "git_mutation",
                "required_before": "creating, pushing, merging, or reverting a Git branch or pull request"
            },
            {
                "kind": "pipeline_mutation",
                "required_before": "rerunning or cancelling Tekton resources"
            },
            {
                "kind": "cluster_mutation",
                "required_before": "Argo sync, rollback, restart, scale, or Kubernetes write"
            },
            {
                "kind": "production_impact",
                "required_before": "any action against production-impacting scope"
            }
        ],
        "non_goals": [
            "No automatic retry",
            "No automatic rollback",
            "No automatic mutation",
            "No secret reads",
            "No ticket creation",
            "No notification dispatch"
        ]
    })
}

pub(in crate::app) fn delivery_failure_observation_capability(action: &str) -> &'static str {
    match action {
        "pipeline_execution_failed" | "pipeline_intent_blocked" => "tekton_get_pipeline_runs",
        "deployment_execution_failed" | "deployment_intent_blocked" | "release_blocked" => {
            "argocd_get_application"
        }
        "git_delivery_failed" | "gitops_delivery_failed" | "gitops_change_set_blocked" => {
            "git_delivery_observation"
        }
        _ => "delivery_evidence_review",
    }
}

#[derive(Debug)]
pub(in crate::app) struct CompletedWorkItemRelease {
    pub(in crate::app) work_item: StoredWorkItem,
    pub(in crate::app) release: StoredRelease,
}

/// Complete a WorkItem only from its already-completed, post-sync verified
/// release. This is a durable bookkeeping transition, never an external
/// deployment operation.
pub(in crate::app) async fn complete_work_item_from_verified_release(
    state: &AppState,
    work_item_id: &str,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<CompletedWorkItemRelease, ApiError> {
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    if work_item.status != "awaiting_approval" {
        return Err(ApiError::conflict(
            "WorkItem completion requires an awaiting_approval WorkItem",
        ));
    }
    if !work_item_target_supported(&work_item) {
        return Err(ApiError::conflict(
            "Controller completion requires a supported dev or exact protected-production WorkItem",
        ));
    }
    let work_plan = state
        .store
        .get_work_plan_by_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("WorkItem completion requires a WorkPlan"))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&work_plan.id)
        .await?
        .ok_or_else(|| ApiError::conflict("WorkItem completion requires a ChangeSet"))?;
    let pipeline_intent = state
        .store
        .get_pipeline_intent_by_change_set(&change_set.id)
        .await?
        .ok_or_else(|| ApiError::conflict("WorkItem completion requires a PipelineIntent"))?;
    let deployment_intent = state
        .store
        .get_deployment_intent_by_pipeline_intent(&pipeline_intent.id)
        .await?
        .ok_or_else(|| ApiError::conflict("WorkItem completion requires a DeploymentIntent"))?;
    let release = state
        .store
        .get_release_by_deployment_intent(&deployment_intent.id)
        .await?
        .ok_or_else(|| ApiError::conflict("WorkItem completion requires a Release"))?;

    let lineage_matches = work_plan.status == "approved"
        && change_set.status == "approved"
        && pipeline_intent.status == "approved"
        && deployment_intent.status == "approved"
        && release.work_plan_id == work_plan.id
        && release.change_set_id == change_set.id
        && release.pipeline_intent_id == pipeline_intent.id
        && release.deployment_intent_id == deployment_intent.id;
    if !lineage_matches {
        return Err(ApiError::conflict(
            "WorkItem completion requires current approved delivery lineage",
        ));
    }
    let gitops_merge =
        observed_gitops_merge_for_deployment(&state.store, &work_item, &pipeline_intent).await?;
    let post_sync_verified = release.status == "completed"
        && release
            .release_json
            .pointer("/post_sync_verification/status")
            .and_then(Value::as_str)
            == Some("verified")
        && release
            .release_json
            .pointer("/post_sync_verification/runtime_ready")
            .and_then(Value::as_bool)
            == Some(true);
    if !post_sync_verified {
        return Err(ApiError::conflict(
            "WorkItem completion requires completed post-sync verified Release evidence",
        ));
    }

    let reason = reason.or_else(|| {
        Some(format!(
            "controller completed WorkItem from verified Release {}",
            release.id
        ))
    });
    let completed = state
        .store
        .update_work_item_status(work_item_id, "completed", actor.clone(), reason.clone())
        .await?;
    append_work_item_audit_event(
        &state.store,
        &completed,
        "work_item.completed_from_verified_release",
        actor,
        json!({
            "source": "work_item.reconcile",
            "work_plan_id": work_plan.id,
            "change_set_id": change_set.id,
            "pipeline_intent_id": pipeline_intent.id,
            "deployment_intent_id": deployment_intent.id,
            "release_id": release.id,
            "release_status": release.status,
            "gitops_delivery_merge_artifact_id": gitops_merge.as_ref().map(|artifact| &artifact.id),
            "post_sync_verification": release.release_json.get("post_sync_verification"),
            "reason": reason,
        }),
    )
    .await?;
    Ok(CompletedWorkItemRelease {
        work_item: completed,
        release,
    })
}

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

pub(in crate::app) struct WorkItemDeliveryReconcileContext<'a> {
    change_set: Option<&'a StoredChangeSet>,
    git_delivery: Option<&'a GitDeliveryFlowResponse>,
    pipeline_intent: Option<&'a StoredPipelineIntent>,
    pipeline_execution_ready: Option<bool>,
    deployment_intent: Option<&'a StoredDeploymentIntent>,
    deployment_execution_preflight: Option<&'a DeploymentIntentExecutionPreflight>,
    deployment_dispatch_ready: Option<bool>,
    deployment_delivery: Option<&'a DeploymentIntentDeliveryFlowResponse>,
    gitops_change_set: Option<&'a StoredGitOpsChangeSet>,
    gitops_delivery: Option<&'a GitOpsDeliveryFlowResponse>,
    gitops_base_revision: Option<GitOpsBaseRevisionReconcileState>,
    rollback_prepared: bool,
}

pub(in crate::app) fn work_item_reconcile_action(
    work_item: &StoredWorkItem,
    work_plan: Option<&StoredWorkPlan>,
    delivery: WorkItemDeliveryReconcileContext<'_>,
) -> WorkItemReconcileAction {
    match work_item.status.as_str() {
        "submitted" | "planning" => WorkItemReconcileAction::DeclareWorkPlan,
        "awaiting_approval" => match delivery.change_set {
            Some(change_set) if change_set.status == "approved" => {
                let git_action = git_delivery_reconcile_action(delivery.git_delivery);
                if git_action == WorkItemReconcileAction::AwaitingPipelineIntentDefinition {
                    let pipeline_action = pipeline_intent_reconcile_action(
                        delivery.pipeline_intent,
                        delivery.pipeline_execution_ready,
                        delivery.deployment_intent,
                    );
                    if pipeline_action == WorkItemReconcileAction::AwaitingGitOpsUpdatePlan {
                        let gitops_action = gitops_change_set_reconcile_action(
                            delivery.gitops_change_set,
                            delivery.gitops_delivery,
                            delivery.gitops_base_revision,
                        );
                        if gitops_action == WorkItemReconcileAction::AwaitingGitOpsDeliveryPlan
                            && work_item.production_impacting
                            && !delivery.rollback_prepared
                        {
                            return WorkItemReconcileAction::PrepareRollbackIntent;
                        }
                        if gitops_action == WorkItemReconcileAction::AwaitingDeploymentIntentReview
                        {
                            deployment_intent_reconcile_action(
                                delivery.deployment_intent,
                                delivery.deployment_execution_preflight,
                                delivery.deployment_dispatch_ready,
                                delivery.deployment_delivery,
                            )
                        } else {
                            gitops_action
                        }
                    } else {
                        pipeline_action
                    }
                } else {
                    git_action
                }
            }
            Some(_) => WorkItemReconcileAction::AwaitingChangeSetApproval,
            None if work_plan.is_some_and(|plan| plan.status == "approved") => {
                WorkItemReconcileAction::StartCodingAttempt
            }
            None => WorkItemReconcileAction::AwaitingWorkPlanApproval,
        },
        "executing" => WorkItemReconcileAction::WaitForCodingAttempt,
        "verifying" => WorkItemReconcileAction::CaptureChangeSet,
        "blocked" | "failed" => WorkItemReconcileAction::RequiresReplan,
        "completed" | "cancelled" => WorkItemReconcileAction::Terminal,
        _ => WorkItemReconcileAction::RequiresReplan,
    }
}

pub(in crate::app) fn pipeline_intent_reconcile_action(
    pipeline_intent: Option<&StoredPipelineIntent>,
    pipeline_execution_ready: Option<bool>,
    deployment_intent: Option<&StoredDeploymentIntent>,
) -> WorkItemReconcileAction {
    let Some(pipeline_intent) = pipeline_intent else {
        return WorkItemReconcileAction::AwaitingPipelineIntentDefinition;
    };
    match pipeline_intent.status.as_str() {
        "proposed" => WorkItemReconcileAction::AwaitingPipelineIntentApproval,
        "executing" => WorkItemReconcileAction::WaitForPipelineExecution,
        "failed" => WorkItemReconcileAction::PipelineExecutionFailed,
        "rejected" | "stale" => WorkItemReconcileAction::PipelineIntentBlocked,
        "approved" => match pipeline_intent_execution_state(pipeline_intent) {
            Some("pipeline_run_succeeded") => {
                if !pipeline_evidence_is_satisfied(pipeline_intent) {
                    WorkItemReconcileAction::AwaitingPipelineEvidenceReview
                } else if !pipeline_build_output_is_verified(pipeline_intent) {
                    WorkItemReconcileAction::AwaitingPipelineBuildOutputReview
                } else if deployment_intent.is_none() {
                    WorkItemReconcileAction::AwaitingDeploymentIntentDefinition
                } else {
                    WorkItemReconcileAction::AwaitingGitOpsUpdatePlan
                }
            }
            Some("pipeline_run_failed") | Some("failed") | Some("dispatch_failed") => {
                WorkItemReconcileAction::PipelineExecutionFailed
            }
            _ if pipeline_execution_ready == Some(true) => {
                WorkItemReconcileAction::AwaitingPipelineExecution
            }
            _ => WorkItemReconcileAction::AwaitingPipelineExecutionAuthorization,
        },
        _ => WorkItemReconcileAction::PipelineIntentBlocked,
    }
}

pub(in crate::app) fn pipeline_intent_execution_state(
    intent: &StoredPipelineIntent,
) -> Option<&str> {
    intent
        .intent_json
        .pointer("/execution_state/state")
        .and_then(Value::as_str)
}

pub(in crate::app) fn pipeline_execution_attempt(intent_json: &Value) -> Result<u64, ApiError> {
    let attempt = intent_json
        .get("execution_attempt")
        .map(|value| {
            value.as_u64().ok_or_else(|| {
                ApiError::conflict("PipelineIntent execution_attempt must be a positive integer")
            })
        })
        .transpose()?
        .unwrap_or(1);
    if !(1..=MAX_PIPELINE_EXECUTION_ATTEMPTS).contains(&attempt) {
        return Err(ApiError::conflict(format!(
            "PipelineIntent execution_attempt must be between 1 and {MAX_PIPELINE_EXECUTION_ATTEMPTS}"
        )));
    }
    Ok(attempt)
}

pub(in crate::app) fn pipeline_build_output_is_verified(intent: &StoredPipelineIntent) -> bool {
    intent
        .intent_json
        .pointer("/build_output/status")
        .and_then(Value::as_str)
        == Some("verified")
}

pub(in crate::app) fn pipeline_evidence_is_satisfied(intent: &StoredPipelineIntent) -> bool {
    pipeline_intent_attached_evidence_status(intent) == Some("satisfied")
}

pub(in crate::app) fn pipeline_intent_is_gitops_update_eligible(
    intent: &StoredPipelineIntent,
) -> bool {
    pipeline_intent_is_deployment_eligible(&intent.status) && pipeline_evidence_is_satisfied(intent)
}

pub(in crate::app) fn pipeline_intent_requires_execution_preflight(
    intent: &StoredPipelineIntent,
) -> bool {
    intent.status == "approved" && pipeline_intent_execution_state(intent).is_none()
}

/// Deployment execution preflight validates immutable GitOps merge provenance.
/// Keep it out of earlier review stages so an approved DeploymentIntent can
/// still expose the proposed GitOps ChangeSet and its delivery actions.
pub(in crate::app) fn deployment_intent_requires_execution_preflight(
    gitops_repo: Option<&str>,
    gitops_ref: Option<&str>,
    intent_status: &str,
    gitops_merge_observed: bool,
) -> Result<bool, ApiError> {
    if intent_status != "approved" {
        return Ok(false);
    }
    match (gitops_repo, gitops_ref) {
        (None, None) => Ok(true),
        (Some(_), Some(_)) => Ok(gitops_merge_observed),
        _ => Err(ApiError::conflict(
            "WorkItem must declare both gitops_repo and gitops_ref before Argo execution",
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum GitOpsBaseRevisionReconcileState {
    Missing,
    Resolving,
    Resolved,
}

pub(in crate::app) async fn gitops_base_revision_reconcile_state(
    store: &SqliteStore,
    change_set: &StoredGitOpsChangeSet,
) -> Result<GitOpsBaseRevisionReconcileState, ApiError> {
    let artifacts = store.list_artifacts(&change_set.run_id).await?;
    if artifacts
        .iter()
        .any(|artifact| gitops_base_revision_matches_change_set(artifact, change_set))
    {
        return Ok(GitOpsBaseRevisionReconcileState::Resolved);
    }
    let latest_execution = artifacts
        .iter()
        .filter(|artifact| {
            artifact.kind == "gitops_base_revision_execution"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("gitops_change_set_id").and_then(Value::as_str)
                        == Some(change_set.id.as_str())
                        && content.get("material_hash").and_then(Value::as_str)
                            == Some(change_set.material_hash.as_str())
                        && gitops_artifact_change_set_revision(content) == change_set.revision
                })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id));
    let resolving = latest_execution.is_some_and(|execution| {
        let execution_id = execution
            .content_json
            .as_ref()
            .and_then(|content| content.get("execution_id"))
            .and_then(Value::as_str);
        execution_id.is_some_and(|execution_id| {
            !artifacts.iter().any(|artifact| {
                artifact.kind == "gitops_base_revision"
                    && artifact.content_json.as_ref().is_some_and(|content| {
                        content.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                            && content.get("status").and_then(Value::as_str) == Some("failed")
                    })
            })
        })
    });
    Ok(if resolving {
        GitOpsBaseRevisionReconcileState::Resolving
    } else {
        GitOpsBaseRevisionReconcileState::Missing
    })
}

pub(in crate::app) fn gitops_change_set_reconcile_action(
    change_set: Option<&StoredGitOpsChangeSet>,
    delivery: Option<&GitOpsDeliveryFlowResponse>,
    base_revision: Option<GitOpsBaseRevisionReconcileState>,
) -> WorkItemReconcileAction {
    let Some(change_set) = change_set else {
        return WorkItemReconcileAction::AwaitingGitOpsUpdatePlan;
    };
    match change_set.status.as_str() {
        "proposed" => WorkItemReconcileAction::AwaitingGitOpsChangeSetApproval,
        "rejected" | "stale" => WorkItemReconcileAction::GitOpsChangeSetBlocked,
        "applied" => WorkItemReconcileAction::AwaitingDeploymentIntentReview,
        "approved" => match delivery {
            Some(delivery) => gitops_delivery_reconcile_action(delivery),
            None => match base_revision.unwrap_or(GitOpsBaseRevisionReconcileState::Missing) {
                GitOpsBaseRevisionReconcileState::Missing => {
                    WorkItemReconcileAction::AwaitingGitOpsBaseRevision
                }
                GitOpsBaseRevisionReconcileState::Resolving => {
                    WorkItemReconcileAction::WaitForGitOpsBaseRevision
                }
                GitOpsBaseRevisionReconcileState::Resolved => {
                    WorkItemReconcileAction::AwaitingGitOpsDeliveryPlan
                }
            },
        },
        _ => WorkItemReconcileAction::GitOpsChangeSetBlocked,
    }
}

pub(in crate::app) fn gitops_delivery_reconcile_action(
    delivery: &GitOpsDeliveryFlowResponse,
) -> WorkItemReconcileAction {
    if delivery.latest_merge.is_some() {
        return WorkItemReconcileAction::AwaitingDeploymentIntentReview;
    }
    if let Some(observation) = delivery.latest_observation.as_ref() {
        let status = observation
            .content_json
            .as_ref()
            .and_then(|content| content.get("status"))
            .and_then(Value::as_str);
        if status == Some("failed") {
            return WorkItemReconcileAction::AwaitingGitOpsPullRequestObservation;
        }
        if gitops_observation_closed_unmerged(observation.content_json.as_ref()) {
            return WorkItemReconcileAction::GitOpsDeliveryFailed;
        }
        let merged = observation
            .content_json
            .as_ref()
            .and_then(|content| content.get("merged"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        return if merged {
            WorkItemReconcileAction::AwaitingDeploymentIntentReview
        } else {
            WorkItemReconcileAction::AwaitingGitOpsPullRequestMerge
        };
    }
    if let Some(result) = delivery.latest_result.as_ref() {
        return match result
            .content_json
            .as_ref()
            .and_then(|content| content.get("status"))
            .and_then(Value::as_str)
        {
            Some("completed") => WorkItemReconcileAction::AwaitingGitOpsPullRequestObservation,
            Some("failed") | Some("dispatch_failed") => {
                WorkItemReconcileAction::GitOpsDeliveryFailed
            }
            _ => WorkItemReconcileAction::WaitForGitOpsDelivery,
        };
    }
    if delivery.latest_execution.is_some() {
        return WorkItemReconcileAction::WaitForGitOpsDelivery;
    }
    match delivery
        .latest_preflight
        .as_ref()
        .and_then(|artifact| artifact.content_json.as_ref())
        .and_then(|content| content.get("status"))
        .and_then(Value::as_str)
    {
        Some("ready_for_writer") => {
            let dispatch_ready = delivery
                .latest_preflight
                .as_ref()
                .and_then(|artifact| artifact.content_json.as_ref())
                .and_then(|content| content.get("dispatch_ready"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if dispatch_ready {
                WorkItemReconcileAction::AwaitingGitOpsDeliveryExecution
            } else {
                WorkItemReconcileAction::AwaitingGitOpsWriterAvailability
            }
        }
        _ => WorkItemReconcileAction::AwaitingGitOpsDeliveryAuthorization,
    }
}

pub(in crate::app) fn gitops_observation_closed_unmerged(content: Option<&Value>) -> bool {
    content.is_some_and(|content| {
        content.get("status").and_then(Value::as_str) == Some("observed")
            && content.get("pull_request_state").and_then(Value::as_str) == Some("closed")
            && content.get("merged").and_then(Value::as_bool) == Some(false)
    })
}

pub(in crate::app) fn gitops_observation_refreshable(content: Option<&Value>) -> bool {
    content.is_some_and(|content| {
        let status = content.get("status").and_then(Value::as_str);
        if status == Some("failed") {
            return true;
        }
        status == Some("observed")
            && content.get("merged").and_then(Value::as_bool) != Some(true)
            && content.get("pull_request_state").and_then(Value::as_str) != Some("closed")
    })
}

pub(in crate::app) fn deployment_intent_reconcile_action(
    intent: Option<&StoredDeploymentIntent>,
    preflight: Option<&DeploymentIntentExecutionPreflight>,
    dispatch_ready: Option<bool>,
    delivery: Option<&DeploymentIntentDeliveryFlowResponse>,
) -> WorkItemReconcileAction {
    let Some(intent) = intent else {
        return WorkItemReconcileAction::AwaitingDeploymentIntentDefinition;
    };
    match intent.status.as_str() {
        "proposed" => WorkItemReconcileAction::AwaitingDeploymentIntentReview,
        "rejected" | "stale" => WorkItemReconcileAction::DeploymentIntentBlocked,
        "approved" => {
            let Some(delivery) = delivery else {
                return WorkItemReconcileAction::AwaitingDeploymentAuthorization;
            };
            if let Some(result) = delivery.latest_result.as_ref() {
                return match result
                    .content_json
                    .as_ref()
                    .and_then(|content| content.get("status"))
                    .and_then(Value::as_str)
                {
                    Some("completed") => release_reconcile_action(delivery.release.as_ref()),
                    Some("failed") | Some("cancelled") | Some("dispatch_failed") => {
                        WorkItemReconcileAction::DeploymentExecutionFailed
                    }
                    _ => WorkItemReconcileAction::WaitForDeploymentExecution,
                };
            }
            if delivery.latest_execution.is_some() {
                return WorkItemReconcileAction::WaitForDeploymentExecution;
            }
            let Some(preflight) = preflight else {
                return WorkItemReconcileAction::AwaitingDeploymentAuthorization;
            };
            if !preflight.ready {
                return WorkItemReconcileAction::AwaitingDeploymentAuthorization;
            }
            if dispatch_ready == Some(true) {
                WorkItemReconcileAction::AwaitingDeploymentExecution
            } else {
                WorkItemReconcileAction::AwaitingArgoRunnerAvailability
            }
        }
        _ => WorkItemReconcileAction::DeploymentIntentBlocked,
    }
}

pub(in crate::app) fn release_reconcile_action(
    release: Option<&ReleaseResponse>,
) -> WorkItemReconcileAction {
    let Some(release) = release else {
        return WorkItemReconcileAction::AwaitingReleaseDefinition;
    };
    match release.status.as_str() {
        "proposed" => WorkItemReconcileAction::AwaitingReleaseApproval,
        "approved" => WorkItemReconcileAction::AwaitingReleaseVerification,
        "completed" => WorkItemReconcileAction::CompleteWorkItem,
        "rejected" | "stale" => WorkItemReconcileAction::ReleaseBlocked,
        _ => WorkItemReconcileAction::ReleaseBlocked,
    }
}

pub(in crate::app) fn git_delivery_reconcile_action(
    git_delivery: Option<&GitDeliveryFlowResponse>,
) -> WorkItemReconcileAction {
    let Some(git_delivery) = git_delivery else {
        return WorkItemReconcileAction::PrepareGitDelivery;
    };

    if git_delivery.latest_merge.is_some() {
        return WorkItemReconcileAction::AwaitingPipelineIntentDefinition;
    }

    if let Some(observation) = git_delivery.latest_observation.as_ref() {
        let observation_status = observation
            .content_json
            .as_ref()
            .and_then(|content| content.get("status"))
            .and_then(Value::as_str);
        if observation_status == Some("failed") {
            return WorkItemReconcileAction::AwaitingPullRequestObservation;
        }
        let merged = observation
            .content_json
            .as_ref()
            .and_then(|content| content.get("merged"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if merged {
            return WorkItemReconcileAction::AwaitingPipelineIntentDefinition;
        }
        return WorkItemReconcileAction::AwaitingPullRequestMerge;
    }

    if let Some(result) = git_delivery.latest_result.as_ref() {
        return match result
            .content_json
            .as_ref()
            .and_then(|content| content.get("status"))
            .and_then(Value::as_str)
        {
            Some("completed") => WorkItemReconcileAction::AwaitingPullRequestObservation,
            Some("failed") | Some("dispatch_failed") => WorkItemReconcileAction::GitDeliveryFailed,
            _ => WorkItemReconcileAction::WaitForGitDelivery,
        };
    }

    if git_delivery.latest_execution.is_some() {
        return WorkItemReconcileAction::WaitForGitDelivery;
    }

    match git_delivery
        .latest_preflight
        .as_ref()
        .and_then(|artifact| artifact.content_json.as_ref())
        .and_then(|content| content.get("status"))
        .and_then(Value::as_str)
    {
        Some("ready_for_writer") => {
            let dispatch_ready = git_delivery
                .latest_preflight
                .as_ref()
                .and_then(|artifact| artifact.content_json.as_ref())
                .and_then(|content| content.get("dispatch_ready"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if dispatch_ready {
                WorkItemReconcileAction::AwaitingGitDeliveryExecution
            } else {
                WorkItemReconcileAction::AwaitingGitWriterAvailability
            }
        }
        _ => WorkItemReconcileAction::AwaitingGitDeliveryAuthorization,
    }
}

pub(in crate::app) async fn git_delivery_preflight_response(
    store: &SqliteStore,
    git_delivery: Option<&GitDeliveryFlowResponse>,
) -> Result<Option<GitDeliveryPreflightResponse>, ApiError> {
    let Some(git_delivery) = git_delivery else {
        return Ok(None);
    };
    let Some(artifact) = git_delivery.latest_preflight.as_ref() else {
        return Ok(None);
    };
    let Some(content) = artifact.content_json.as_ref() else {
        return Ok(None);
    };
    let Some(status) = content.get("status").and_then(Value::as_str) else {
        return Ok(None);
    };
    let permission_grant = match content
        .get("permission_grant_id")
        .and_then(Value::as_str)
        .filter(|grant_id| !grant_id.is_empty())
    {
        Some(grant_id) => store.get_permission_grant(grant_id).await?.map(Into::into),
        None => None,
    };
    let checks = content
        .get("checks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    Ok(Some(GitDeliveryPreflightResponse {
        status: status.to_string(),
        approval_gate_ready: content
            .get("approval_gate_ready")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        authorization_ready: content
            .get("authorization_ready")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        dispatch_ready: content
            .get("dispatch_ready")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        plan: git_delivery.plan.clone(),
        permission_grant,
        checks,
        artifact: artifact.clone(),
        created: false,
    }))
}

pub(in crate::app) async fn gitops_delivery_preflight_response(
    store: &SqliteStore,
    gitops_delivery: Option<&GitOpsDeliveryFlowResponse>,
) -> Result<Option<GitOpsDeliveryPreflightResponse>, ApiError> {
    let Some(gitops_delivery) = gitops_delivery else {
        return Ok(None);
    };
    let Some(artifact) = gitops_delivery.latest_preflight.as_ref() else {
        return Ok(None);
    };
    let Some(content) = artifact.content_json.as_ref() else {
        return Ok(None);
    };
    let Some(status) = content.get("status").and_then(Value::as_str) else {
        return Ok(None);
    };
    let permission_grant = match content
        .get("permission_grant_id")
        .and_then(Value::as_str)
        .filter(|grant_id| !grant_id.is_empty())
    {
        Some(grant_id) => store.get_permission_grant(grant_id).await?.map(Into::into),
        None => None,
    };
    let checks = content
        .get("checks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(Some(GitOpsDeliveryPreflightResponse {
        status: status.to_string(),
        approval_gate_ready: content
            .get("approval_gate_ready")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        authorization_ready: content
            .get("authorization_ready")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        dispatch_ready: content
            .get("dispatch_ready")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        plan: gitops_delivery.plan.clone(),
        base_revision: gitops_delivery.base_revision.clone(),
        permission_grant,
        checks,
        artifact: artifact.clone(),
        created: false,
    }))
}

pub(in crate::app) fn deployment_intent_execution_preflight_response(
    preflight: DeploymentIntentExecutionPreflight,
    dispatch_ready: Option<bool>,
) -> DeploymentIntentPreflightResponse {
    let ready_for_argo_runner = preflight.ready;
    DeploymentIntentPreflightResponse {
        status: if ready_for_argo_runner {
            "ready_for_argo_runner"
        } else {
            "blocked"
        }
        .to_string(),
        ready_for_argo_runner,
        dispatch_ready: dispatch_ready.unwrap_or(false),
        deployment_intent: preflight.intent.into(),
        deployment_contract: preflight.contract.map(Into::into),
        permission_grant: preflight.grant.map(Into::into),
        checks: preflight.checks,
    }
}

pub(in crate::app) async fn reconcile_work_item_response(
    state: &AppState,
    work_item_id: &str,
    action: WorkItemReconcileAction,
    applied: bool,
    git_delivery_preflight: Option<GitDeliveryPreflightResponse>,
    message: String,
) -> Result<ReconcileWorkItemResponse, ApiError> {
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let work_plan = state.store.get_work_plan_by_work_item(work_item_id).await?;
    let controller_wait = state
        .store
        .get_active_controller_wait_for_work_item(work_item_id)
        .await?;
    let change_set = match &work_plan {
        Some(work_plan) => {
            state
                .store
                .get_change_set_by_work_plan(&work_plan.id)
                .await?
        }
        None => None,
    };
    let workspace = state
        .store
        .list_workspaces(WorkspaceListFilter {
            work_item_id: Some(work_item_id.to_string()),
            limit: 50,
            ..WorkspaceListFilter::default()
        })
        .await?
        .into_iter()
        .max_by(|left, right| {
            left.updated_at
                .cmp(&right.updated_at)
                .then_with(|| left.id.cmp(&right.id))
        });
    let run = match &work_item.current_run_id {
        Some(run_id) => state.store.get_run(run_id).await?,
        None => None,
    };
    let pipeline_intent = match change_set.as_ref() {
        Some(change_set) => {
            state
                .store
                .get_pipeline_intent_by_change_set(&change_set.id)
                .await?
        }
        None => None,
    };
    let pipeline_execution_preflight = match pipeline_intent
        .as_ref()
        .filter(|intent| pipeline_intent_requires_execution_preflight(intent))
    {
        Some(intent) => Some(pipeline_intent_execution_preflight(state, &intent.id).await?),
        None => None,
    };
    let deployment_intent = match pipeline_intent.as_ref() {
        Some(intent) => {
            state
                .store
                .get_deployment_intent_by_pipeline_intent(&intent.id)
                .await?
        }
        None => None,
    };
    let gitops_change_set = match pipeline_intent.as_ref() {
        Some(intent) => {
            state
                .store
                .get_gitops_change_set_by_pipeline_intent(&intent.id)
                .await?
        }
        None => None,
    };
    let gitops_delivery = gitops_delivery_flow(&state.store, gitops_change_set.as_ref()).await?;
    let gitops_merge_observed = gitops_delivery
        .as_ref()
        .and_then(|delivery| delivery.latest_merge.as_ref())
        .is_some();
    let deployment_execution_preflight = match deployment_intent.as_ref() {
        Some(intent)
            if deployment_intent_requires_execution_preflight(
                work_item.gitops_repo.as_deref(),
                work_item.gitops_ref.as_deref(),
                &intent.status,
                gitops_merge_observed,
            )? =>
        {
            Some(deployment_intent_execution_preflight(state, &intent.id).await?)
        }
        _ => None,
    };
    let deployment_dispatch_ready = deployment_execution_preflight.as_ref().map(|preflight| {
        state.worker.argo_executor_available()
            && deployment_target(&preflight.intent)
                .ok()
                .is_some_and(|target| {
                    state
                        .worker
                        .argo_executor_allows_application(&target.application)
                })
    });
    let deployment_delivery =
        deployment_intent_delivery_flow(&state.store, deployment_intent.as_ref()).await?;
    let gitops_delivery_preflight =
        gitops_delivery_preflight_response(&state.store, gitops_delivery.as_ref()).await?;

    let can_apply = action.is_applyable() && controller_wait.is_none();
    let mut blockers = Vec::new();
    if let Some(wait) = &controller_wait {
        blockers.push(ReconcileBlockerResponse {
            code: "controller_wait".to_string(),
            summary: format!(
                "{} is active for {} and must resolve or expire before another controller action can run",
                wait.wait_kind, wait.subject_id
            ),
        });
    } else if !action.is_applyable() {
        blockers.push(ReconcileBlockerResponse {
            code: action.as_str().to_string(),
            summary: message.clone(),
        });
    }
    let authorization_checks =
        if action == WorkItemReconcileAction::AwaitingPipelineExecutionAuthorization {
            pipeline_execution_authorization_checks(pipeline_execution_preflight.as_ref())
        } else {
            reconcile_authorization_checks(action)
        };
    let effect_summary = if can_apply {
        format!(
            "Applying this controller action will {}",
            action_effect(action)
        )
    } else {
        message.clone()
    };

    Ok(ReconcileWorkItemResponse {
        action: action.as_str().to_string(),
        applied,
        work_item: work_item.into(),
        work_plan: work_plan.map(Into::into),
        workspace: workspace.map(Into::into),
        run: run.map(Into::into),
        change_set: change_set.map(Into::into),
        git_delivery_preflight,
        pipeline_intent: pipeline_intent.map(Into::into),
        pipeline_execution_preflight: pipeline_execution_preflight
            .map(pipeline_execution_preflight_response),
        deployment_intent: deployment_intent.map(Into::into),
        deployment_execution_preflight: deployment_execution_preflight.map(|preflight| {
            deployment_intent_execution_preflight_response(preflight, deployment_dispatch_ready)
        }),
        deployment_delivery,
        gitops_change_set: gitops_change_set.map(Into::into),
        gitops_delivery,
        gitops_delivery_preflight,
        controller_wait: controller_wait.map(Into::into),
        message,
        boundary: action.as_str().to_string(),
        can_apply,
        effect_summary,
        blockers,
        authorization_checks,
    })
}

pub(in crate::app) fn action_effect(action: WorkItemReconcileAction) -> &'static str {
    match action {
        WorkItemReconcileAction::DeclareWorkPlan => {
            "declare one deterministic WorkPlan and its ephemeral workspace"
        }
        WorkItemReconcileAction::StartCodingAttempt => {
            "dispatch one bounded model-backed coding attempt"
        }
        WorkItemReconcileAction::CaptureChangeSet => {
            "capture the completed workspace diff and test evidence as a proposed ChangeSet"
        }
        WorkItemReconcileAction::PrepareGitDelivery => {
            "prepare a review-only source Git delivery plan"
        }
        WorkItemReconcileAction::AwaitingGitDeliveryExecution => {
            "dispatch one isolated source branch-and-pull-request writer"
        }
        WorkItemReconcileAction::AwaitingPullRequestObservation => {
            "dispatch one read-only source pull-request observer"
        }
        WorkItemReconcileAction::AwaitingPipelineExecution => {
            "dispatch one isolated Tekton executor"
        }
        WorkItemReconcileAction::AwaitingGitOpsBaseRevision => {
            "dispatch one read-only GitOps base-revision observer"
        }
        WorkItemReconcileAction::PrepareRollbackIntent => {
            "observe the protected production baseline and prepare one digest-bound RollbackIntent without executing it"
        }
        WorkItemReconcileAction::AwaitingGitOpsDeliveryPlan => {
            "prepare one immutable, base-revision-bound GitOps delivery plan"
        }
        WorkItemReconcileAction::AwaitingGitOpsDeliveryExecution => {
            "dispatch one isolated GitOps branch-and-pull-request writer"
        }
        WorkItemReconcileAction::AwaitingGitOpsPullRequestObservation => {
            "dispatch one read-only GitOps pull-request observer"
        }
        WorkItemReconcileAction::AwaitingGitOpsPullRequestMerge => {
            "refresh the read-only GitOps pull-request observation to capture manual merge provenance"
        }
        WorkItemReconcileAction::AwaitingDeploymentExecution => {
            "dispatch one isolated Argo reconciliation runner"
        }
        WorkItemReconcileAction::AwaitingReleaseDefinition => {
            "create one proposed Release bound to the completed Argo sync and verified build digest"
        }
        WorkItemReconcileAction::AwaitingReleaseVerification => {
            "record the bounded release verification action"
        }
        WorkItemReconcileAction::CompleteWorkItem => "mark the verified WorkItem complete",
        _ => "perform the next bounded controller action",
    }
}

pub(in crate::app) fn reconcile_authorization_checks(
    action: WorkItemReconcileAction,
) -> Vec<ReconcileAuthorizationCheckResponse> {
    let authorization_missing = matches!(
        action,
        WorkItemReconcileAction::AwaitingGitDeliveryAuthorization
            | WorkItemReconcileAction::AwaitingPipelineExecutionAuthorization
            | WorkItemReconcileAction::AwaitingGitOpsDeliveryAuthorization
            | WorkItemReconcileAction::AwaitingDeploymentAuthorization
    );
    let executor_unavailable = matches!(
        action,
        WorkItemReconcileAction::AwaitingGitWriterAvailability
            | WorkItemReconcileAction::AwaitingGitOpsWriterAvailability
            | WorkItemReconcileAction::AwaitingArgoRunnerAvailability
    );
    vec![
        ReconcileAuthorizationCheckResponse {
            kind: "approval_gate".to_string(),
            status: if authorization_missing { "missing" } else { "not_required" }.to_string(),
            summary: if authorization_missing {
                "A matching approval gate must be satisfied before this controller boundary can advance."
            } else {
                "No approval gate decision is required for the current controller boundary."
            }
            .to_string(),
            resource_id: None,
        },
        ReconcileAuthorizationCheckResponse {
            kind: "permission_grant".to_string(),
            status: if authorization_missing { "missing" } else { "not_required" }.to_string(),
            summary: if authorization_missing {
                "A matching scoped PermissionGrant or trusted envelope is required."
            } else {
                "No scoped mutation grant is required for the current controller boundary."
            }
            .to_string(),
            resource_id: None,
        },
        ReconcileAuthorizationCheckResponse {
            kind: "executor_allowlist".to_string(),
            status: if executor_unavailable { "unavailable" } else { "not_required" }.to_string(),
            summary: if executor_unavailable {
                "The required dedicated executor is not configured for this exact target."
            } else {
                "No dedicated executor availability check is required for the current boundary."
            }
            .to_string(),
            resource_id: None,
        },
    ]
}

pub(in crate::app) fn pipeline_execution_authorization_checks(
    preflight: Option<&PipelineIntentExecutionPreflight>,
) -> Vec<ReconcileAuthorizationCheckResponse> {
    let Some(preflight) = preflight else {
        return vec![
            ReconcileAuthorizationCheckResponse {
                kind: "pipeline_contract".to_string(),
                status: "unavailable".to_string(),
                summary: "Pipeline execution preflight is unavailable.".to_string(),
                resource_id: None,
            },
            ReconcileAuthorizationCheckResponse {
                kind: "approval_gate".to_string(),
                status: "unavailable".to_string(),
                summary: "Pipeline gate state cannot be verified without preflight.".to_string(),
                resource_id: None,
            },
            ReconcileAuthorizationCheckResponse {
                kind: "permission_grant".to_string(),
                status: "unavailable".to_string(),
                summary: "Pipeline grant state cannot be verified without preflight.".to_string(),
                resource_id: None,
            },
        ];
    };
    let failed_summaries = |predicate: &dyn Fn(&str) -> bool| {
        preflight
            .checks
            .iter()
            .filter_map(|check| {
                let code = check.get("code").and_then(Value::as_str)?;
                (predicate(code) && check.get("passed").and_then(Value::as_bool) != Some(true))
                    .then(|| {
                        check
                            .get("summary")
                            .and_then(Value::as_str)
                            .unwrap_or("Preflight check failed")
                            .to_string()
                    })
            })
            .collect::<Vec<_>>()
    };
    let gate_failures = failed_summaries(&|code| code.starts_with("approval_gate_"));
    let contract_failures = failed_summaries(&|code| {
        !code.starts_with("approval_gate_") && code != "trusted_execution_envelope"
    });
    let grant_check = preflight.checks.iter().find(|check| {
        check.get("code").and_then(Value::as_str) == Some("trusted_execution_envelope")
    });
    let grant_ready = grant_check
        .and_then(|check| check.get("passed"))
        .and_then(Value::as_bool)
        == Some(true);
    vec![
        ReconcileAuthorizationCheckResponse {
            kind: "pipeline_contract".to_string(),
            status: if contract_failures.is_empty() {
                "ready"
            } else {
                "blocked"
            }
            .to_string(),
            summary: if contract_failures.is_empty() {
                "PipelineIntent, source provenance, and pinned PipelineContract checks passed."
                    .to_string()
            } else {
                contract_failures.join("; ")
            },
            resource_id: None,
        },
        ReconcileAuthorizationCheckResponse {
            kind: "approval_gate".to_string(),
            status: if gate_failures.is_empty() {
                "ready"
            } else {
                "missing"
            }
            .to_string(),
            summary: if gate_failures.is_empty() {
                "Required pipeline mutation and production-impact gates are satisfied or waived."
                    .to_string()
            } else {
                gate_failures.join("; ")
            },
            resource_id: None,
        },
        ReconcileAuthorizationCheckResponse {
            kind: "permission_grant".to_string(),
            status: if grant_ready { "ready" } else { "missing" }.to_string(),
            summary: grant_check
                .and_then(|check| check.get("summary"))
                .and_then(Value::as_str)
                .unwrap_or("Pipeline execution grant check is unavailable")
                .to_string(),
            resource_id: preflight.grant_id.clone(),
        },
    ]
}

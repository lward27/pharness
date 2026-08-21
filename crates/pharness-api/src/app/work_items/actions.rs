use super::super::*;

pub(in crate::app) async fn execute_work_item_action(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path((work_item_id, action_id)): Path<(String, String)>,
    Json(request): Json<ExecuteWorkItemActionRequest>,
) -> Result<Json<Value>, ApiError> {
    if request.reason.trim().is_empty() {
        return Err(ApiError::bad_request("action execution reason is required"));
    }
    if matches!(
        action_id.as_str(),
        "approve_rollback"
            | "execute_rollback_gitops_pr"
            | "approve_rollback_argo_sync"
            | "execute_rollback_argo_sync"
            | "observe_rollback_merge"
            | "observe_rollback_argo_sync"
    ) {
        let item = state
            .store
            .get_work_item(&work_item_id)
            .await?
            .ok_or_else(|| ApiError::not_found("work_item", &work_item_id))?;
        let rollback = latest_rollback_intent(&state, &item, None)
            .await?
            .ok_or_else(|| ApiError::conflict("WorkItem has no RollbackIntent action"))?;
        let rollback_id = rollback
            .pointer("/content/rollback_intent_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::conflict("RollbackIntent ID is unavailable"))?;
        let expected_action_id = match rollback.pointer("/content/status").and_then(Value::as_str) {
            Some("prepared") => "approve_rollback",
            Some("approved") => "execute_rollback_gitops_pr",
            Some("awaiting_manual_merge") => "observe_rollback_merge",
            Some("ready_for_argo_sync") => "approve_rollback_argo_sync",
            Some("argo_approved") => "execute_rollback_argo_sync",
            Some("argo_syncing") => "observe_rollback_argo_sync",
            _ => {
                return Err(ApiError::conflict(
                    "RollbackIntent has no executable action at its current lifecycle state",
                ))
            }
        };
        let current_hash = format!("{:x}", Sha256::digest(rollback.to_string().as_bytes()));
        if action_id != expected_action_id || request.state_hash != current_hash {
            return Err(ApiError::conflict(
                "action preview is stale; reload the WorkItem flow before executing",
            ));
        }
        let expires_at = item
            .production_impacting
            .then(|| (current_millis() + 30 * 60 * 1_000).to_string());
        let response = match action_id.as_str() {
            "approve_rollback" | "approve_rollback_argo_sync" => {
                approve_rollback_intent(
                    State(state.clone()),
                    identity,
                    Path(rollback_id.to_string()),
                    Json(RollbackIntentRequest {
                        actor: request.actor,
                        reason: request.reason,
                        expires_at,
                    }),
                )
                .await?
                .0
            }
            "execute_rollback_gitops_pr" | "execute_rollback_argo_sync" => {
                execute_rollback_intent(State(state), Path(rollback_id.to_string()))
                    .await?
                    .0
            }
            "observe_rollback_merge" | "observe_rollback_argo_sync" => {
                observe_rollback_intent(State(state), Path(rollback_id.to_string()))
                    .await?
                    .0
            }
            _ => unreachable!(),
        };
        return Ok(Json(response));
    }
    let actor = identity
        .clone()
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor.clone()))
        .ok_or_else(|| ApiError::bad_request("action execution actor is required"))?;
    let Json(flow) = work_item_flow(State(state.clone()), Path(work_item_id.clone())).await?;
    if let Some(action) = flow
        .action_rail
        .iter()
        .find(|action| action.id == action_id)
    {
        if action.state_hash != request.state_hash {
            return Err(ApiError::conflict(
                "action preview is stale; reload the WorkItem flow before executing",
            ));
        }
        if action.status != "ready" {
            return Err(ApiError::conflict(format!(
                "action {} is blocked: {}",
                action.id,
                action
                    .blockers
                    .iter()
                    .map(|blocker| blocker.summary.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            )));
        }
        let transition_target = if action.id.starts_with("approve_") {
            "approved"
        } else {
            "rejected"
        };
        let value = match action.id.as_str() {
            "approve_work_plan" | "reject_work_plan" => serde_json::to_value(
                transition_work_plan(
                    State(state.clone()),
                    Path(action.resource.clone()),
                    Json(TransitionWorkPlanRequest {
                        target_status: transition_target.to_string(),
                        actor: Some(actor.clone()),
                        reason: Some(request.reason.clone()),
                    }),
                )
                .await?
                .0,
            ),
            "authorize_workspace_and_start" => serde_json::to_value(
                execute_work_item(
                    State(state.clone()),
                    identity.clone(),
                    Path(work_item_id.clone()),
                    Json(ExecuteWorkItemRequest {
                        actor: Some(actor.clone()),
                        reason: Some(request.reason.clone()),
                        max_turns: None,
                    }),
                )
                .await?
                .0,
            ),
            "replan_work_item" => serde_json::to_value(
                replan_work_item(
                    State(state.clone()),
                    identity.clone(),
                    Path(work_item_id.clone()),
                    Json(ReplanWorkItemRequest {
                        actor: Some(actor.clone()),
                        reason: request.reason.clone(),
                    }),
                )
                .await?
                .0,
            ),
            "approve_budget_extension" => {
                let extension = state
                    .store
                    .get_budget_extension(&action.resource)
                    .await?
                    .ok_or_else(|| ApiError::not_found("budget_extension", &action.resource))?;
                serde_json::to_value(
                    runs::approve_run_budget_extension(
                        State(state.clone()),
                        identity.clone(),
                        Path((extension.run_id.to_string(), extension.id)),
                        Json(ApproveBudgetExtensionRequest {
                            actor: actor.clone(),
                            reason: request.reason.clone(),
                            state_hash: request.state_hash.clone(),
                        }),
                    )
                    .await?
                    .0,
                )
            }
            action_id if action_id.starts_with("satisfy_approval_gate:") => {
                let gate_id = action_id
                    .strip_prefix("satisfy_approval_gate:")
                    .ok_or_else(|| ApiError::conflict("approval gate action is malformed"))?;
                serde_json::to_value(
                    decide_approval_gate(
                        state.clone(),
                        gate_id.to_string(),
                        "satisfied",
                        DecideApprovalGateRequest {
                            decided_by: Some(actor.clone()),
                            reason: Some(request.reason.clone()),
                        },
                    )
                    .await?
                    .0,
                )
            }
            "approve_change_set" | "reject_change_set" => serde_json::to_value(
                transition_change_set(
                    State(state.clone()),
                    Path(action.resource.clone()),
                    Json(TransitionChangeSetRequest {
                        target_status: transition_target.to_string(),
                        actor: Some(actor.clone()),
                        reason: Some(request.reason.clone()),
                    }),
                )
                .await?
                .0,
            ),
            "approve_pipeline_intent" | "reject_pipeline_intent" => serde_json::to_value(
                transition_pipeline_intent(
                    State(state.clone()),
                    Path(action.resource.clone()),
                    Json(TransitionPipelineIntentRequest {
                        target_status: transition_target.to_string(),
                        actor: Some(actor.clone()),
                        reason: Some(request.reason.clone()),
                    }),
                )
                .await?
                .0,
            ),
            "retry_pipeline_intent" => serde_json::to_value(
                retry_failed_pipeline_intent(
                    &state,
                    &action.resource,
                    actor.clone(),
                    request.reason.clone(),
                )
                .await?,
            ),
            "repropose_gitops_change_set" => serde_json::to_value(
                repropose_failed_gitops_change_set(
                    &state,
                    &action.resource,
                    actor.clone(),
                    request.reason.clone(),
                )
                .await?,
            ),
            "authorize_pipeline_execution" => serde_json::to_value(
                create_pipeline_intent_trusted_envelope(
                    State(state.clone()),
                    Path(action.resource.clone()),
                    Json(CreatePipelineIntentTrustedEnvelopeRequest {
                        subject: None,
                        created_by: Some(actor.clone()),
                        reason: request.reason.clone(),
                        expires_at: flow
                            .work_item
                            .production_impacting
                            .then(|| (current_millis() + 30 * 60 * 1_000).to_string()),
                    }),
                )
                .await?
                .0,
            ),
            "authorize_gitops_delivery" => {
                let expires_at = flow
                    .work_item
                    .production_impacting
                    .then(|| (current_millis() + 30 * 60 * 1_000).to_string());
                let Json(authorization) = authorize_gitops_change_set_delivery(
                    State(state.clone()),
                    identity.clone(),
                    Path(action.resource.clone()),
                    Json(CreateGitOpsDeliveryAuthorizationRequest {
                        subject: None,
                        created_by: Some(actor.clone()),
                        reason: request.reason.clone(),
                        expires_at,
                    }),
                )
                .await?;
                let Json(preflight) = preflight_gitops_change_set_delivery(
                    State(state.clone()),
                    identity.clone(),
                    Path(action.resource.clone()),
                    Json(GitOpsDeliveryPreflightRequest {
                        subject: None,
                        actor: Some(actor.clone()),
                        reason: Some(
                            "record readiness after the state-hashed GitOps writer authorization"
                                .to_string(),
                        ),
                    }),
                )
                .await?;
                serde_json::to_value(json!({
                    "authorization": authorization,
                    "preflight": preflight,
                }))
            }
            "approve_deployment_intent" | "reject_deployment_intent" => serde_json::to_value(
                transition_deployment_intent(
                    State(state.clone()),
                    Path(action.resource.clone()),
                    Json(TransitionDeploymentIntentRequest {
                        target_status: transition_target.to_string(),
                        actor: Some(actor.clone()),
                        reason: Some(request.reason.clone()),
                    }),
                )
                .await?
                .0,
            ),
            "authorize_deployment_execution" => serde_json::to_value(
                create_deployment_intent_trusted_envelope(
                    State(state.clone()),
                    Path(action.resource.clone()),
                    Json(CreateDeploymentIntentTrustedEnvelopeRequest {
                        subject: None,
                        created_by: Some(actor.clone()),
                        reason: request.reason.clone(),
                        expires_at: flow
                            .work_item
                            .production_impacting
                            .then(|| (current_millis() + 30 * 60 * 1_000).to_string()),
                    }),
                )
                .await?
                .0,
            ),
            "approve_gitops_change_set" | "reject_gitops_change_set" => serde_json::to_value(
                transition_gitops_change_set(
                    State(state.clone()),
                    Path(action.resource.clone()),
                    Json(TransitionGitOpsChangeSetRequest {
                        target_status: transition_target.to_string(),
                        actor: Some(actor.clone()),
                        reason: Some(request.reason.clone()),
                    }),
                )
                .await?
                .0,
            ),
            "approve_release" | "reject_release" => serde_json::to_value(
                transition_release(
                    State(state.clone()),
                    Path(action.resource.clone()),
                    Json(TransitionReleaseRequest {
                        target_status: transition_target.to_string(),
                        actor: Some(actor.clone()),
                        reason: Some(request.reason.clone()),
                    }),
                )
                .await?
                .0,
            ),
            _ => {
                // The controller-derived reconcile action below remains the
                // compatibility path for non-review lifecycle actions.
                serde_json::to_value(Value::Null)
            }
        }
        .map_err(|error| {
            ApiError::internal(format!("failed to serialize action result: {error}"))
        })?;
        if value != Value::Null {
            return Ok(Json(value));
        }
    }
    let Json(preview) = reconcile_work_item(
        State(state.clone()),
        identity.clone(),
        Path(work_item_id.clone()),
        Json(ReconcileWorkItemRequest {
            apply: false,
            actor: request.actor.clone(),
            reason: Some(request.reason.clone()),
            max_turns: None,
        }),
    )
    .await?;
    let action = work_item_action_response(&preview);
    if action.id != action_id || action.state_hash != request.state_hash {
        return Err(ApiError::conflict(
            "action preview is stale; reload the WorkItem flow before executing",
        ));
    }
    if !preview.can_apply {
        return Err(ApiError::conflict(format!(
            "action {} is blocked: {}",
            action.id, preview.message
        )));
    }
    let Json(response) = reconcile_work_item(
        State(state),
        identity,
        Path(work_item_id),
        Json(ReconcileWorkItemRequest {
            apply: true,
            actor: request.actor,
            reason: Some(request.reason),
            max_turns: None,
        }),
    )
    .await?;
    Ok(Json(serde_json::to_value(response).map_err(|error| {
        ApiError::internal(format!("failed to serialize reconcile response: {error}"))
    })?))
}

pub(in crate::app) async fn advance_work_item(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(work_item_id): Path<String>,
    Json(request): Json<AdvanceWorkItemRequest>,
) -> Result<Json<AdvanceWorkItemResponse>, ApiError> {
    if request.reason.trim().is_empty() {
        return Err(ApiError::bad_request("advance reason is required"));
    }
    let max_steps = request.max_steps.unwrap_or(10).clamp(1, 10);
    let mut steps = Vec::new();
    for _ in 0..max_steps {
        let Json(preview) = reconcile_work_item(
            State(state.clone()),
            identity.clone(),
            Path(work_item_id.clone()),
            Json(ReconcileWorkItemRequest {
                apply: false,
                actor: request.actor.clone(),
                reason: Some(request.reason.clone()),
                max_turns: None,
            }),
        )
        .await?;
        let safe_internal = matches!(
            preview.action.as_str(),
            "declare_work_plan"
                | "capture_change_set"
                | "prepare_git_delivery"
                | "awaiting_gitops_delivery_plan"
                | "awaiting_release_definition"
                | "complete_work_item"
        );
        if !safe_internal || !preview.can_apply {
            return Ok(Json(AdvanceWorkItemResponse {
                stopped_at: work_item_action_response(&preview),
                steps,
            }));
        }
        let Json(applied) = reconcile_work_item(
            State(state.clone()),
            identity.clone(),
            Path(work_item_id.clone()),
            Json(ReconcileWorkItemRequest {
                apply: true,
                actor: request.actor.clone(),
                reason: Some(request.reason.clone()),
                max_turns: None,
            }),
        )
        .await?;
        steps.push(applied);
    }
    let Json(preview) = reconcile_work_item(
        State(state),
        identity,
        Path(work_item_id),
        Json(ReconcileWorkItemRequest {
            apply: false,
            actor: request.actor,
            reason: Some(request.reason),
            max_turns: None,
        }),
    )
    .await?;
    Ok(Json(AdvanceWorkItemResponse {
        stopped_at: work_item_action_response(&preview),
        steps,
    }))
}

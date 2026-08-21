use super::super::approvals::grant_is_unexpired;
use super::super::audit::append_deployment_intent_audit_event;
use super::super::auth::OperatorIdentity;
use super::super::clock::{current_millis, unique_suffix};
use super::super::execution_checks::{
    argo_executor_poll_seconds, execution_check, normalized_executor_error_code,
};
use super::super::gitops::deployment_evidence::observed_gitops_merge_for_deployment;
use super::super::identifiers::{is_git_sha, is_sha256_digest};
use super::super::pipeline::readiness::ensure_pipeline_evidence_ready_for_deployment;
use super::super::principals::DEFAULT_ARGO_RUNNER_SUBJECT;
use super::super::risk::risk_rank;
use super::super::system::{immutable_image_digest, PROTECTED_ENVIRONMENT};
use super::super::validation::clean_optional_text;
use super::super::work_items::lifecycle::work_item_gate_scope_matches;
use super::super::work_items::rollback_state::latest_rollback_intent;
use super::super::{ApiError, AppState};
use super::contracts::{
    deployment_contract_spec, validate_deployment_contract_spec,
    validate_protected_production_deployment_contract,
};
use super::target::{deployment_target, ensure_supported_deployment_target, DeploymentTarget};
use crate::dispatch::ArgoSyncExecutionRequest;
use crate::dto::{
    ArgoSyncContextResponse, ArgoSyncControlResponse, ArgoSyncOutcomeRequest, ArtifactResponse,
    DeploymentIntentDeliveryFlowResponse, DeploymentIntentPreflightRequest,
    DeploymentIntentPreflightResponse, ExecuteDeploymentIntentRequest,
    ExecuteDeploymentIntentResponse,
};
use axum::extract::{Path, Query, State};
use axum::{Extension, Json};
use pharness_core::{
    CapabilityKind, PermissionGrantPolicy, PermissionGrantScope, PolicyMode, RiskLevel, RunId,
};
use pharness_store::{
    ApprovalGateListFilter, CreateArtifact, DeploymentContractListFilter, SqliteStore,
    StoredArtifact, StoredDeploymentContract, StoredDeploymentIntent, StoredPermissionGrant,
    StoredWorkItem, StoredWorkPlan,
};
use serde_json::{json, Value};

pub(in crate::app) async fn deployment_intent_delivery_flow(
    store: &SqliteStore,
    intent: Option<&StoredDeploymentIntent>,
) -> Result<Option<DeploymentIntentDeliveryFlowResponse>, ApiError> {
    let Some(intent) = intent else {
        return Ok(None);
    };
    let release = store
        .get_release_by_deployment_intent(&intent.id)
        .await?
        .map(Into::into);
    let Some(run_id) = intent.run_id.as_ref() else {
        return Ok(Some(DeploymentIntentDeliveryFlowResponse {
            latest_execution: None,
            latest_result: None,
            release,
        }));
    };
    let artifacts = store.list_artifacts(run_id).await?;
    let latest_execution = artifacts
        .iter()
        .filter(|artifact| {
            artifact.kind == "argo_sync_execution"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("deployment_intent_id").and_then(Value::as_str)
                        == Some(intent.id.as_str())
                })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id));
    let execution_id = latest_execution
        .and_then(|artifact| artifact.content_json.as_ref())
        .and_then(|content| content.get("execution_id"))
        .and_then(Value::as_str);
    let latest_result = execution_id.and_then(|execution_id| {
        artifacts
            .iter()
            .filter(|artifact| {
                artifact.kind == "argo_sync_result"
                    && artifact.content_json.as_ref().is_some_and(|content| {
                        content.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                    })
            })
            .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
    });

    Ok(Some(DeploymentIntentDeliveryFlowResponse {
        latest_execution: latest_execution.cloned().map(Into::into),
        latest_result: latest_result.cloned().map(Into::into),
        release,
    }))
}

pub(in crate::app) struct DeploymentIntentExecutionPreflight {
    pub(in crate::app) ready: bool,
    pub(in crate::app) intent: StoredDeploymentIntent,
    pub(in crate::app) contract: Option<StoredDeploymentContract>,
    pub(in crate::app) grant: Option<StoredPermissionGrant>,
    pub(in crate::app) gitops_merge: Option<ArtifactResponse>,
    pub(in crate::app) checks: Vec<Value>,
}

pub(in crate::app) async fn deployment_intent_execution_preflight(
    state: &AppState,
    deployment_intent_id: &str,
) -> Result<DeploymentIntentExecutionPreflight, ApiError> {
    let intent = state
        .store
        .get_deployment_intent(deployment_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("deployment_intent", deployment_intent_id))?;
    let pipeline_intent = state
        .store
        .get_pipeline_intent(&intent.pipeline_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_intent", &intent.pipeline_intent_id))?;
    let change_set = state
        .store
        .get_change_set(&intent.change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", &intent.change_set_id))?;
    let work_plan = state
        .store
        .get_work_plan(&intent.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &intent.work_plan_id))?;
    let work_item = match work_plan.work_item_id.as_deref() {
        Some(work_item_id) => Some(
            state
                .store
                .get_work_item(work_item_id)
                .await?
                .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?,
        ),
        None => None,
    };
    let target = deployment_target(&intent).ok();
    let pipeline_evidence = ensure_pipeline_evidence_ready_for_deployment(&pipeline_intent);
    let mut checks = vec![
        execution_check(
            "deployment_intent_approved",
            intent.status == "approved",
            format!("DeploymentIntent status is {}", intent.status),
        ),
        execution_check(
            "pipeline_intent_approved",
            pipeline_intent.status == "approved",
            format!("PipelineIntent status is {}", pipeline_intent.status),
        ),
        execution_check(
            "change_set_approved",
            change_set.status == "approved",
            format!("ChangeSet status is {}", change_set.status),
        ),
        execution_check(
            "work_plan_approved",
            work_plan.status == "approved",
            format!("WorkPlan status is {}", work_plan.status),
        ),
        execution_check(
            "pipeline_evidence_ready",
            pipeline_evidence.is_ok(),
            pipeline_evidence
                .err()
                .map(|error| error.message)
                .unwrap_or_else(|| {
                    "PipelineRun evidence is satisfied and matches the executed PipelineRun"
                        .to_string()
                }),
        ),
    ];

    let development_target = work_item
        .as_ref()
        .zip(target.as_ref())
        .and_then(|(item, target)| ensure_supported_deployment_target(item, target).err());
    checks.push(execution_check(
        "supported_work_item_target",
        work_item.is_some() && target.is_some() && development_target.is_none(),
        match (work_item.as_ref(), target.as_ref(), development_target) {
            (None, _, _) => "Argo preflight requires a WorkItem-backed delivery chain".to_string(),
            (_, None, _) => {
                "DeploymentIntent needs target environment, namespace, and Argo application"
                    .to_string()
            }
            (_, _, Some(error)) => error.message,
            _ => {
                "DeploymentIntent exactly matches a supported dev or protected-production WorkItem target".to_string()
            }
        },
    ));
    if let Some(item) = work_item.as_ref().filter(|item| item.production_impacting) {
        let rollback_ready = latest_rollback_intent(state, item, None)
            .await?
            .is_some_and(|intent| {
                matches!(
                    intent.pointer("/content/status").and_then(Value::as_str),
                    Some("prepared" | "approved")
                ) && intent
                    .pointer("/content/baseline/image_digest")
                    .and_then(Value::as_str)
                    .is_some_and(immutable_image_digest)
            });
        checks.push(execution_check(
            "production_baseline_and_rollback",
            rollback_ready,
            if rollback_ready {
                "Production baseline and digest-bound RollbackIntent are present".to_string()
            } else {
                "Production Argo execution requires a captured baseline and prepared RollbackIntent"
                    .to_string()
            },
        ));
    }

    let gitops_merge = match work_item.as_ref() {
        Some(work_item) => {
            match observed_gitops_merge_for_deployment(&state.store, work_item, &pipeline_intent)
                .await
            {
                Ok(Some(merge)) => {
                    let merge_sha = merge
                        .content_json
                        .as_ref()
                        .and_then(|content| content.get("merge_commit_sha"))
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    checks.push(execution_check(
                        "gitops_revision_merged",
                        true,
                        format!(
                            "GitOps merge artifact {} records immutable revision {}",
                            merge.id, merge_sha
                        ),
                    ));
                    Some(merge)
                }
                Ok(None) => {
                    checks.push(execution_check(
                    "gitops_revision_merged",
                    true,
                    "WorkItem does not declare a GitOps repository/ref; no GitOps merge is required",
                ));
                    None
                }
                Err(error) => {
                    checks.push(execution_check(
                        "gitops_revision_merged",
                        false,
                        error.message,
                    ));
                    None
                }
            }
        }
        None => {
            checks.push(execution_check(
                "gitops_revision_merged",
                false,
                "Argo preflight requires a WorkItem-backed delivery chain",
            ));
            None
        }
    };

    let contracts = if let Some(target) = target.as_ref() {
        state
            .store
            .list_deployment_contracts(DeploymentContractListFilter {
                target_environment: Some(target.environment.clone()),
                target_namespace: Some(target.namespace.clone()),
                argo_application: Some(target.application.clone()),
                status: Some("active".to_string()),
                limit: 10,
                ..DeploymentContractListFilter::default()
            })
            .await?
    } else {
        Vec::new()
    };
    let contract = match contracts.as_slice() {
        [contract] => match deployment_contract_spec(&contract.contract_json).and_then(|spec| {
            validate_deployment_contract_spec(&spec)?;
            if contract.target_environment == PROTECTED_ENVIRONMENT {
                validate_protected_production_deployment_contract(&spec)?;
            }
            Ok(())
        }) {
            Ok(()) => {
                checks.push(execution_check(
                    "active_deployment_contract",
                    true,
                    format!(
                        "Active DeploymentContract {} version {} exactly matches target",
                        contract.id, contract.version
                    ),
                ));
                Some(contract.clone())
            }
            Err(error) => {
                checks.push(execution_check(
                    "active_deployment_contract",
                    false,
                    error.message,
                ));
                None
            }
        },
        [] => {
            checks.push(execution_check(
                "active_deployment_contract",
                false,
                "No active DeploymentContract exactly matches the deployment target",
            ));
            None
        }
        _ => {
            checks.push(execution_check(
                "active_deployment_contract",
                false,
                "Multiple active DeploymentContracts match the target; retire the older contract",
            ));
            None
        }
    };

    let deployment_gate_kinds: &[&str] = if work_item
        .as_ref()
        .is_some_and(|item| item.production_impacting)
    {
        &[
            "cluster_mutation",
            "production_impact",
            "production_deployment",
        ]
    } else {
        &["cluster_mutation"]
    };
    for gate_kind in deployment_gate_kinds {
        let matching_gate = match work_item.as_ref() {
            Some(work_item) => state
                .store
                .list_approval_gates(ApprovalGateListFilter {
                    work_item_id: Some(work_item.id.clone()),
                    gate_kind: Some((*gate_kind).to_string()),
                    limit: 20,
                    ..ApprovalGateListFilter::default()
                })
                .await?
                .into_iter()
                .find(|gate| work_item_gate_scope_matches(gate, work_item, &work_plan, gate_kind)),
            None => None,
        };
        let approval_gate_ready = matching_gate
            .as_ref()
            .is_some_and(|gate| matches!(gate.status.as_str(), "satisfied" | "waived"));
        checks.push(execution_check(
            format!("approval_gate_{gate_kind}"),
            approval_gate_ready,
            matching_gate
                .as_ref()
                .map(|gate| format!("Scoped {gate_kind} gate {} is {}", gate.id, gate.status))
                .unwrap_or_else(|| format!("Required scoped WorkItem {gate_kind} gate is missing")),
        ));
    }

    let grant = match (target.as_ref(), work_item.as_ref()) {
        (Some(target), Some(work_item)) => {
            matching_deployment_execution_grant(
                &state.store,
                &intent,
                &work_plan,
                work_item,
                target,
            )
            .await?
        }
        _ => None,
    };
    checks.push(execution_check(
        "trusted_execution_envelope",
        grant.is_some(),
        grant
            .as_ref()
            .map(|grant| {
                format!(
                    "Active supervised-autonomy grant {} matches the DeploymentIntent",
                    grant.id
                )
            })
            .unwrap_or_else(|| {
                "No active supervised-autonomy grant matches this DeploymentIntent".to_string()
            }),
    ));

    let ready = checks
        .iter()
        .all(|check| check.get("passed").and_then(Value::as_bool) == Some(true));
    Ok(DeploymentIntentExecutionPreflight {
        ready,
        intent,
        contract,
        grant,
        gitops_merge,
        checks,
    })
}

/// Return immutable GitOps merge evidence when the WorkItem declares a GitOps
/// source of truth. A missing target intentionally stays compatible with the
/// existing non-GitOps dev delivery path; a partially declared or unmerged
/// target blocks Argo execution.
pub(in crate::app) async fn matching_deployment_execution_grant(
    store: &SqliteStore,
    intent: &StoredDeploymentIntent,
    work_plan: &StoredWorkPlan,
    work_item: &StoredWorkItem,
    target: &DeploymentTarget,
) -> Result<Option<StoredPermissionGrant>, ApiError> {
    let now = current_millis();
    let production_binding = if work_item.production_impacting {
        let pipeline_intent = store
            .get_pipeline_intent(&intent.pipeline_intent_id)
            .await?
            .ok_or_else(|| ApiError::not_found("pipeline_intent", &intent.pipeline_intent_id))?;
        let source_merge_sha = pipeline_intent
            .intent_json
            .pointer("/source_provenance/merge_commit_sha")
            .and_then(Value::as_str)
            .filter(|value| is_git_sha(value))
            .ok_or_else(|| ApiError::conflict("source merge provenance is unavailable"))?
            .to_string();
        let image_digest = pipeline_intent
            .intent_json
            .pointer("/build_output/image_digest")
            .and_then(Value::as_str)
            .filter(|value| is_sha256_digest(value))
            .ok_or_else(|| ApiError::conflict("build image digest provenance is unavailable"))?
            .to_string();
        let gitops_merge = observed_gitops_merge_for_deployment(store, work_item, &pipeline_intent)
            .await?
            .ok_or_else(|| ApiError::conflict("GitOps merge provenance is unavailable"))?;
        let gitops_merge_sha = gitops_merge
            .content_json
            .as_ref()
            .and_then(|content| content.get("merge_commit_sha"))
            .and_then(Value::as_str)
            .filter(|value| is_git_sha(value))
            .ok_or_else(|| ApiError::conflict("GitOps merge provenance is malformed"))?
            .to_string();
        Some((source_merge_sha, gitops_merge_sha, image_digest))
    } else {
        None
    };
    for grant in store.list_permission_grants(Some("active"), 200).await? {
        if !grant_is_unexpired(&grant, now) {
            continue;
        }
        let scope = serde_json::from_value::<PermissionGrantScope>(grant.scope_json.clone())
            .map_err(|error| {
                ApiError::internal(format!(
                    "permission grant {} has invalid scope: {error}",
                    grant.id
                ))
            })?;
        let grant_policy = serde_json::from_value::<PermissionGrantPolicy>(
            grant.policy_json.clone(),
        )
        .map_err(|error| {
            ApiError::internal(format!(
                "permission grant {} has invalid policy: {error}",
                grant.id
            ))
        })?;
        let production_binding_matches = match production_binding.as_ref() {
            Some((source_merge_sha, gitops_merge_sha, image_digest)) => {
                scope.work_item_ids == [work_item.id.clone()]
                    && scope.pipeline_contract_ids
                        == work_item
                            .pipeline_contract_id
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>()
                    && scope.deployment_contract_ids
                        == work_item
                            .deployment_contract_id
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>()
                    && scope.source_merge_shas == [source_merge_sha.clone()]
                    && scope.gitops_merge_shas == [gitops_merge_sha.clone()]
                    && scope.image_digests == [image_digest.clone()]
            }
            None => true,
        };
        let matches = grant.subject == DEFAULT_ARGO_RUNNER_SUBJECT
            && scope.environment.as_deref() == Some(target.environment.as_str())
            && grant_policy.policy_mode == PolicyMode::SupervisedAutonomy
            && scope.capability_kinds.contains(&CapabilityKind::ArgoSync)
            && scope.actions.iter().any(|action| action == "argocd_sync")
            && scope
                .max_risk
                .is_some_and(|risk| risk_rank(risk) >= risk_rank(RiskLevel::High))
            && scope
                .namespaces
                .iter()
                .any(|namespace| namespace == &target.namespace)
            && scope.work_plan_ids.iter().any(|id| id == &work_plan.id)
            && scope
                .change_set_ids
                .iter()
                .any(|id| id == &intent.change_set_id)
            && scope
                .pipeline_intent_ids
                .iter()
                .any(|id| id == &intent.pipeline_intent_id)
            && scope
                .deployment_intent_ids
                .iter()
                .any(|id| id == &intent.id)
            && scope
                .argo_applications
                .iter()
                .any(|application| application == &target.application)
            && scope.production_impacting == Some(work_item.production_impacting)
            && production_binding_matches;
        if matches {
            return Ok(Some(grant));
        }
    }
    Ok(None)
}

pub(in crate::app) async fn preflight_deployment_intent(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(deployment_intent_id): Path<String>,
    Json(request): Json<DeploymentIntentPreflightRequest>,
) -> Result<Json<DeploymentIntentPreflightResponse>, ApiError> {
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = clean_optional_text(request.reason);
    let preflight = deployment_intent_execution_preflight(&state, &deployment_intent_id).await?;
    let dispatch_ready = state.worker.argo_executor_available()
        && deployment_target(&preflight.intent)
            .ok()
            .is_some_and(|target| {
                state
                    .worker
                    .argo_executor_allows_application(&target.application)
            });
    append_deployment_intent_audit_event(
        &state.store,
        &preflight.intent,
        "deployment_intent.preflighted",
        actor,
        reason,
        json!({
            "ready_for_argo_runner": preflight.ready,
            "dispatch_ready": dispatch_ready,
            "deployment_contract_id": preflight.contract.as_ref().map(|contract| &contract.id),
            "permission_grant_id": preflight.grant.as_ref().map(|grant| &grant.id),
            "gitops_delivery_merge_artifact_id": preflight.gitops_merge.as_ref().map(|artifact| &artifact.id),
            "checks": preflight.checks,
        }),
    )
    .await?;

    Ok(Json(DeploymentIntentPreflightResponse {
        status: if preflight.ready {
            "ready_for_argo_runner"
        } else {
            "blocked"
        }
        .to_string(),
        ready_for_argo_runner: preflight.ready,
        dispatch_ready,
        deployment_intent: preflight.intent.into(),
        deployment_contract: preflight.contract.map(Into::into),
        permission_grant: preflight.grant.map(Into::into),
        checks: preflight.checks,
    }))
}

pub(in crate::app) async fn execute_deployment_intent(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(deployment_intent_id): Path<String>,
    Json(request): Json<ExecuteDeploymentIntentRequest>,
) -> Result<Json<ExecuteDeploymentIntentResponse>, ApiError> {
    let actor = identity
        .as_ref()
        .map(|Extension(OperatorIdentity(name))| name.clone())
        .or_else(|| clean_optional_text(request.actor.clone()));
    let preflight = deployment_intent_execution_preflight(&state, &deployment_intent_id).await?;
    let target = deployment_target(&preflight.intent)?;
    let gitops_merge = preflight.gitops_merge.clone();
    let dispatch_ready = state.worker.argo_executor_available()
        && state
            .worker
            .argo_executor_allows_application(&target.application);
    let response_status = if preflight.ready && dispatch_ready {
        "ready"
    } else {
        "blocked"
    };
    if request.dry_run || !preflight.ready || !dispatch_ready {
        return Ok(Json(ExecuteDeploymentIntentResponse {
            status: response_status.to_string(),
            ready: preflight.ready && dispatch_ready,
            dry_run: request.dry_run,
            deployment_intent: preflight.intent.into(),
            deployment_contract: preflight.contract.map(Into::into),
            permission_grant: preflight.grant.map(Into::into),
            checks: preflight.checks,
            execution: None,
            execution_id: None,
            executor_job_name: None,
            created: false,
        }));
    }

    let reason = clean_optional_text(request.reason)
        .ok_or_else(|| ApiError::bad_request("Argo sync execution reason is required"))?;
    let intent = preflight.intent;
    let contract = preflight
        .contract
        .ok_or_else(|| ApiError::internal("ready Argo preflight omitted deployment contract"))?;
    let grant = preflight
        .grant
        .ok_or_else(|| ApiError::internal("ready Argo preflight omitted permission grant"))?;
    let run_id = intent
        .run_id
        .clone()
        .ok_or_else(|| ApiError::conflict("DeploymentIntent has no coding run provenance"))?;
    let artifacts = state.store.list_artifacts(&run_id).await?;
    if let Some(existing) = artifacts.iter().find(|artifact| {
        argo_sync_execution_matches(artifact, &intent, &contract, &grant, gitops_merge.as_ref())
    }) {
        let execution_id = existing
            .content_json
            .as_ref()
            .and_then(|value| value.get("execution_id"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let status = execution_id
            .as_deref()
            .and_then(|execution_id| {
                artifacts.iter().find_map(|artifact| {
                    (artifact.kind == "argo_sync_result")
                        .then_some(artifact.content_json.as_ref())
                        .flatten()
                        .filter(|content| {
                            content.get("execution_id").and_then(Value::as_str)
                                == Some(execution_id)
                        })
                        .and_then(|content| content.get("status").and_then(Value::as_str))
                })
            })
            .unwrap_or("dispatched")
            .to_string();
        return Ok(Json(ExecuteDeploymentIntentResponse {
            status,
            ready: true,
            dry_run: false,
            deployment_intent: intent.into(),
            deployment_contract: Some(contract.into()),
            permission_grant: Some(grant.into()),
            checks: preflight.checks,
            execution: Some(existing.clone().into()),
            execution_id,
            executor_job_name: None,
            created: false,
        }));
    }

    let execution_id = format!("aexec_{}", unique_suffix());
    let execution = state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_argo_sync_execution", unique_suffix()),
            session_id: intent.session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "argo_sync_execution".to_string(),
            label: format!("Argo sync execution for DeploymentIntent {}", intent.id),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "execution_id": execution_id,
                "status": "dispatched",
                "deployment_intent_id": intent.id,
                "deployment_contract_id": contract.id,
                "permission_grant_id": grant.id,
                "gitops_delivery_merge_artifact_id": gitops_merge.as_ref().map(|artifact| &artifact.id),
                "gitops_merge_commit_sha": gitops_merge.as_ref().and_then(|artifact| artifact.content_json.as_ref()).and_then(|content| content.get("merge_commit_sha")).and_then(Value::as_str),
                "target": {
                    "environment": target.environment,
                    "namespace": target.namespace,
                    "argo_application": target.application,
                },
                "dispatched_by": actor,
                "reason": reason,
            })),
        })
        .await?;

    match state
        .worker
        .dispatch_argo_sync_execution(ArgoSyncExecutionRequest {
            deployment_intent_id: intent.id.clone(),
            execution_id: execution_id.clone(),
        })
        .await
    {
        Ok(receipt) => {
            let dispatch = state
                .store
                .create_artifact(CreateArtifact {
                    id: format!("art_{}_argo_sync_dispatch", unique_suffix()),
                    session_id: intent.session_id.clone(),
                    run_id: Some(run_id),
                    kind: "argo_sync_dispatch".to_string(),
                    label: format!("Argo sync Job dispatch for DeploymentIntent {}", intent.id),
                    mime_type: Some("application/json".to_string()),
                    path: None,
                    content_text: None,
                    content_json: Some(json!({
                        "execution_id": execution_id,
                        "argo_sync_execution_artifact_id": execution.id,
                        "deployment_intent_id": intent.id,
                        "executor_job_name": receipt.job_name,
                    })),
                })
                .await?;
            append_deployment_intent_audit_event(
                &state.store,
                &intent,
                "deployment_intent.argo_sync_dispatched",
                actor,
                Some(reason),
                json!({
                    "execution_id": execution_id,
                    "execution_artifact_id": execution.id,
                    "dispatch_artifact_id": dispatch.id,
                    "executor_job_name": receipt.job_name,
                    "deployment_contract_id": contract.id,
                    "permission_grant_id": grant.id,
                    "gitops_delivery_merge_artifact_id": gitops_merge.as_ref().map(|artifact| &artifact.id),
                }),
            )
            .await?;
            Ok(Json(ExecuteDeploymentIntentResponse {
                status: "dispatched".to_string(),
                ready: true,
                dry_run: false,
                deployment_intent: intent.into(),
                deployment_contract: Some(contract.into()),
                permission_grant: Some(grant.into()),
                checks: preflight.checks,
                execution: Some(execution.into()),
                execution_id: Some(execution_id),
                executor_job_name: Some(receipt.job_name),
                created: true,
            }))
        }
        Err(error) => {
            tracing::warn!(deployment_intent_id = %intent.id, %error, "Argo executor dispatch failed");
            let result = persist_argo_sync_result(
                &state.store,
                &intent,
                &run_id,
                &execution_id,
                "dispatch_failed",
                json!({ "error_code": "job_dispatch_failed" }),
            )
            .await?;
            append_deployment_intent_audit_event(
                &state.store,
                &intent,
                "deployment_intent.argo_sync_dispatch_failed",
                None,
                None,
                json!({
                    "execution_id": execution_id,
                    "execution_artifact_id": execution.id,
                    "result_artifact_id": result.id,
                    "error_code": "job_dispatch_failed",
                    "gitops_delivery_merge_artifact_id": gitops_merge.as_ref().map(|artifact| &artifact.id),
                }),
            )
            .await?;
            Ok(Json(ExecuteDeploymentIntentResponse {
                status: "dispatch_failed".to_string(),
                ready: true,
                dry_run: false,
                deployment_intent: intent.into(),
                deployment_contract: Some(contract.into()),
                permission_grant: Some(grant.into()),
                checks: preflight.checks,
                execution: Some(execution.into()),
                execution_id: Some(execution_id),
                executor_job_name: None,
                created: true,
            }))
        }
    }
}

#[derive(Debug, serde::Deserialize)]
pub(in crate::app) struct InternalArgoSyncQuery {
    pub(in crate::app) execution_id: String,
}

pub(in crate::app) async fn internal_argo_sync_context(
    State(state): State<AppState>,
    Path(deployment_intent_id): Path<String>,
    Query(query): Query<InternalArgoSyncQuery>,
) -> Result<Json<ArgoSyncContextResponse>, ApiError> {
    let (intent, _run_id, execution) =
        current_argo_sync_execution(&state, &deployment_intent_id, &query.execution_id).await?;
    let preflight = deployment_intent_execution_preflight(&state, &intent.id).await?;
    let target = deployment_target(&preflight.intent)?;
    if !preflight.ready
        || !state
            .worker
            .argo_executor_allows_application(&target.application)
    {
        return Err(ApiError::conflict(
            "Argo sync context is no longer authorized or the executor is unavailable",
        ));
    }
    let content = execution
        .content_json
        .as_ref()
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Argo sync execution has no structured content"))?;
    if content
        .get("deployment_contract_id")
        .and_then(Value::as_str)
        != preflight
            .contract
            .as_ref()
            .map(|contract| contract.id.as_str())
        || content.get("permission_grant_id").and_then(Value::as_str)
            != preflight.grant.as_ref().map(|grant| grant.id.as_str())
    {
        return Err(ApiError::conflict(
            "Argo sync execution is stale relative to its contract or permission grant",
        ));
    }
    if content
        .get("gitops_delivery_merge_artifact_id")
        .and_then(Value::as_str)
        != preflight
            .gitops_merge
            .as_ref()
            .map(|artifact| artifact.id.as_str())
    {
        return Err(ApiError::conflict(
            "Argo sync execution is stale relative to its observed GitOps merge",
        ));
    }
    Ok(Json(ArgoSyncContextResponse {
        execution_id: query.execution_id,
        target_namespace: target.namespace,
        argo_application: target.application,
        revision: preflight.gitops_merge.as_ref().and_then(|artifact| {
            artifact
                .content_json
                .as_ref()
                .and_then(|content| content.get("merge_commit_sha"))
                .and_then(Value::as_str)
                .filter(|revision| is_git_sha(revision))
                .map(str::to_string)
        }),
        poll_seconds: argo_executor_poll_seconds(&state.worker.config_json()),
    }))
}

pub(in crate::app) async fn internal_argo_sync_control(
    State(state): State<AppState>,
    Path(deployment_intent_id): Path<String>,
    Query(query): Query<InternalArgoSyncQuery>,
) -> Result<Json<ArgoSyncControlResponse>, ApiError> {
    let (intent, _run_id, _execution) =
        current_argo_sync_execution(&state, &deployment_intent_id, &query.execution_id).await?;
    let work_plan = state
        .store
        .get_work_plan(&intent.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::conflict("Argo sync WorkPlan is unavailable"))?;
    let cancelled = match work_plan.work_item_id.as_deref() {
        Some(work_item_id) => state
            .store
            .get_work_item(work_item_id)
            .await?
            .is_some_and(|work_item| work_item.status == "cancelled"),
        None => false,
    };
    Ok(Json(ArgoSyncControlResponse { cancelled }))
}

pub(in crate::app) async fn internal_argo_sync_outcome(
    State(state): State<AppState>,
    Path(deployment_intent_id): Path<String>,
    Json(request): Json<ArgoSyncOutcomeRequest>,
) -> Result<Json<ArtifactResponse>, ApiError> {
    let (intent, run_id, execution) =
        current_argo_sync_execution(&state, &deployment_intent_id, &request.execution_id).await?;
    let execution_content = execution
        .content_json
        .as_ref()
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Argo sync execution has no structured content"))?;
    let result = match request.status.as_str() {
        "submitted" => {
            persist_argo_sync_result(
                &state.store,
                &intent,
                &run_id,
                &request.execution_id,
                "submitted",
                json!({}),
            )
            .await?
        }
        "completed" => {
            let sync_status = clean_optional_text(request.sync_status).ok_or_else(|| {
                ApiError::bad_request("completed Argo outcome requires sync_status")
            })?;
            let operation_phase =
                clean_optional_text(request.operation_phase).ok_or_else(|| {
                    ApiError::bad_request("completed Argo outcome requires operation_phase")
                })?;
            if sync_status != "Synced" || operation_phase != "Succeeded" {
                return Err(ApiError::conflict(
                    "completed Argo outcome requires Synced status and Succeeded operation phase",
                ));
            }
            persist_argo_sync_result(
                &state.store,
                &intent,
                &run_id,
                &request.execution_id,
                "completed",
                json!({
                    "sync_status": sync_status,
                    "health_status": clean_optional_text(request.health_status),
                    "operation_phase": operation_phase,
                    "revision": clean_optional_text(request.revision),
                }),
            )
            .await?
        }
        "failed" | "cancelled" => {
            let fallback = if request.status == "cancelled" {
                "cancelled"
            } else {
                "argo_sync_failed"
            };
            persist_argo_sync_result(
                &state.store,
                &intent,
                &run_id,
                &request.execution_id,
                &request.status,
                json!({
                    "error_code": normalized_executor_error_code(request.error_code, fallback),
                    "sync_status": clean_optional_text(request.sync_status),
                    "health_status": clean_optional_text(request.health_status),
                    "operation_phase": clean_optional_text(request.operation_phase),
                    "revision": clean_optional_text(request.revision),
                }),
            )
            .await?
        }
        _ => {
            return Err(ApiError::bad_request(
                "Argo sync outcome status must be submitted, completed, failed, or cancelled",
            ))
        }
    };
    append_deployment_intent_audit_event(
        &state.store,
        &intent,
        &format!("deployment_intent.argo_sync_{}", request.status),
        Some(DEFAULT_ARGO_RUNNER_SUBJECT.to_string()),
        None,
        json!({
            "execution_id": request.execution_id,
            "execution_artifact_id": execution.id,
            "result_artifact_id": result.id,
            "deployment_contract_id": execution_content.get("deployment_contract_id"),
            "permission_grant_id": execution_content.get("permission_grant_id"),
        }),
    )
    .await?;
    Ok(Json(result))
}

pub(in crate::app) async fn current_argo_sync_execution(
    state: &AppState,
    deployment_intent_id: &str,
    execution_id: &str,
) -> Result<(StoredDeploymentIntent, RunId, StoredArtifact), ApiError> {
    let intent = state
        .store
        .get_deployment_intent(deployment_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("deployment_intent", deployment_intent_id))?;
    let run_id = intent.run_id.clone().ok_or_else(|| {
        ApiError::conflict("Argo sync DeploymentIntent has no coding run provenance")
    })?;
    let artifacts = state.store.list_artifacts(&run_id).await?;
    let execution = artifacts
        .iter()
        .filter(|artifact| {
            artifact.kind == "argo_sync_execution"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("deployment_intent_id").and_then(Value::as_str)
                        == Some(intent.id.as_str())
                        && content.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .ok_or_else(|| ApiError::conflict("Argo sync execution is unavailable"))?;
    let latest = artifacts
        .iter()
        .filter(|artifact| {
            artifact.kind == "argo_sync_execution"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("deployment_intent_id").and_then(Value::as_str)
                        == Some(intent.id.as_str())
                })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id));
    if latest.map(|artifact| artifact.id.as_str()) != Some(execution.id.as_str()) {
        return Err(ApiError::conflict(
            "Argo sync execution is no longer current for this DeploymentIntent",
        ));
    }
    Ok((intent, run_id, execution))
}

pub(in crate::app) fn argo_sync_execution_matches(
    artifact: &StoredArtifact,
    intent: &StoredDeploymentIntent,
    contract: &StoredDeploymentContract,
    grant: &StoredPermissionGrant,
    gitops_merge: Option<&ArtifactResponse>,
) -> bool {
    artifact.kind == "argo_sync_execution"
        && artifact.content_json.as_ref().is_some_and(|content| {
            content.get("deployment_intent_id").and_then(Value::as_str) == Some(intent.id.as_str())
                && content
                    .get("deployment_contract_id")
                    .and_then(Value::as_str)
                    == Some(contract.id.as_str())
                && content.get("permission_grant_id").and_then(Value::as_str)
                    == Some(grant.id.as_str())
                && content
                    .get("gitops_delivery_merge_artifact_id")
                    .and_then(Value::as_str)
                    == gitops_merge.map(|artifact| artifact.id.as_str())
        })
}

pub(in crate::app) async fn persist_argo_sync_result(
    store: &SqliteStore,
    intent: &StoredDeploymentIntent,
    run_id: &RunId,
    execution_id: &str,
    status: &str,
    details: Value,
) -> Result<ArtifactResponse, ApiError> {
    if let Some(existing) = store
        .list_artifacts(run_id)
        .await?
        .into_iter()
        .find(|artifact| {
            artifact.kind == "argo_sync_result"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                        && content.get("status").and_then(Value::as_str) == Some(status)
                })
        })
    {
        return Ok(existing.into());
    }
    Ok(store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_argo_sync_result", unique_suffix()),
            session_id: intent.session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "argo_sync_result".to_string(),
            label: format!("Argo sync {} for DeploymentIntent {}", status, intent.id),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "execution_id": execution_id,
                "status": status,
                "deployment_intent_id": intent.id,
                "details": details,
            })),
        })
        .await?
        .into())
}

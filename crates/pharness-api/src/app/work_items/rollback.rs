use super::super::approvals::{create_permission_grant_record, grant_is_unexpired};
use super::super::auth::OperatorIdentity;
use super::super::capabilities::execute_direct_capability;
use super::super::clock::{current_millis, unique_suffix};
use super::super::delivery_actions::{ARGO_SYNC_ACTIONS, GITOPS_DELIVERY_ACTIONS};
use super::super::deployment::contracts::{
    deployment_contract_spec, validate_protected_production_deployment_contract,
};
use super::super::execution_checks::{
    argo_executor_poll_seconds, execution_check, normalized_executor_error_code,
};
use super::super::gitops::deployment_evidence::observed_gitops_merge_for_deployment;
use super::super::identifiers::{is_git_sha, is_github_pr_url, safe_id_fragment};
use super::super::principals::{DEFAULT_ARGO_RUNNER_SUBJECT, DEFAULT_GITOPS_WRITER_SUBJECT};
use super::super::releases::verify_required_prometheus_inventory;
use super::super::system::{
    capability_statuses, immutable_image_digest, protected_target_json, PROTECTED_ARGO_APPLICATION,
    PROTECTED_ENVIRONMENT, PROTECTED_GITOPS_REPO, PROTECTED_IMAGE_NAME,
    PROTECTED_KUSTOMIZATION_PATH, PROTECTED_NAMESPACE, PROTECTED_ROLLBACK_OWNER,
    PROTECTED_WORKLOAD_NAME,
};
use super::super::validation::{clean_optional_text, required_json_string, required_text};
use super::super::{ApiError, AppState};
use super::preflight::{
    bounded_production_grant_expiry, stored_work_item_matches_protected_target,
};
use super::rollback_state::{
    latest_rollback_intent, rollback_intent_response, work_item_provenance_run_id,
};
use crate::dispatch::{
    ArgoSyncExecutionRequest, GitOpsDeliveryExecutionRequest, GitOpsDeliveryObservationRequest,
};
use crate::dto::{
    ArgoSyncContextResponse, ArgoSyncControlResponse, ArgoSyncOutcomeRequest, ArtifactResponse,
    CreatePermissionGrantRequest, ExecuteCapabilityResponse, GitOpsDeliveryContextResponse,
    GitOpsDeliveryObservationContextResponse, GitOpsDeliveryObservationOutcomeRequest,
    GitOpsDeliveryOutcomeRequest,
};
use axum::extract::{Path, State};
use axum::{Extension, Json};
use pharness_core::{ActionId, AgentAction, CapabilityKind, PermissionGrantScope, ToolResult};
use pharness_store::{
    CreateApprovalGate, CreateArtifact, StoredArtifact, StoredPermissionGrant, StoredWorkItem,
    WorkItemListFilter,
};
use serde_json::{json, Value};

#[derive(Debug, serde::Deserialize)]
pub(in crate::app) struct RollbackIntentRequest {
    pub(in crate::app) actor: Option<String>,
    pub(in crate::app) reason: String,
    #[serde(default)]
    pub(in crate::app) expires_at: Option<String>,
}

pub(in crate::app) fn required_baseline_capability_result(
    response: ExecuteCapabilityResponse,
    resource: &str,
) -> Result<ToolResult, ApiError> {
    response.result.ok_or_else(|| {
        let detail = response
            .error
            .as_deref()
            .filter(|error| !error.trim().is_empty())
            .unwrap_or(response.status.as_str());
        ApiError::conflict(format!(
            "production baseline {resource} observation failed: {detail}"
        ))
    })
}

pub(in crate::app) async fn prepare_work_item_rollback_intent(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(work_item_id): Path<String>,
    Json(request): Json<RollbackIntentRequest>,
) -> Result<Json<Value>, ApiError> {
    let reason = required_text(request.reason.clone(), "reason")?;
    let item = state
        .store
        .get_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &work_item_id))?;
    if !stored_work_item_matches_protected_target(&item) {
        return Err(ApiError::conflict(
            "RollbackIntent is limited to the exact protected production target",
        ));
    }
    if let Some(existing) = latest_rollback_intent(&state, &item, None).await? {
        return Ok(Json(existing));
    }
    let deployment = execute_direct_capability(
        &state,
        AgentAction::KubernetesGet {
            id: ActionId::new(format!("act_{}_baseline_deployment", unique_suffix())),
            reason: reason.clone(),
            resource: "deployments".to_string(),
            namespace: Some(PROTECTED_NAMESPACE.to_string()),
            name: Some(PROTECTED_WORKLOAD_NAME.to_string()),
            all_namespaces: false,
            label_selector: None,
        },
        None,
    )
    .await?;
    let argo = execute_direct_capability(
        &state,
        AgentAction::ArgoGetApp {
            id: ActionId::new(format!("act_{}_baseline_argo", unique_suffix())),
            reason: reason.clone(),
            app: PROTECTED_ARGO_APPLICATION.to_string(),
        },
        None,
    )
    .await?;
    let pods = execute_direct_capability(
        &state,
        AgentAction::KubernetesGet {
            id: ActionId::new(format!("act_{}_baseline_pods", unique_suffix())),
            reason: reason.clone(),
            resource: "pods".to_string(),
            namespace: Some(PROTECTED_NAMESPACE.to_string()),
            name: None,
            all_namespaces: false,
            label_selector: Some("app=yfinance-wrapper".to_string()),
        },
        None,
    )
    .await?;
    let deployment_result = required_baseline_capability_result(deployment, "Deployment")?;
    let argo_result = required_baseline_capability_result(argo, "Argo Application")?;
    let pods_result = required_baseline_capability_result(pods, "Pod")?;
    let expected_image = item
        .gitops_image_name
        .as_deref()
        .ok_or_else(|| ApiError::conflict("protected target has no image name"))?;
    let images = deployment_result
        .content
        .pointer("/analysis/containers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let image_ref = images
        .iter()
        .filter_map(|container| container.get("image").and_then(Value::as_str))
        .find(|image| image.starts_with(&format!("{expected_image}@sha256:")))
        .ok_or_else(|| {
            ApiError::conflict(
                "running protected Deployment does not expose the expected immutable image",
            )
        })?
        .to_string();
    let image_digest = image_ref
        .split_once('@')
        .map(|(_, digest)| digest.to_string())
        .filter(|digest| immutable_image_digest(digest))
        .ok_or_else(|| ApiError::conflict("production baseline image digest is malformed"))?;
    let running_image_ids = pods_result
        .content
        .pointer("/output/items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|pod| {
            pod.pointer("/status/containers")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(|container| container.get("imageID").and_then(Value::as_str))
        .collect::<Vec<_>>();
    if running_image_ids.is_empty()
        || !running_image_ids
            .iter()
            .all(|image_id| image_id.ends_with(&image_digest))
    {
        return Err(ApiError::conflict(
            "running Pod imageID does not exactly match the declared production baseline digest",
        ));
    }
    let ready = deployment_result
        .content
        .pointer("/analysis/status")
        .and_then(Value::as_str)
        == Some("healthy");
    if !ready {
        return Err(ApiError::conflict(
            "production baseline Deployment must be healthy before rollback preparation",
        ));
    }
    let argo_revision = argo_result
        .content
        .pointer("/analysis/revision")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("production baseline Argo revision is unavailable"))?;
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let rollback_intent_id = format!("rollback_{}", unique_suffix());
    let work_plan = state
        .store
        .get_work_plan_by_work_item(&item.id)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent requires a WorkPlan"))?;
    let run_id = work_item_provenance_run_id(&state, &item)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent requires coding run provenance"))?;
    let run = state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;
    let gate = state
        .store
        .create_approval_gate(CreateApprovalGate {
            id: format!("agate_{}_rollback", unique_suffix()),
            work_item_id: Some(item.id.clone()),
            remediation_plan_id: None,
            incident_id: None,
            session_id: run.session_id.clone(),
            run_id: Some(run_id.clone()),
            status: "pending".to_string(),
            gate_kind: "production_rollback".to_string(),
            gate_order: 90,
            title: format!("Approve rollback for {}", item.title),
            summary: format!("Restore only {image_digest} through a manual GitOps pull request"),
            risk_level: "critical".to_string(),
            resource_namespace: item.target_namespace.clone(),
            resource_kind: item.workload_kind.clone(),
            resource_name: item.workload_name.clone(),
            gate_json: json!({
                "rollback_intent_id": rollback_intent_id,
                "work_plan_id": work_plan.id,
                "baseline_digest": image_digest,
                "argo_application": PROTECTED_ARGO_APPLICATION,
            }),
        })
        .await?;
    let content = json!({
        "rollback_intent_id": rollback_intent_id,
        "work_item_id": item.id,
        "status": "prepared",
        "baseline": {
            "image_ref": image_ref,
            "image_digest": image_digest,
            "deployment_ready": true,
            "running_image_ids_verified": true,
            "argo_revision": argo_revision,
            "gitops_revision": argo_revision,
        },
        "target": protected_target_json(),
        "rollback_owner": PROTECTED_ROLLBACK_OWNER,
        "approval_gate_id": gate.id,
        "manual_merge_required": true,
        "automatic_rollback": false,
        "reason": reason,
        "created_by": actor,
    });
    let artifact = append_rollback_intent_artifact(&state, &run, &content).await?;
    Ok(Json(rollback_intent_response(&artifact)))
}

pub(in crate::app) async fn get_work_item_rollback_intent(
    State(state): State<AppState>,
    Path(work_item_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let item = state
        .store
        .get_work_item(&work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &work_item_id))?;
    Ok(Json(
        latest_rollback_intent(&state, &item, None)
            .await?
            .unwrap_or_else(|| json!({ "status": "unavailable", "work_item_id": item.id })),
    ))
}

pub(in crate::app) async fn approve_rollback_intent(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(rollback_intent_id): Path<String>,
    Json(request): Json<RollbackIntentRequest>,
) -> Result<Json<Value>, ApiError> {
    let reason = required_text(request.reason.clone(), "reason")?;
    let (item, current, run) = rollback_intent_context(&state, &rollback_intent_id).await?;
    let current_status = current.pointer("/content/status").and_then(Value::as_str);
    if current_status == Some("ready_for_argo_sync") {
        return approve_rollback_argo_sync(
            RollbackArgoApprovalContext {
                state: &state,
                rollback_intent_id: &rollback_intent_id,
                item: &item,
                current: &current,
                run: &run,
            },
            identity,
            request,
            reason,
        )
        .await;
    }
    if current_status != Some("prepared") {
        return Err(ApiError::conflict(
            "RollbackIntent must be prepared for its writer or ready for its explicit Argo approval",
        ));
    }
    let expires_at = bounded_production_grant_expiry(&item, request.expires_at)?;
    let gate_id = current
        .pointer("/content/approval_gate_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("RollbackIntent approval gate is unavailable"))?;
    let gate = state
        .store
        .get_approval_gate(gate_id)
        .await?
        .filter(|gate| matches!(gate.status.as_str(), "pending" | "satisfied"))
        .ok_or_else(|| {
            ApiError::conflict("RollbackIntent approval gate is neither pending nor satisfied")
        })?;
    let baseline_digest_from_intent = current
        .pointer("/content/baseline/image_digest")
        .and_then(Value::as_str)
        .filter(|value| immutable_image_digest(value))
        .ok_or_else(|| ApiError::conflict("RollbackIntent baseline digest is invalid"))?;
    if gate.work_item_id.as_deref() != Some(item.id.as_str())
        || gate.gate_kind != "production_rollback"
        || gate
            .gate_json
            .get("rollback_intent_id")
            .and_then(Value::as_str)
            != Some(rollback_intent_id.as_str())
        || gate
            .gate_json
            .get("baseline_digest")
            .and_then(Value::as_str)
            != Some(baseline_digest_from_intent)
        || gate
            .gate_json
            .get("argo_application")
            .and_then(Value::as_str)
            != Some(PROTECTED_ARGO_APPLICATION)
    {
        return Err(ApiError::conflict(
            "RollbackIntent approval gate no longer matches its immutable target binding",
        ));
    }
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    if gate.status == "pending" {
        state
            .store
            .decide_approval_gate(gate_id, "satisfied", actor.clone(), Some(reason.clone()))
            .await?;
    }
    let content = current
        .get("content")
        .cloned()
        .ok_or_else(|| ApiError::internal("RollbackIntent content is unavailable"))?;
    let mut content = content
        .as_object()
        .cloned()
        .ok_or_else(|| ApiError::internal("RollbackIntent content is malformed"))?;
    let baseline_digest = content
        .get("baseline")
        .and_then(|value| value.get("image_digest"))
        .and_then(Value::as_str)
        .filter(|value| immutable_image_digest(value))
        .ok_or_else(|| ApiError::conflict("RollbackIntent baseline digest is invalid"))?
        .to_string();
    let work_plan = state
        .store
        .get_work_plan_by_work_item(&item.id)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent WorkPlan is unavailable"))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&work_plan.id)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent ChangeSet is unavailable"))?;
    let pipeline_intent = state
        .store
        .get_pipeline_intent_by_change_set(&change_set.id)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent PipelineIntent is unavailable"))?;
    let source_merge_sha = pipeline_intent
        .intent_json
        .pointer("/source_provenance/merge_commit_sha")
        .and_then(Value::as_str)
        .filter(|value| is_git_sha(value))
        .ok_or_else(|| ApiError::conflict("rollback source merge provenance is unavailable"))?
        .to_string();
    let rollback_base_commit =
        observed_gitops_merge_for_deployment(&state.store, &item, &pipeline_intent)
            .await?
            .and_then(|artifact| artifact.content_json)
            .and_then(|content| {
                content
                    .get("merge_commit_sha")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .filter(|value| is_git_sha(value))
            .ok_or_else(|| {
                ApiError::conflict(
                    "rollback writer authorization requires the observed deployment GitOps merge",
                )
            })?;
    let branch = format!(
        "pharness/rollback-{}",
        safe_id_fragment(&rollback_intent_id)
    );
    let permission_grant = create_permission_grant_record(
        &state.store,
        CreatePermissionGrantRequest {
            subject: DEFAULT_GITOPS_WRITER_SUBJECT.to_string(),
            created_by: actor.clone(),
            reason: format!("RollbackIntent {rollback_intent_id}: {reason}"),
            scope: json!({
                "environment": PROTECTED_ENVIRONMENT,
                "capability_kinds": ["git"],
                "actions": GITOPS_DELIVERY_ACTIONS,
                "max_risk": "critical",
                "repos": [PROTECTED_GITOPS_REPO],
                "branches": [branch],
                "work_item_ids": [item.id],
                "work_plan_ids": [work_plan.id],
                "change_set_ids": [change_set.id],
                "pipeline_intent_ids": [pipeline_intent.id],
                "pipeline_contract_ids": [item.pipeline_contract_id],
                "deployment_contract_ids": [item.deployment_contract_id],
                "source_merge_shas": [source_merge_sha],
                "gitops_merge_shas": [rollback_base_commit],
                "image_digests": [baseline_digest],
                "production_impacting": true,
            }),
            policy: json!({ "policy_mode": "supervised_autonomy" }),
            expires_at: expires_at.clone(),
        },
    )
    .await?;
    content.insert("status".to_string(), json!("approved"));
    content.insert("approved_by".to_string(), json!(actor));
    content.insert("approval_reason".to_string(), json!(reason));
    content.insert("authorization_expires_at".to_string(), json!(expires_at));
    content.insert(
        "permission_grant_id".to_string(),
        json!(permission_grant.id),
    );
    content.insert(
        "rollback_base_commit".to_string(),
        json!(rollback_base_commit),
    );
    let artifact = append_rollback_intent_artifact(&state, &run, &Value::Object(content)).await?;
    Ok(Json(rollback_intent_response(&artifact)))
}

struct RollbackArgoApprovalContext<'a> {
    state: &'a AppState,
    rollback_intent_id: &'a str,
    item: &'a StoredWorkItem,
    current: &'a Value,
    run: &'a pharness_store::StoredRun,
}

async fn approve_rollback_argo_sync(
    context: RollbackArgoApprovalContext<'_>,
    identity: Option<Extension<OperatorIdentity>>,
    request: RollbackIntentRequest,
    reason: String,
) -> Result<Json<Value>, ApiError> {
    let RollbackArgoApprovalContext {
        state,
        rollback_intent_id,
        item,
        current,
        run,
    } = context;
    if !stored_work_item_matches_protected_target(item) {
        return Err(ApiError::conflict(
            "rollback Argo approval is limited to the exact protected production target",
        ));
    }
    let expires_at = bounded_production_grant_expiry(item, request.expires_at)?;
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let content = current
        .get("content")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| ApiError::conflict("RollbackIntent content is unavailable"))?;
    let baseline_digest = content
        .get("baseline")
        .and_then(|value| value.get("image_digest"))
        .and_then(Value::as_str)
        .filter(|value| immutable_image_digest(value))
        .ok_or_else(|| ApiError::conflict("RollbackIntent baseline digest is invalid"))?
        .to_string();
    let gitops_merge_sha = content
        .get("gitops_merge_sha")
        .and_then(Value::as_str)
        .filter(|value| is_git_sha(value))
        .ok_or_else(|| ApiError::conflict("rollback GitOps merge provenance is unavailable"))?
        .to_string();
    let work_plan = state
        .store
        .get_work_plan_by_work_item(&item.id)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent WorkPlan is unavailable"))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&work_plan.id)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent ChangeSet is unavailable"))?;
    let pipeline_intent = state
        .store
        .get_pipeline_intent_by_change_set(&change_set.id)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent PipelineIntent is unavailable"))?;
    let source_merge_sha = pipeline_intent
        .intent_json
        .pointer("/source_provenance/merge_commit_sha")
        .and_then(Value::as_str)
        .filter(|value| is_git_sha(value))
        .ok_or_else(|| {
            ApiError::conflict("rollback authorization requires source merge provenance")
        })?
        .to_string();

    let mut gate_ids = Vec::new();
    for (order, gate_kind) in [
        (91, "production_rollback_deployment"),
        (92, "cluster_mutation"),
        (93, "production_impact"),
    ] {
        let gate = state
            .store
            .create_approval_gate(CreateApprovalGate {
                id: format!("agate_{}_rollback_argo", unique_suffix()),
                work_item_id: Some(item.id.clone()),
                remediation_plan_id: None,
                incident_id: None,
                session_id: run.session_id.clone(),
                run_id: Some(run.id.clone()),
                status: "pending".to_string(),
                gate_kind: gate_kind.to_string(),
                gate_order: order,
                title: format!("Approve {gate_kind} for {}", item.title),
                summary: format!(
                    "Sync {PROTECTED_ARGO_APPLICATION} only to rollback merge {gitops_merge_sha} and restore {baseline_digest}"
                ),
                risk_level: "critical".to_string(),
                resource_namespace: Some(PROTECTED_NAMESPACE.to_string()),
                resource_kind: Some("Application".to_string()),
                resource_name: Some(PROTECTED_ARGO_APPLICATION.to_string()),
                gate_json: json!({
                    "rollback_intent_id": rollback_intent_id,
                    "work_plan_id": work_plan.id,
                    "gitops_merge_sha": gitops_merge_sha,
                    "baseline_digest": baseline_digest,
                    "argo_application": PROTECTED_ARGO_APPLICATION,
                    "scope": {
                        "work_item_id": item.id,
                        "work_plan_id": work_plan.id,
                        "environment": PROTECTED_ENVIRONMENT,
                        "production_impacting": true,
                        "source_repository": item.source_repo,
                        "source_ref": item.source_ref,
                        "gitops_repository": item.gitops_repo,
                        "gitops_ref": item.gitops_ref,
                        "target_namespace": PROTECTED_NAMESPACE,
                        "argo_application": PROTECTED_ARGO_APPLICATION,
                        "actions": ARGO_SYNC_ACTIONS,
                    },
                }),
            })
            .await?;
        state
            .store
            .decide_approval_gate(&gate.id, "satisfied", actor.clone(), Some(reason.clone()))
            .await?;
        gate_ids.push(gate.id);
    }
    let grant = create_permission_grant_record(
        &state.store,
        CreatePermissionGrantRequest {
            subject: DEFAULT_ARGO_RUNNER_SUBJECT.to_string(),
            created_by: actor.clone(),
            reason: format!("RollbackIntent {rollback_intent_id} Argo sync: {reason}"),
            scope: json!({
                "environment": PROTECTED_ENVIRONMENT,
                "capability_kinds": ["argo_sync"],
                "actions": ARGO_SYNC_ACTIONS,
                "max_risk": "critical",
                "namespaces": [PROTECTED_NAMESPACE],
                "work_item_ids": [item.id],
                "work_plan_ids": [work_plan.id],
                "change_set_ids": [change_set.id],
                "pipeline_intent_ids": [pipeline_intent.id],
                "deployment_intent_ids": [rollback_intent_id],
                "argo_applications": [PROTECTED_ARGO_APPLICATION],
                "pipeline_contract_ids": [item.pipeline_contract_id],
                "deployment_contract_ids": [item.deployment_contract_id],
                "source_merge_shas": [source_merge_sha],
                "gitops_merge_shas": [gitops_merge_sha],
                "image_digests": [baseline_digest],
                "production_impacting": true,
            }),
            policy: json!({ "policy_mode": "supervised_autonomy" }),
            expires_at: expires_at.clone(),
        },
    )
    .await?;
    let mut updated = content;
    updated.insert("status".to_string(), json!("argo_approved"));
    updated.insert("argo_approved_by".to_string(), json!(actor));
    updated.insert("argo_approval_reason".to_string(), json!(reason));
    updated.insert(
        "argo_authorization_expires_at".to_string(),
        json!(expires_at),
    );
    updated.insert("argo_permission_grant_id".to_string(), json!(grant.id));
    updated.insert("argo_approval_gate_ids".to_string(), json!(gate_ids));
    let artifact = append_rollback_intent_artifact(state, run, &Value::Object(updated)).await?;
    Ok(Json(rollback_intent_response(&artifact)))
}

pub(in crate::app) async fn preflight_rollback_intent(
    State(state): State<AppState>,
    Path(rollback_intent_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let (item, current, _run) = rollback_intent_context(&state, &rollback_intent_id).await?;
    let status = current.pointer("/content/status").and_then(Value::as_str);
    let argo_phase = status == Some("argo_approved");
    let expires_at = current
        .pointer(if argo_phase {
            "/content/argo_authorization_expires_at"
        } else {
            "/content/authorization_expires_at"
        })
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<u128>().ok());
    let authorization_fresh = expires_at.is_some_and(|value| value > current_millis());
    let permission_grant_id = current
        .pointer(if argo_phase {
            "/content/argo_permission_grant_id"
        } else {
            "/content/permission_grant_id"
        })
        .and_then(Value::as_str);
    let grant_fresh = match permission_grant_id {
        Some(grant_id) => state
            .store
            .get_permission_grant(grant_id)
            .await?
            .is_some_and(|grant| {
                grant.status == "active" && grant_is_unexpired(&grant, current_millis())
            }),
        None => false,
    };
    let exact_binding = if argo_phase {
        match current.get("content").and_then(Value::as_object) {
            Some(content) => {
                validate_rollback_argo_grant(&state, &item, &rollback_intent_id, content)
                    .await
                    .is_ok()
            }
            None => false,
        }
    } else {
        true
    };
    let ready = matches!(status, Some("approved" | "argo_approved"))
        && authorization_fresh
        && grant_fresh
        && exact_binding;
    Ok(Json(json!({
        "rollback_intent_id": rollback_intent_id,
        "status": if ready { if argo_phase { "ready_for_argo" } else { "ready_for_writer" } } else { "blocked" },
        "ready": ready,
        "authorization_fresh": authorization_fresh,
        "grant_fresh": grant_fresh,
        "writer_grant_fresh": !argo_phase && grant_fresh,
        "argo_grant_fresh": argo_phase && grant_fresh,
        "exact_binding": exact_binding,
        "manual_merge_required": true,
        "automatic_rollback": false,
        "content": current.get("content"),
    })))
}

pub(in crate::app) async fn execute_rollback_intent(
    State(state): State<AppState>,
    Path(rollback_intent_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let Json(preflight) =
        preflight_rollback_intent(State(state.clone()), Path(rollback_intent_id.clone())).await?;
    if preflight.get("ready").and_then(Value::as_bool) != Some(true) {
        return Err(ApiError::conflict(
            "RollbackIntent is not ready for its next isolated executor",
        ));
    }
    if preflight.get("status").and_then(Value::as_str) == Some("ready_for_argo") {
        return dispatch_rollback_argo_sync(&state, &rollback_intent_id).await;
    }
    let (item, current, run) = rollback_intent_context(&state, &rollback_intent_id).await?;
    require_fresh_capability(&state, "gitops_writer").await?;
    let settings = state
        .worker
        .gitops_writer_settings()
        .ok_or_else(|| ApiError::conflict("GitOps rollback writer is not configured"))?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == PROTECTED_GITOPS_REPO)
    {
        return Err(ApiError::conflict(
            "protected GitOps repository is not allowlisted for the rollback writer",
        ));
    }
    let content = current
        .get("content")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("RollbackIntent content is unavailable"))?;
    let baseline_digest = content
        .get("baseline")
        .and_then(|value| value.get("image_digest"))
        .and_then(Value::as_str)
        .filter(|value| immutable_image_digest(value))
        .ok_or_else(|| ApiError::conflict("RollbackIntent baseline digest is invalid"))?;
    let base_commit = content
        .get("rollback_base_commit")
        .and_then(Value::as_str)
        .filter(|value| is_git_sha(value))
        .ok_or_else(|| ApiError::conflict("RollbackIntent writer base revision is invalid"))?;
    let permission_grant_id = content
        .get("permission_grant_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("RollbackIntent has no short-lived writer grant"))?;
    let permission_grant = state
        .store
        .get_permission_grant(permission_grant_id)
        .await?
        .filter(|grant| grant.status == "active" && grant_is_unexpired(grant, current_millis()))
        .ok_or_else(|| ApiError::conflict("RollbackIntent writer grant is expired or revoked"))?;
    let grant_scope =
        serde_json::from_value::<PermissionGrantScope>(permission_grant.scope_json)
            .map_err(|_| ApiError::conflict("RollbackIntent writer grant scope is malformed"))?;
    let expected_branch = format!(
        "pharness/rollback-{}",
        safe_id_fragment(&rollback_intent_id)
    );
    let work_plan = state
        .store
        .get_work_plan_by_work_item(&item.id)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent WorkPlan is unavailable"))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&work_plan.id)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent ChangeSet is unavailable"))?;
    let pipeline_intent = state
        .store
        .get_pipeline_intent_by_change_set(&change_set.id)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent PipelineIntent is unavailable"))?;
    let source_merge_sha = pipeline_intent
        .intent_json
        .pointer("/source_provenance/merge_commit_sha")
        .and_then(Value::as_str)
        .filter(|value| is_git_sha(value))
        .ok_or_else(|| ApiError::conflict("rollback source merge provenance is unavailable"))?;
    if permission_grant.subject != DEFAULT_GITOPS_WRITER_SUBJECT
        || grant_scope.environment.as_deref() != Some(PROTECTED_ENVIRONMENT)
        || grant_scope.capability_kinds != vec![CapabilityKind::Git]
        || grant_scope.repos != vec![PROTECTED_GITOPS_REPO.to_string()]
        || grant_scope.branches != vec![expected_branch.clone()]
        || grant_scope.work_item_ids != vec![item.id.clone()]
        || grant_scope.work_plan_ids != vec![work_plan.id]
        || grant_scope.change_set_ids != vec![change_set.id]
        || grant_scope.pipeline_intent_ids != vec![pipeline_intent.id]
        || grant_scope.pipeline_contract_ids
            != vec![item.pipeline_contract_id.clone().unwrap_or_default()]
        || grant_scope.deployment_contract_ids
            != vec![item.deployment_contract_id.clone().unwrap_or_default()]
        || grant_scope.source_merge_shas != vec![source_merge_sha.to_string()]
        || grant_scope.gitops_merge_shas != vec![base_commit.to_string()]
        || grant_scope.image_digests != vec![baseline_digest.to_string()]
        || grant_scope.production_impacting != Some(true)
        || !GITOPS_DELIVERY_ACTIONS
            .iter()
            .all(|action| grant_scope.actions.iter().any(|allowed| allowed == action))
    {
        return Err(ApiError::conflict(
            "RollbackIntent writer grant no longer matches the exact WorkItem, contracts, digest, repository, and branch",
        ));
    }
    let artifacts = state.store.list_artifacts(&run.id).await?;
    if let Some(existing) = artifacts.iter().find(|artifact| {
        artifact.kind == "rollback_delivery_execution"
            && artifact.content_json.as_ref().is_some_and(|value| {
                value.get("rollback_intent_id").and_then(Value::as_str)
                    == Some(rollback_intent_id.as_str())
            })
    }) {
        let execution_id = existing
            .content_json
            .as_ref()
            .and_then(|value| value.get("execution_id"))
            .and_then(Value::as_str);
        let status = execution_id
            .and_then(|id| {
                artifacts.iter().find_map(|artifact| {
                    (artifact.kind == "rollback_delivery_result")
                        .then_some(artifact.content_json.as_ref())
                        .flatten()
                        .filter(|value| {
                            value.get("execution_id").and_then(Value::as_str) == Some(id)
                        })
                        .and_then(|value| value.get("status").and_then(Value::as_str))
                })
            })
            .unwrap_or("dispatched");
        return Ok(Json(json!({
            "rollback_intent_id": rollback_intent_id,
            "status": status,
            "execution": ArtifactResponse::from(existing.clone()),
            "created": false,
            "manual_merge_required": true,
            "automatic_rollback": false,
        })));
    }
    let execution_id = format!("rbexec_{}", unique_suffix());
    let context = json!({
        "execution_id": execution_id,
        "repository": PROTECTED_GITOPS_REPO,
        "base_ref": item.gitops_ref.as_deref().unwrap_or("main"),
        "base_commit": base_commit,
        "head_branch": expected_branch,
        "kustomization_path": PROTECTED_KUSTOMIZATION_PATH,
        "image_name": PROTECTED_IMAGE_NAME,
        "image_ref": format!("{PROTECTED_IMAGE_NAME}@{baseline_digest}"),
        "commit_subject": format!("rollback(yfinance-wrapper): restore {}", &baseline_digest[..19]),
        "commit_body": format!("Restore the captured known-good digest for RollbackIntent {rollback_intent_id}.\n\nManual merge and explicit Argo approval remain required."),
        "pull_request_title": format!("Rollback yfinance-wrapper to {}", &baseline_digest[..19]),
        "pull_request_body": format!("Restore only the captured known-good image digest.\n\nPHarness RollbackIntent: {rollback_intent_id}\nWorkItem: {}\nRollback owner: {PROTECTED_ROLLBACK_OWNER}", item.id),
        "github_api_url": settings.github_api_url,
        "author_name": settings.author_name,
        "author_email": settings.author_email,
    });
    let execution = state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_rollback_delivery_execution", unique_suffix()),
            session_id: run.session_id.clone(),
            run_id: Some(run.id.clone()),
            kind: "rollback_delivery_execution".to_string(),
            label: format!("Rollback GitOps delivery for {rollback_intent_id}"),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "rollback_intent_id": rollback_intent_id,
                "execution_id": execution_id,
                "status": "dispatched",
                "context": context,
            })),
        })
        .await?;
    match state
        .worker
        .dispatch_gitops_delivery(GitOpsDeliveryExecutionRequest {
            gitops_change_set_id: rollback_intent_id.clone(),
            execution_id: execution_id.clone(),
        })
        .await
    {
        Ok(receipt) => Ok(Json(json!({
            "rollback_intent_id": rollback_intent_id,
            "status": "dispatched",
            "execution": ArtifactResponse::from(execution),
            "job_name": receipt.job_name,
            "created": true,
            "manual_merge_required": true,
            "automatic_rollback": false,
        }))),
        Err(error) => {
            tracing::warn!(rollback_intent_id = %rollback_intent_id, %error, "rollback writer dispatch failed");
            let failure = append_rollback_delivery_result(
                &state,
                &run,
                &rollback_intent_id,
                &execution_id,
                "dispatch_failed",
                json!({ "error_code": "job_dispatch_failed" }),
            )
            .await?;
            let mut updated = content.clone();
            updated.insert("status".to_string(), json!("attention_required"));
            updated.insert("writer_failure_result_id".to_string(), json!(failure.id));
            append_rollback_intent_artifact(&state, &run, &Value::Object(updated)).await?;
            Ok(Json(json!({
                "rollback_intent_id": rollback_intent_id,
                "status": "dispatch_failed",
                "execution": ArtifactResponse::from(failure),
                "created": true,
                "manual_merge_required": true,
                "automatic_rollback": false,
            })))
        }
    }
}

pub(in crate::app) async fn dispatch_rollback_argo_sync(
    state: &AppState,
    rollback_intent_id: &str,
) -> Result<Json<Value>, ApiError> {
    let (item, current, run) = rollback_intent_context(state, rollback_intent_id).await?;
    let content = current
        .get("content")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("RollbackIntent content is unavailable"))?;
    validate_rollback_argo_grant(state, &item, rollback_intent_id, content).await?;
    if !state.worker.argo_executor_available()
        || !state
            .worker
            .argo_executor_allows_application(PROTECTED_ARGO_APPLICATION)
    {
        return Err(ApiError::conflict(
            "the isolated Argo executor is unavailable for the protected Application",
        ));
    }
    let artifacts = state.store.list_artifacts(&run.id).await?;
    if let Some(existing) = artifacts
        .iter()
        .filter(|artifact| artifact.kind == "rollback_argo_sync_execution")
        .filter(|artifact| {
            artifact.content_json.as_ref().is_some_and(|value| {
                value.get("rollback_intent_id").and_then(Value::as_str) == Some(rollback_intent_id)
            })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
    {
        return Ok(Json(json!({
            "rollback_intent_id": rollback_intent_id,
            "status": "argo_syncing",
            "execution": ArtifactResponse::from(existing.clone()),
            "created": false,
            "automatic_rollback": false,
        })));
    }
    let execution_id = format!("rbaexec_{}", unique_suffix());
    let execution = state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_rollback_argo_sync_execution", unique_suffix()),
            session_id: run.session_id.clone(),
            run_id: Some(run.id.clone()),
            kind: "rollback_argo_sync_execution".to_string(),
            label: format!("Rollback Argo sync for {rollback_intent_id}"),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "rollback_intent_id": rollback_intent_id,
                "execution_id": execution_id,
                "status": "dispatched",
                "permission_grant_id": content.get("argo_permission_grant_id"),
                "deployment_contract_id": item.deployment_contract_id,
                "gitops_merge_sha": content.get("gitops_merge_sha"),
                "baseline_digest": content.get("baseline").and_then(|value| value.get("image_digest")),
                "target": protected_target_json(),
            })),
        })
        .await?;
    match state
        .worker
        .dispatch_argo_sync_execution(ArgoSyncExecutionRequest {
            deployment_intent_id: rollback_intent_id.to_string(),
            execution_id: execution_id.clone(),
        })
        .await
    {
        Ok(receipt) => {
            let mut updated = content.clone();
            updated.insert("status".to_string(), json!("argo_syncing"));
            updated.insert("argo_execution_id".to_string(), json!(execution_id));
            append_rollback_intent_artifact(state, &run, &Value::Object(updated)).await?;
            Ok(Json(json!({
                "rollback_intent_id": rollback_intent_id,
                "status": "argo_syncing",
                "execution": ArtifactResponse::from(execution),
                "job_name": receipt.job_name,
                "created": true,
                "automatic_rollback": false,
            })))
        }
        Err(error) => {
            tracing::warn!(rollback_intent_id, %error, "rollback Argo executor dispatch failed");
            let result = append_rollback_argo_sync_result(
                state,
                &run,
                rollback_intent_id,
                &execution_id,
                "dispatch_failed",
                json!({ "error_code": "job_dispatch_failed" }),
            )
            .await?;
            let mut updated = content.clone();
            updated.insert("status".to_string(), json!("attention_required"));
            updated.insert("argo_failure_result_id".to_string(), json!(result.id));
            append_rollback_intent_artifact(state, &run, &Value::Object(updated)).await?;
            Ok(Json(json!({
                "rollback_intent_id": rollback_intent_id,
                "status": "dispatch_failed",
                "execution": ArtifactResponse::from(result),
                "created": true,
                "automatic_rollback": false,
            })))
        }
    }
}

pub(in crate::app) async fn validate_rollback_argo_grant(
    state: &AppState,
    item: &StoredWorkItem,
    rollback_intent_id: &str,
    content: &serde_json::Map<String, Value>,
) -> Result<StoredPermissionGrant, ApiError> {
    let grant_id = content
        .get("argo_permission_grant_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("RollbackIntent has no Argo permission grant"))?;
    let grant = state
        .store
        .get_permission_grant(grant_id)
        .await?
        .filter(|grant| grant.status == "active" && grant_is_unexpired(grant, current_millis()))
        .ok_or_else(|| {
            ApiError::conflict("rollback Argo permission grant is expired or revoked")
        })?;
    let scope = serde_json::from_value::<PermissionGrantScope>(grant.scope_json.clone())
        .map_err(|_| ApiError::conflict("rollback Argo permission grant scope is malformed"))?;
    let baseline_digest = content
        .get("baseline")
        .and_then(|value| value.get("image_digest"))
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("RollbackIntent baseline digest is unavailable"))?;
    let gitops_merge_sha = content
        .get("gitops_merge_sha")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("rollback GitOps merge SHA is unavailable"))?;
    let expected_pipeline_contracts = item
        .pipeline_contract_id
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    let expected_deployment_contracts = item
        .deployment_contract_id
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    let work_plan = state
        .store
        .get_work_plan_by_work_item(&item.id)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent WorkPlan is unavailable"))?;
    let change_set = state
        .store
        .get_change_set_by_work_plan(&work_plan.id)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent ChangeSet is unavailable"))?;
    let pipeline_intent = state
        .store
        .get_pipeline_intent_by_change_set(&change_set.id)
        .await?
        .ok_or_else(|| ApiError::conflict("RollbackIntent PipelineIntent is unavailable"))?;
    let source_merge_sha = pipeline_intent
        .intent_json
        .pointer("/source_provenance/merge_commit_sha")
        .and_then(Value::as_str)
        .filter(|value| is_git_sha(value))
        .ok_or_else(|| ApiError::conflict("rollback source merge provenance is unavailable"))?;
    if grant.subject != DEFAULT_ARGO_RUNNER_SUBJECT
        || scope.environment.as_deref() != Some(PROTECTED_ENVIRONMENT)
        || scope.capability_kinds != vec![CapabilityKind::ArgoSync]
        || !ARGO_SYNC_ACTIONS
            .iter()
            .all(|action| scope.actions.iter().any(|allowed| allowed == action))
        || scope.namespaces != vec![PROTECTED_NAMESPACE.to_string()]
        || scope.work_item_ids != vec![item.id.clone()]
        || scope.work_plan_ids != vec![work_plan.id.clone()]
        || scope.change_set_ids != vec![change_set.id.clone()]
        || scope.pipeline_intent_ids != vec![pipeline_intent.id.clone()]
        || scope.deployment_intent_ids != vec![rollback_intent_id.to_string()]
        || scope.argo_applications != vec![PROTECTED_ARGO_APPLICATION.to_string()]
        || scope.pipeline_contract_ids != expected_pipeline_contracts
        || scope.deployment_contract_ids != expected_deployment_contracts
        || scope.source_merge_shas != vec![source_merge_sha.to_string()]
        || scope.gitops_merge_shas != vec![gitops_merge_sha.to_string()]
        || scope.image_digests != vec![baseline_digest.to_string()]
        || scope.production_impacting != Some(true)
    {
        return Err(ApiError::conflict(
            "rollback Argo grant no longer matches the exact WorkItem, contracts, merge, digest, namespace, and Application",
        ));
    }
    let expected_gate_kinds = [
        "production_rollback_deployment",
        "cluster_mutation",
        "production_impact",
    ];
    let gate_ids = content
        .get("argo_approval_gate_ids")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::conflict("rollback Argo approval gates are unavailable"))?;
    if gate_ids.len() != expected_gate_kinds.len() {
        return Err(ApiError::conflict(
            "rollback Argo approval gate set is incomplete",
        ));
    }
    for (gate_id, expected_kind) in gate_ids.iter().zip(expected_gate_kinds) {
        let gate_id = gate_id
            .as_str()
            .ok_or_else(|| ApiError::conflict("rollback Argo approval gate ID is malformed"))?;
        let gate = state
            .store
            .get_approval_gate(gate_id)
            .await?
            .filter(|gate| gate.status == "satisfied")
            .ok_or_else(|| ApiError::conflict("rollback Argo approval gate is not satisfied"))?;
        let gate_scope = gate
            .gate_json
            .get("scope")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                ApiError::conflict("rollback Argo approval gate scope is unavailable")
            })?;
        if gate.work_item_id.as_deref() != Some(item.id.as_str())
            || gate.gate_kind != expected_kind
            || gate_scope.get("work_item_id").and_then(Value::as_str) != Some(item.id.as_str())
            || gate_scope.get("work_plan_id").and_then(Value::as_str) != Some(work_plan.id.as_str())
            || gate_scope.get("environment").and_then(Value::as_str) != Some(PROTECTED_ENVIRONMENT)
            || gate_scope
                .get("production_impacting")
                .and_then(Value::as_bool)
                != Some(true)
            || gate_scope.get("target_namespace").and_then(Value::as_str)
                != Some(PROTECTED_NAMESPACE)
            || gate_scope.get("argo_application").and_then(Value::as_str)
                != Some(PROTECTED_ARGO_APPLICATION)
            || gate
                .gate_json
                .get("gitops_merge_sha")
                .and_then(Value::as_str)
                != Some(gitops_merge_sha)
            || gate
                .gate_json
                .get("baseline_digest")
                .and_then(Value::as_str)
                != Some(baseline_digest)
        {
            return Err(ApiError::conflict(
                "rollback Argo approval gate no longer matches its immutable target binding",
            ));
        }
    }
    Ok(grant)
}

pub(in crate::app) async fn append_rollback_argo_sync_result(
    state: &AppState,
    run: &pharness_store::StoredRun,
    rollback_intent_id: &str,
    execution_id: &str,
    status: &str,
    details: Value,
) -> Result<StoredArtifact, ApiError> {
    if let Some(existing) =
        state
            .store
            .list_artifacts(&run.id)
            .await?
            .into_iter()
            .find(|artifact| {
                artifact.kind == "rollback_argo_sync_result"
                    && artifact.content_json.as_ref().is_some_and(|content| {
                        content.get("rollback_intent_id").and_then(Value::as_str)
                            == Some(rollback_intent_id)
                            && content.get("execution_id").and_then(Value::as_str)
                                == Some(execution_id)
                            && content.get("status").and_then(Value::as_str) == Some(status)
                    })
            })
    {
        return Ok(existing);
    }
    Ok(state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_rollback_argo_sync_result", unique_suffix()),
            session_id: run.session_id.clone(),
            run_id: Some(run.id.clone()),
            kind: "rollback_argo_sync_result".to_string(),
            label: format!("Rollback Argo sync {status} for {rollback_intent_id}"),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "rollback_intent_id": rollback_intent_id,
                "execution_id": execution_id,
                "status": status,
                "details": details,
            })),
        })
        .await?)
}

pub(in crate::app) async fn observe_rollback_intent(
    State(state): State<AppState>,
    Path(rollback_intent_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let (_item, current, run) = rollback_intent_context(&state, &rollback_intent_id).await?;
    if current.pointer("/content/status").and_then(Value::as_str) == Some("argo_syncing") {
        return observe_rollback_argo_sync(&state, &rollback_intent_id, &current, &run).await;
    }
    require_fresh_capability(&state, "gitops_observer").await?;
    let settings = state
        .worker
        .gitops_observer_settings()
        .ok_or_else(|| ApiError::conflict("GitOps rollback observer is not configured"))?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == PROTECTED_GITOPS_REPO)
    {
        return Err(ApiError::conflict(
            "protected GitOps repository is not allowlisted for the rollback observer",
        ));
    }
    let artifacts = state.store.list_artifacts(&run.id).await?;
    let result = artifacts
        .iter()
        .filter(|artifact| artifact.kind == "rollback_delivery_result")
        .filter(|artifact| {
            artifact.content_json.as_ref().is_some_and(|value| {
                value.get("rollback_intent_id").and_then(Value::as_str)
                    == Some(rollback_intent_id.as_str())
                    && value.get("status").and_then(Value::as_str) == Some("completed")
            })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .ok_or_else(|| {
            ApiError::conflict(
                "rollback observation requires a completed GitOps pull-request delivery",
            )
        })?;
    let details = result
        .content_json
        .as_ref()
        .and_then(|value| value.get("details"))
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ApiError::conflict("rollback delivery result has no pull-request provenance")
        })?;
    let source_commit_sha =
        required_json_string(details, "commit_sha", "rollback delivery result")?;
    let pull_request_url =
        required_json_string(details, "pull_request_url", "rollback delivery result")?;
    let pull_request_number = details
        .get("pull_request_number")
        .and_then(Value::as_u64)
        .ok_or_else(|| ApiError::conflict("rollback delivery result has no pull-request number"))?;
    let head_branch = required_json_string(details, "branch", "rollback delivery result")?;
    if !is_git_sha(&source_commit_sha) || !is_github_pr_url(&pull_request_url) {
        return Err(ApiError::conflict(
            "rollback delivery result has invalid immutable GitHub provenance",
        ));
    }
    if let Some(merged) = artifacts
        .iter()
        .filter(|artifact| artifact.kind == "rollback_delivery_observation")
        .filter(|artifact| {
            artifact.content_json.as_ref().is_some_and(|value| {
                value.get("rollback_intent_id").and_then(Value::as_str)
                    == Some(rollback_intent_id.as_str())
                    && value.get("merged").and_then(Value::as_bool) == Some(true)
            })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
    {
        return Ok(Json(
            json!({ "rollback_intent_id": rollback_intent_id, "status": "merged", "observation": ArtifactResponse::from(merged.clone()), "created": false, "manual_merge_required": true }),
        ));
    }
    let observation_index = artifacts
        .iter()
        .filter(|artifact| {
            artifact.kind == "rollback_delivery_observation_execution"
                && artifact.content_json.as_ref().is_some_and(|value| {
                    value.get("rollback_intent_id").and_then(Value::as_str)
                        == Some(rollback_intent_id.as_str())
                })
        })
        .count();
    let execution_id = format!("rbobs_{}", unique_suffix());
    let execution = state.store.create_artifact(CreateArtifact {
        id: format!("art_{}_rollback_delivery_observation", unique_suffix()),
        session_id: run.session_id.clone(), run_id: Some(run.id.clone()),
        kind: "rollback_delivery_observation_execution".to_string(),
        label: format!("Rollback PR observation {} for {}", observation_index + 1, rollback_intent_id),
        mime_type: Some("application/json".to_string()), path: None, content_text: None,
        content_json: Some(json!({ "rollback_intent_id": rollback_intent_id, "execution_id": execution_id, "status": "dispatched", "source": { "repository": PROTECTED_GITOPS_REPO, "head_branch": head_branch, "source_commit_sha": source_commit_sha, "pull_request_url": pull_request_url, "pull_request_number": pull_request_number, "github_api_url": settings.github_api_url } })),
    }).await?;
    match state
        .worker
        .dispatch_gitops_delivery_observation(GitOpsDeliveryObservationRequest {
            gitops_change_set_id: rollback_intent_id.clone(),
            execution_id: execution_id.clone(),
        })
        .await
    {
        Ok(receipt) => Ok(Json(
            json!({ "rollback_intent_id": rollback_intent_id, "status": "observation_dispatched", "execution": ArtifactResponse::from(execution), "job_name": receipt.job_name, "created": true, "manual_merge_required": true }),
        )),
        Err(error) => {
            tracing::warn!(rollback_intent_id = %rollback_intent_id, %error, "rollback observer dispatch failed");
            let failure = append_rollback_delivery_observation(
                &state,
                &run,
                &rollback_intent_id,
                &execution_id,
                "failed",
                json!({ "error_code": "job_dispatch_failed" }),
            )
            .await?;
            Ok(Json(
                json!({ "rollback_intent_id": rollback_intent_id, "status": "dispatch_failed", "observation": ArtifactResponse::from(failure), "created": true, "manual_merge_required": true }),
            ))
        }
    }
}

pub(in crate::app) async fn observe_rollback_argo_sync(
    state: &AppState,
    rollback_intent_id: &str,
    current: &Value,
    run: &pharness_store::StoredRun,
) -> Result<Json<Value>, ApiError> {
    let content = current
        .get("content")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("RollbackIntent content is unavailable"))?;
    let execution_id = content
        .get("argo_execution_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("RollbackIntent has no Argo execution"))?;
    let expected_revision = content
        .get("gitops_merge_sha")
        .and_then(Value::as_str)
        .filter(|value| is_git_sha(value))
        .ok_or_else(|| ApiError::conflict("rollback GitOps merge SHA is unavailable"))?;
    let expected_digest = content
        .get("baseline")
        .and_then(|value| value.get("image_digest"))
        .and_then(Value::as_str)
        .filter(|value| immutable_image_digest(value))
        .ok_or_else(|| ApiError::conflict("RollbackIntent baseline digest is invalid"))?;
    let artifacts = state.store.list_artifacts(&run.id).await?;
    let completed = artifacts
        .iter()
        .filter(|artifact| artifact.kind == "rollback_argo_sync_result")
        .filter(|artifact| {
            artifact.content_json.as_ref().is_some_and(|value| {
                value.get("rollback_intent_id").and_then(Value::as_str) == Some(rollback_intent_id)
                    && value.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                    && value.get("status").and_then(Value::as_str) == Some("completed")
            })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id));
    if completed.is_none() {
        let terminal = artifacts.iter().find(|artifact| {
            artifact.kind == "rollback_argo_sync_result"
                && artifact.content_json.as_ref().is_some_and(|value| {
                    value.get("rollback_intent_id").and_then(Value::as_str)
                        == Some(rollback_intent_id)
                        && value.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                        && matches!(
                            value.get("status").and_then(Value::as_str),
                            Some("failed" | "cancelled" | "dispatch_failed")
                        )
                })
        });
        return Ok(Json(json!({
            "rollback_intent_id": rollback_intent_id,
            "status": if terminal.is_some() { "attention_required" } else { "argo_syncing" },
            "ready": false,
            "automatic_rollback": false,
            "result": terminal.cloned().map(ArtifactResponse::from),
        })));
    }

    let (item, _, _) = rollback_intent_context(state, rollback_intent_id).await?;
    let contract_id = item
        .deployment_contract_id
        .as_deref()
        .ok_or_else(|| ApiError::conflict("RollbackIntent DeploymentContract is unavailable"))?;
    let contract = state
        .store
        .get_deployment_contract(contract_id)
        .await?
        .filter(|contract| contract.status == "active")
        .ok_or_else(|| ApiError::conflict("RollbackIntent DeploymentContract is not active"))?;
    let spec = deployment_contract_spec(&contract.contract_json)?;
    validate_protected_production_deployment_contract(&spec)?;
    if contract.target_environment != PROTECTED_ENVIRONMENT
        || contract.target_namespace != PROTECTED_NAMESPACE
        || contract.argo_application != PROTECTED_ARGO_APPLICATION
    {
        return Err(ApiError::conflict(
            "RollbackIntent DeploymentContract no longer matches the protected target",
        ));
    }

    let argo = execute_direct_capability(
        state,
        AgentAction::ArgoGetApp {
            id: ActionId::new(format!("act_{}_rollback_verify_argo", unique_suffix())),
            reason: format!("verify RollbackIntent {rollback_intent_id} Argo state"),
            app: PROTECTED_ARGO_APPLICATION.to_string(),
        },
        None,
    )
    .await?;
    let deployment = execute_direct_capability(
        state,
        AgentAction::KubernetesGet {
            id: ActionId::new(format!(
                "act_{}_rollback_verify_deployment",
                unique_suffix()
            )),
            reason: format!("verify RollbackIntent {rollback_intent_id} Deployment"),
            resource: "deployments".to_string(),
            namespace: Some(PROTECTED_NAMESPACE.to_string()),
            name: Some(PROTECTED_WORKLOAD_NAME.to_string()),
            all_namespaces: false,
            label_selector: None,
        },
        None,
    )
    .await?;
    let pods = execute_direct_capability(
        state,
        AgentAction::KubernetesGet {
            id: ActionId::new(format!("act_{}_rollback_verify_pods", unique_suffix())),
            reason: format!("verify RollbackIntent {rollback_intent_id} Pod imageIDs"),
            resource: "pods".to_string(),
            namespace: Some(PROTECTED_NAMESPACE.to_string()),
            name: None,
            all_namespaces: false,
            label_selector: Some("app=yfinance-wrapper".to_string()),
        },
        None,
    )
    .await?;
    let argo_content = argo
        .result
        .as_ref()
        .map(|result| &result.content)
        .ok_or_else(|| ApiError::conflict("rollback Argo verification did not execute"))?;
    let deployment_content = deployment
        .result
        .as_ref()
        .map(|result| &result.content)
        .ok_or_else(|| ApiError::conflict("rollback Deployment verification did not execute"))?;
    let pod_content = pods
        .result
        .as_ref()
        .map(|result| &result.content)
        .ok_or_else(|| ApiError::conflict("rollback Pod verification did not execute"))?;
    let argo_healthy = argo_content
        .pointer("/analysis/sync_status")
        .and_then(Value::as_str)
        == Some("Synced")
        && argo_content
            .pointer("/analysis/health_status")
            .and_then(Value::as_str)
            == Some("Healthy")
        && argo_content
            .pointer("/analysis/revision")
            .and_then(Value::as_str)
            == Some(expected_revision);
    let deployment_healthy = deployment_content
        .pointer("/analysis/status")
        .and_then(Value::as_str)
        == Some("healthy");
    let image_ids = pod_content
        .pointer("/output/items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|pod| {
            pod.pointer("/status/containers")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(|container| container.get("imageID").and_then(Value::as_str))
        .collect::<Vec<_>>();
    let digest_healthy = !image_ids.is_empty()
        && image_ids
            .iter()
            .all(|image_id| image_id.ends_with(expected_digest));
    let healthz = state
        .worker
        .verify_capability("yfinance_healthz", None)
        .await;
    let healthz_healthy = healthz.as_ref().is_ok_and(|outcome| outcome.available);
    let (prometheus_observation, prometheus_check) =
        verify_required_prometheus_inventory(state, None).await?;
    let prometheus_healthy = prometheus_check
        .get("passed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let verified = argo_healthy
        && deployment_healthy
        && digest_healthy
        && healthz_healthy
        && prometheus_healthy;
    let checks = vec![
        execution_check(
            "argo_application_synced_healthy_at_rollback_merge",
            argo_healthy,
            format!("{PROTECTED_ARGO_APPLICATION} checked at {expected_revision}"),
        ),
        execution_check(
            "declared_deployment_rollout_healthy",
            deployment_healthy,
            format!("{PROTECTED_NAMESPACE}/{PROTECTED_WORKLOAD_NAME} rollout checked"),
        ),
        execution_check(
            "running_image_digest",
            digest_healthy,
            format!(
                "{} running imageID(s) checked against {expected_digest}",
                image_ids.len()
            ),
        ),
        execution_check(
            "service_healthz",
            healthz_healthy,
            "Exact apps-prod/yfinance-wrapper Service /healthz checked",
        ),
        prometheus_check,
    ];
    let verification = state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_rollback_verification", unique_suffix()),
            session_id: run.session_id.clone(),
            run_id: Some(run.id.clone()),
            kind: "rollback_verification".to_string(),
            label: format!("Rollback verification for {rollback_intent_id}"),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "rollback_intent_id": rollback_intent_id,
                "status": if verified { "verified" } else { "attention_required" },
                "gitops_merge_sha": expected_revision,
                "expected_digest": expected_digest,
                "argo_observation_id": argo.observation_id,
                "deployment_observation_id": deployment.observation_id,
                "pod_observation_id": pods.observation_id,
                "prometheus_observation_id": prometheus_observation.map(|observation| observation.id),
                "checks": checks,
            })),
        })
        .await?;
    let mut updated = content.clone();
    updated.insert(
        "status".to_string(),
        json!(if verified {
            "verified"
        } else {
            "attention_required"
        }),
    );
    updated.insert(
        "verification_artifact_id".to_string(),
        json!(verification.id),
    );
    append_rollback_intent_artifact(state, run, &Value::Object(updated)).await?;
    Ok(Json(json!({
        "rollback_intent_id": rollback_intent_id,
        "status": if verified { "verified" } else { "attention_required" },
        "verified": verified,
        "verification": ArtifactResponse::from(verification),
        "automatic_rollback": false,
    })))
}

pub(in crate::app) async fn require_fresh_capability(
    state: &AppState,
    capability: &str,
) -> Result<(), ApiError> {
    let available = capability_statuses(state)
        .await?
        .into_iter()
        .any(|entry| entry.capability == capability && entry.status == "available");
    if available {
        Ok(())
    } else {
        Err(ApiError::conflict(format!(
            "{capability} requires a fresh passing isolated capability verification"
        )))
    }
}

pub(in crate::app) async fn append_rollback_delivery_result(
    state: &AppState,
    run: &pharness_store::StoredRun,
    rollback_intent_id: &str,
    execution_id: &str,
    status: &str,
    details: Value,
) -> Result<StoredArtifact, ApiError> {
    Ok(state.store.create_artifact(CreateArtifact { id: format!("art_{}_rollback_delivery_result", unique_suffix()), session_id: run.session_id.clone(), run_id: Some(run.id.clone()), kind: "rollback_delivery_result".to_string(), label: format!("Rollback GitOps delivery {status} for {rollback_intent_id}"), mime_type: Some("application/json".to_string()), path: None, content_text: None, content_json: Some(json!({ "rollback_intent_id": rollback_intent_id, "execution_id": execution_id, "status": status, "details": details })) }).await?)
}

pub(in crate::app) async fn append_rollback_delivery_observation(
    state: &AppState,
    run: &pharness_store::StoredRun,
    rollback_intent_id: &str,
    execution_id: &str,
    status: &str,
    details: Value,
) -> Result<StoredArtifact, ApiError> {
    Ok(state.store.create_artifact(CreateArtifact { id: format!("art_{}_rollback_delivery_observation", unique_suffix()), session_id: run.session_id.clone(), run_id: Some(run.id.clone()), kind: "rollback_delivery_observation".to_string(), label: format!("Rollback pull-request observation {status} for {rollback_intent_id}"), mime_type: Some("application/json".to_string()), path: None, content_text: None, content_json: Some(json!({ "rollback_intent_id": rollback_intent_id, "execution_id": execution_id, "status": status, "details": details, "pull_request_state": details.get("pull_request_state"), "merged": details.get("merged"), "merge_commit_sha": details.get("merge_commit_sha") })) }).await?)
}

pub(in crate::app) async fn rollback_intent_context(
    state: &AppState,
    rollback_intent_id: &str,
) -> Result<(StoredWorkItem, Value, pharness_store::StoredRun), ApiError> {
    let items = state
        .store
        .list_work_items(WorkItemListFilter {
            limit: 200,
            ..Default::default()
        })
        .await?;
    for item in items {
        if let Some(current) =
            latest_rollback_intent(state, &item, Some(rollback_intent_id)).await?
        {
            let run_id = work_item_provenance_run_id(state, &item)
                .await?
                .ok_or_else(|| {
                    ApiError::conflict("RollbackIntent run provenance is unavailable")
                })?;
            let run = state
                .store
                .get_run(&run_id)
                .await?
                .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;
            return Ok((item, current, run));
        }
    }
    Err(ApiError::not_found("rollback_intent", rollback_intent_id))
}

pub(in crate::app) async fn append_rollback_intent_artifact(
    state: &AppState,
    run: &pharness_store::StoredRun,
    content: &Value,
) -> Result<StoredArtifact, ApiError> {
    Ok(state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_rollback_intent", unique_suffix()),
            session_id: run.session_id.clone(),
            run_id: Some(run.id.clone()),
            kind: "rollback_intent".to_string(),
            label: format!(
                "RollbackIntent {}",
                content
                    .get("rollback_intent_id")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
            ),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(content.clone()),
        })
        .await?)
}

pub(in crate::app) async fn current_rollback_argo_sync_execution(
    state: &AppState,
    rollback_intent_id: &str,
    execution_id: &str,
) -> Result<
    (
        StoredWorkItem,
        Value,
        pharness_store::StoredRun,
        StoredArtifact,
    ),
    ApiError,
> {
    let (item, current, run) = rollback_intent_context(state, rollback_intent_id).await?;
    let artifacts = state.store.list_artifacts(&run.id).await?;
    let execution = artifacts
        .iter()
        .filter(|artifact| artifact.kind == "rollback_argo_sync_execution")
        .filter(|artifact| {
            artifact.content_json.as_ref().is_some_and(|content| {
                content.get("rollback_intent_id").and_then(Value::as_str)
                    == Some(rollback_intent_id)
                    && content.get("execution_id").and_then(Value::as_str) == Some(execution_id)
            })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .ok_or_else(|| ApiError::conflict("rollback Argo sync execution is unavailable"))?;
    let latest = artifacts
        .iter()
        .filter(|artifact| artifact.kind == "rollback_argo_sync_execution")
        .filter(|artifact| {
            artifact.content_json.as_ref().is_some_and(|content| {
                content.get("rollback_intent_id").and_then(Value::as_str)
                    == Some(rollback_intent_id)
            })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id));
    if latest.map(|artifact| artifact.id.as_str()) != Some(execution.id.as_str()) {
        return Err(ApiError::conflict(
            "rollback Argo sync execution is no longer current",
        ));
    }
    Ok((item, current, run, execution))
}

pub(in crate::app) async fn internal_rollback_argo_sync_context(
    state: &AppState,
    rollback_intent_id: &str,
    execution_id: &str,
) -> Result<Json<ArgoSyncContextResponse>, ApiError> {
    let (item, current, _run, execution) =
        current_rollback_argo_sync_execution(state, rollback_intent_id, execution_id).await?;
    let content = current
        .get("content")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("RollbackIntent content is unavailable"))?;
    validate_rollback_argo_grant(state, &item, rollback_intent_id, content).await?;
    if current.pointer("/content/status").and_then(Value::as_str) != Some("argo_syncing")
        || !state
            .worker
            .argo_executor_allows_application(PROTECTED_ARGO_APPLICATION)
    {
        return Err(ApiError::conflict(
            "rollback Argo sync is no longer authorized for the protected Application",
        ));
    }
    let revision = content
        .get("gitops_merge_sha")
        .and_then(Value::as_str)
        .filter(|value| is_git_sha(value))
        .ok_or_else(|| ApiError::conflict("rollback GitOps merge SHA is unavailable"))?;
    let execution_content = execution
        .content_json
        .as_ref()
        .ok_or_else(|| ApiError::conflict("rollback Argo execution content is unavailable"))?;
    if execution_content
        .get("permission_grant_id")
        .and_then(Value::as_str)
        != content
            .get("argo_permission_grant_id")
            .and_then(Value::as_str)
        || execution_content
            .get("gitops_merge_sha")
            .and_then(Value::as_str)
            != Some(revision)
    {
        return Err(ApiError::conflict(
            "rollback Argo execution is stale relative to its grant or observed merge",
        ));
    }
    Ok(Json(ArgoSyncContextResponse {
        execution_id: execution_id.to_string(),
        target_namespace: PROTECTED_NAMESPACE.to_string(),
        argo_application: PROTECTED_ARGO_APPLICATION.to_string(),
        revision: Some(revision.to_string()),
        poll_seconds: argo_executor_poll_seconds(&state.worker.config_json()),
    }))
}

pub(in crate::app) async fn internal_rollback_argo_sync_control(
    state: &AppState,
    rollback_intent_id: &str,
) -> Result<Json<ArgoSyncControlResponse>, ApiError> {
    let (item, _current, _run) = rollback_intent_context(state, rollback_intent_id).await?;
    Ok(Json(ArgoSyncControlResponse {
        cancelled: item.status == "cancelled",
    }))
}

pub(in crate::app) async fn internal_rollback_argo_sync_outcome(
    state: &AppState,
    rollback_intent_id: &str,
    request: ArgoSyncOutcomeRequest,
) -> Result<Json<ArtifactResponse>, ApiError> {
    let (_item, current, run, _execution) =
        current_rollback_argo_sync_execution(state, rollback_intent_id, &request.execution_id)
            .await?;
    let expected_revision = current
        .pointer("/content/gitops_merge_sha")
        .and_then(Value::as_str)
        .filter(|value| is_git_sha(value))
        .ok_or_else(|| ApiError::conflict("rollback GitOps merge SHA is unavailable"))?;
    let result = match request.status.as_str() {
        "submitted" => {
            append_rollback_argo_sync_result(
                state,
                &run,
                rollback_intent_id,
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
            let revision = clean_optional_text(request.revision)
                .filter(|revision| revision == expected_revision)
                .ok_or_else(|| {
                    ApiError::conflict(
                        "rollback Argo outcome revision does not match the observed GitOps merge",
                    )
                })?;
            if sync_status != "Synced" || operation_phase != "Succeeded" {
                return Err(ApiError::conflict(
                    "completed rollback Argo outcome requires Synced and Succeeded",
                ));
            }
            append_rollback_argo_sync_result(
                state,
                &run,
                rollback_intent_id,
                &request.execution_id,
                "completed",
                json!({
                    "sync_status": sync_status,
                    "health_status": clean_optional_text(request.health_status),
                    "operation_phase": operation_phase,
                    "revision": revision,
                }),
            )
            .await?
        }
        "failed" | "cancelled" => append_rollback_argo_sync_result(
            state,
            &run,
            rollback_intent_id,
            &request.execution_id,
            &request.status,
            json!({
                "error_code": normalized_executor_error_code(
                    request.error_code,
                    if request.status == "cancelled" { "cancelled" } else { "argo_sync_failed" },
                ),
                "sync_status": clean_optional_text(request.sync_status),
                "health_status": clean_optional_text(request.health_status),
                "operation_phase": clean_optional_text(request.operation_phase),
                "revision": clean_optional_text(request.revision),
            }),
        )
        .await?,
        _ => {
            return Err(ApiError::bad_request(
                "rollback Argo outcome status must be submitted, completed, failed, or cancelled",
            ))
        }
    };
    if matches!(request.status.as_str(), "failed" | "cancelled") {
        let mut updated = current
            .get("content")
            .and_then(Value::as_object)
            .cloned()
            .ok_or_else(|| ApiError::conflict("RollbackIntent content is unavailable"))?;
        updated.insert("status".to_string(), json!("attention_required"));
        updated.insert("argo_failure_result_id".to_string(), json!(result.id));
        append_rollback_intent_artifact(state, &run, &Value::Object(updated)).await?;
    }
    Ok(Json(result.into()))
}

pub(in crate::app) async fn internal_rollback_delivery_context(
    state: &AppState,
    rollback_intent_id: &str,
    execution_id: &str,
) -> Result<Json<GitOpsDeliveryContextResponse>, ApiError> {
    let (_item, _intent, run) = rollback_intent_context(state, rollback_intent_id).await?;
    let execution = state
        .store
        .list_artifacts(&run.id)
        .await?
        .into_iter()
        .find(|artifact| {
            artifact.kind == "rollback_delivery_execution"
                && artifact.content_json.as_ref().is_some_and(|value| {
                    value.get("rollback_intent_id").and_then(Value::as_str)
                        == Some(rollback_intent_id)
                        && value.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                })
        })
        .ok_or_else(|| ApiError::conflict("rollback delivery execution is not current"))?;
    let context = execution
        .content_json
        .as_ref()
        .and_then(|value| value.get("context"))
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ApiError::conflict("rollback delivery execution has no immutable context")
        })?;
    let repository = required_json_string(context, "repository", "rollback delivery context")?;
    let settings = state
        .worker
        .gitops_writer_settings()
        .ok_or_else(|| ApiError::conflict("GitOps rollback writer is not configured"))?;
    if repository != PROTECTED_GITOPS_REPO
        || !settings
            .allowed_repos
            .iter()
            .any(|repo| repo == &repository)
    {
        return Err(ApiError::conflict(
            "rollback repository is not allowlisted for the isolated GitOps writer",
        ));
    }
    let response = GitOpsDeliveryContextResponse {
        execution_id: required_json_string(context, "execution_id", "rollback delivery context")?,
        repository,
        base_ref: required_json_string(context, "base_ref", "rollback delivery context")?,
        base_commit: required_json_string(context, "base_commit", "rollback delivery context")?,
        head_branch: required_json_string(context, "head_branch", "rollback delivery context")?,
        kustomization_path: required_json_string(
            context,
            "kustomization_path",
            "rollback delivery context",
        )?,
        image_name: required_json_string(context, "image_name", "rollback delivery context")?,
        image_ref: required_json_string(context, "image_ref", "rollback delivery context")?,
        commit_subject: required_json_string(
            context,
            "commit_subject",
            "rollback delivery context",
        )?,
        commit_body: required_json_string(context, "commit_body", "rollback delivery context")?,
        pull_request_title: required_json_string(
            context,
            "pull_request_title",
            "rollback delivery context",
        )?,
        pull_request_body: required_json_string(
            context,
            "pull_request_body",
            "rollback delivery context",
        )?,
        github_api_url: settings.github_api_url,
        author_name: settings.author_name,
        author_email: settings.author_email,
    };
    if response.execution_id != execution_id
        || response.kustomization_path != PROTECTED_KUSTOMIZATION_PATH
        || response.image_name != PROTECTED_IMAGE_NAME
        || !response
            .image_ref
            .starts_with(&format!("{PROTECTED_IMAGE_NAME}@sha256:"))
        || !is_git_sha(&response.base_commit)
    {
        return Err(ApiError::conflict(
            "rollback delivery context no longer matches the protected target",
        ));
    }
    Ok(Json(response))
}

pub(in crate::app) async fn internal_rollback_delivery_outcome(
    state: &AppState,
    rollback_intent_id: &str,
    request: GitOpsDeliveryOutcomeRequest,
) -> Result<Json<ArtifactResponse>, ApiError> {
    let Json(context) =
        internal_rollback_delivery_context(state, rollback_intent_id, &request.execution_id)
            .await?;
    let (_item, current, run) = rollback_intent_context(state, rollback_intent_id).await?;
    let result = match request.status.as_str() {
        "completed" => {
            let branch = clean_optional_text(request.branch).ok_or_else(|| ApiError::bad_request("completed rollback delivery requires branch"))?;
            let commit_sha = clean_optional_text(request.commit_sha).ok_or_else(|| ApiError::bad_request("completed rollback delivery requires commit_sha"))?;
            let pull_request_url = clean_optional_text(request.pull_request_url).ok_or_else(|| ApiError::bad_request("completed rollback delivery requires pull_request_url"))?;
            let pull_request_number = request.pull_request_number.ok_or_else(|| ApiError::bad_request("completed rollback delivery requires pull_request_number"))?;
            let expected_prefix = "https://github.com/lward27/lucas_engineering/pull/";
            if branch != context.head_branch || !is_git_sha(&commit_sha) || !pull_request_url.starts_with(expected_prefix) || !pull_request_url.ends_with(&pull_request_number.to_string()) {
                return Err(ApiError::conflict("rollback delivery outcome does not match immutable GitHub provenance"));
            }
            append_rollback_delivery_result(state, &run, rollback_intent_id, &request.execution_id, "completed", json!({ "branch": branch, "commit_sha": commit_sha, "pull_request_url": pull_request_url, "pull_request_number": pull_request_number })).await?
        }
        "failed" => append_rollback_delivery_result(state, &run, rollback_intent_id, &request.execution_id, "failed", json!({ "error_code": clean_optional_text(request.error_code).unwrap_or_else(|| "gitops_writer_failed".to_string()) })).await?,
        _ => return Err(ApiError::bad_request("rollback delivery outcome status must be completed or failed")),
    };
    if request.status == "completed" {
        let mut content = current
            .get("content")
            .and_then(Value::as_object)
            .cloned()
            .ok_or_else(|| ApiError::conflict("RollbackIntent content is unavailable"))?;
        content.insert("status".to_string(), json!("awaiting_manual_merge"));
        content.insert(
            "pull_request".to_string(),
            result
                .content_json
                .as_ref()
                .and_then(|value| value.get("details"))
                .cloned()
                .unwrap_or(Value::Null),
        );
        append_rollback_intent_artifact(state, &run, &Value::Object(content)).await?;
    } else if request.status == "failed" {
        let mut content = current
            .get("content")
            .and_then(Value::as_object)
            .cloned()
            .ok_or_else(|| ApiError::conflict("RollbackIntent content is unavailable"))?;
        content.insert("status".to_string(), json!("attention_required"));
        content.insert("writer_failure_result_id".to_string(), json!(result.id));
        append_rollback_intent_artifact(state, &run, &Value::Object(content)).await?;
    }
    Ok(Json(result.into()))
}

pub(in crate::app) async fn internal_rollback_delivery_observation_context(
    state: &AppState,
    rollback_intent_id: &str,
    execution_id: &str,
) -> Result<Json<GitOpsDeliveryObservationContextResponse>, ApiError> {
    let (_item, _intent, run) = rollback_intent_context(state, rollback_intent_id).await?;
    let execution = state
        .store
        .list_artifacts(&run.id)
        .await?
        .into_iter()
        .find(|artifact| {
            artifact.kind == "rollback_delivery_observation_execution"
                && artifact.content_json.as_ref().is_some_and(|value| {
                    value.get("rollback_intent_id").and_then(Value::as_str)
                        == Some(rollback_intent_id)
                        && value.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                })
        })
        .ok_or_else(|| ApiError::conflict("rollback observation execution is not current"))?;
    let source = execution
        .content_json
        .as_ref()
        .and_then(|value| value.get("source"))
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("rollback observation has no source provenance"))?;
    let repository = required_json_string(source, "repository", "rollback observation source")?;
    let settings = state
        .worker
        .gitops_observer_settings()
        .ok_or_else(|| ApiError::conflict("GitOps rollback observer is not configured"))?;
    if repository != PROTECTED_GITOPS_REPO
        || !settings
            .allowed_repos
            .iter()
            .any(|repo| repo == &repository)
    {
        return Err(ApiError::conflict(
            "rollback repository is not allowlisted for the isolated GitOps observer",
        ));
    }
    Ok(Json(GitOpsDeliveryObservationContextResponse {
        execution_id: execution_id.to_string(),
        repository,
        head_branch: required_json_string(source, "head_branch", "rollback observation source")?,
        source_commit_sha: required_json_string(
            source,
            "source_commit_sha",
            "rollback observation source",
        )?,
        pull_request_url: required_json_string(
            source,
            "pull_request_url",
            "rollback observation source",
        )?,
        pull_request_number: source
            .get("pull_request_number")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                ApiError::conflict("rollback observation source has no pull-request number")
            })?,
        github_api_url: settings.github_api_url,
    }))
}

pub(in crate::app) async fn internal_rollback_delivery_observation_outcome(
    state: &AppState,
    rollback_intent_id: &str,
    request: GitOpsDeliveryObservationOutcomeRequest,
) -> Result<Json<ArtifactResponse>, ApiError> {
    let Json(context) = internal_rollback_delivery_observation_context(
        state,
        rollback_intent_id,
        &request.execution_id,
    )
    .await?;
    let (_item, current, run) = rollback_intent_context(state, rollback_intent_id).await?;
    let artifact = match request.status.as_str() {
        "observed" => {
            let pull_request_state = clean_optional_text(request.pull_request_state).ok_or_else(|| ApiError::bad_request("observed rollback outcome requires pull_request_state"))?;
            let merged = request.merged.ok_or_else(|| ApiError::bad_request("observed rollback outcome requires merged"))?;
            let head_branch = clean_optional_text(request.head_branch).ok_or_else(|| ApiError::bad_request("observed rollback outcome requires head_branch"))?;
            let head_commit_sha = clean_optional_text(request.head_commit_sha).ok_or_else(|| ApiError::bad_request("observed rollback outcome requires head_commit_sha"))?;
            let merge_commit_sha = clean_optional_text(request.merge_commit_sha);
            if !matches!(pull_request_state.as_str(), "open" | "closed") || head_branch != context.head_branch || head_commit_sha != context.source_commit_sha || !is_git_sha(&head_commit_sha) || (merged && (pull_request_state != "closed" || !merge_commit_sha.as_deref().is_some_and(is_git_sha))) || (!merged && merge_commit_sha.is_some()) {
                return Err(ApiError::conflict("rollback observation does not match the delivered immutable pull request"));
            }
            append_rollback_delivery_observation(state, &run, rollback_intent_id, &request.execution_id, "observed", json!({ "pull_request_state": pull_request_state, "merged": merged, "head_branch": head_branch, "head_commit_sha": head_commit_sha, "merge_commit_sha": merge_commit_sha })).await?
        }
        "failed" => append_rollback_delivery_observation(state, &run, rollback_intent_id, &request.execution_id, "failed", json!({ "error_code": clean_optional_text(request.error_code).unwrap_or_else(|| "gitops_observer_failed".to_string()) })).await?,
        _ => return Err(ApiError::bad_request("rollback observation outcome status must be observed or failed")),
    };
    if request.status == "observed" {
        let merged = artifact
            .content_json
            .as_ref()
            .and_then(|value| value.get("merged"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut content = current
            .get("content")
            .and_then(Value::as_object)
            .cloned()
            .ok_or_else(|| ApiError::conflict("RollbackIntent content is unavailable"))?;
        content.insert(
            "status".to_string(),
            json!(if merged {
                "ready_for_argo_sync"
            } else {
                "awaiting_manual_merge"
            }),
        );
        if merged {
            content.insert(
                "gitops_merge_sha".to_string(),
                artifact
                    .content_json
                    .as_ref()
                    .and_then(|value| value.get("merge_commit_sha"))
                    .cloned()
                    .unwrap_or(Value::Null),
            );
        }
        append_rollback_intent_artifact(state, &run, &Value::Object(content)).await?;
    } else if request.status == "failed" {
        let mut content = current
            .get("content")
            .and_then(Value::as_object)
            .cloned()
            .ok_or_else(|| ApiError::conflict("RollbackIntent content is unavailable"))?;
        content.insert("status".to_string(), json!("attention_required"));
        content.insert("observer_failure_result_id".to_string(), json!(artifact.id));
        append_rollback_intent_artifact(state, &run, &Value::Object(content)).await?;
    }
    Ok(Json(artifact.into()))
}

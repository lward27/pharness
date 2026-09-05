use super::super::approvals::{
    create_permission_grant_record, ensure_approved_for_trusted_envelope, grant_is_unexpired,
};
use super::super::audit::append_change_set_audit_event;
use super::super::auth::OperatorIdentity;
use super::super::clock::{current_millis, unique_suffix};
use super::super::delivery_actions::GIT_DELIVERY_ACTIONS;
use super::super::execution_checks::execution_check;
use super::super::identifiers::{is_git_sha, is_github_pr_url};
use super::super::principals::DEFAULT_GIT_WRITER_SUBJECT;
use super::super::risk::risk_rank;
use super::super::text::compact_delivery_subject;
use super::super::validation::{clean_optional_text, required_json_string};
use super::super::work_items::lifecycle::work_item_gate_scope_matches;
use super::super::work_items::preflight::{
    bounded_production_grant_expiry, work_item_target_supported,
};
use super::super::{ApiError, AppState};
use super::change_sets::coding_run_scope_matches_source;
use super::delivery_flow::{
    git_delivery_artifact_matches_plan, git_delivery_plan_matches_change_set,
};
use crate::dispatch::{GitDeliveryExecutionRequest, GitDeliveryObservationRequest};
use crate::dto::{
    ArtifactResponse, CreateGitDeliveryAuthorizationRequest, CreatePermissionGrantRequest,
    ExecuteGitDeliveryRequest, ExecuteGitDeliveryResponse, GitDeliveryAuthorizationResponse,
    GitDeliveryContextResponse, GitDeliveryObservationContextResponse,
    GitDeliveryObservationOutcomeRequest, GitDeliveryOutcomeRequest, GitDeliveryPlanResponse,
    GitDeliveryPreflightRequest, GitDeliveryPreflightResponse, ObserveGitDeliveryRequest,
    ObserveGitDeliveryResponse, PrepareGitDeliveryRequest,
};
use axum::extract::{Path, Query, State};
use axum::{Extension, Json};
use pharness_core::{
    CapabilityKind, PermissionGrantPolicy, PermissionGrantScope, PolicyMode, RiskLevel, RunId,
    RunScope,
};
use pharness_runhost::WorkspaceSourceSpec;
use pharness_store::{
    ApprovalGateListFilter, CreateArtifact, SqliteStore, StoredArtifact, StoredChangeSet,
    StoredPermissionGrant, StoredWorkItem,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub(in crate::app) async fn prepare_change_set_git_delivery(
    State(state): State<AppState>,
    Path(change_set_id): Path<String>,
    Json(request): Json<PrepareGitDeliveryRequest>,
) -> Result<Json<GitDeliveryPlanResponse>, ApiError> {
    let change_set = state
        .store
        .get_change_set(&change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", &change_set_id))?;
    ensure_approved_for_trusted_envelope("change_set", &change_set.id, &change_set.status)?;
    let work_plan = state
        .store
        .get_work_plan(&change_set.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &change_set.work_plan_id))?;
    ensure_approved_for_trusted_envelope("work_plan", &work_plan.id, &work_plan.status)?;
    let work_item_id = change_set.work_item_id.as_deref().ok_or_else(|| {
        ApiError::conflict("Git delivery preflight requires a WorkItem-backed ChangeSet")
    })?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    if !work_item_target_supported(&work_item) {
        return Err(ApiError::conflict(
            "Git delivery preflight is limited to dev or the exact protected production target",
        ));
    }
    let run_id = change_set
        .run_id
        .clone()
        .ok_or_else(|| ApiError::conflict("ChangeSet has no coding run provenance"))?;
    let source = change_set
        .change_set_json
        .get("source")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("ChangeSet has no workspace Git source provenance"))?;
    if source.get("kind").and_then(Value::as_str) != Some("workspace_git") {
        return Err(ApiError::conflict(
            "Git delivery preflight requires workspace Git source provenance",
        ));
    }
    let workspace_id = required_json_string(source, "workspace_id", "workspace source")?;
    let base_commit = required_json_string(source, "base_commit", "workspace source")?;
    let branch = required_json_string(source, "branch", "workspace source")?;
    WorkspaceSourceSpec {
        workspace_id: workspace_id.clone(),
        source_repo: work_item.source_repo.clone(),
        source_ref: work_item.source_ref.clone(),
        source_commit: None,
        branch: branch.clone(),
        resolved_commit: Some(base_commit.clone()),
    }
    .validate()
    .map_err(|error| ApiError::conflict(error.to_string()))?;
    let workspace = state
        .store
        .get_workspace(&workspace_id)
        .await?
        .ok_or_else(|| ApiError::conflict("ChangeSet workspace provenance is unavailable"))?;
    if workspace.work_item_id != work_item.id
        || workspace.run_id.as_ref() != Some(&run_id)
        || workspace.source_repo != work_item.source_repo
        || workspace.source_ref != work_item.source_ref
        || workspace.resolved_commit.as_deref() != Some(base_commit.as_str())
        || workspace.branch.as_deref() != Some(branch.as_str())
    {
        return Err(ApiError::conflict(
            "ChangeSet source provenance does not match the durable workspace",
        ));
    }
    let run = state
        .store
        .get_run(&run_id)
        .await?
        .ok_or_else(|| ApiError::not_found("run", run_id.as_str()))?;
    let run_scope = RunScope::from_execution_target(&run.execution_target_json).unwrap_or_default();
    if !coding_run_scope_matches_source(
        &run_scope,
        &work_item.id,
        &workspace.id,
        &work_item.source_repo,
        &branch,
        work_item.production_impacting,
    ) {
        return Err(ApiError::conflict(
            "ChangeSet source provenance does not match the coding run scope",
        ));
    }

    let evidence = change_set
        .change_set_json
        .get("evidence")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("ChangeSet has no Git evidence provenance"))?;
    let diff_artifact_id = required_json_string(evidence, "git_diff_artifact_id", "Git evidence")?;
    let status_artifact_id =
        required_json_string(evidence, "git_status_artifact_id", "Git evidence")?;
    let expected_diff_sha256 = required_json_string(evidence, "diff_sha256", "Git evidence")?;
    let artifacts = state.store.list_artifacts(&run_id).await?;
    let diff_artifact = artifacts
        .iter()
        .find(|artifact| artifact.id == diff_artifact_id && artifact.kind == "workspace_git_diff")
        .ok_or_else(|| ApiError::conflict("ChangeSet Git diff artifact is unavailable"))?;
    let diff = diff_artifact
        .content_text
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::conflict("ChangeSet Git diff artifact has no diff content"))?;
    if diff.len() > 512 * 1024
        || format!("{:x}", Sha256::digest(diff.as_bytes())) != expected_diff_sha256
    {
        return Err(ApiError::conflict(
            "ChangeSet Git diff artifact does not match its recorded digest",
        ));
    }
    let status_artifact = artifacts
        .iter()
        .find(|artifact| {
            artifact.id == status_artifact_id && artifact.kind == "workspace_git_status"
        })
        .ok_or_else(|| ApiError::conflict("ChangeSet Git status artifact is unavailable"))?;
    let changed_paths = status_artifact
        .content_json
        .as_ref()
        .and_then(|content| content.get("changed_paths"))
        .and_then(Value::as_array)
        .filter(|paths| !paths.is_empty())
        .ok_or_else(|| ApiError::conflict("ChangeSet Git status artifact has no changed paths"))?;
    if changed_paths.iter().any(|path| path.as_str().is_none()) {
        return Err(ApiError::conflict(
            "ChangeSet Git status artifact has an invalid changed path",
        ));
    }

    if let Some(existing) = artifacts.iter().find(|artifact| {
        artifact.kind == "git_delivery_plan"
            && artifact.content_json.as_ref().is_some_and(|plan| {
                plan.get("change_set")
                    .and_then(|value| value.get("id"))
                    .and_then(Value::as_str)
                    == Some(change_set.id.as_str())
                    && plan
                        .get("change_set")
                        .and_then(|value| value.get("revision"))
                        .and_then(Value::as_i64)
                        == Some(change_set.revision)
                    && plan
                        .get("change_set")
                        .and_then(|value| value.get("material_hash"))
                        .and_then(Value::as_str)
                        == Some(change_set.material_hash.as_str())
            })
    }) {
        return Ok(Json(GitDeliveryPlanResponse {
            artifact: existing.clone().into(),
            created: false,
        }));
    }

    let title = compact_delivery_subject(&change_set.title);
    let plan = json!({
        "kind": "git_delivery_plan",
        "version": 1,
        "operation": "branch_and_pull_request",
        "change_set": {
            "id": change_set.id,
            "revision": change_set.revision,
            "material_hash": change_set.material_hash,
            "work_plan_id": change_set.work_plan_id,
            "work_item_id": work_item.id,
        },
        "source": {
            "repository": work_item.source_repo,
            "base_ref": work_item.source_ref,
            "base_commit": base_commit,
            "head_branch": branch,
            "workspace_id": workspace_id,
        },
        "evidence": {
            "git_diff_artifact_id": diff_artifact.id,
            "git_status_artifact_id": status_artifact.id,
            "diff_sha256": expected_diff_sha256,
            "changed_paths": changed_paths,
        },
        "commit": {
            "subject": title,
            "body": format!("ChangeSet {} revision {}\n\n{}", change_set.id, change_set.revision, change_set.summary),
        },
        "pull_request": {
            "title": compact_delivery_subject(&change_set.title),
            "body": format!("{}\n\nPharness ChangeSet: {}\nWorkItem: {}", change_set.summary, change_set.id, work_item.id),
        },
        "authorization": {
            "state": "not_authorized",
            "reason": "Git writer identity and typed Git delivery grant are required before this plan can execute",
        },
    });
    let artifact = state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_git_delivery", unique_suffix()),
            session_id: change_set.session_id.clone(),
            run_id: Some(run_id),
            kind: "git_delivery_plan".to_string(),
            label: format!("Git delivery plan for ChangeSet {}", change_set.id),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(plan),
        })
        .await?;
    append_change_set_audit_event(
        &state.store,
        &change_set,
        "change_set.git_delivery_prepared",
        clean_optional_text(request.actor),
        clean_optional_text(request.reason),
        json!({ "git_delivery_plan_artifact_id": artifact.id }),
    )
    .await?;

    Ok(Json(GitDeliveryPlanResponse {
        artifact: artifact.into(),
        created: true,
    }))
}

pub(in crate::app) async fn authorize_change_set_git_delivery(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(change_set_id): Path<String>,
    Json(request): Json<CreateGitDeliveryAuthorizationRequest>,
) -> Result<Json<GitDeliveryAuthorizationResponse>, ApiError> {
    let change_set = state
        .store
        .get_change_set(&change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", &change_set_id))?;
    ensure_approved_for_trusted_envelope("change_set", &change_set.id, &change_set.status)?;
    let work_plan = state
        .store
        .get_work_plan(&change_set.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &change_set.work_plan_id))?;
    ensure_approved_for_trusted_envelope("work_plan", &work_plan.id, &work_plan.status)?;
    let work_item_id = change_set.work_item_id.as_deref().ok_or_else(|| {
        ApiError::conflict("Git delivery authorization requires a WorkItem-backed ChangeSet")
    })?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    if !work_item_target_supported(&work_item) {
        return Err(ApiError::conflict(
            "Git delivery authorization is limited to dev or the exact protected production target",
        ));
    }
    let run_id = change_set
        .run_id
        .clone()
        .ok_or_else(|| ApiError::conflict("ChangeSet has no coding run provenance"))?;
    let plan = current_git_delivery_plan(&state.store, &run_id, &change_set).await?;
    let source = git_delivery_plan_source(&plan, &work_item)?;
    let branch = source.head_branch;

    let subject = clean_optional_text(request.subject)
        .unwrap_or_else(|| DEFAULT_GIT_WRITER_SUBJECT.to_string());
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.created_by.clone()));
    let reason = clean_optional_text(Some(request.reason))
        .ok_or_else(|| ApiError::bad_request("Git delivery authorization reason is required"))?;
    let expires_at = bounded_production_grant_expiry(&work_item, request.expires_at)?;
    if let Some(existing) = matching_git_delivery_grant(
        &state.store,
        &subject,
        &change_set,
        &work_item,
        &branch,
        &plan.id,
    )
    .await?
    {
        return Ok(Json(GitDeliveryAuthorizationResponse {
            grant: existing.into(),
            plan: plan.into(),
            created: false,
        }));
    }

    let grant = create_permission_grant_record(
        &state.store,
        CreatePermissionGrantRequest {
            subject,
            created_by: actor.clone(),
            reason: reason.clone(),
            scope: json!({
                "environment": work_item.target_environment,
                "capability_kinds": ["git"],
                "actions": GIT_DELIVERY_ACTIONS,
                "max_risk": "high",
                "repos": [work_item.source_repo],
                "branches": [branch],
                "work_plan_ids": [change_set.work_plan_id],
                "change_set_ids": [change_set.id],
                "git_delivery_plan_artifact_ids": [plan.id],
                "production_impacting": work_item.production_impacting,
            }),
            policy: json!({ "policy_mode": "supervised_autonomy" }),
            expires_at,
        },
    )
    .await?;
    append_change_set_audit_event(
        &state.store,
        &change_set,
        "change_set.git_delivery_authorized",
        actor,
        Some(reason),
        json!({
            "permission_grant_id": grant.id,
            "git_delivery_plan_artifact_id": plan.id,
            "subject": grant.subject,
        }),
    )
    .await?;

    Ok(Json(GitDeliveryAuthorizationResponse {
        grant: grant.into(),
        plan: plan.into(),
        created: true,
    }))
}

pub(in crate::app) async fn preflight_change_set_git_delivery(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(change_set_id): Path<String>,
    Json(request): Json<GitDeliveryPreflightRequest>,
) -> Result<Json<GitDeliveryPreflightResponse>, ApiError> {
    let change_set = state
        .store
        .get_change_set(&change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", &change_set_id))?;
    let work_plan = state
        .store
        .get_work_plan(&change_set.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &change_set.work_plan_id))?;
    let work_item_id = change_set.work_item_id.as_deref().ok_or_else(|| {
        ApiError::conflict("Git delivery preflight requires a WorkItem-backed ChangeSet")
    })?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let run_id = change_set
        .run_id
        .clone()
        .ok_or_else(|| ApiError::conflict("ChangeSet has no coding run provenance"))?;
    let plan = current_git_delivery_plan(&state.store, &run_id, &change_set).await?;
    let source = git_delivery_plan_source(&plan, &work_item)?;
    let approval_gate = state
        .store
        .list_approval_gates(ApprovalGateListFilter {
            work_item_id: Some(work_item.id.clone()),
            gate_kind: Some("git_mutation".to_string()),
            limit: 20,
            ..ApprovalGateListFilter::default()
        })
        .await?
        .into_iter()
        .find(|gate| work_item_gate_scope_matches(gate, &work_item, &work_plan, "git_mutation"));
    let approval_gate_ready = approval_gate
        .as_ref()
        .is_some_and(|gate| matches!(gate.status.as_str(), "satisfied" | "waived"));
    let source_approval_gate = if work_item.production_impacting {
        state
            .store
            .list_approval_gates(ApprovalGateListFilter {
                work_item_id: Some(work_item.id.clone()),
                gate_kind: Some("source_mutation".to_string()),
                limit: 20,
                ..ApprovalGateListFilter::default()
            })
            .await?
            .into_iter()
            .find(|gate| {
                work_item_gate_scope_matches(gate, &work_item, &work_plan, "source_mutation")
            })
    } else {
        None
    };
    let source_approval_ready = !work_item.production_impacting
        || source_approval_gate
            .as_ref()
            .is_some_and(|gate| matches!(gate.status.as_str(), "satisfied" | "waived"));
    let subject = clean_optional_text(request.subject)
        .unwrap_or_else(|| DEFAULT_GIT_WRITER_SUBJECT.to_string());
    let grant = matching_git_delivery_grant(
        &state.store,
        &subject,
        &change_set,
        &work_item,
        &source.head_branch,
        &plan.id,
    )
    .await?;
    let authorization_ready = grant.is_some();
    let dispatch_ready = state.worker.git_writer_available();
    let checks = vec![
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
            "supported_target",
            work_item_target_supported(&work_item),
            format!(
                "WorkItem targets {}{}",
                work_item.target_environment,
                if work_item.production_impacting {
                    " (protected production)"
                } else {
                    ""
                }
            ),
        ),
        execution_check(
            "immutable_source_provenance",
            true,
            format!(
                "Plan pins {} at {} from workspace {}",
                source.repository, source.base_commit, source.workspace_id
            ),
        ),
        execution_check(
            "work_item_git_mutation_gate",
            approval_gate_ready,
            approval_gate
                .as_ref()
                .map(|gate| format!("Git mutation gate {} is {}", gate.id, gate.status))
                .unwrap_or_else(|| {
                    "No scoped WorkItem git_mutation gate matches this Git delivery plan"
                        .to_string()
                }),
        ),
        execution_check(
            "work_item_source_mutation_gate",
            source_approval_ready,
            if work_item.production_impacting {
                source_approval_gate
                    .as_ref()
                    .map(|gate| format!("Source mutation gate {} is {}", gate.id, gate.status))
                    .unwrap_or_else(|| {
                        "No scoped WorkItem source_mutation gate matches this Git delivery plan"
                            .to_string()
                    })
            } else {
                "Separate source_mutation gate is not required for legacy dev delivery".to_string()
            },
        ),
        execution_check(
            "trusted_git_delivery_grant",
            authorization_ready,
            grant
                .as_ref()
                .map(|grant| {
                    format!(
                        "Active supervised-autonomy grant {} matches Git writer {}",
                        grant.id, subject
                    )
                })
                .unwrap_or_else(|| {
                    format!(
                        "No active supervised-autonomy Git delivery grant matches writer {}",
                        subject
                    )
                }),
        ),
        execution_check(
            "git_writer_executor_available",
            dispatch_ready,
            if dispatch_ready {
                "Dedicated Git writer Job is configured"
            } else {
                "No Git writer executor is configured; remote branch, commit, push, and pull-request creation remain unavailable"
            },
        ),
    ];
    let prerequisites_ready = checks
        .iter()
        .filter(|check| {
            check.get("code").and_then(Value::as_str) != Some("git_writer_executor_available")
        })
        .all(|check| check.get("passed").and_then(Value::as_bool) == Some(true));
    // "ready_for_writer" says the immutable plan and grant are ready. The
    // separate dispatch flag tells the operator whether this environment has
    // actually provisioned the isolated writer identity yet.
    let status = if prerequisites_ready {
        "ready_for_writer"
    } else {
        "blocked"
    };
    let grant_id = grant.as_ref().map(|grant| grant.id.clone());

    let artifacts = state.store.list_artifacts(&run_id).await?;
    if let Some(existing) = artifacts.into_iter().find(|artifact| {
        artifact.kind == "git_delivery_preflight"
            && artifact.content_json.as_ref().is_some_and(|content| {
                content
                    .get("git_delivery_plan_artifact_id")
                    .and_then(Value::as_str)
                    == Some(plan.id.as_str())
                    && content.get("subject").and_then(Value::as_str) == Some(subject.as_str())
                    && content.get("permission_grant_id").and_then(Value::as_str)
                        == grant_id.as_deref()
                    && content.get("approval_gate_id").and_then(Value::as_str)
                        == approval_gate.as_ref().map(|gate| gate.id.as_str())
                    && content.get("approval_gate_status").and_then(Value::as_str)
                        == approval_gate.as_ref().map(|gate| gate.status.as_str())
            })
    }) {
        return Ok(Json(GitDeliveryPreflightResponse {
            status: status.to_string(),
            approval_gate_ready,
            authorization_ready,
            dispatch_ready,
            plan: plan.into(),
            permission_grant: grant.map(Into::into),
            checks,
            artifact: existing.into(),
            created: false,
        }));
    }

    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = clean_optional_text(request.reason);
    let artifact = state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_git_delivery_preflight", unique_suffix()),
            session_id: change_set.session_id.clone(),
            run_id: Some(run_id),
            kind: "git_delivery_preflight".to_string(),
            label: format!("Git delivery preflight for ChangeSet {}", change_set.id),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "change_set_id": change_set.id,
                "work_plan_id": change_set.work_plan_id,
                "work_item_id": work_item.id,
                "git_delivery_plan_artifact_id": plan.id,
                "subject": subject,
                "permission_grant_id": grant_id,
                "approval_gate_id": approval_gate.as_ref().map(|gate| &gate.id),
                "approval_gate_status": approval_gate.as_ref().map(|gate| &gate.status),
                "approval_gate_ready": approval_gate_ready,
                "status": status,
                "authorization_ready": authorization_ready,
                "dispatch_ready": dispatch_ready,
                "checks": checks,
                "dispatch": {
                    "state": if dispatch_ready { "configured" } else { "not_configured" },
                    "summary": if dispatch_ready { "Dedicated Git writer is configured; execution remains operator-invoked" } else { "Git writer execution is intentionally unavailable until its isolated identity and executor are configured" },
                },
                "reason": reason,
            })),
        })
        .await?;
    append_change_set_audit_event(
        &state.store,
        &change_set,
        "change_set.git_delivery_preflighted",
        actor,
        reason,
        json!({
            "git_delivery_plan_artifact_id": plan.id,
            "git_delivery_preflight_artifact_id": artifact.id,
            "permission_grant_id": grant_id,
            "approval_gate_id": approval_gate.as_ref().map(|gate| &gate.id),
            "approval_gate_ready": approval_gate_ready,
            "subject": subject,
            "status": status,
            "authorization_ready": authorization_ready,
            "dispatch_ready": dispatch_ready,
        }),
    )
    .await?;

    Ok(Json(GitDeliveryPreflightResponse {
        status: status.to_string(),
        approval_gate_ready,
        authorization_ready,
        dispatch_ready,
        plan: plan.into(),
        permission_grant: grant.map(Into::into),
        checks,
        artifact: artifact.into(),
        created: true,
    }))
}

pub(in crate::app) async fn execute_change_set_git_delivery(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(change_set_id): Path<String>,
    Json(request): Json<ExecuteGitDeliveryRequest>,
) -> Result<Json<ExecuteGitDeliveryResponse>, ApiError> {
    let subject = clean_optional_text(request.subject.clone())
        .unwrap_or_else(|| DEFAULT_GIT_WRITER_SUBJECT.to_string());
    let actor = identity
        .as_ref()
        .map(|Extension(OperatorIdentity(name))| name.clone())
        .or_else(|| clean_optional_text(request.actor.clone()));
    let reason = clean_optional_text(Some(request.reason))
        .ok_or_else(|| ApiError::bad_request("Git delivery execution reason is required"))?;

    // Re-run the exact preflight just before dispatch. Stored preflights are
    // evidence, not authorization: a ChangeSet revision or grant revocation
    // must stop a subsequent executor call.
    let Json(preflight) = preflight_change_set_git_delivery(
        State(state.clone()),
        identity,
        Path(change_set_id.clone()),
        Json(GitDeliveryPreflightRequest {
            subject: Some(subject.clone()),
            actor: actor.clone(),
            reason: Some(reason.clone()),
        }),
    )
    .await?;
    if preflight.status != "ready_for_writer" || !preflight.dispatch_ready {
        return Err(ApiError::conflict(
            "Git delivery execution requires a current approved plan, matching writer grant, and configured dedicated writer",
        ));
    }
    let grant = preflight.permission_grant.clone().ok_or_else(|| {
        ApiError::conflict("Git delivery execution requires an active matching writer grant")
    })?;
    let change_set = state
        .store
        .get_change_set(&change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", &change_set_id))?;
    let run_id = change_set
        .run_id
        .clone()
        .ok_or_else(|| ApiError::conflict("ChangeSet has no coding run provenance"))?;
    let plan = state
        .store
        .get_artifact(&preflight.plan.id)
        .await?
        .ok_or_else(|| ApiError::conflict("current Git delivery plan is unavailable"))?;
    let work_item_id = change_set.work_item_id.as_deref().ok_or_else(|| {
        ApiError::conflict("Git delivery execution requires a WorkItem-backed ChangeSet")
    })?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let source = git_delivery_plan_source(&plan, &work_item)?;
    let settings = state
        .worker
        .git_writer_settings()
        .ok_or_else(|| ApiError::conflict("Git writer executor is not configured"))?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == &source.repository)
    {
        return Err(ApiError::conflict(
            "Git delivery repository is not allowlisted for the Git writer",
        ));
    }

    let artifacts = state.store.list_artifacts(&run_id).await?;
    if let Some(existing) = artifacts.iter().find(|artifact| {
        git_delivery_artifact_matches_plan(artifact, "git_delivery_execution", &plan.id)
            && artifact.content_json.as_ref().is_some_and(|content| {
                content.get("permission_grant_id").and_then(Value::as_str)
                    == Some(grant.id.as_str())
            })
    }) {
        let execution_id = existing
            .content_json
            .as_ref()
            .and_then(|value| value.get("execution_id"))
            .and_then(Value::as_str);
        let terminal_status = execution_id.and_then(|execution_id| {
            artifacts.iter().find_map(|artifact| {
                (artifact.kind == "git_delivery_result")
                    .then_some(artifact.content_json.as_ref())
                    .flatten()
                    .filter(|content| {
                        content.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                    })
                    .and_then(|content| content.get("status").and_then(Value::as_str))
            })
        });
        return Ok(Json(ExecuteGitDeliveryResponse {
            status: terminal_status.unwrap_or("dispatched").to_string(),
            execution: existing.clone().into(),
            plan: plan.into(),
            permission_grant: grant,
            job_name: existing
                .content_json
                .as_ref()
                .and_then(|value| value.get("job_name"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            created: false,
        }));
    }

    let execution_id = format!("gexec_{}", unique_suffix());
    let execution = state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_git_delivery_execution", unique_suffix()),
            session_id: change_set.session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "git_delivery_execution".to_string(),
            label: format!("Git delivery execution for ChangeSet {}", change_set.id),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "execution_id": execution_id,
                "status": "dispatched",
                "change_set_id": change_set.id,
                "git_delivery_plan_artifact_id": plan.id,
                "permission_grant_id": grant.id,
                "subject": subject,
                "dispatched_by": actor,
                "reason": reason,
                "source": {
                    "repository": source.repository,
                    "base_ref": source.base_ref,
                    "base_commit": source.base_commit,
                    "head_branch": source.head_branch,
                },
            })),
        })
        .await?;

    match state
        .worker
        .dispatch_git_delivery(GitDeliveryExecutionRequest {
            change_set_id: change_set.id.clone(),
            execution_id: execution_id.clone(),
        })
        .await
    {
        Ok(receipt) => {
            append_change_set_audit_event(
                &state.store,
                &change_set,
                "change_set.git_delivery_dispatched",
                actor,
                Some(reason),
                json!({
                    "execution_id": execution_id,
                    "git_delivery_plan_artifact_id": plan.id,
                    "permission_grant_id": grant.id,
                    "job_name": receipt.job_name,
                }),
            )
            .await?;
            Ok(Json(ExecuteGitDeliveryResponse {
                status: "dispatched".to_string(),
                execution: execution.into(),
                plan: plan.into(),
                permission_grant: grant,
                job_name: Some(receipt.job_name),
                created: true,
            }))
        }
        Err(error) => {
            let failure = persist_git_delivery_result(
                &state.store,
                &change_set,
                &run_id,
                &plan.id,
                &execution_id,
                "dispatch_failed",
                json!({ "error_code": "job_dispatch_failed" }),
            )
            .await?;
            append_change_set_audit_event(
                &state.store,
                &change_set,
                "change_set.git_delivery_dispatch_failed",
                None,
                None,
                json!({ "execution_id": execution_id, "error_code": "job_dispatch_failed" }),
            )
            .await?;
            tracing::warn!(change_set_id = %change_set.id, %error, "Git writer dispatch failed");
            Ok(Json(ExecuteGitDeliveryResponse {
                status: "dispatch_failed".to_string(),
                execution: failure,
                plan: plan.into(),
                permission_grant: grant,
                job_name: None,
                created: true,
            }))
        }
    }
}

pub(in crate::app) async fn observe_change_set_git_delivery(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(change_set_id): Path<String>,
    Json(request): Json<ObserveGitDeliveryRequest>,
) -> Result<Json<ObserveGitDeliveryResponse>, ApiError> {
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = clean_optional_text(Some(request.reason))
        .ok_or_else(|| ApiError::bad_request("Git delivery observation reason is required"))?;
    let change_set = state
        .store
        .get_change_set(&change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", &change_set_id))?;
    let run_id = change_set
        .run_id
        .clone()
        .ok_or_else(|| ApiError::conflict("Git delivery ChangeSet has no coding run provenance"))?;
    let work_item = state
        .store
        .get_work_item(change_set.work_item_id.as_deref().ok_or_else(|| {
            ApiError::conflict("Git delivery observation requires a WorkItem-backed ChangeSet")
        })?)
        .await?
        .ok_or_else(|| ApiError::conflict("Git delivery WorkItem is unavailable"))?;
    if !work_item_target_supported(&work_item) {
        return Err(ApiError::conflict(
            "Git delivery observation is limited to dev or the exact protected production target",
        ));
    }
    let plan = current_git_delivery_plan(&state.store, &run_id, &change_set).await?;
    let source = git_delivery_plan_source(&plan, &work_item)?;
    let settings = state
        .worker
        .git_observer_settings()
        .ok_or_else(|| ApiError::conflict("Git observer executor is not configured"))?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == &source.repository)
    {
        return Err(ApiError::conflict(
            "Git delivery repository is not allowlisted for the Git observer",
        ));
    }
    let artifacts = state.store.list_artifacts(&run_id).await?;
    let delivery_result = artifacts
        .iter()
        .filter(|artifact| {
            git_delivery_artifact_matches_plan(artifact, "git_delivery_result", &plan.id)
        })
        .filter(|artifact| {
            artifact
                .content_json
                .as_ref()
                .and_then(|value| value.get("status"))
                .and_then(Value::as_str)
                == Some("completed")
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .ok_or_else(|| {
            ApiError::conflict("Git delivery observation requires a completed branch-and-PR result")
        })?;
    let details = delivery_result
        .content_json
        .as_ref()
        .and_then(|value| value.get("details"))
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Git delivery result has no pull-request provenance"))?;
    let pull_request_number = details
        .get("pull_request_number")
        .and_then(Value::as_u64)
        .ok_or_else(|| ApiError::conflict("Git delivery result has no pull-request number"))?;
    let pull_request_url =
        required_json_string(details, "pull_request_url", "Git delivery result")?;
    let source_commit_sha = required_json_string(details, "commit_sha", "Git delivery result")?;
    if !is_git_sha(&source_commit_sha) || !is_github_pr_url(&pull_request_url) {
        return Err(ApiError::conflict(
            "Git delivery result has invalid GitHub provenance",
        ));
    }
    if let Some(existing) = artifacts.iter().find(|artifact| {
        artifact.kind == "git_delivery_observation_execution"
            && artifact.content_json.as_ref().is_some_and(|content| {
                content
                    .get("git_delivery_plan_artifact_id")
                    .and_then(Value::as_str)
                    == Some(plan.id.as_str())
                    && content
                        .get("git_delivery_result_artifact_id")
                        .and_then(Value::as_str)
                        == Some(delivery_result.id.as_str())
                    && !artifacts.iter().any(|failure| {
                        failure.kind == "git_delivery_observation_dispatch_failure"
                            && failure
                                .content_json
                                .as_ref()
                                .is_some_and(|failure_content| {
                                    failure_content.get("execution_id").and_then(Value::as_str)
                                        == content.get("execution_id").and_then(Value::as_str)
                                })
                    })
            })
    }) {
        return Ok(Json(ObserveGitDeliveryResponse {
            status: existing
                .content_json
                .as_ref()
                .and_then(|value| value.get("status"))
                .and_then(Value::as_str)
                .unwrap_or("dispatched")
                .to_string(),
            execution: existing.clone().into(),
            job_name: existing
                .content_json
                .as_ref()
                .and_then(|value| value.get("job_name"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            created: false,
        }));
    }
    let execution_id = format!("gobs_{}", unique_suffix());
    let execution = state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_git_delivery_observation", unique_suffix()),
            session_id: change_set.session_id.clone(),
            run_id: Some(run_id.clone()),
            kind: "git_delivery_observation_execution".to_string(),
            label: format!("Git delivery observation for ChangeSet {}", change_set.id),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "execution_id": execution_id,
                "status": "dispatched",
                "change_set_id": change_set.id,
                "git_delivery_plan_artifact_id": plan.id,
                "git_delivery_result_artifact_id": delivery_result.id,
                "source": { "repository": source.repository, "head_branch": source.head_branch, "source_commit_sha": source_commit_sha, "pull_request_url": pull_request_url, "pull_request_number": pull_request_number },
                "dispatched_by": actor,
                "reason": reason,
            })),
        })
        .await?;
    match state
        .worker
        .dispatch_git_delivery_observation(GitDeliveryObservationRequest {
            change_set_id: change_set.id.clone(),
            execution_id: execution_id.clone(),
        })
        .await
    {
        Ok(receipt) => {
            append_change_set_audit_event(&state.store, &change_set, "change_set.git_delivery_observation_dispatched", actor, Some(reason), json!({ "execution_id": execution_id, "git_delivery_plan_artifact_id": plan.id, "job_name": receipt.job_name })).await?;
            Ok(Json(ObserveGitDeliveryResponse {
                status: "dispatched".to_string(),
                execution: execution.into(),
                job_name: Some(receipt.job_name),
                created: true,
            }))
        }
        Err(error) => {
            tracing::warn!(change_set_id = %change_set.id, %error, "Git observer dispatch failed");
            let failure = state
                .store
                .create_artifact(CreateArtifact {
                    id: format!(
                        "art_{}_git_delivery_observation_dispatch_failure",
                        unique_suffix()
                    ),
                    session_id: change_set.session_id.clone(),
                    run_id: Some(run_id),
                    kind: "git_delivery_observation_dispatch_failure".to_string(),
                    label: format!(
                        "Git delivery observation dispatch failure for ChangeSet {}",
                        change_set.id
                    ),
                    mime_type: Some("application/json".to_string()),
                    path: None,
                    content_text: None,
                    content_json: Some(json!({
                        "execution_id": execution_id,
                        "status": "dispatch_failed",
                        "change_set_id": change_set.id,
                        "git_delivery_plan_artifact_id": plan.id,
                        "git_delivery_result_artifact_id": delivery_result.id,
                        "error_code": "git_observer_dispatch_failed",
                    })),
                })
                .await?;
            append_change_set_audit_event(
                &state.store,
                &change_set,
                "change_set.git_delivery_observation_dispatch_failed",
                actor,
                Some(reason),
                json!({
                    "execution_id": execution_id,
                    "git_delivery_plan_artifact_id": plan.id,
                    "dispatch_failure_artifact_id": failure.id,
                    "error_code": "git_observer_dispatch_failed",
                }),
            )
            .await?;
            Ok(Json(ObserveGitDeliveryResponse {
                status: "dispatch_failed".to_string(),
                execution: execution.into(),
                job_name: None,
                created: true,
            }))
        }
    }
}

#[derive(Debug, serde::Deserialize)]
pub(in crate::app) struct InternalGitDeliveryContextQuery {
    execution_id: String,
}

pub(in crate::app) async fn internal_git_delivery_context(
    State(state): State<AppState>,
    Path(change_set_id): Path<String>,
    Query(query): Query<InternalGitDeliveryContextQuery>,
) -> Result<Json<GitDeliveryContextResponse>, ApiError> {
    let (change_set, run_id, plan, execution) =
        current_git_delivery_execution(&state, &change_set_id, &query.execution_id).await?;
    let work_item = state
        .store
        .get_work_item(
            change_set
                .work_item_id
                .as_deref()
                .ok_or_else(|| ApiError::conflict("Git delivery ChangeSet has no WorkItem"))?,
        )
        .await?
        .ok_or_else(|| ApiError::conflict("Git delivery WorkItem is unavailable"))?;
    let source = git_delivery_plan_source(&plan, &work_item)?;
    let settings = state
        .worker
        .git_writer_settings()
        .ok_or_else(|| ApiError::conflict("Git writer executor is not configured"))?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == &source.repository)
    {
        return Err(ApiError::conflict(
            "Git delivery repository is not allowlisted for the Git writer",
        ));
    }
    let evidence = plan
        .content_json
        .as_ref()
        .and_then(|value| value.get("evidence"))
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Git delivery plan has no evidence"))?;
    let diff_id = required_json_string(evidence, "git_diff_artifact_id", "Git delivery evidence")?;
    let diff = state
        .store
        .list_artifacts(&run_id)
        .await?
        .into_iter()
        .find(|artifact| artifact.id == diff_id && artifact.kind == "workspace_git_diff")
        .and_then(|artifact| artifact.content_text)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::conflict("Git delivery diff evidence is unavailable"))?;
    let plan_json = plan
        .content_json
        .as_ref()
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Git delivery plan has no structured content"))?;
    let commit = plan_json
        .get("commit")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Git delivery plan has no commit metadata"))?;
    let pull_request = plan_json
        .get("pull_request")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Git delivery plan has no pull request metadata"))?;
    let _ = execution;
    Ok(Json(GitDeliveryContextResponse {
        execution_id: query.execution_id,
        repository: source.repository,
        base_ref: source.base_ref,
        base_commit: source.base_commit,
        head_branch: source.head_branch,
        diff,
        commit_subject: required_json_string(commit, "subject", "Git delivery commit")?,
        commit_body: required_json_string(commit, "body", "Git delivery commit")?,
        pull_request_title: required_json_string(
            pull_request,
            "title",
            "Git delivery pull request",
        )?,
        pull_request_body: required_json_string(pull_request, "body", "Git delivery pull request")?,
        github_api_url: settings.github_api_url,
        author_name: settings.author_name,
        author_email: settings.author_email,
    }))
}

pub(in crate::app) async fn internal_git_delivery_outcome(
    State(state): State<AppState>,
    Path(change_set_id): Path<String>,
    Json(request): Json<GitDeliveryOutcomeRequest>,
) -> Result<Json<ArtifactResponse>, ApiError> {
    let (change_set, run_id, plan, _execution) =
        current_git_delivery_execution(&state, &change_set_id, &request.execution_id).await?;
    let result = match request.status.as_str() {
        "completed" => {
            let branch = clean_optional_text(request.branch).ok_or_else(|| ApiError::bad_request("completed Git delivery outcome requires branch"))?;
            let sha = clean_optional_text(request.commit_sha).ok_or_else(|| ApiError::bad_request("completed Git delivery outcome requires commit_sha"))?;
            let url = clean_optional_text(request.pull_request_url).ok_or_else(|| ApiError::bad_request("completed Git delivery outcome requires pull_request_url"))?;
            let number = request.pull_request_number.ok_or_else(|| ApiError::bad_request("completed Git delivery outcome requires pull_request_number"))?;
            if !is_git_sha(&sha) || !is_github_pr_url(&url) {
                return Err(ApiError::bad_request(
                    "completed Git delivery outcome has invalid GitHub provenance",
                ));
            }
            let work_item = state.store.get_work_item(change_set.work_item_id.as_deref().ok_or_else(|| ApiError::conflict("Git delivery ChangeSet has no WorkItem"))?).await?
                .ok_or_else(|| ApiError::conflict("Git delivery WorkItem is unavailable"))?;
            let source = git_delivery_plan_source(&plan, &work_item)?;
            if branch != source.head_branch { return Err(ApiError::conflict("Git delivery outcome branch does not match immutable plan")); }
            let expected_pr_prefix = format!(
                "https://github.com/{}/pull/",
                source.repository.trim_start_matches("https://github.com/").trim_end_matches(".git")
            );
            if !url.starts_with(&expected_pr_prefix) || !url.ends_with(&number.to_string()) {
                return Err(ApiError::conflict(
                    "Git delivery pull request does not match immutable repository provenance",
                ));
            }
            persist_git_delivery_result(&state.store, &change_set, &run_id, &plan.id, &request.execution_id, "completed", json!({
                "branch": branch, "commit_sha": sha, "pull_request_url": url, "pull_request_number": number,
            })).await?
        }
        "failed" => persist_git_delivery_result(&state.store, &change_set, &run_id, &plan.id, &request.execution_id, "failed", json!({
            "error_code": clean_optional_text(request.error_code).unwrap_or_else(|| "git_writer_failed".to_string()),
        })).await?,
        _ => return Err(ApiError::bad_request("Git delivery outcome status must be completed or failed")),
    };
    append_change_set_audit_event(&state.store, &change_set, &format!("change_set.git_delivery_{}", request.status), Some(DEFAULT_GIT_WRITER_SUBJECT.to_string()), None,
        json!({ "execution_id": request.execution_id, "git_delivery_plan_artifact_id": plan.id, "result_artifact_id": result.id }))
        .await?;
    Ok(Json(result))
}

pub(in crate::app) async fn internal_git_delivery_observation_context(
    State(state): State<AppState>,
    Path(change_set_id): Path<String>,
    Query(query): Query<InternalGitDeliveryContextQuery>,
) -> Result<Json<GitDeliveryObservationContextResponse>, ApiError> {
    let (change_set, run_id, plan, execution) =
        current_git_delivery_observation(&state, &change_set_id, &query.execution_id).await?;
    let work_item = state
        .store
        .get_work_item(change_set.work_item_id.as_deref().ok_or_else(|| {
            ApiError::conflict("Git delivery observation ChangeSet has no WorkItem")
        })?)
        .await?
        .ok_or_else(|| ApiError::conflict("Git delivery observation WorkItem is unavailable"))?;
    let source = git_delivery_plan_source(&plan, &work_item)?;
    let settings = state
        .worker
        .git_observer_settings()
        .ok_or_else(|| ApiError::conflict("Git observer executor is not configured"))?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == &source.repository)
    {
        return Err(ApiError::conflict(
            "Git delivery repository is not allowlisted for the Git observer",
        ));
    }
    let execution_content = execution
        .content_json
        .as_ref()
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Git observation execution has no structured content"))?;
    let source_content = execution_content
        .get("source")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Git observation execution has no source provenance"))?;
    let _ = run_id;
    Ok(Json(GitDeliveryObservationContextResponse {
        expected_base_commit_sha: None,
        execution_id: query.execution_id,
        repository: source.repository,
        base_ref: source.base_ref,
        head_branch: required_json_string(source_content, "head_branch", "Git observation source")?,
        source_commit_sha: required_json_string(
            source_content,
            "source_commit_sha",
            "Git observation source",
        )?,
        pull_request_url: required_json_string(
            source_content,
            "pull_request_url",
            "Git observation source",
        )?,
        pull_request_number: source_content
            .get("pull_request_number")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                ApiError::conflict("Git observation source has no pull-request number")
            })?,
        github_api_url: settings.github_api_url,
    }))
}

pub(in crate::app) async fn internal_git_delivery_observation_outcome(
    State(state): State<AppState>,
    Path(change_set_id): Path<String>,
    Json(request): Json<GitDeliveryObservationOutcomeRequest>,
) -> Result<Json<ArtifactResponse>, ApiError> {
    let (change_set, run_id, plan, execution) =
        current_git_delivery_observation(&state, &change_set_id, &request.execution_id).await?;
    let execution_content = execution
        .content_json
        .as_ref()
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Git observation execution has no structured content"))?;
    let expected = execution_content
        .get("source")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Git observation execution has no source provenance"))?;
    let artifact: ArtifactResponse = match request.status.as_str() {
        "observed" => {
            let pull_request_state = clean_optional_text(request.pull_request_state)
                .ok_or_else(|| ApiError::bad_request("observed Git outcome requires pull_request_state"))?;
            if !matches!(pull_request_state.as_str(), "open" | "closed") {
                return Err(ApiError::bad_request("Git pull_request_state must be open or closed"));
            }
            let merged = request.merged.ok_or_else(|| ApiError::bad_request("observed Git outcome requires merged"))?;
            let head_branch = clean_optional_text(request.head_branch)
                .ok_or_else(|| ApiError::bad_request("observed Git outcome requires head_branch"))?;
            let head_commit_sha = clean_optional_text(request.head_commit_sha)
                .ok_or_else(|| ApiError::bad_request("observed Git outcome requires head_commit_sha"))?;
            if !is_git_sha(&head_commit_sha)
                || expected.get("head_branch").and_then(Value::as_str) != Some(head_branch.as_str())
                || expected.get("source_commit_sha").and_then(Value::as_str) != Some(head_commit_sha.as_str())
            {
                return Err(ApiError::conflict("Git observation does not match the delivered branch commit"));
            }
            let merge_commit_sha = clean_optional_text(request.merge_commit_sha);
            if merged {
                let merge_commit_sha = merge_commit_sha.as_deref().ok_or_else(|| {
                    ApiError::bad_request("merged Git outcome requires merge_commit_sha")
                })?;
                if pull_request_state != "closed" || !is_git_sha(merge_commit_sha) {
                    return Err(ApiError::bad_request("merged Git outcome has invalid merge provenance"));
                }
            } else if merge_commit_sha.is_some() {
                return Err(ApiError::bad_request("unmerged Git outcome must not include merge_commit_sha"));
            }
            let observation = state.store.create_artifact(CreateArtifact {
                id: format!("art_{}_git_delivery_pr_observation", unique_suffix()),
                session_id: change_set.session_id.clone(), run_id: Some(run_id.clone()),
                kind: "git_delivery_pr_observation".to_string(), label: format!("GitHub PR observation for ChangeSet {}", change_set.id),
                mime_type: Some("application/json".to_string()), path: None, content_text: None,
                content_json: Some(json!({ "execution_id": request.execution_id, "status": "observed", "change_set_id": change_set.id, "git_delivery_plan_artifact_id": plan.id, "pull_request_state": pull_request_state, "merged": merged, "head_branch": head_branch, "head_commit_sha": head_commit_sha, "merge_commit_sha": merge_commit_sha })),
            }).await?;
            if let Some(merge_commit_sha) = merge_commit_sha {
                state.store.create_artifact(CreateArtifact {
                    id: format!("art_{}_git_delivery_merge", unique_suffix()),
                    session_id: change_set.session_id.clone(), run_id: Some(run_id.clone()),
                    kind: "git_delivery_merge".to_string(), label: format!("Immutable Git merge for ChangeSet {}", change_set.id),
                    mime_type: Some("application/json".to_string()), path: None, content_text: None,
                    content_json: Some(json!({ "execution_id": request.execution_id, "change_set_id": change_set.id, "git_delivery_plan_artifact_id": plan.id, "pull_request_url": expected.get("pull_request_url"), "pull_request_number": expected.get("pull_request_number"), "head_commit_sha": head_commit_sha, "merge_commit_sha": merge_commit_sha })),
                }).await?;
            }
            observation.into()
        }
        "failed" => state.store.create_artifact(CreateArtifact {
            id: format!("art_{}_git_delivery_pr_observation", unique_suffix()),
            session_id: change_set.session_id.clone(), run_id: Some(run_id.clone()),
            kind: "git_delivery_pr_observation".to_string(), label: format!("Failed GitHub PR observation for ChangeSet {}", change_set.id),
            mime_type: Some("application/json".to_string()), path: None, content_text: None,
            content_json: Some(json!({ "execution_id": request.execution_id, "status": "failed", "change_set_id": change_set.id, "git_delivery_plan_artifact_id": plan.id, "error_code": clean_optional_text(request.error_code).unwrap_or_else(|| "git_observer_failed".to_string()) })),
        }).await?.into(),
        _ => return Err(ApiError::bad_request("Git observation outcome status must be observed or failed")),
    };
    append_change_set_audit_event(&state.store, &change_set, &format!("change_set.git_delivery_observation_{}", request.status), Some("agent:git-observer".to_string()), None, json!({ "execution_id": request.execution_id, "git_delivery_plan_artifact_id": plan.id, "observation_artifact_id": artifact.id }))
        .await?;
    Ok(Json(artifact))
}

pub(in crate::app) async fn current_git_delivery_execution(
    state: &AppState,
    change_set_id: &str,
    execution_id: &str,
) -> Result<(StoredChangeSet, RunId, StoredArtifact, StoredArtifact), ApiError> {
    let change_set = state
        .store
        .get_change_set(change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", change_set_id))?;
    let run_id = change_set
        .run_id
        .clone()
        .ok_or_else(|| ApiError::conflict("Git delivery ChangeSet has no coding run provenance"))?;
    let artifacts = state.store.list_artifacts(&run_id).await?;
    let execution = artifacts
        .iter()
        .find(|artifact| {
            artifact.kind == "git_delivery_execution"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                })
        })
        .cloned()
        .ok_or_else(|| ApiError::conflict("Git delivery execution is not current"))?;
    let plan_id = execution
        .content_json
        .as_ref()
        .and_then(|value| value.get("git_delivery_plan_artifact_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("Git delivery execution has no plan provenance"))?;
    let plan = artifacts
        .into_iter()
        .find(|artifact| {
            artifact.id == plan_id && git_delivery_plan_matches_change_set(artifact, &change_set)
        })
        .ok_or_else(|| ApiError::conflict("Git delivery execution plan is no longer current"))?;
    Ok((change_set, run_id, plan, execution))
}

pub(in crate::app) async fn current_git_delivery_observation(
    state: &AppState,
    change_set_id: &str,
    execution_id: &str,
) -> Result<(StoredChangeSet, RunId, StoredArtifact, StoredArtifact), ApiError> {
    let change_set = state
        .store
        .get_change_set(change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("change_set", change_set_id))?;
    let run_id = change_set.run_id.clone().ok_or_else(|| {
        ApiError::conflict("Git observation ChangeSet has no coding run provenance")
    })?;
    let artifacts = state.store.list_artifacts(&run_id).await?;
    let execution = artifacts
        .iter()
        .find(|artifact| {
            artifact.kind == "git_delivery_observation_execution"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                })
        })
        .cloned()
        .ok_or_else(|| ApiError::conflict("Git observation execution is not current"))?;
    let plan_id = execution
        .content_json
        .as_ref()
        .and_then(|value| value.get("git_delivery_plan_artifact_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("Git observation execution has no plan provenance"))?;
    let plan = artifacts
        .into_iter()
        .find(|artifact| {
            artifact.id == plan_id && git_delivery_plan_matches_change_set(artifact, &change_set)
        })
        .ok_or_else(|| ApiError::conflict("Git observation execution plan is no longer current"))?;
    Ok((change_set, run_id, plan, execution))
}

pub(in crate::app) async fn persist_git_delivery_result(
    store: &SqliteStore,
    change_set: &StoredChangeSet,
    run_id: &RunId,
    plan_id: &str,
    execution_id: &str,
    status: &str,
    details: Value,
) -> Result<ArtifactResponse, ApiError> {
    Ok(store.create_artifact(CreateArtifact {
        id: format!("art_{}_git_delivery_result", unique_suffix()),
        session_id: change_set.session_id.clone(), run_id: Some(run_id.clone()),
        kind: "git_delivery_result".to_string(), label: format!("Git delivery {} for ChangeSet {}", status, change_set.id),
        mime_type: Some("application/json".to_string()), path: None, content_text: None,
        content_json: Some(json!({ "execution_id": execution_id, "status": status, "change_set_id": change_set.id, "git_delivery_plan_artifact_id": plan_id, "details": details })),
    }).await?.into())
}

#[derive(Debug, Clone)]
struct GitDeliveryPlanSource {
    repository: String,
    base_ref: String,
    base_commit: String,
    head_branch: String,
    workspace_id: String,
}

fn git_delivery_plan_source(
    plan: &StoredArtifact,
    work_item: &StoredWorkItem,
) -> Result<GitDeliveryPlanSource, ApiError> {
    let plan_json = plan
        .content_json
        .as_ref()
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Git delivery plan has no structured content"))?;
    if plan_json.get("operation").and_then(Value::as_str) != Some("branch_and_pull_request") {
        return Err(ApiError::conflict(
            "Git delivery plan does not describe a branch-and-pull-request operation",
        ));
    }
    let source = plan_json
        .get("source")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("Git delivery plan has no source provenance"))?;
    let repository = required_json_string(source, "repository", "Git delivery plan source")?;
    let base_ref = required_json_string(source, "base_ref", "Git delivery plan source")?;
    let base_commit = required_json_string(source, "base_commit", "Git delivery plan source")?;
    let head_branch = required_json_string(source, "head_branch", "Git delivery plan source")?;
    let workspace_id = required_json_string(source, "workspace_id", "Git delivery plan source")?;
    WorkspaceSourceSpec {
        workspace_id: workspace_id.clone(),
        source_repo: repository.clone(),
        source_ref: base_ref.clone(),
        source_commit: None,
        branch: head_branch.clone(),
        resolved_commit: Some(base_commit.clone()),
    }
    .validate()
    .map_err(|error| ApiError::conflict(error.to_string()))?;
    if repository != work_item.source_repo || base_ref != work_item.source_ref {
        return Err(ApiError::conflict(
            "Git delivery plan source does not match the WorkItem target",
        ));
    }
    Ok(GitDeliveryPlanSource {
        repository,
        base_ref,
        base_commit,
        head_branch,
        workspace_id,
    })
}

pub(in crate::app) async fn current_git_delivery_plan(
    store: &SqliteStore,
    run_id: &RunId,
    change_set: &StoredChangeSet,
) -> Result<StoredArtifact, ApiError> {
    store
        .list_artifacts(run_id)
        .await?
        .into_iter()
        .find(|artifact| git_delivery_plan_matches_change_set(artifact, change_set))
        .ok_or_else(|| {
            ApiError::conflict(
                "ChangeSet needs a current immutable Git delivery plan before authorization",
            )
        })
}

pub(in crate::app) async fn matching_git_delivery_grant(
    store: &SqliteStore,
    subject: &str,
    change_set: &StoredChangeSet,
    work_item: &StoredWorkItem,
    branch: &str,
    plan_artifact_id: &str,
) -> Result<Option<StoredPermissionGrant>, ApiError> {
    let now = current_millis();
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
        let policy = serde_json::from_value::<PermissionGrantPolicy>(grant.policy_json.clone())
            .map_err(|error| {
                ApiError::internal(format!(
                    "permission grant {} has invalid policy: {error}",
                    grant.id
                ))
            })?;
        let has_all_actions = GIT_DELIVERY_ACTIONS
            .iter()
            .all(|action| scope.actions.iter().any(|allowed| allowed == action));
        let matches = grant.subject == subject
            && policy.policy_mode == PolicyMode::SupervisedAutonomy
            && scope.environment.as_deref() == Some(work_item.target_environment.as_str())
            && scope.capability_kinds == vec![CapabilityKind::Git]
            && scope.actions.len() == GIT_DELIVERY_ACTIONS.len()
            && has_all_actions
            && scope
                .max_risk
                .is_some_and(|risk| risk_rank(risk) >= risk_rank(RiskLevel::High))
            && scope.repos == vec![work_item.source_repo.clone()]
            && scope.branches == vec![branch.to_string()]
            && scope.work_plan_ids == vec![change_set.work_plan_id.clone()]
            && scope.change_set_ids == vec![change_set.id.clone()]
            && scope.git_delivery_plan_artifact_ids == vec![plan_artifact_id.to_string()]
            && scope.production_impacting == Some(work_item.production_impacting);
        if matches {
            return Ok(Some(grant));
        }
    }
    Ok(None)
}

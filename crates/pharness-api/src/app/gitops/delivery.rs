use super::super::*;

pub(in crate::app) async fn resolve_gitops_base_revision(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(gitops_change_set_id): Path<String>,
    Json(request): Json<ResolveGitOpsBaseRevisionRequest>,
) -> Result<Json<ResolveGitOpsBaseRevisionResponse>, ApiError> {
    let change_set = state
        .store
        .get_gitops_change_set(&gitops_change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("gitops_change_set", &gitops_change_set_id))?;
    if !matches!(change_set.status.as_str(), "proposed" | "approved") {
        return Err(ApiError::conflict(
            "GitOps base revision resolution requires a proposed or approved GitOps ChangeSet",
        ));
    }
    let settings = state.worker.gitops_observer_settings().ok_or_else(|| {
        ApiError::conflict(
            "read-only GitOps observer identity is not configured for GitOps revision resolution",
        )
    })?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == &change_set.gitops_repo)
    {
        return Err(ApiError::conflict(
            "GitOps repository is not allowlisted for the read-only Git observer identity",
        ));
    }
    let reason = required_text(request.reason, "reason")?;
    let artifacts = state.store.list_artifacts(&change_set.run_id).await?;
    if let Some(existing) = artifacts
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
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
    {
        let execution_id = existing
            .content_json
            .as_ref()
            .and_then(|content| content.get("execution_id"))
            .and_then(Value::as_str);
        let status = execution_id
            .and_then(|execution_id| {
                artifacts.iter().find_map(|artifact| {
                    (artifact.kind == "gitops_base_revision")
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
        if status != "failed" {
            return Ok(Json(ResolveGitOpsBaseRevisionResponse {
                status,
                execution: existing.clone().into(),
                job_name: None,
                created: false,
            }));
        }
    }

    let execution_id = format!("grev_{}", unique_suffix());
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let execution = state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_gitops_base_revision_execution", unique_suffix()),
            session_id: change_set.session_id.clone(),
            run_id: Some(change_set.run_id.clone()),
            kind: "gitops_base_revision_execution".to_string(),
            label: format!("GitOps base revision resolution for {}", change_set.id),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "execution_id": execution_id,
                "status": "dispatched",
                "gitops_change_set_id": change_set.id,
                "gitops_change_set_revision": change_set.revision,
                "material_hash": change_set.material_hash,
                "repository": change_set.gitops_repo,
                "base_ref": change_set.gitops_ref,
                "operation": "resolve_base_revision",
                "identity": "agent:git-observer",
                "reason": reason,
            })),
        })
        .await?;
    match state
        .worker
        .dispatch_gitops_revision_resolution(GitOpsRevisionResolutionRequest {
            gitops_change_set_id: change_set.id.clone(),
            execution_id: execution_id.clone(),
        })
        .await
    {
        Ok(receipt) => {
            append_gitops_change_set_audit_event(
                &state.store,
                &change_set,
                "gitops_change_set.base_revision_dispatched",
                actor,
                Some(reason),
                json!({ "execution_id": execution_id, "execution_artifact_id": execution.id, "job_name": receipt.job_name }),
            )
            .await?;
            Ok(Json(ResolveGitOpsBaseRevisionResponse {
                status: "dispatched".to_string(),
                execution: execution.into(),
                job_name: Some(receipt.job_name),
                created: true,
            }))
        }
        Err(error) => {
            tracing::warn!(gitops_change_set_id = %change_set.id, %error, "GitOps base revision resolver dispatch failed");
            let result = state
                .store
                .create_artifact(CreateArtifact {
                    id: format!("art_{}_gitops_base_revision", unique_suffix()),
                    session_id: change_set.session_id.clone(),
                    run_id: Some(change_set.run_id.clone()),
                    kind: "gitops_base_revision".to_string(),
                    label: format!(
                        "Failed GitOps base revision resolution for {}",
                        change_set.id
                    ),
                    mime_type: Some("application/json".to_string()),
                    path: None,
                    content_text: None,
                    content_json: Some(json!({
                        "execution_id": execution_id,
                        "status": "failed",
                        "gitops_change_set_id": change_set.id,
                        "gitops_change_set_revision": change_set.revision,
                        "material_hash": change_set.material_hash,
                        "repository": change_set.gitops_repo,
                        "base_ref": change_set.gitops_ref,
                        "execution_artifact_id": execution.id,
                        "identity": "agent:git-observer",
                        "error_code": "job_dispatch_failed",
                    })),
                })
                .await?;
            append_gitops_change_set_audit_event(
                &state.store,
                &change_set,
                "gitops_change_set.base_revision_dispatch_failed",
                actor,
                Some(reason),
                json!({ "execution_id": execution_id, "execution_artifact_id": execution.id, "result_artifact_id": result.id }),
            )
            .await?;
            Ok(Json(ResolveGitOpsBaseRevisionResponse {
                status: "dispatch_failed".to_string(),
                execution: execution.into(),
                job_name: None,
                created: true,
            }))
        }
    }
}

/// Bind an approved GitOps ChangeSet to a read-only, immutable base revision.
/// This produces only a durable writer input: it cannot create a branch,
/// commit a manifest, open a pull request, or trigger Argo reconciliation.
pub(in crate::app) async fn prepare_gitops_change_set_delivery(
    State(state): State<AppState>,
    Path(gitops_change_set_id): Path<String>,
    Json(request): Json<PrepareGitOpsDeliveryRequest>,
) -> Result<Json<GitOpsDeliveryPlanResponse>, ApiError> {
    let change_set = state
        .store
        .get_gitops_change_set(&gitops_change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("gitops_change_set", &gitops_change_set_id))?;
    if change_set.status != "approved" {
        return Err(ApiError::conflict(
            "GitOps delivery planning requires an approved GitOps ChangeSet",
        ));
    }
    let work_plan = state
        .store
        .get_work_plan(&change_set.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &change_set.work_plan_id))?;
    ensure_approved_for_trusted_envelope("work_plan", &work_plan.id, &work_plan.status)?;
    let work_item = state
        .store
        .get_work_item(&change_set.work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &change_set.work_item_id))?;
    ensure_gitops_delivery_target(&work_item, &change_set)?;
    if work_item.production_impacting
        && !latest_rollback_intent(&state, &work_item, None)
            .await?
            .is_some_and(|intent| {
                matches!(
                    intent.pointer("/content/status").and_then(Value::as_str),
                    Some("prepared" | "approved")
                ) && intent
                    .pointer("/content/baseline/image_digest")
                    .and_then(Value::as_str)
                    .is_some_and(immutable_image_digest)
            })
    {
        return Err(ApiError::conflict(
            "production GitOps authorization requires a captured baseline and prepared RollbackIntent",
        ));
    }

    let artifacts = state.store.list_artifacts(&change_set.run_id).await?;
    let base_revision = current_gitops_base_revision(&artifacts, &change_set)?;
    if let Some(existing) = artifacts
        .iter()
        .find(|artifact| gitops_delivery_plan_matches_change_set(artifact, &change_set))
    {
        return Ok(Json(GitOpsDeliveryPlanResponse {
            artifact: existing.clone().into(),
            base_revision: base_revision.into(),
            created: false,
        }));
    }
    let base_commit = base_revision
        .content_json
        .as_ref()
        .and_then(|content| content.get("base_commit"))
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("GitOps base revision has no resolved commit"))?;
    let plan = state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_gitops_delivery_plan", unique_suffix()),
            session_id: change_set.session_id.clone(),
            run_id: Some(change_set.run_id.clone()),
            kind: "gitops_delivery_plan".to_string(),
            label: format!("GitOps delivery plan for {}", change_set.id),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "kind": "gitops_delivery_plan",
                "version": 1,
                "operation": "branch_and_pull_request",
                "gitops_change_set": {
                    "id": change_set.id,
                    "revision": change_set.revision,
                    "material_hash": change_set.material_hash,
                    "work_item_id": change_set.work_item_id,
                    "work_plan_id": change_set.work_plan_id,
                    "source_change_set_id": change_set.source_change_set_id,
                    "pipeline_intent_id": change_set.pipeline_intent_id,
                    "deployment_intent_id": change_set.deployment_intent_id,
                },
                "source": {
                    "repository": change_set.gitops_repo,
                    "base_ref": change_set.gitops_ref,
                    "base_commit": base_commit,
                    "head_branch": change_set.head_branch,
                    "base_revision_artifact_id": base_revision.id,
                    "identity": "agent:git-observer",
                },
                "update": {
                    "operation": "kustomize_set_image",
                    "kustomization_path": change_set.kustomization_path,
                    "image_name": change_set.image_name,
                    "new_image": change_set.image_ref,
                },
                "authorization": {
                    "state": "not_authorized",
                    "reason": "requires a satisfied gitops_mutation gate, matching GitOps writer grant, and dedicated GitOps writer preflight",
                },
                "execution": {
                    "enabled": true,
                    "mode": "gitops_writer_job",
                    "reason": "requires a satisfied gitops_mutation gate, matching plan-scoped grant, configured dedicated GitOps writer, and explicit delivery execution request",
                },
            })),
        })
        .await?;
    append_gitops_change_set_audit_event(
        &state.store,
        &change_set,
        "gitops_change_set.delivery_prepared",
        clean_optional_text(request.actor),
        clean_optional_text(request.reason),
        json!({
            "gitops_delivery_plan_artifact_id": plan.id,
            "gitops_base_revision_artifact_id": base_revision.id,
            "base_commit": base_commit,
        }),
    )
    .await?;

    Ok(Json(GitOpsDeliveryPlanResponse {
        artifact: plan.into(),
        base_revision: base_revision.into(),
        created: true,
    }))
}

pub(in crate::app) fn current_gitops_base_revision(
    artifacts: &[StoredArtifact],
    change_set: &StoredGitOpsChangeSet,
) -> Result<StoredArtifact, ApiError> {
    artifacts
        .iter()
        .filter(|artifact| gitops_base_revision_matches_change_set(artifact, change_set))
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .ok_or_else(|| {
            ApiError::conflict(
                "GitOps delivery planning requires a current resolved immutable base revision",
            )
        })
}

pub(in crate::app) fn gitops_base_revision_matches_change_set(
    artifact: &StoredArtifact,
    change_set: &StoredGitOpsChangeSet,
) -> bool {
    artifact.kind == "gitops_base_revision"
        && artifact.content_json.as_ref().is_some_and(|content| {
            content.get("status").and_then(Value::as_str) == Some("resolved")
                && content.get("gitops_change_set_id").and_then(Value::as_str)
                    == Some(change_set.id.as_str())
                && content.get("material_hash").and_then(Value::as_str)
                    == Some(change_set.material_hash.as_str())
                && gitops_artifact_change_set_revision(content) == change_set.revision
                && content.get("repository").and_then(Value::as_str)
                    == Some(change_set.gitops_repo.as_str())
                && content.get("base_ref").and_then(Value::as_str)
                    == Some(change_set.gitops_ref.as_str())
                && content
                    .get("base_commit")
                    .and_then(Value::as_str)
                    .is_some_and(is_git_sha)
        })
}

pub(in crate::app) fn gitops_artifact_change_set_revision(content: &Value) -> i64 {
    content
        .get("gitops_change_set_revision")
        .and_then(Value::as_i64)
        .unwrap_or(1)
}

pub(in crate::app) fn gitops_delivery_plan_matches_change_set(
    artifact: &StoredArtifact,
    change_set: &StoredGitOpsChangeSet,
) -> bool {
    artifact.kind == "gitops_delivery_plan"
        && artifact.content_json.as_ref().is_some_and(|plan| {
            plan.get("gitops_change_set")
                .and_then(|value| value.get("id"))
                .and_then(Value::as_str)
                == Some(change_set.id.as_str())
                && plan
                    .get("gitops_change_set")
                    .and_then(|value| value.get("revision"))
                    .and_then(Value::as_i64)
                    == Some(change_set.revision)
                && plan
                    .get("gitops_change_set")
                    .and_then(|value| value.get("material_hash"))
                    .and_then(Value::as_str)
                    == Some(change_set.material_hash.as_str())
        })
}

pub(in crate::app) async fn current_gitops_delivery_plan(
    store: &SqliteStore,
    change_set: &StoredGitOpsChangeSet,
) -> Result<(StoredArtifact, StoredArtifact), ApiError> {
    let artifacts = store.list_artifacts(&change_set.run_id).await?;
    let plan = artifacts
        .iter()
        .filter(|artifact| gitops_delivery_plan_matches_change_set(artifact, change_set))
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .ok_or_else(|| {
            ApiError::conflict(
                "GitOps ChangeSet needs a current immutable delivery plan before authorization",
            )
        })?;
    let base_revision_id = plan
        .content_json
        .as_ref()
        .and_then(|content| content.pointer("/source/base_revision_artifact_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ApiError::conflict("GitOps delivery plan has no base revision provenance")
        })?;
    let base_revision = artifacts
        .into_iter()
        .find(|artifact| {
            artifact.id == base_revision_id
                && gitops_base_revision_matches_change_set(artifact, change_set)
        })
        .ok_or_else(|| {
            ApiError::conflict("GitOps delivery plan base revision is no longer current")
        })?;
    Ok((plan, base_revision))
}

pub(in crate::app) async fn authorize_gitops_change_set_delivery(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(gitops_change_set_id): Path<String>,
    Json(request): Json<CreateGitOpsDeliveryAuthorizationRequest>,
) -> Result<Json<GitOpsDeliveryAuthorizationResponse>, ApiError> {
    let change_set = state
        .store
        .get_gitops_change_set(&gitops_change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("gitops_change_set", &gitops_change_set_id))?;
    if change_set.status != "approved" {
        return Err(ApiError::conflict(
            "GitOps delivery authorization requires an approved GitOps ChangeSet",
        ));
    }
    let work_plan = state
        .store
        .get_work_plan(&change_set.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &change_set.work_plan_id))?;
    ensure_approved_for_trusted_envelope("work_plan", &work_plan.id, &work_plan.status)?;
    let work_item = state
        .store
        .get_work_item(&change_set.work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &change_set.work_item_id))?;
    ensure_gitops_delivery_target(&work_item, &change_set)?;
    let (plan, _) = current_gitops_delivery_plan(&state.store, &change_set).await?;
    let subject = clean_optional_text(request.subject)
        .unwrap_or_else(|| DEFAULT_GITOPS_WRITER_SUBJECT.to_string());
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.created_by.clone()));
    let reason = required_text(request.reason, "reason")?;
    let expires_at = bounded_production_grant_expiry(&work_item, request.expires_at)?;
    if let Some(existing) =
        matching_gitops_delivery_grant(&state.store, &subject, &change_set, &work_item, &plan.id)
            .await?
    {
        return Ok(Json(GitOpsDeliveryAuthorizationResponse {
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
                "actions": GITOPS_DELIVERY_ACTIONS,
                "max_risk": "high",
                "repos": [change_set.gitops_repo],
                "branches": [change_set.head_branch],
                "work_plan_ids": [change_set.work_plan_id],
                "gitops_change_set_ids": [change_set.id],
                "gitops_delivery_plan_artifact_ids": [plan.id],
                "production_impacting": work_item.production_impacting,
            }),
            policy: json!({ "policy_mode": "supervised_autonomy" }),
            expires_at,
        },
    )
    .await?;
    append_gitops_change_set_audit_event(
        &state.store,
        &change_set,
        "gitops_change_set.delivery_authorized",
        actor,
        Some(reason),
        json!({
            "permission_grant_id": grant.id,
            "gitops_delivery_plan_artifact_id": plan.id,
            "subject": grant.subject,
        }),
    )
    .await?;
    Ok(Json(GitOpsDeliveryAuthorizationResponse {
        grant: grant.into(),
        plan: plan.into(),
        created: true,
    }))
}

pub(in crate::app) async fn preflight_gitops_change_set_delivery(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(gitops_change_set_id): Path<String>,
    Json(request): Json<GitOpsDeliveryPreflightRequest>,
) -> Result<Json<GitOpsDeliveryPreflightResponse>, ApiError> {
    let change_set = state
        .store
        .get_gitops_change_set(&gitops_change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("gitops_change_set", &gitops_change_set_id))?;
    let work_plan = state
        .store
        .get_work_plan(&change_set.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &change_set.work_plan_id))?;
    let work_item = state
        .store
        .get_work_item(&change_set.work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", &change_set.work_item_id))?;
    let (plan, base_revision) = current_gitops_delivery_plan(&state.store, &change_set).await?;
    let approval_gate = state
        .store
        .list_approval_gates(ApprovalGateListFilter {
            work_item_id: Some(work_item.id.clone()),
            gate_kind: Some("gitops_mutation".to_string()),
            limit: 20,
            ..ApprovalGateListFilter::default()
        })
        .await?
        .into_iter()
        .find(|gate| work_item_gate_scope_matches(gate, &work_item, &work_plan, "gitops_mutation"));
    let approval_gate_ready = approval_gate
        .as_ref()
        .is_some_and(|gate| matches!(gate.status.as_str(), "satisfied" | "waived"));
    let subject = clean_optional_text(request.subject)
        .unwrap_or_else(|| DEFAULT_GITOPS_WRITER_SUBJECT.to_string());
    let grant =
        matching_gitops_delivery_grant(&state.store, &subject, &change_set, &work_item, &plan.id)
            .await?;
    let authorization_ready = grant.is_some();
    let writer_settings = state.worker.gitops_writer_settings();
    let dispatch_ready = writer_settings.as_ref().is_some_and(|settings| {
        settings
            .allowed_repos
            .iter()
            .any(|repo| repo == &change_set.gitops_repo)
    });
    let target_valid = ensure_gitops_delivery_target(&work_item, &change_set).is_ok();
    let base_commit = base_revision
        .content_json
        .as_ref()
        .and_then(|content| content.get("base_commit"))
        .and_then(Value::as_str)
        .unwrap_or("missing");
    let checks = vec![
        execution_check(
            "gitops_change_set_approved",
            change_set.status == "approved",
            format!("GitOps ChangeSet status is {}", change_set.status),
        ),
        execution_check(
            "work_plan_approved",
            work_plan.status == "approved",
            format!("WorkPlan status is {}", work_plan.status),
        ),
        execution_check(
            "supported_gitops_target",
            target_valid,
            if target_valid {
                format!(
                    "GitOps target is {} at {}",
                    change_set.gitops_repo, change_set.gitops_ref
                )
            } else {
                "GitOps ChangeSet no longer matches a supported dev or exact protected-production WorkItem target"
                    .to_string()
            },
        ),
        execution_check(
            "immutable_gitops_base_revision",
            is_git_sha(base_commit),
            format!("Observer resolved GitOps base commit {base_commit}"),
        ),
        execution_check(
            "work_item_gitops_mutation_gate",
            approval_gate_ready,
            approval_gate
                .as_ref()
                .map(|gate| format!("GitOps mutation gate {} is {}", gate.id, gate.status))
                .unwrap_or_else(|| {
                    "No scoped WorkItem gitops_mutation gate matches this delivery plan".to_string()
                }),
        ),
        execution_check(
            "trusted_gitops_delivery_grant",
            authorization_ready,
            grant
                .as_ref()
                .map(|grant| {
                    format!(
                        "Active supervised-autonomy grant {} matches GitOps writer {}",
                        grant.id, subject
                    )
                })
                .unwrap_or_else(|| {
                    format!(
                        "No active supervised-autonomy GitOps delivery grant matches writer {}",
                        subject
                    )
                }),
        ),
        execution_check(
            "gitops_writer_executor_available",
            dispatch_ready,
            if writer_settings.is_none() {
                "No dedicated GitOps writer executor is configured; branch, commit, push, and pull-request creation remain unavailable".to_string()
            } else {
                format!(
                    "Dedicated GitOps writer is configured but does not allow repository {}",
                    change_set.gitops_repo
                )
            },
        ),
    ];
    let prerequisites_ready = checks
        .iter()
        .filter(|check| {
            check.get("code").and_then(Value::as_str) != Some("gitops_writer_executor_available")
        })
        .all(|check| check.get("passed").and_then(Value::as_bool) == Some(true));
    let status = if prerequisites_ready {
        "ready_for_writer"
    } else {
        "blocked"
    };
    let grant_id = grant.as_ref().map(|grant| grant.id.clone());
    let artifacts = state.store.list_artifacts(&change_set.run_id).await?;
    if let Some(existing) = artifacts.into_iter().find(|artifact| {
        artifact.kind == "gitops_delivery_preflight"
            && artifact.content_json.as_ref().is_some_and(|content| {
                content
                    .get("gitops_delivery_plan_artifact_id")
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
        return Ok(Json(GitOpsDeliveryPreflightResponse {
            status: status.to_string(),
            approval_gate_ready,
            authorization_ready,
            dispatch_ready,
            plan: plan.into(),
            base_revision: base_revision.into(),
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
            id: format!("art_{}_gitops_delivery_preflight", unique_suffix()),
            session_id: change_set.session_id.clone(),
            run_id: Some(change_set.run_id.clone()),
            kind: "gitops_delivery_preflight".to_string(),
            label: format!("GitOps delivery preflight for {}", change_set.id),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "gitops_change_set_id": change_set.id,
                "work_plan_id": change_set.work_plan_id,
                "work_item_id": work_item.id,
                "gitops_delivery_plan_artifact_id": plan.id,
                "gitops_base_revision_artifact_id": base_revision.id,
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
                    "summary": if dispatch_ready { "Dedicated GitOps writer is configured for this exact repository; an explicit delivery execution request will still revalidate the gate and plan-scoped grant before it can create a branch and pull request" } else { "GitOps writer execution is unavailable until its separate identity and executor are configured" },
                },
                "reason": reason,
            })),
        })
        .await?;
    append_gitops_change_set_audit_event(
        &state.store,
        &change_set,
        "gitops_change_set.delivery_preflighted",
        actor,
        reason,
        json!({
            "gitops_delivery_plan_artifact_id": plan.id,
            "gitops_delivery_preflight_artifact_id": artifact.id,
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
    Ok(Json(GitOpsDeliveryPreflightResponse {
        status: status.to_string(),
        approval_gate_ready,
        authorization_ready,
        dispatch_ready,
        plan: plan.into(),
        base_revision: base_revision.into(),
        permission_grant: grant.map(Into::into),
        checks,
        artifact: artifact.into(),
        created: true,
    }))
}

pub(in crate::app) async fn execute_gitops_change_set_delivery(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(gitops_change_set_id): Path<String>,
    Json(request): Json<ExecuteGitOpsDeliveryRequest>,
) -> Result<Json<ExecuteGitOpsDeliveryResponse>, ApiError> {
    let subject = clean_optional_text(request.subject.clone())
        .unwrap_or_else(|| DEFAULT_GITOPS_WRITER_SUBJECT.to_string());
    let actor = identity
        .as_ref()
        .map(|Extension(OperatorIdentity(name))| name.clone())
        .or_else(|| clean_optional_text(request.actor.clone()));
    let reason = clean_optional_text(Some(request.reason))
        .ok_or_else(|| ApiError::bad_request("GitOps delivery execution reason is required"))?;
    let Json(preflight) = preflight_gitops_change_set_delivery(
        State(state.clone()),
        identity,
        Path(gitops_change_set_id.clone()),
        Json(GitOpsDeliveryPreflightRequest {
            subject: Some(subject.clone()),
            actor: actor.clone(),
            reason: Some(reason.clone()),
        }),
    )
    .await?;
    if preflight.status != "ready_for_writer" || !preflight.dispatch_ready {
        return Err(ApiError::conflict(
            "GitOps delivery execution requires a current approved plan, satisfied gate, matching writer grant, and configured dedicated writer",
        ));
    }
    let grant = preflight.permission_grant.clone().ok_or_else(|| {
        ApiError::conflict("GitOps delivery execution requires an active matching writer grant")
    })?;
    let change_set = state
        .store
        .get_gitops_change_set(&gitops_change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("gitops_change_set", &gitops_change_set_id))?;
    let plan = state
        .store
        .get_artifact(&preflight.plan.id)
        .await?
        .ok_or_else(|| ApiError::conflict("current GitOps delivery plan is unavailable"))?;
    let source = gitops_delivery_plan_source(&plan, &change_set)?;
    let settings = state
        .worker
        .gitops_writer_settings()
        .ok_or_else(|| ApiError::conflict("GitOps writer executor is not configured"))?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == &source.repository)
    {
        return Err(ApiError::conflict(
            "GitOps delivery repository is not allowlisted for the dedicated GitOps writer",
        ));
    }
    let artifacts = state.store.list_artifacts(&change_set.run_id).await?;
    if let Some(existing) = artifacts.iter().find(|artifact| {
        gitops_delivery_artifact_matches_plan(artifact, "gitops_delivery_execution", &plan.id)
            && artifact.content_json.as_ref().is_some_and(|content| {
                content.get("permission_grant_id").and_then(Value::as_str)
                    == Some(grant.id.as_str())
            })
    }) {
        let terminal_status = existing
            .content_json
            .as_ref()
            .and_then(|content| content.get("execution_id"))
            .and_then(Value::as_str)
            .and_then(|execution_id| {
                artifacts.iter().find_map(|artifact| {
                    (artifact.kind == "gitops_delivery_result")
                        .then_some(artifact.content_json.as_ref())
                        .flatten()
                        .filter(|content| {
                            content.get("execution_id").and_then(Value::as_str)
                                == Some(execution_id)
                        })
                        .and_then(|content| content.get("status").and_then(Value::as_str))
                })
            })
            .unwrap_or("dispatched");
        return Ok(Json(ExecuteGitOpsDeliveryResponse {
            status: terminal_status.to_string(),
            execution: existing.clone().into(),
            plan: plan.into(),
            permission_grant: grant,
            job_name: existing
                .content_json
                .as_ref()
                .and_then(|content| content.get("job_name"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            created: false,
        }));
    }
    let execution_id = format!("gopsexec_{}", unique_suffix());
    let execution = state
        .store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_gitops_delivery_execution", unique_suffix()),
            session_id: change_set.session_id.clone(),
            run_id: Some(change_set.run_id.clone()),
            kind: "gitops_delivery_execution".to_string(),
            label: format!("GitOps delivery execution for {}", change_set.id),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "execution_id": execution_id,
                "status": "dispatched",
                "gitops_change_set_id": change_set.id,
                "gitops_delivery_plan_artifact_id": plan.id,
                "permission_grant_id": grant.id,
                "subject": subject,
                "dispatched_by": actor,
                "reason": reason,
                "source": {
                    "repository": source.repository,
                    "base_ref": source.base_ref,
                    "base_commit": source.base_commit,
                    "head_branch": source.head_branch,
                    "kustomization_path": source.kustomization_path,
                    "image_name": source.image_name,
                    "image_ref": source.image_ref,
                },
            })),
        })
        .await?;
    match state
        .worker
        .dispatch_gitops_delivery(GitOpsDeliveryExecutionRequest {
            gitops_change_set_id: change_set.id.clone(),
            execution_id: execution_id.clone(),
        })
        .await
    {
        Ok(receipt) => {
            append_gitops_change_set_audit_event(
                &state.store,
                &change_set,
                "gitops_change_set.delivery_dispatched",
                actor,
                Some(reason),
                json!({
                    "execution_id": execution_id,
                    "gitops_delivery_plan_artifact_id": plan.id,
                    "permission_grant_id": grant.id,
                    "job_name": receipt.job_name,
                }),
            )
            .await?;
            Ok(Json(ExecuteGitOpsDeliveryResponse {
                status: "dispatched".to_string(),
                execution: execution.into(),
                plan: plan.into(),
                permission_grant: grant,
                job_name: Some(receipt.job_name),
                created: true,
            }))
        }
        Err(error) => {
            let failure = persist_gitops_delivery_result(
                &state.store,
                &change_set,
                &plan.id,
                &execution_id,
                "dispatch_failed",
                json!({ "error_code": "job_dispatch_failed" }),
            )
            .await?;
            tracing::warn!(gitops_change_set_id = %change_set.id, %error, "GitOps writer dispatch failed");
            Ok(Json(ExecuteGitOpsDeliveryResponse {
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

pub(in crate::app) async fn observe_gitops_change_set_delivery(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(gitops_change_set_id): Path<String>,
    Json(request): Json<ObserveGitOpsDeliveryRequest>,
) -> Result<Json<ObserveGitOpsDeliveryResponse>, ApiError> {
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = clean_optional_text(Some(request.reason))
        .ok_or_else(|| ApiError::bad_request("GitOps delivery observation reason is required"))?;
    let change_set = state
        .store
        .get_gitops_change_set(&gitops_change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("gitops_change_set", &gitops_change_set_id))?;
    let (plan, _) = current_gitops_delivery_plan(&state.store, &change_set).await?;
    let source = gitops_delivery_plan_source(&plan, &change_set)?;
    let settings = state
        .worker
        .gitops_observer_settings()
        .ok_or_else(|| ApiError::conflict("GitOps observer executor is not configured"))?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == &source.repository)
    {
        return Err(ApiError::conflict(
            "GitOps delivery repository is not allowlisted for the Git observer",
        ));
    }
    let artifacts = state.store.list_artifacts(&change_set.run_id).await?;
    let delivery_result = artifacts
        .iter()
        .filter(|artifact| {
            gitops_delivery_artifact_matches_plan(artifact, "gitops_delivery_result", &plan.id)
        })
        .filter(|artifact| {
            artifact
                .content_json
                .as_ref()
                .and_then(|content| content.get("status"))
                .and_then(Value::as_str)
                == Some("completed")
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .ok_or_else(|| {
            ApiError::conflict(
                "GitOps delivery observation requires a completed branch-and-PR result",
            )
        })?;
    let details = delivery_result
        .content_json
        .as_ref()
        .and_then(|content| content.get("details"))
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ApiError::conflict("GitOps delivery result has no pull-request provenance")
        })?;
    let pull_request_number = details
        .get("pull_request_number")
        .and_then(Value::as_u64)
        .ok_or_else(|| ApiError::conflict("GitOps delivery result has no pull-request number"))?;
    let pull_request_url =
        required_json_string(details, "pull_request_url", "GitOps delivery result")?;
    let source_commit_sha = required_json_string(details, "commit_sha", "GitOps delivery result")?;
    if !is_git_sha(&source_commit_sha) || !is_github_pr_url(&pull_request_url) {
        return Err(ApiError::conflict(
            "GitOps delivery result has invalid GitHub provenance",
        ));
    }
    if let Some(existing) = artifacts
        .iter()
        .filter(|artifact| {
            artifact.kind == "gitops_delivery_observation_execution"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content
                        .get("gitops_delivery_plan_artifact_id")
                        .and_then(Value::as_str)
                        == Some(plan.id.as_str())
                        && content
                            .get("gitops_delivery_result_artifact_id")
                            .and_then(Value::as_str)
                            == Some(delivery_result.id.as_str())
                        && !artifacts.iter().any(|failure| {
                            failure.kind == "gitops_delivery_observation_dispatch_failure"
                                && failure
                                    .content_json
                                    .as_ref()
                                    .is_some_and(|failure_content| {
                                        failure_content.get("execution_id").and_then(Value::as_str)
                                            == content.get("execution_id").and_then(Value::as_str)
                                    })
                        })
                })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
    {
        let execution_id = existing
            .content_json
            .as_ref()
            .and_then(|content| content.get("execution_id"))
            .and_then(Value::as_str);
        let terminal_observation = execution_id.and_then(|execution_id| {
            artifacts
                .iter()
                .filter(|artifact| {
                    gitops_delivery_artifact_matches_plan(
                        artifact,
                        "gitops_delivery_pr_observation",
                        &plan.id,
                    ) && artifact.content_json.as_ref().is_some_and(|content| {
                        content.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                    })
                })
                .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        });
        if !terminal_observation.is_some_and(|observation| {
            gitops_observation_refreshable(observation.content_json.as_ref())
        }) {
            return Ok(Json(ObserveGitOpsDeliveryResponse {
                status: existing
                    .content_json
                    .as_ref()
                    .and_then(|content| content.get("status"))
                    .and_then(Value::as_str)
                    .unwrap_or("dispatched")
                    .to_string(),
                execution: existing.clone().into(),
                job_name: existing
                    .content_json
                    .as_ref()
                    .and_then(|content| content.get("job_name"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                created: false,
            }));
        }
    }
    let execution_id = format!("gopsobs_{}", unique_suffix());
    let execution = state.store.create_artifact(CreateArtifact {
        id: format!("art_{}_gitops_delivery_observation", unique_suffix()),
        session_id: change_set.session_id.clone(), run_id: Some(change_set.run_id.clone()),
        kind: "gitops_delivery_observation_execution".to_string(),
        label: format!("GitOps delivery observation for {}", change_set.id),
        mime_type: Some("application/json".to_string()), path: None, content_text: None,
        content_json: Some(json!({"execution_id":execution_id,"status":"dispatched","gitops_change_set_id":change_set.id,"gitops_delivery_plan_artifact_id":plan.id,"gitops_delivery_result_artifact_id":delivery_result.id,
            "source":{"repository":source.repository,"head_branch":source.head_branch,"source_commit_sha":source_commit_sha,"pull_request_url":pull_request_url,"pull_request_number":pull_request_number},"dispatched_by":actor,"reason":reason})),
    }).await?;
    match state
        .worker
        .dispatch_gitops_delivery_observation(GitOpsDeliveryObservationRequest {
            gitops_change_set_id: change_set.id.clone(),
            execution_id: execution_id.clone(),
        })
        .await
    {
        Ok(receipt) => {
            append_gitops_change_set_audit_event(&state.store, &change_set, "gitops_change_set.delivery_observation_dispatched", actor, Some(reason), json!({"execution_id":execution_id,"gitops_delivery_plan_artifact_id":plan.id,"job_name":receipt.job_name})).await?;
            Ok(Json(ObserveGitOpsDeliveryResponse {
                status: "dispatched".to_string(),
                execution: execution.into(),
                job_name: Some(receipt.job_name),
                created: true,
            }))
        }
        Err(error) => {
            tracing::warn!(gitops_change_set_id = %change_set.id, %error, "GitOps observer dispatch failed");
            let failure = state
                .store
                .create_artifact(CreateArtifact {
                    id: format!(
                        "art_{}_gitops_delivery_observation_dispatch_failure",
                        unique_suffix()
                    ),
                    session_id: change_set.session_id.clone(),
                    run_id: Some(change_set.run_id.clone()),
                    kind: "gitops_delivery_observation_dispatch_failure".to_string(),
                    label: format!(
                        "GitOps delivery observation dispatch failure for {}",
                        change_set.id
                    ),
                    mime_type: Some("application/json".to_string()),
                    path: None,
                    content_text: None,
                    content_json: Some(json!({
                        "execution_id": execution_id,
                        "status": "dispatch_failed",
                        "gitops_change_set_id": change_set.id,
                        "gitops_delivery_plan_artifact_id": plan.id,
                        "gitops_delivery_result_artifact_id": delivery_result.id,
                        "error_code": "gitops_observer_dispatch_failed",
                    })),
                })
                .await?;
            append_gitops_change_set_audit_event(
                &state.store,
                &change_set,
                "gitops_change_set.delivery_observation_dispatch_failed",
                actor,
                Some(reason),
                json!({
                    "execution_id": execution_id,
                    "gitops_delivery_plan_artifact_id": plan.id,
                    "dispatch_failure_artifact_id": failure.id,
                    "error_code": "gitops_observer_dispatch_failed",
                }),
            )
            .await?;
            Ok(Json(ObserveGitOpsDeliveryResponse {
                status: "dispatch_failed".to_string(),
                execution: execution.into(),
                job_name: None,
                created: true,
            }))
        }
    }
}

pub(in crate::app) fn ensure_gitops_delivery_target(
    work_item: &StoredWorkItem,
    change_set: &StoredGitOpsChangeSet,
) -> Result<(), ApiError> {
    if !work_item_target_supported(work_item) {
        return Err(ApiError::conflict(
            "GitOps delivery is limited to dev or the exact protected production target",
        ));
    }
    if work_item.gitops_repo.as_deref() != Some(change_set.gitops_repo.as_str())
        || work_item.gitops_ref.as_deref() != Some(change_set.gitops_ref.as_str())
        || !safe_relative_gitops_path(&change_set.kustomization_path)
        || !change_set.image_ref.contains("@sha256:")
    {
        return Err(ApiError::conflict(
            "GitOps ChangeSet no longer matches its declared WorkItem target or safety constraints",
        ));
    }
    Ok(())
}

pub(in crate::app) async fn gitops_delivery_flow(
    store: &SqliteStore,
    change_set: Option<&StoredGitOpsChangeSet>,
) -> Result<Option<GitOpsDeliveryFlowResponse>, ApiError> {
    let Some(change_set) = change_set else {
        return Ok(None);
    };
    let artifacts = store.list_artifacts(&change_set.run_id).await?;
    let Some(plan) = artifacts
        .iter()
        .find(|artifact| gitops_delivery_plan_matches_change_set(artifact, change_set))
    else {
        return Ok(None);
    };
    let base_revision_id = plan
        .content_json
        .as_ref()
        .and_then(|content| content.pointer("/source/base_revision_artifact_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ApiError::conflict("GitOps delivery plan has no base revision provenance")
        })?;
    let base_revision = artifacts
        .iter()
        .find(|artifact| {
            artifact.id == base_revision_id
                && gitops_base_revision_matches_change_set(artifact, change_set)
        })
        .cloned()
        .ok_or_else(|| {
            ApiError::conflict("GitOps delivery plan base revision is no longer current")
        })?;
    let latest_preflight = artifacts
        .iter()
        .filter(|artifact| {
            artifact.kind == "gitops_delivery_preflight"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content
                        .get("gitops_delivery_plan_artifact_id")
                        .and_then(Value::as_str)
                        == Some(plan.id.as_str())
                })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .map(Into::into);
    let latest_execution = artifacts
        .iter()
        .filter(|artifact| {
            gitops_delivery_artifact_matches_plan(artifact, "gitops_delivery_execution", &plan.id)
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .map(Into::into);
    let latest_result = artifacts
        .iter()
        .filter(|artifact| {
            gitops_delivery_artifact_matches_plan(artifact, "gitops_delivery_result", &plan.id)
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .map(Into::into);
    let latest_observation = artifacts
        .iter()
        .filter(|artifact| {
            gitops_delivery_artifact_matches_plan(
                artifact,
                "gitops_delivery_pr_observation",
                &plan.id,
            )
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .map(Into::into);
    let latest_merge = artifacts
        .iter()
        .filter(|artifact| {
            gitops_delivery_artifact_matches_plan(artifact, "gitops_delivery_merge", &plan.id)
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))
        .cloned()
        .map(Into::into);
    Ok(Some(GitOpsDeliveryFlowResponse {
        plan: plan.clone().into(),
        base_revision: base_revision.into(),
        latest_preflight,
        latest_execution,
        latest_result,
        latest_observation,
        latest_merge,
    }))
}

#[derive(Debug, serde::Deserialize)]
pub(in crate::app) struct InternalGitOpsBaseRevisionQuery {
    execution_id: String,
}

pub(in crate::app) async fn internal_gitops_base_revision_context(
    State(state): State<AppState>,
    Path(gitops_change_set_id): Path<String>,
    Query(query): Query<InternalGitOpsBaseRevisionQuery>,
) -> Result<Json<GitOpsBaseRevisionContextResponse>, ApiError> {
    let (change_set, execution) =
        current_gitops_base_revision_execution(&state, &gitops_change_set_id, &query.execution_id)
            .await?;
    let settings = state.worker.gitops_observer_settings().ok_or_else(|| {
        ApiError::conflict(
            "read-only GitOps observer identity is not configured for GitOps revision resolution",
        )
    })?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == &change_set.gitops_repo)
    {
        return Err(ApiError::conflict(
            "GitOps repository is not allowlisted for the read-only Git observer identity",
        ));
    }
    let _ = execution;
    Ok(Json(GitOpsBaseRevisionContextResponse {
        execution_id: query.execution_id,
        repository: change_set.gitops_repo,
        base_ref: change_set.gitops_ref,
        github_api_url: settings.github_api_url,
    }))
}

pub(in crate::app) async fn internal_gitops_base_revision_outcome(
    State(state): State<AppState>,
    Path(gitops_change_set_id): Path<String>,
    Json(request): Json<GitOpsBaseRevisionOutcomeRequest>,
) -> Result<Json<ArtifactResponse>, ApiError> {
    let (change_set, execution) = current_gitops_base_revision_execution(
        &state,
        &gitops_change_set_id,
        &request.execution_id,
    )
    .await?;
    let result = match request.status.as_str() {
        "resolved" => {
            let base_commit = clean_optional_text(request.base_commit)
                .ok_or_else(|| ApiError::bad_request("resolved base revision outcome requires base_commit"))?;
            if !is_git_sha(&base_commit) {
                return Err(ApiError::bad_request(
                    "resolved base revision outcome requires a 40-character Git SHA",
                ));
            }
            state
                .store
                .create_artifact(CreateArtifact {
                    id: format!("art_{}_gitops_base_revision", unique_suffix()),
                    session_id: change_set.session_id.clone(),
                    run_id: Some(change_set.run_id.clone()),
                    kind: "gitops_base_revision".to_string(),
                    label: format!("Resolved GitOps base revision for {}", change_set.id),
                    mime_type: Some("application/json".to_string()),
                    path: None,
                    content_text: None,
                    content_json: Some(json!({
                        "execution_id": request.execution_id,
                        "status": "resolved",
                        "gitops_change_set_id": change_set.id,
                        "gitops_change_set_revision": change_set.revision,
                        "material_hash": change_set.material_hash,
                        "repository": change_set.gitops_repo,
                        "base_ref": change_set.gitops_ref,
                        "base_commit": base_commit,
                        "execution_artifact_id": execution.id,
                        "identity": "agent:git-observer",
                    })),
                })
                .await?
        }
        "failed" => state
            .store
            .create_artifact(CreateArtifact {
                id: format!("art_{}_gitops_base_revision", unique_suffix()),
                session_id: change_set.session_id.clone(),
                run_id: Some(change_set.run_id.clone()),
                kind: "gitops_base_revision".to_string(),
                label: format!("Failed GitOps base revision resolution for {}", change_set.id),
                mime_type: Some("application/json".to_string()),
                path: None,
                content_text: None,
                content_json: Some(json!({
                    "execution_id": request.execution_id,
                    "status": "failed",
                    "gitops_change_set_id": change_set.id,
                    "gitops_change_set_revision": change_set.revision,
                    "material_hash": change_set.material_hash,
                    "repository": change_set.gitops_repo,
                    "base_ref": change_set.gitops_ref,
                    "execution_artifact_id": execution.id,
                    "identity": "agent:git-observer",
                    "error_code": clean_optional_text(request.error_code).unwrap_or_else(|| "gitops_revision_resolver_failed".to_string()),
                })),
            })
            .await?,
        _ => {
            return Err(ApiError::bad_request(
                "GitOps base revision outcome status must be resolved or failed",
            ))
        }
    };
    append_gitops_change_set_audit_event(
        &state.store,
        &change_set,
        &format!("gitops_change_set.base_revision_{}", request.status),
        Some("agent:git-observer".to_string()),
        None,
        json!({ "execution_id": request.execution_id, "execution_artifact_id": execution.id, "result_artifact_id": result.id }),
    )
    .await?;
    Ok(Json(result.into()))
}

pub(in crate::app) async fn current_gitops_base_revision_execution(
    state: &AppState,
    gitops_change_set_id: &str,
    execution_id: &str,
) -> Result<(StoredGitOpsChangeSet, StoredArtifact), ApiError> {
    let change_set = state
        .store
        .get_gitops_change_set(gitops_change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("gitops_change_set", gitops_change_set_id))?;
    let execution = state
        .store
        .list_artifacts(&change_set.run_id)
        .await?
        .into_iter()
        .find(|artifact| {
            artifact.kind == "gitops_base_revision_execution"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                        && content.get("gitops_change_set_id").and_then(Value::as_str)
                            == Some(change_set.id.as_str())
                        && content.get("material_hash").and_then(Value::as_str)
                            == Some(change_set.material_hash.as_str())
                        && gitops_artifact_change_set_revision(content) == change_set.revision
                })
        })
        .ok_or_else(|| ApiError::conflict("GitOps base revision execution is not current"))?;
    Ok((change_set, execution))
}

#[derive(Debug, serde::Deserialize)]
pub(in crate::app) struct InternalGitOpsDeliveryQuery {
    execution_id: String,
}

pub(in crate::app) async fn internal_gitops_delivery_context(
    State(state): State<AppState>,
    Path(gitops_change_set_id): Path<String>,
    Query(query): Query<InternalGitOpsDeliveryQuery>,
) -> Result<Json<GitOpsDeliveryContextResponse>, ApiError> {
    if gitops_change_set_id.starts_with("rollback_") {
        return internal_rollback_delivery_context(
            &state,
            &gitops_change_set_id,
            &query.execution_id,
        )
        .await;
    }
    let (change_set, plan, _execution) =
        current_gitops_delivery_execution(&state, &gitops_change_set_id, &query.execution_id)
            .await?;
    let source = gitops_delivery_plan_source(&plan, &change_set)?;
    let settings = state.worker.gitops_writer_settings().ok_or_else(|| {
        ApiError::conflict("GitOps writer executor is not configured for delivery context")
    })?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == &source.repository)
    {
        return Err(ApiError::conflict(
            "GitOps repository is not allowlisted for the dedicated GitOps writer",
        ));
    }
    Ok(Json(GitOpsDeliveryContextResponse {
        execution_id: query.execution_id,
        repository: source.repository,
        base_ref: source.base_ref,
        base_commit: source.base_commit,
        head_branch: source.head_branch,
        kustomization_path: source.kustomization_path,
        image_name: source.image_name,
        image_ref: source.image_ref,
        commit_subject: compact_delivery_subject(&change_set.title),
        commit_body: format!(
            "GitOps ChangeSet {} revision {}\n\n{}",
            change_set.id, change_set.revision, change_set.summary
        ),
        pull_request_title: compact_delivery_subject(&change_set.title),
        pull_request_body: format!(
            "{}\n\nPharness GitOps ChangeSet: {}\nWorkItem: {}",
            change_set.summary, change_set.id, change_set.work_item_id
        ),
        github_api_url: settings.github_api_url,
        author_name: settings.author_name,
        author_email: settings.author_email,
    }))
}

pub(in crate::app) async fn internal_gitops_delivery_outcome(
    State(state): State<AppState>,
    Path(gitops_change_set_id): Path<String>,
    Json(request): Json<GitOpsDeliveryOutcomeRequest>,
) -> Result<Json<ArtifactResponse>, ApiError> {
    if gitops_change_set_id.starts_with("rollback_") {
        return internal_rollback_delivery_outcome(&state, &gitops_change_set_id, request).await;
    }
    let (change_set, plan, _execution) =
        current_gitops_delivery_execution(&state, &gitops_change_set_id, &request.execution_id)
            .await?;
    let result = match request.status.as_str() {
        "completed" => {
            let branch = clean_optional_text(request.branch).ok_or_else(|| {
                ApiError::bad_request("completed GitOps delivery requires branch")
            })?;
            let commit_sha = clean_optional_text(request.commit_sha).ok_or_else(|| {
                ApiError::bad_request("completed GitOps delivery requires commit_sha")
            })?;
            let pull_request_url =
                clean_optional_text(request.pull_request_url).ok_or_else(|| {
                    ApiError::bad_request("completed GitOps delivery requires pull_request_url")
                })?;
            let pull_request_number = request.pull_request_number.ok_or_else(|| {
                ApiError::bad_request("completed GitOps delivery requires pull_request_number")
            })?;
            let source = gitops_delivery_plan_source(&plan, &change_set)?;
            if branch != source.head_branch
                || !is_git_sha(&commit_sha)
                || !is_github_pr_url(&pull_request_url)
            {
                return Err(ApiError::conflict(
                    "GitOps delivery outcome does not match immutable branch or GitHub provenance",
                ));
            }
            let expected_pr_prefix = format!(
                "https://github.com/{}/pull/",
                source
                    .repository
                    .trim_start_matches("https://github.com/")
                    .trim_end_matches(".git")
            );
            if !pull_request_url.starts_with(&expected_pr_prefix)
                || !pull_request_url.ends_with(&pull_request_number.to_string())
            {
                return Err(ApiError::conflict(
                    "GitOps pull request does not match immutable repository provenance",
                ));
            }
            persist_gitops_delivery_result(
                &state.store,
                &change_set,
                &plan.id,
                &request.execution_id,
                "completed",
                json!({
                    "branch": branch,
                    "commit_sha": commit_sha,
                    "pull_request_url": pull_request_url,
                    "pull_request_number": pull_request_number,
                }),
            )
            .await?
        }
        "failed" => {
            persist_gitops_delivery_result(
                &state.store,
                &change_set,
                &plan.id,
                &request.execution_id,
                "failed",
                json!({
                    "error_code": clean_optional_text(request.error_code)
                        .unwrap_or_else(|| "gitops_writer_failed".to_string()),
                }),
            )
            .await?
        }
        _ => {
            return Err(ApiError::bad_request(
                "GitOps delivery outcome status must be completed or failed",
            ))
        }
    };
    append_gitops_change_set_audit_event(
        &state.store,
        &change_set,
        &format!("gitops_change_set.delivery_{}", request.status),
        Some(DEFAULT_GITOPS_WRITER_SUBJECT.to_string()),
        None,
        json!({
            "execution_id": request.execution_id,
            "gitops_delivery_plan_artifact_id": plan.id,
            "result_artifact_id": result.id,
        }),
    )
    .await?;
    Ok(Json(result))
}

pub(in crate::app) async fn internal_gitops_delivery_observation_context(
    State(state): State<AppState>,
    Path(gitops_change_set_id): Path<String>,
    Query(query): Query<InternalGitOpsDeliveryQuery>,
) -> Result<Json<GitOpsDeliveryObservationContextResponse>, ApiError> {
    if gitops_change_set_id.starts_with("rollback_") {
        return internal_rollback_delivery_observation_context(
            &state,
            &gitops_change_set_id,
            &query.execution_id,
        )
        .await;
    }
    let (change_set, _plan, execution) =
        current_gitops_delivery_observation(&state, &gitops_change_set_id, &query.execution_id)
            .await?;
    let settings = state
        .worker
        .gitops_observer_settings()
        .ok_or_else(|| ApiError::conflict("GitOps observer executor is not configured"))?;
    let source = execution
        .content_json
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|content| content.get("source"))
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ApiError::conflict("GitOps observation execution has no source provenance")
        })?;
    let repository = required_json_string(source, "repository", "GitOps observation source")?;
    if !settings
        .allowed_repos
        .iter()
        .any(|repo| repo == &repository)
    {
        return Err(ApiError::conflict(
            "GitOps delivery repository is not allowlisted for the Git observer",
        ));
    }
    let _ = change_set;
    Ok(Json(GitOpsDeliveryObservationContextResponse {
        execution_id: query.execution_id,
        repository,
        head_branch: required_json_string(source, "head_branch", "GitOps observation source")?,
        source_commit_sha: required_json_string(
            source,
            "source_commit_sha",
            "GitOps observation source",
        )?,
        pull_request_url: required_json_string(
            source,
            "pull_request_url",
            "GitOps observation source",
        )?,
        pull_request_number: source
            .get("pull_request_number")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                ApiError::conflict("GitOps observation source has no pull-request number")
            })?,
        github_api_url: settings.github_api_url,
    }))
}

pub(in crate::app) async fn internal_gitops_delivery_observation_outcome(
    State(state): State<AppState>,
    Path(gitops_change_set_id): Path<String>,
    Json(request): Json<GitOpsDeliveryObservationOutcomeRequest>,
) -> Result<Json<ArtifactResponse>, ApiError> {
    if gitops_change_set_id.starts_with("rollback_") {
        return internal_rollback_delivery_observation_outcome(
            &state,
            &gitops_change_set_id,
            request,
        )
        .await;
    }
    let (change_set, plan, execution) =
        current_gitops_delivery_observation(&state, &gitops_change_set_id, &request.execution_id)
            .await?;
    let expected = execution
        .content_json
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|content| content.get("source"))
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ApiError::conflict("GitOps observation execution has no source provenance")
        })?;
    let artifact: ArtifactResponse = match request.status.as_str() {
        "observed" => {
            let state_value = clean_optional_text(request.pull_request_state).ok_or_else(|| ApiError::bad_request("observed GitOps outcome requires pull_request_state"))?;
            let merged = request.merged.ok_or_else(|| ApiError::bad_request("observed GitOps outcome requires merged"))?;
            let branch = clean_optional_text(request.head_branch).ok_or_else(|| ApiError::bad_request("observed GitOps outcome requires head_branch"))?;
            let commit = clean_optional_text(request.head_commit_sha).ok_or_else(|| ApiError::bad_request("observed GitOps outcome requires head_commit_sha"))?;
            if !matches!(state_value.as_str(), "open" | "closed") || !is_git_sha(&commit)
                || expected.get("head_branch").and_then(Value::as_str) != Some(branch.as_str())
                || expected.get("source_commit_sha").and_then(Value::as_str) != Some(commit.as_str()) {
                return Err(ApiError::conflict("GitOps observation does not match the delivered branch commit"));
            }
            let merge = clean_optional_text(request.merge_commit_sha);
            if merged && (state_value != "closed" || !merge.as_deref().is_some_and(is_git_sha)) {
                return Err(ApiError::bad_request("merged GitOps outcome has invalid merge provenance"));
            }
            if !merged && merge.is_some() { return Err(ApiError::bad_request("unmerged GitOps outcome must not include merge_commit_sha")); }
            let observation = state.store.create_artifact(CreateArtifact { id:format!("art_{}_gitops_delivery_pr_observation",unique_suffix()),session_id:change_set.session_id.clone(),run_id:Some(change_set.run_id.clone()),kind:"gitops_delivery_pr_observation".to_string(),label:format!("GitOps PR observation for {}",change_set.id),mime_type:Some("application/json".to_string()),path:None,content_text:None,content_json:Some(json!({"execution_id":request.execution_id,"status":"observed","gitops_change_set_id":change_set.id,"gitops_delivery_plan_artifact_id":plan.id,"pull_request_state":state_value,"merged":merged,"head_branch":branch,"head_commit_sha":commit,"merge_commit_sha":merge})) }).await?;
            if let Some(merge_sha) = merge { state.store.create_artifact(CreateArtifact { id:format!("art_{}_gitops_delivery_merge",unique_suffix()),session_id:change_set.session_id.clone(),run_id:Some(change_set.run_id.clone()),kind:"gitops_delivery_merge".to_string(),label:format!("Immutable GitOps merge for {}",change_set.id),mime_type:Some("application/json".to_string()),path:None,content_text:None,content_json:Some(json!({"execution_id":request.execution_id,"gitops_change_set_id":change_set.id,"gitops_delivery_plan_artifact_id":plan.id,"pull_request_url":expected.get("pull_request_url"),"pull_request_number":expected.get("pull_request_number"),"head_commit_sha":commit,"merge_commit_sha":merge_sha})) }).await?; }
            observation.into()
        }
        "failed" => state.store.create_artifact(CreateArtifact { id:format!("art_{}_gitops_delivery_pr_observation",unique_suffix()),session_id:change_set.session_id.clone(),run_id:Some(change_set.run_id.clone()),kind:"gitops_delivery_pr_observation".to_string(),label:format!("Failed GitOps PR observation for {}",change_set.id),mime_type:Some("application/json".to_string()),path:None,content_text:None,content_json:Some(json!({"execution_id":request.execution_id,"status":"failed","gitops_change_set_id":change_set.id,"gitops_delivery_plan_artifact_id":plan.id,"error_code":clean_optional_text(request.error_code).unwrap_or_else(|| "gitops_observer_failed".to_string())})) }).await?.into(),
        _ => return Err(ApiError::bad_request("GitOps observation outcome status must be observed or failed")),
    };
    append_gitops_change_set_audit_event(&state.store,&change_set,&format!("gitops_change_set.delivery_observation_{}",request.status),Some("agent:git-observer".to_string()),None,json!({"execution_id":request.execution_id,"gitops_delivery_plan_artifact_id":plan.id,"observation_artifact_id":artifact.id})).await?;
    Ok(Json(artifact))
}

pub(in crate::app) async fn current_gitops_delivery_observation(
    state: &AppState,
    gitops_change_set_id: &str,
    execution_id: &str,
) -> Result<(StoredGitOpsChangeSet, StoredArtifact, StoredArtifact), ApiError> {
    let change_set = state
        .store
        .get_gitops_change_set(gitops_change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("gitops_change_set", gitops_change_set_id))?;
    let artifacts = state.store.list_artifacts(&change_set.run_id).await?;
    let execution = artifacts
        .iter()
        .find(|artifact| {
            artifact.kind == "gitops_delivery_observation_execution"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                })
        })
        .cloned()
        .ok_or_else(|| ApiError::conflict("GitOps observation execution is not current"))?;
    let plan_id = execution
        .content_json
        .as_ref()
        .and_then(|content| content.get("gitops_delivery_plan_artifact_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("GitOps observation execution has no plan provenance"))?;
    let plan = artifacts
        .into_iter()
        .find(|artifact| {
            artifact.id == plan_id && gitops_delivery_plan_matches_change_set(artifact, &change_set)
        })
        .ok_or_else(|| {
            ApiError::conflict("GitOps observation execution plan is no longer current")
        })?;
    Ok((change_set, plan, execution))
}

pub(in crate::app) async fn current_gitops_delivery_execution(
    state: &AppState,
    gitops_change_set_id: &str,
    execution_id: &str,
) -> Result<(StoredGitOpsChangeSet, StoredArtifact, StoredArtifact), ApiError> {
    let change_set = state
        .store
        .get_gitops_change_set(gitops_change_set_id)
        .await?
        .ok_or_else(|| ApiError::not_found("gitops_change_set", gitops_change_set_id))?;
    let artifacts = state.store.list_artifacts(&change_set.run_id).await?;
    let execution = artifacts
        .iter()
        .find(|artifact| {
            artifact.kind == "gitops_delivery_execution"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                        && content.get("gitops_change_set_id").and_then(Value::as_str)
                            == Some(change_set.id.as_str())
                })
        })
        .cloned()
        .ok_or_else(|| ApiError::conflict("GitOps delivery execution is not current"))?;
    let plan_id = execution
        .content_json
        .as_ref()
        .and_then(|content| content.get("gitops_delivery_plan_artifact_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::conflict("GitOps delivery execution has no plan provenance"))?;
    let plan = artifacts
        .into_iter()
        .find(|artifact| {
            artifact.id == plan_id && gitops_delivery_plan_matches_change_set(artifact, &change_set)
        })
        .ok_or_else(|| ApiError::conflict("GitOps delivery execution plan is no longer current"))?;
    Ok((change_set, plan, execution))
}

#[derive(Debug, Clone)]
pub(in crate::app) struct GitOpsDeliveryPlanSource {
    repository: String,
    base_ref: String,
    base_commit: String,
    head_branch: String,
    kustomization_path: String,
    image_name: String,
    image_ref: String,
}

pub(in crate::app) fn gitops_delivery_plan_source(
    plan: &StoredArtifact,
    change_set: &StoredGitOpsChangeSet,
) -> Result<GitOpsDeliveryPlanSource, ApiError> {
    let content = plan
        .content_json
        .as_ref()
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("GitOps delivery plan has no structured content"))?;
    if content.get("operation").and_then(Value::as_str) != Some("branch_and_pull_request") {
        return Err(ApiError::conflict(
            "GitOps delivery plan does not describe a branch-and-pull-request operation",
        ));
    }
    let source = content
        .get("source")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("GitOps delivery plan has no source provenance"))?;
    let update = content
        .get("update")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::conflict("GitOps delivery plan has no update operation"))?;
    let result = GitOpsDeliveryPlanSource {
        repository: required_json_string(source, "repository", "GitOps delivery plan source")?,
        base_ref: required_json_string(source, "base_ref", "GitOps delivery plan source")?,
        base_commit: required_json_string(source, "base_commit", "GitOps delivery plan source")?,
        head_branch: required_json_string(source, "head_branch", "GitOps delivery plan source")?,
        kustomization_path: required_json_string(
            update,
            "kustomization_path",
            "GitOps delivery plan update",
        )?,
        image_name: required_json_string(update, "image_name", "GitOps delivery plan update")?,
        image_ref: required_json_string(update, "new_image", "GitOps delivery plan update")?,
    };
    if result.repository != change_set.gitops_repo
        || result.base_ref != change_set.gitops_ref
        || result.head_branch != change_set.head_branch
        || result.kustomization_path != change_set.kustomization_path
        || result.image_name != change_set.image_name
        || result.image_ref != change_set.image_ref
        || !is_git_sha(&result.base_commit)
        || !safe_relative_gitops_path(&result.kustomization_path)
        || !result.image_ref.contains("@sha256:")
    {
        return Err(ApiError::conflict(
            "GitOps delivery plan no longer matches the immutable ChangeSet target",
        ));
    }
    Ok(result)
}

pub(in crate::app) fn gitops_delivery_artifact_matches_plan(
    artifact: &StoredArtifact,
    kind: &str,
    plan_id: &str,
) -> bool {
    artifact.kind == kind
        && artifact.content_json.as_ref().is_some_and(|content| {
            content
                .get("gitops_delivery_plan_artifact_id")
                .and_then(Value::as_str)
                == Some(plan_id)
        })
}

pub(in crate::app) async fn persist_gitops_delivery_result(
    store: &SqliteStore,
    change_set: &StoredGitOpsChangeSet,
    plan_id: &str,
    execution_id: &str,
    status: &str,
    details: Value,
) -> Result<ArtifactResponse, ApiError> {
    Ok(store
        .create_artifact(CreateArtifact {
            id: format!("art_{}_gitops_delivery_result", unique_suffix()),
            session_id: change_set.session_id.clone(),
            run_id: Some(change_set.run_id.clone()),
            kind: "gitops_delivery_result".to_string(),
            label: format!("GitOps delivery {} for {}", status, change_set.id),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "execution_id": execution_id,
                "status": status,
                "gitops_change_set_id": change_set.id,
                "gitops_delivery_plan_artifact_id": plan_id,
                "details": details,
            })),
        })
        .await?
        .into())
}

pub(in crate::app) async fn matching_gitops_delivery_grant(
    store: &SqliteStore,
    subject: &str,
    change_set: &StoredGitOpsChangeSet,
    work_item: &StoredWorkItem,
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
        let has_all_actions = GITOPS_DELIVERY_ACTIONS
            .iter()
            .all(|action| scope.actions.iter().any(|allowed| allowed == action));
        let matches = grant.subject == subject
            && policy.policy_mode == PolicyMode::SupervisedAutonomy
            && scope.environment.as_deref() == Some(work_item.target_environment.as_str())
            && scope.capability_kinds == vec![CapabilityKind::Git]
            && scope.actions.len() == GITOPS_DELIVERY_ACTIONS.len()
            && has_all_actions
            && scope
                .max_risk
                .is_some_and(|risk| risk_rank(risk) >= risk_rank(RiskLevel::High))
            && scope.repos == vec![change_set.gitops_repo.clone()]
            && scope.branches == vec![change_set.head_branch.clone()]
            && scope.work_plan_ids == vec![change_set.work_plan_id.clone()]
            && scope.gitops_change_set_ids == vec![change_set.id.clone()]
            && scope.gitops_delivery_plan_artifact_ids == vec![plan_artifact_id.to_string()]
            && scope.production_impacting == Some(work_item.production_impacting)
            && work_item.gitops_repo.as_deref() == Some(change_set.gitops_repo.as_str())
            && work_item.gitops_ref.as_deref() == Some(change_set.gitops_ref.as_str());
        if matches {
            return Ok(Some(grant));
        }
    }
    Ok(None)
}

use super::*;
use axum::routing::{get, post};
use axum::Router;

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/releases", get(list_releases))
        .route(
            "/api/releases/from-deployment-intent",
            post(create_release_from_deployment_intent),
        )
        .route("/api/releases/:release_id", get(get_release))
        .route(
            "/api/releases/:release_id/transition",
            post(transition_release),
        )
        .route(
            "/api/releases/:release_id/evidence",
            post(attach_release_evidence),
        )
        .route("/api/releases/:release_id/verify", post(verify_release))
        .route("/api/registry-evidence", get(list_registry_evidence))
        .route(
            "/api/registry-evidence/from-release",
            post(create_registry_evidence_from_release),
        )
        .route(
            "/api/registry-evidence/from-registry-inspection",
            post(create_registry_evidence_from_registry_inspection),
        )
        .route(
            "/api/registry-evidence/:evidence_id",
            get(get_registry_evidence),
        )
        .route(
            "/api/registry-evidence/:evidence_id/transition",
            post(transition_registry_evidence),
        )
}

#[derive(Debug, Default, serde::Deserialize)]
pub(in crate::app) struct ListReleasesQuery {
    pub(in crate::app) deployment_intent_id: Option<String>,
    pub(in crate::app) pipeline_intent_id: Option<String>,
    pub(in crate::app) change_set_id: Option<String>,
    pub(in crate::app) work_plan_id: Option<String>,
    pub(in crate::app) remediation_plan_id: Option<String>,
    pub(in crate::app) incident_id: Option<String>,
    pub(in crate::app) run_id: Option<String>,
    pub(in crate::app) status: Option<String>,
    pub(in crate::app) release_kind: Option<String>,
    pub(in crate::app) risk_level: Option<String>,
    pub(in crate::app) target_environment: Option<String>,
    pub(in crate::app) target_namespace: Option<String>,
    pub(in crate::app) argo_application: Option<String>,
    pub(in crate::app) version: Option<String>,
    pub(in crate::app) commit_sha: Option<String>,
    pub(in crate::app) image_digest: Option<String>,
    pub(in crate::app) created_after_ms: Option<i64>,
    pub(in crate::app) created_before_ms: Option<i64>,
    pub(in crate::app) limit: Option<u32>,
    pub(in crate::app) offset: Option<u32>,
}

#[derive(Debug, Default, serde::Deserialize)]
pub(in crate::app) struct ListRegistryEvidenceQuery {
    pub(in crate::app) release_id: Option<String>,
    pub(in crate::app) deployment_intent_id: Option<String>,
    pub(in crate::app) pipeline_intent_id: Option<String>,
    pub(in crate::app) change_set_id: Option<String>,
    pub(in crate::app) work_plan_id: Option<String>,
    pub(in crate::app) remediation_plan_id: Option<String>,
    pub(in crate::app) incident_id: Option<String>,
    pub(in crate::app) run_id: Option<String>,
    pub(in crate::app) status: Option<String>,
    pub(in crate::app) risk_level: Option<String>,
    pub(in crate::app) registry: Option<String>,
    pub(in crate::app) repository: Option<String>,
    pub(in crate::app) image_ref: Option<String>,
    pub(in crate::app) image_digest: Option<String>,
    pub(in crate::app) tag: Option<String>,
    pub(in crate::app) source: Option<String>,
    pub(in crate::app) verification_status: Option<String>,
    pub(in crate::app) created_after_ms: Option<i64>,
    pub(in crate::app) created_before_ms: Option<i64>,
    pub(in crate::app) limit: Option<u32>,
    pub(in crate::app) offset: Option<u32>,
}

pub(in crate::app) async fn list_releases(
    State(state): State<AppState>,
    Query(query): Query<ListReleasesQuery>,
) -> Result<Json<ReleasesResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let releases = state
        .store
        .list_releases(ReleaseListFilter {
            deployment_intent_id: clean_optional_text(query.deployment_intent_id),
            pipeline_intent_id: clean_optional_text(query.pipeline_intent_id),
            change_set_id: clean_optional_text(query.change_set_id),
            work_plan_id: clean_optional_text(query.work_plan_id),
            remediation_plan_id: clean_optional_text(query.remediation_plan_id),
            incident_id: clean_optional_text(query.incident_id),
            run_id: clean_optional_text(query.run_id).map(RunId::new),
            status: clean_optional_text(query.status),
            release_kind: clean_optional_text(query.release_kind),
            risk_level: clean_optional_text(query.risk_level),
            target_environment: clean_optional_text(query.target_environment),
            target_namespace: clean_optional_text(query.target_namespace),
            argo_application: clean_optional_text(query.argo_application),
            version: clean_optional_text(query.version),
            commit_sha: clean_optional_text(query.commit_sha),
            image_digest: clean_optional_text(query.image_digest),
            created_after_ms: query.created_after_ms,
            created_before_ms: query.created_before_ms,
            limit,
            offset,
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    let count = releases.len();

    Ok(Json(ReleasesResponse {
        releases,
        count,
        limit,
        offset,
    }))
}

pub(in crate::app) async fn get_release(
    State(state): State<AppState>,
    Path(release_id): Path<String>,
) -> Result<Json<ReleaseResponse>, ApiError> {
    let release = state
        .store
        .get_release(&release_id)
        .await?
        .ok_or_else(|| ApiError::not_found("release", &release_id))?;

    Ok(Json(release.into()))
}

pub(in crate::app) async fn create_release_from_deployment_intent(
    State(state): State<AppState>,
    Json(request): Json<CreateReleaseFromDeploymentIntentRequest>,
) -> Result<Json<CreateReleaseResponse>, ApiError> {
    let deployment_intent_id = clean_optional_text(Some(request.deployment_intent_id))
        .ok_or_else(|| ApiError::bad_request("deployment_intent_id is required"))?;
    let deployment_intent = state
        .store
        .get_deployment_intent(&deployment_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("deployment_intent", &deployment_intent_id))?;
    ensure_approved_for_trusted_envelope(
        "deployment_intent",
        &deployment_intent.id,
        &deployment_intent.status,
    )?;
    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let release_kind =
        clean_optional_text(request.release_kind).unwrap_or_else(|| "gitops_release".to_string());
    let version = clean_optional_text(request.version);
    let commit_sha = clean_optional_text(request.commit_sha);
    let build_output =
        pipeline_build_output_for_deployment_intent(&state, &deployment_intent).await?;
    let requested_image_digest = clean_optional_text(request.image_digest);
    if let (Some(output), Some(requested)) = (&build_output, &requested_image_digest) {
        if requested != &output.image_digest {
            return Err(ApiError::conflict(format!(
                "Release image_digest must match verified Pipeline build output {}",
                output.image_digest
            )));
        }
    }
    let image_digest = requested_image_digest.or_else(|| {
        build_output
            .as_ref()
            .map(|output| output.image_digest.clone())
    });
    let rollback_ref = clean_optional_text(request.rollback_ref);
    let release_json = release_json(
        &deployment_intent,
        ReleaseJsonInput {
            release_kind: &release_kind,
            version: version.as_deref(),
            commit_sha: commit_sha.as_deref(),
            image_digest: image_digest.as_deref(),
            rollback_ref: rollback_ref.as_deref(),
            release_json: request.release_json,
            build_output: build_output.as_ref(),
        },
    )?;
    if let Some(existing) = state
        .store
        .get_release_by_deployment_intent(&deployment_intent_id)
        .await?
    {
        if existing.status == "stale" {
            let release = state
                .store
                .revise_release_draft(
                    &existing.id,
                    UpdateReleaseDraft {
                        title: clean_optional_text(request.title)
                            .unwrap_or_else(|| format!("Release: {}", deployment_intent.title)),
                        summary: clean_optional_text(request.summary).unwrap_or_else(|| {
                            "Propose release after approved deployment intent".to_string()
                        }),
                        risk_level: clean_optional_text(request.risk_level)
                            .unwrap_or_else(|| deployment_intent.risk_level.clone()),
                        release_kind,
                        target_environment: deployment_intent.target_environment,
                        target_namespace: deployment_intent.target_namespace,
                        argo_application: deployment_intent.argo_application,
                        version,
                        commit_sha,
                        image_digest,
                        rollback_ref,
                        release_json,
                        actor: actor.clone(),
                        reason: reason.clone(),
                    },
                )
                .await?;
            append_release_audit_event(
                &state.store,
                &release,
                "release.reproposed",
                actor,
                reason,
                json!({
                    "source": "deployment_intent",
                    "deployment_intent_id": release.deployment_intent_id,
                    "previous_status": existing.status,
                    "execution_enabled": false,
                    "deployment_evidence_status": release
                        .release_json
                        .pointer("/deployment_evidence/status"),
                    "deployment_release_ready": release
                        .release_json
                        .pointer("/deployment_evidence/release_ready"),
                    "pipeline_build_output_artifact_id": release
                        .release_json
                        .pointer("/build_output/artifact_id"),
                }),
            )
            .await?;

            return Ok(Json(CreateReleaseResponse {
                release: release.into(),
                created: false,
            }));
        }

        return Ok(Json(CreateReleaseResponse {
            release: existing.into(),
            created: false,
        }));
    }
    let release = state
        .store
        .create_release(CreateRelease {
            id: format!("rel_{}", unique_suffix()),
            deployment_intent_id: deployment_intent.id.clone(),
            pipeline_intent_id: deployment_intent.pipeline_intent_id.clone(),
            change_set_id: deployment_intent.change_set_id.clone(),
            work_plan_id: deployment_intent.work_plan_id.clone(),
            remediation_plan_id: deployment_intent.remediation_plan_id.clone(),
            incident_id: deployment_intent.incident_id.clone(),
            session_id: deployment_intent.session_id.clone(),
            run_id: deployment_intent.run_id.clone(),
            status: "proposed".to_string(),
            title: clean_optional_text(request.title)
                .unwrap_or_else(|| format!("Release: {}", deployment_intent.title)),
            summary: clean_optional_text(request.summary)
                .unwrap_or_else(|| "Propose release after approved deployment intent".to_string()),
            risk_level: clean_optional_text(request.risk_level)
                .unwrap_or(deployment_intent.risk_level),
            release_kind,
            target_environment: deployment_intent.target_environment,
            target_namespace: deployment_intent.target_namespace,
            argo_application: deployment_intent.argo_application,
            version,
            commit_sha,
            image_digest,
            rollback_ref,
            release_json,
        })
        .await?;
    append_release_audit_event(
        &state.store,
        &release,
        "release.proposed",
        actor,
        reason,
        json!({
            "source": "deployment_intent",
            "deployment_intent_id": release.deployment_intent_id,
            "execution_enabled": false,
            "deployment_evidence_status": release
                .release_json
                .pointer("/deployment_evidence/status"),
            "deployment_release_ready": release
                .release_json
                .pointer("/deployment_evidence/release_ready"),
            "pipeline_build_output_artifact_id": release
                .release_json
                .pointer("/build_output/artifact_id"),
        }),
    )
    .await?;

    Ok(Json(CreateReleaseResponse {
        release: release.into(),
        created: true,
    }))
}

pub(in crate::app) async fn transition_release(
    State(state): State<AppState>,
    Path(release_id): Path<String>,
    Json(request): Json<TransitionReleaseRequest>,
) -> Result<Json<TransitionReleaseResponse>, ApiError> {
    let current = state
        .store
        .get_release(&release_id)
        .await?
        .ok_or_else(|| ApiError::not_found("release", &release_id))?;
    let target = clean_optional_text(Some(request.target_status))
        .ok_or_else(|| ApiError::bad_request("target_status is required"))?;
    validate_release_transition(&current.status, &target)?;
    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let release = state
        .store
        .update_release_status(&release_id, &target, actor.clone(), reason.clone())
        .await?;
    append_release_audit_event(
        &state.store,
        &release,
        &format!("release.{target}"),
        actor,
        reason,
        json!({
            "previous_status": current.status,
            "status": release.status,
        }),
    )
    .await?;

    Ok(Json(TransitionReleaseResponse {
        release: release.into(),
    }))
}

pub(in crate::app) async fn attach_release_evidence(
    State(state): State<AppState>,
    Path(release_id): Path<String>,
    Json(request): Json<AttachReleaseEvidenceRequest>,
) -> Result<Json<AttachReleaseEvidenceResponse>, ApiError> {
    let current = state
        .store
        .get_release(&release_id)
        .await?
        .ok_or_else(|| ApiError::not_found("release", &release_id))?;
    if matches!(current.status.as_str(), "stale" | "rejected") {
        return Err(ApiError::conflict(format!(
            "cannot attach evidence to {} release {release_id}",
            current.status
        )));
    }

    let observation_id = clean_optional_text(Some(request.observation_id))
        .ok_or_else(|| ApiError::bad_request("observation_id is required"))?;
    let observation = state
        .store
        .get_observation(&observation_id)
        .await?
        .ok_or_else(|| ApiError::not_found("observation", &observation_id))?;
    validate_release_observation(&observation)?;

    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let release_json = release_json_with_observability_evidence(&current, &observation);
    let release = state
        .store
        .update_release_evidence(
            &release_id,
            UpdateReleaseEvidence {
                release_json,
                actor: actor.clone(),
                reason: reason.clone(),
            },
        )
        .await?;
    append_release_audit_event(
        &state.store,
        &release,
        "release.evidence_attached",
        actor.clone(),
        reason.clone(),
        json!({
            "observation_id": observation.id,
            "artifact_id": observation.artifact_id,
            "evidence_status": release_observability_evidence_status(&release),
            "resource": {
                "source": observation.source,
                "kind": observation.kind,
                "namespace": observation.resource_namespace,
                "resource_kind": observation.resource_kind,
                "name": observation.resource_name,
            },
        }),
    )
    .await?;
    let incident = create_release_observability_incident(
        &state.store,
        &release,
        &observation,
        actor.clone(),
        reason.clone(),
    )
    .await?;
    let remediation_plan = match incident.as_ref() {
        Some(incident) => {
            create_release_observability_remediation_plan(
                &state.store,
                incident,
                actor.clone(),
                reason.clone(),
            )
            .await?
        }
        None => None,
    };

    Ok(Json(AttachReleaseEvidenceResponse {
        release: release.into(),
        observation: observation.into(),
        incident: incident.map(Into::into),
        remediation_plan: remediation_plan.map(Into::into),
    }))
}

/// Verifies the state that an Argo sync deliberately does not assert: the
/// Application must be synced and healthy, and the declared Deployment must
/// report a healthy rollout. This is a typed read-only path; it has no Argo
/// mutation or shell escape hatch.
pub(in crate::app) async fn verify_release(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(release_id): Path<String>,
    Json(request): Json<VerifyReleaseRequest>,
) -> Result<Json<VerifyReleaseResponse>, ApiError> {
    let current = state
        .store
        .get_release(&release_id)
        .await?
        .ok_or_else(|| ApiError::not_found("release", &release_id))?;
    if !matches!(current.status.as_str(), "approved" | "completed") {
        return Err(ApiError::conflict(format!(
            "post-sync verification requires an approved or completed Release; {} is {}",
            current.id, current.status
        )));
    }
    let intent = state
        .store
        .get_deployment_intent(&current.deployment_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("deployment_intent", &current.deployment_intent_id))?;
    let work_plan = state
        .store
        .get_work_plan(&intent.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &intent.work_plan_id))?;
    let work_item_id = work_plan.work_item_id.as_deref().ok_or_else(|| {
        ApiError::conflict("post-sync verification requires a WorkItem-backed delivery chain")
    })?;
    let work_item = state
        .store
        .get_work_item(work_item_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?;
    let target = deployment_target(&intent)?;
    ensure_supported_deployment_target(&work_item, &target)?;
    let run_id = intent.run_id.clone().ok_or_else(|| {
        ApiError::conflict("post-sync verification requires DeploymentIntent coding run provenance")
    })?;
    let artifacts = state.store.list_artifacts(&run_id).await?;
    let (sync_execution, sync_result) = completed_argo_sync_result(&artifacts, &intent).ok_or_else(|| {
        ApiError::conflict(
            "post-sync verification requires the current Argo sync execution to have a completed result",
        )
    })?;
    let verification_contract =
        deployment_contract_for_sync_execution(&state.store, &target, sync_execution).await?;
    let prometheus_inventory_required = verification_contract
        .as_ref()
        .map(|contract| deployment_contract_spec(&contract.contract_json))
        .transpose()?
        .map(|contract| {
            contract.post_sync_verification.prometheus_inventory
                == VerificationRequirement::Required
        })
        .unwrap_or(false);

    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor));
    let reason = clean_optional_text(request.reason);
    if request.complete && reason.is_none() {
        return Err(ApiError::bad_request(
            "release completion requires an explicit verification reason",
        ));
    }

    let argo_action = AgentAction::ArgoGetApp {
        id: "release.verify_argo_application".into(),
        reason: format!("verify Release {} post-sync Argo state", current.id),
        app: target.application.clone(),
    };
    let argo_response = execute_direct_capability(&state, argo_action, request.timeout_ms).await?;
    let argo_observation_id = successful_direct_observation_id(&argo_response, "Argo Application")?;
    let argo_observation = state
        .store
        .get_observation(&argo_observation_id)
        .await?
        .ok_or_else(|| ApiError::internal("Argo verification observation was not persisted"))?;

    let workload_action =
        release_workload_verification_action(&intent, verification_contract.as_ref(), &current.id)?;
    let workload_response =
        execute_direct_capability(&state, workload_action, request.timeout_ms).await?;
    let workload_observation_id =
        successful_direct_observation_id(&workload_response, "Deployment rollout")?;
    let workload_observation = state
        .store
        .get_observation(&workload_observation_id)
        .await?
        .ok_or_else(|| ApiError::internal("workload verification observation was not persisted"))?;

    let argo_healthy = argo_observation
        .data_json
        .pointer("/analysis/sync_status")
        .and_then(Value::as_str)
        == Some("Synced")
        && argo_observation
            .data_json
            .pointer("/analysis/health_status")
            .and_then(Value::as_str)
            == Some("Healthy");
    let rollout_healthy = workload_observation
        .data_json
        .pointer("/analysis/status")
        .and_then(Value::as_str)
        == Some("healthy");
    let runtime_image_check = if work_item.production_impacting {
        let expected_digest = pipeline_build_output_for_deployment_intent(&state, &intent)
            .await?
            .map(|output| output.image_digest)
            .ok_or_else(|| {
                ApiError::conflict(
                    "production verification requires the verified Pipeline build digest",
                )
            })?;
        let response = execute_direct_capability(
            &state,
            AgentAction::KubernetesGet {
                id: "release.verify_running_image_ids".into(),
                reason: format!("verify Release {} running Pod imageIDs", current.id),
                resource: "pods".to_string(),
                namespace: Some(PROTECTED_NAMESPACE.to_string()),
                name: None,
                all_namespaces: false,
                label_selector: Some("app=yfinance-wrapper".to_string()),
            },
            request.timeout_ms,
        )
        .await?;
        let image_ids = response
            .result
            .as_ref()
            .and_then(|result| result.content.pointer("/output/items"))
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
        execution_check(
            "running_image_digest",
            !image_ids.is_empty()
                && image_ids
                    .iter()
                    .all(|image_id| image_id.ends_with(&expected_digest)),
            format!(
                "{} running container imageID(s) checked against {}",
                image_ids.len(),
                expected_digest
            ),
        )
    } else {
        execution_check(
            "running_image_digest",
            true,
            "Exact Pod imageID verification is not required for legacy dev delivery",
        )
    };
    let service_health_check = if work_item.production_impacting {
        let outcome = state
            .worker
            .verify_capability("yfinance_healthz", None)
            .await;
        execution_check(
            "service_healthz",
            outcome.as_ref().is_ok_and(|outcome| outcome.available),
            if outcome.as_ref().is_ok_and(|outcome| outcome.available) {
                "Exact apps-prod/yfinance-wrapper Service /healthz check passed"
            } else {
                "Exact apps-prod/yfinance-wrapper Service /healthz check failed"
            },
        )
    } else {
        execution_check(
            "service_healthz",
            true,
            "Bounded Service /healthz verification is not required for legacy dev delivery",
        )
    };
    let (observability_observation, observability_check) = if prometheus_inventory_required {
        verify_required_prometheus_inventory(&state, request.timeout_ms).await?
    } else {
        (
            None,
            execution_check(
                "prometheus_inventory",
                true,
                "Prometheus inventory verification is disabled by the active DeploymentContract",
            ),
        )
    };
    let observability_healthy = observability_check
        .get("passed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let running_image_healthy = runtime_image_check
        .get("passed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let service_healthy = service_health_check
        .get("passed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let verified = argo_healthy
        && rollout_healthy
        && running_image_healthy
        && service_healthy
        && observability_healthy;
    let mut checks = vec![
        execution_check(
            "completed_argo_sync",
            true,
            format!(
                "completed sync result artifact {} is current",
                sync_result.id
            ),
        ),
        execution_check(
            "argo_application_synced_healthy",
            argo_healthy,
            verification_observation_summary(&argo_observation),
        ),
        execution_check(
            "declared_deployment_rollout_healthy",
            rollout_healthy,
            verification_observation_summary(&workload_observation),
        ),
    ];
    checks.push(runtime_image_check);
    checks.push(service_health_check);
    checks.push(observability_check);

    let release_json = release_json_with_post_sync_verification(
        &current,
        PostSyncVerificationEvidence {
            sync_result,
            argo_observation: &argo_observation,
            workload_observation: &workload_observation,
            deployment_contract: verification_contract.as_ref(),
            observability_observation: observability_observation.as_ref(),
            prometheus_inventory_required,
            verified,
            checks: &checks,
        },
    );
    let mut release = state
        .store
        .update_release_evidence(
            &current.id,
            UpdateReleaseEvidence {
                release_json,
                actor: actor.clone(),
                reason: reason.clone(),
            },
        )
        .await?;
    append_release_audit_event(
        &state.store,
        &release,
        if verified {
            "release.post_sync_verified"
        } else {
            "release.post_sync_attention_required"
        },
        actor.clone(),
        reason.clone(),
        json!({
            "argo_sync_result_artifact_id": sync_result.id,
            "argo_observation_id": argo_observation.id,
            "workload_observation_id": workload_observation.id,
            "deployment_contract_id": verification_contract.as_ref().map(|contract| &contract.id),
            "observability_observation_id": observability_observation.as_ref().map(|observation| &observation.id),
            "checks": checks,
        }),
    )
    .await?;

    let mut completed = false;
    if request.complete && verified && release.status == "approved" {
        release = state
            .store
            .update_release_status(&release.id, "completed", actor.clone(), reason.clone())
            .await?;
        append_release_audit_event(
            &state.store,
            &release,
            "release.completed",
            actor,
            reason,
            json!({
                "verification": "post_sync",
                "argo_observation_id": argo_observation.id,
                "workload_observation_id": workload_observation.id,
                "observability_observation_id": observability_observation.as_ref().map(|observation| &observation.id),
            }),
        )
        .await?;
        completed = true;
    }

    Ok(Json(VerifyReleaseResponse {
        status: if verified {
            "verified".to_string()
        } else {
            "attention_required".to_string()
        },
        verified,
        completed,
        release: release.into(),
        argo_observation: argo_observation.into(),
        workload_observation: workload_observation.into(),
        observability_observation: observability_observation.map(Into::into),
        checks,
    }))
}

pub(in crate::app) async fn deployment_contract_for_sync_execution(
    store: &SqliteStore,
    target: &DeploymentTarget,
    execution: &StoredArtifact,
) -> Result<Option<StoredDeploymentContract>, ApiError> {
    let contract_id = execution
        .content_json
        .as_ref()
        .and_then(|content| content.get("deployment_contract_id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let Some(contract_id) = contract_id else {
        // Legacy receipt: no contract-backed runtime criterion is available to
        // adopt after a sync. It may still use the original rollout checks.
        return Ok(None);
    };
    let contract = store
        .get_deployment_contract(&contract_id)
        .await?
        .ok_or_else(|| {
            ApiError::conflict("Argo sync execution references a missing DeploymentContract")
        })?;
    if contract.status != "active" {
        return Err(ApiError::conflict(
            "Argo sync execution DeploymentContract is no longer active; run a new reviewed sync",
        ));
    }
    if contract.target_environment != target.environment
        || contract.target_namespace != target.namespace
        || contract.argo_application != target.application
    {
        return Err(ApiError::conflict(
            "Argo sync execution DeploymentContract does not match the Release target",
        ));
    }
    let spec = deployment_contract_spec(&contract.contract_json)?;
    validate_deployment_contract_spec(&spec)?;
    if contract.target_environment == PROTECTED_ENVIRONMENT {
        validate_protected_production_deployment_contract(&spec)?;
    }
    Ok(Some(contract))
}

pub(in crate::app) async fn verify_required_prometheus_inventory(
    state: &AppState,
    timeout_ms: Option<u64>,
) -> Result<(Option<StoredObservation>, Value), ApiError> {
    let response = execute_direct_capability(
        state,
        AgentAction::PrometheusInventory {
            id: "release.verify_prometheus_inventory".into(),
            reason: "verify Release post-sync Prometheus inventory".to_string(),
        },
        timeout_ms,
    )
    .await?;
    if response.status != "ok" || !response.executed {
        return Ok((
            None,
            execution_check(
                "prometheus_inventory",
                false,
                format!(
                    "required Prometheus inventory was unavailable: {}",
                    response.error.unwrap_or(response.status)
                ),
            ),
        ));
    }
    let Some(observation_id) = response.observation_id else {
        return Ok((
            None,
            execution_check(
                "prometheus_inventory",
                false,
                "required Prometheus inventory did not persist an observation",
            ),
        ));
    };
    let observation = state
        .store
        .get_observation(&observation_id)
        .await?
        .ok_or_else(|| {
            ApiError::internal("Prometheus verification observation was not persisted")
        })?;
    let healthy = release_prometheus_inventory_collected(&observation.data_json);
    Ok((
        Some(observation.clone()),
        execution_check(
            "prometheus_inventory",
            healthy,
            release_prometheus_inventory_summary(&observation.data_json),
        ),
    ))
}

pub(in crate::app) fn release_prometheus_inventory_collected(data: &Value) -> bool {
    ["targets", "rules", "alerts"].into_iter().all(|section| {
        data.pointer(&format!("/inventory/{section}/status"))
            .and_then(Value::as_str)
            == Some("success")
    })
}

pub(in crate::app) fn release_prometheus_inventory_summary(data: &Value) -> String {
    let unhealthy_targets = data
        .pointer("/inventory/targets/unhealthy_count")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let problem_rules = data
        .pointer("/inventory/rules/problem_rule_count")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let alerts = data
        .pointer("/inventory/alerts/alert_count")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    format!(
        "Prometheus inventory collected; recorded {unhealthy_targets} unhealthy target(s), {problem_rules} problem rule(s), and {alerts} alert(s) as non-workload-scoped evidence"
    )
}

pub(in crate::app) fn successful_direct_observation_id(
    response: &ExecuteCapabilityResponse,
    description: &str,
) -> Result<String, ApiError> {
    if response.status != "ok" || !response.executed {
        return Err(ApiError::conflict(format!(
            "{description} verification failed: {}",
            response
                .error
                .as_deref()
                .unwrap_or(response.status.as_str())
        )));
    }
    response.observation_id.clone().ok_or_else(|| {
        ApiError::internal(format!(
            "{description} verification did not produce an observation"
        ))
    })
}

pub(in crate::app) fn release_workload_verification_action(
    intent: &StoredDeploymentIntent,
    deployment_contract: Option<&StoredDeploymentContract>,
    release_id: &str,
) -> Result<AgentAction, ApiError> {
    let (resource_kind, namespace, name) = if let Some(contract) = deployment_contract {
        let spec = deployment_contract_spec(&contract.contract_json)?;
        match (spec.workload_kind, spec.workload_name) {
            (Some(kind), Some(name)) => (kind, contract.target_namespace.clone(), name),
            (None, None) => release_intent_workload_target(intent)?,
            _ => {
                return Err(ApiError::conflict(
                    "DeploymentContract post-sync verification must declare both workload_kind and workload_name",
                ))
            }
        }
    } else {
        release_intent_workload_target(intent)?
    };
    let resource_kind = resource_kind.trim().to_ascii_lowercase();
    if !matches!(resource_kind.as_str(), "deployment" | "deployments") {
        return Err(ApiError::conflict(
            "post-sync verification currently supports only a declared Deployment resource",
        ));
    }
    Ok(AgentAction::KubernetesGet {
        id: "release.verify_deployment".into(),
        reason: format!("verify Release {release_id} declared Deployment rollout"),
        resource: "deployments".to_string(),
        namespace: Some(namespace),
        name: Some(name),
        all_namespaces: false,
        label_selector: None,
    })
}

pub(in crate::app) fn release_intent_workload_target(
    intent: &StoredDeploymentIntent,
) -> Result<(String, String, String), ApiError> {
    let resource_kind = intent.resource_kind.clone().ok_or_else(|| {
        ApiError::conflict(
            "post-sync verification currently supports only a declared Deployment resource",
        )
    })?;
    let namespace = intent.resource_namespace.clone().ok_or_else(|| {
        ApiError::conflict("post-sync verification requires a declared Deployment namespace")
    })?;
    let name = intent.resource_name.clone().ok_or_else(|| {
        ApiError::conflict("post-sync verification requires a declared Deployment name")
    })?;
    Ok((resource_kind, namespace, name))
}

pub(in crate::app) fn completed_argo_sync_result<'a>(
    artifacts: &'a [StoredArtifact],
    intent: &StoredDeploymentIntent,
) -> Option<(&'a StoredArtifact, &'a StoredArtifact)> {
    let execution = artifacts
        .iter()
        .filter(|artifact| {
            artifact.kind == "argo_sync_execution"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("deployment_intent_id").and_then(Value::as_str)
                        == Some(intent.id.as_str())
                })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))?;
    let execution_id = execution
        .content_json
        .as_ref()?
        .get("execution_id")
        .and_then(Value::as_str)?;
    let result = artifacts
        .iter()
        .filter(|artifact| {
            artifact.kind == "argo_sync_result"
                && artifact.content_json.as_ref().is_some_and(|content| {
                    content.get("execution_id").and_then(Value::as_str) == Some(execution_id)
                        && content.get("status").and_then(Value::as_str) == Some("completed")
                })
        })
        .max_by_key(|artifact| (&artifact.created_at, &artifact.id))?;
    Some((execution, result))
}

pub(in crate::app) struct PostSyncVerificationEvidence<'a> {
    sync_result: &'a StoredArtifact,
    argo_observation: &'a StoredObservation,
    workload_observation: &'a StoredObservation,
    deployment_contract: Option<&'a StoredDeploymentContract>,
    observability_observation: Option<&'a StoredObservation>,
    prometheus_inventory_required: bool,
    verified: bool,
    checks: &'a [Value],
}

pub(in crate::app) fn release_json_with_post_sync_verification(
    current: &StoredRelease,
    evidence: PostSyncVerificationEvidence<'_>,
) -> Value {
    let mut release_json = current.release_json.clone();
    let verification = json!({
        "status": if evidence.verified { "verified" } else { "attention_required" },
        "runtime_ready": evidence.verified,
        "review_required": !evidence.verified,
        "argo_sync_result_artifact_id": evidence.sync_result.id,
        "argo_observation_id": evidence.argo_observation.id,
        "workload_observation_id": evidence.workload_observation.id,
        "deployment_contract_id": evidence.deployment_contract.map(|contract| contract.id.clone()),
        "deployment_contract_version": evidence.deployment_contract.map(|contract| contract.version.clone()),
        "observability": {
            "prometheus_inventory": {
                "required": evidence.prometheus_inventory_required,
                "status": if !evidence.prometheus_inventory_required {
                    "disabled"
                } else if evidence.observability_observation
                    .map(|observation| release_prometheus_inventory_collected(&observation.data_json))
                    .unwrap_or(false)
                {
                    "observed"
                } else {
                    "attention_required"
                },
                "observation_id": evidence.observability_observation.map(|observation| observation.id.clone()),
            }
        },
        "checks": evidence.checks,
    });
    if let Some(object) = release_json.as_object_mut() {
        object.insert("post_sync_verification".to_string(), verification);
    }
    release_json
}

pub(in crate::app) fn verification_observation_summary(observation: &StoredObservation) -> String {
    observation.summary.chars().take(256).collect::<String>()
}

pub(in crate::app) fn validate_release_observation(
    observation: &StoredObservation,
) -> Result<(), ApiError> {
    match (observation.source.as_str(), observation.kind.as_str()) {
        ("prometheus", "inventory" | "prometheus_read") => Ok(()),
        ("loki", "log_summary") => Ok(()),
        _ => Err(ApiError::bad_request(
            "release evidence must be a Prometheus inventory/query or Loki log summary observation",
        )),
    }
}

pub(in crate::app) fn release_json_with_observability_evidence(
    current: &StoredRelease,
    observation: &StoredObservation,
) -> Value {
    let mut release_json = current.release_json.clone();
    let evidence = release_observability_evidence_json(observation);
    if let Some(object) = release_json.as_object_mut() {
        let items = object
            .entry("observability_evidence")
            .or_insert_with(|| Value::Array(Vec::new()));
        if let Some(items) = items.as_array_mut() {
            items.retain(|item| {
                item.get("observation_id").and_then(Value::as_str) != Some(observation.id.as_str())
            });
            items.push(evidence);
        } else {
            object.insert("observability_evidence".to_string(), json!([evidence]));
        }
    }
    release_json
}

pub(in crate::app) fn release_observability_evidence_json(
    observation: &StoredObservation,
) -> Value {
    json!({
        "status": release_observability_status(observation),
        "source": "observation",
        "observation_source": observation.source,
        "observation_kind": observation.kind,
        "observation_id": observation.id,
        "artifact_id": observation.artifact_id,
        "runtime_ready": release_observability_status(observation) == "observed",
        "review_required": release_observability_status(observation) != "observed",
        "resource": {
            "namespace": observation.resource_namespace,
            "kind": observation.resource_kind,
            "name": observation.resource_name,
        },
        "summary": release_observability_summary(observation),
    })
}

pub(in crate::app) fn release_observability_status(
    observation: &StoredObservation,
) -> &'static str {
    match (observation.source.as_str(), observation.kind.as_str()) {
        ("prometheus", "inventory") => {
            prometheus_inventory_observability_status(&observation.data_json)
        }
        ("prometheus", "prometheus_read") => {
            prometheus_query_observability_status(&observation.data_json)
        }
        ("loki", "log_summary") => loki_observability_status(&observation.data_json),
        _ => "unknown",
    }
}

pub(in crate::app) fn prometheus_inventory_observability_status(data: &Value) -> &'static str {
    let unhealthy_targets = data
        .pointer("/inventory/targets/unhealthy_count")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let problem_rules = data
        .pointer("/inventory/rules/problem_rule_count")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let alerts = data
        .pointer("/inventory/alerts/alert_count")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    if unhealthy_targets > 0 || problem_rules > 0 || alerts > 0 {
        "attention_required"
    } else if data.get("inventory").is_some() {
        "observed"
    } else {
        "unknown"
    }
}

pub(in crate::app) fn prometheus_query_observability_status(data: &Value) -> &'static str {
    match data.pointer("/response/status").and_then(Value::as_str) {
        Some("success") => "observed",
        Some(_) => "attention_required",
        None => "unknown",
    }
}

pub(in crate::app) fn loki_observability_status(data: &Value) -> &'static str {
    match data.pointer("/response/status").and_then(Value::as_str) {
        Some("success") => "observed",
        Some(_) => "attention_required",
        None => "unknown",
    }
}

pub(in crate::app) fn release_observability_summary(observation: &StoredObservation) -> Value {
    match (observation.source.as_str(), observation.kind.as_str()) {
        ("prometheus", "inventory") => json!({
            "unhealthy_targets": observation.data_json.pointer("/inventory/targets/unhealthy_count"),
            "problem_rules": observation.data_json.pointer("/inventory/rules/problem_rule_count"),
            "alerts": observation.data_json.pointer("/inventory/alerts/alert_count"),
        }),
        ("prometheus", "prometheus_read") => json!({
            "query": observation.data_json.get("query"),
            "status": observation.data_json.pointer("/response/status"),
            "result_count": observation.data_json.pointer("/response/data/result_count"),
        }),
        ("loki", "log_summary") => json!({
            "query": observation.data_json.get("query"),
            "status": observation.data_json.pointer("/response/status"),
            "stream_count": observation.data_json.pointer("/response/data/stream_count"),
            "entry_count": observation.data_json.pointer("/response/data/entry_count"),
        }),
        _ => json!({}),
    }
}

pub(in crate::app) async fn create_release_observability_incident(
    store: &SqliteStore,
    release: &StoredRelease,
    observation: &StoredObservation,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<Option<StoredIncident>, ApiError> {
    if release_observability_status(observation) != "attention_required" {
        return Ok(None);
    }

    let incident_id = release_observability_incident_id(release, observation);
    if let Some(existing) = store.get_incident(&incident_id).await? {
        return Ok(Some(existing));
    }

    let summary = release_observability_incident_summary(observation);
    let incident = store
        .create_incident(CreateIncident {
            id: incident_id,
            observation_id: observation.id.clone(),
            session_id: observation.session_id.clone(),
            run_id: observation.run_id.clone(),
            status: "candidate".to_string(),
            severity: release_observability_incident_severity(observation).to_string(),
            title: format!(
                "Release observability issue: {}",
                release_observability_resource_label(observation)
            ),
            summary: summary.clone(),
            resource_namespace: observation.resource_namespace.clone(),
            resource_kind: observation.resource_kind.clone(),
            resource_name: observation.resource_name.clone(),
            data_json: json!({
                "source": "release_observability_evidence",
                "release_id": release.id,
                "deployment_intent_id": release.deployment_intent_id,
                "pipeline_intent_id": release.pipeline_intent_id,
                "change_set_id": release.change_set_id,
                "work_plan_id": release.work_plan_id,
                "observation_id": observation.id,
                "observation_source": observation.source,
                "observation_kind": observation.kind,
                "evidence_status": "attention_required",
                "summary": release_observability_summary(observation),
            }),
        })
        .await?;
    append_incident_audit_event(
        store,
        &incident,
        "incident.created",
        actor,
        reason.or_else(|| Some("release observability evidence requires review".to_string())),
    )
    .await?;

    Ok(Some(incident))
}

pub(in crate::app) async fn create_release_observability_remediation_plan(
    store: &SqliteStore,
    incident: &StoredIncident,
    actor: Option<String>,
    reason: Option<String>,
) -> Result<Option<StoredRemediationPlan>, ApiError> {
    if incident.status != "candidate" {
        return Ok(None);
    }
    if incident.data_json.get("source").and_then(Value::as_str)
        != Some("release_observability_evidence")
    {
        return Ok(None);
    }

    let plan_id = format!("rplan_{}", incident.id);
    if let Some(existing) = store.get_remediation_plan(&plan_id).await? {
        return Ok(Some(existing));
    }

    let resource = incident_resource_label(incident);
    let plan_json = release_observability_remediation_plan_json(incident, &resource);
    let plan = store
        .create_remediation_plan(CreateRemediationPlan {
            id: plan_id,
            incident_id: incident.id.clone(),
            session_id: incident.session_id.clone(),
            run_id: incident.run_id.clone(),
            status: "draft".to_string(),
            title: format!("Draft remediation for release observability issue: {resource}"),
            summary: "Re-read bounded observability evidence, confirm release health, then require approval before any file, pipeline, or cluster mutation.".to_string(),
            risk_level: incident.severity.clone(),
            requires_approval: true,
            resource_namespace: incident.resource_namespace.clone(),
            resource_kind: incident.resource_kind.clone(),
            resource_name: incident.resource_name.clone(),
            plan_json,
        })
        .await?;
    append_remediation_plan_audit_event(
        store,
        &plan,
        "remediation_plan.created",
        actor,
        reason.or_else(|| Some("release observability incident requires review".to_string())),
    )
    .await?;

    for gate in approval_gates_from_remediation_plan(&plan) {
        let gate = store.create_approval_gate(gate).await?;
        append_approval_gate_audit_event(store, &gate, "approval_gate.created", "created").await?;
    }

    Ok(Some(plan))
}

pub(in crate::app) fn release_observability_remediation_plan_json(
    incident: &StoredIncident,
    resource: &str,
) -> Value {
    json!({
        "mode": "read_only_draft",
        "source": "release_observability_evidence",
        "incident_id": incident.id,
        "resource": {
            "namespace": incident.resource_namespace,
            "kind": incident.resource_kind,
            "name": incident.resource_name,
            "label": resource,
        },
        "evidence": {
            "summary": incident.summary,
            "release_id": incident.data_json.get("release_id"),
            "deployment_intent_id": incident.data_json.get("deployment_intent_id"),
            "pipeline_intent_id": incident.data_json.get("pipeline_intent_id"),
            "change_set_id": incident.data_json.get("change_set_id"),
            "observation_id": incident.data_json.get("observation_id"),
            "observation_source": incident.data_json.get("observation_source"),
            "observation_kind": incident.data_json.get("observation_kind"),
            "details": incident.data_json.get("summary"),
        },
        "steps": [
            {
                "order": 1,
                "kind": "read_only",
                "capability": "prometheus_inventory",
                "summary": "Refresh bounded Prometheus inventory and compare active alerts, unhealthy targets, and problem rules against the attached evidence."
            },
            {
                "order": 2,
                "kind": "read_only",
                "capability": "loki_log_summary",
                "summary": "Inspect bounded, redacted application and controller logs for the affected namespace if Loki is configured."
            },
            {
                "order": 3,
                "kind": "read_only",
                "capability": "argocd_get_application",
                "summary": "Confirm Argo sync and health before proposing release, rollback, or rollout remediation."
            },
            {
                "order": 4,
                "kind": "proposal",
                "capability": "worktree_change",
                "summary": "If evidence points to repo configuration or application code, prepare a ChangeSet and require approval before file writes."
            },
            {
                "order": 5,
                "kind": "proposal",
                "capability": "deployment_or_pipeline_intent",
                "summary": "If evidence points to runtime or delivery state, propose a PipelineIntent or DeploymentIntent and require approval before mutation."
            }
        ],
        "approval_gates": [
            {
                "kind": "file_write",
                "required_before": "creating or patching a ChangeSet"
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
            "No automatic mutation in V1",
            "No secret reads",
            "No ticket creation",
            "No notification dispatch"
        ]
    })
}

pub(in crate::app) fn approval_gates_from_remediation_plan(
    plan: &StoredRemediationPlan,
) -> Vec<CreateApprovalGate> {
    let gates = plan
        .plan_json
        .get("approval_gates")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    gates
        .into_iter()
        .enumerate()
        .filter_map(|(index, gate_json)| {
            let gate_kind = approval_gate_kind(&gate_json)?;
            let gate_order = i64::try_from(index).ok()?.saturating_add(1);
            let required_before = gate_json
                .get("required_before")
                .and_then(Value::as_str)
                .unwrap_or("executing a risky action");
            Some(CreateApprovalGate {
                id: format!(
                    "agate_{}_{}_{}",
                    plan.id,
                    gate_order,
                    safe_id_fragment(&gate_kind)
                ),
                work_item_id: None,
                remediation_plan_id: Some(plan.id.clone()),
                incident_id: Some(plan.incident_id.clone()),
                session_id: plan.session_id.clone(),
                run_id: plan.run_id.clone(),
                status: "pending".to_string(),
                gate_kind: gate_kind.clone(),
                gate_order,
                title: format!("Approve {}", gate_kind.replace('_', " ")),
                summary: format!("Approval required before {required_before}."),
                risk_level: plan.risk_level.clone(),
                resource_namespace: plan.resource_namespace.clone(),
                resource_kind: plan.resource_kind.clone(),
                resource_name: plan.resource_name.clone(),
                gate_json,
            })
        })
        .collect()
}

pub(in crate::app) fn approval_gate_kind(gate_json: &Value) -> Option<String> {
    gate_json
        .get("kind")
        .and_then(Value::as_str)
        .or_else(|| gate_json.as_str())
        .map(str::trim)
        .filter(|kind| !kind.is_empty())
        .map(str::to_string)
}

pub(in crate::app) fn incident_resource_label(incident: &StoredIncident) -> String {
    match (
        incident.resource_namespace.as_deref(),
        incident.resource_kind.as_deref(),
        incident.resource_name.as_deref(),
    ) {
        (Some(namespace), Some(kind), Some(name)) => format!("{namespace}/{kind}/{name}"),
        (Some(namespace), _, Some(name)) => format!("{namespace}/{name}"),
        (_, Some(kind), Some(name)) => format!("{kind}/{name}"),
        (_, _, Some(name)) => name.to_string(),
        (_, Some(kind), _) => kind.to_string(),
        _ => incident.id.clone(),
    }
}

pub(in crate::app) fn safe_id_fragment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

pub(in crate::app) fn release_observability_incident_id(
    release: &StoredRelease,
    observation: &StoredObservation,
) -> String {
    release_observability_incident_id_for_ids(&release.id, &observation.id)
}

pub(in crate::app) fn release_observability_incident_id_for_ids(
    release_id: &str,
    observation_id: &str,
) -> String {
    let digest = Sha256::digest(format!("{release_id}:{observation_id}"));
    let hash = format!("{digest:x}");
    format!("inc_relobs_{}", &hash[..16])
}

pub(in crate::app) fn release_observability_incident_summary(
    observation: &StoredObservation,
) -> String {
    match (observation.source.as_str(), observation.kind.as_str()) {
        ("prometheus", "inventory") => {
            let unhealthy_targets = observation
                .data_json
                .pointer("/inventory/targets/unhealthy_count")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let problem_rules = observation
                .data_json
                .pointer("/inventory/rules/problem_rule_count")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let alerts = observation
                .data_json
                .pointer("/inventory/alerts/alert_count")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            format!(
                "Prometheus inventory reports {alerts} active alerts, {unhealthy_targets} unhealthy targets, and {problem_rules} problem rules"
            )
        }
        ("prometheus", "prometheus_read") => format!(
            "Prometheus query returned status {}",
            observation
                .data_json
                .pointer("/response/status")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ),
        ("loki", "log_summary") => format!(
            "Loki log summary returned status {}",
            observation
                .data_json
                .pointer("/response/status")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ),
        _ => observation.summary.clone(),
    }
}

pub(in crate::app) fn release_observability_incident_severity(
    observation: &StoredObservation,
) -> &'static str {
    if observation
        .data_json
        .pointer("/inventory/alerts/alert_count")
        .and_then(Value::as_i64)
        .unwrap_or_default()
        > 0
    {
        "high"
    } else {
        "medium"
    }
}

pub(in crate::app) fn release_observability_resource_label(
    observation: &StoredObservation,
) -> String {
    if let Some(namespace) = &observation.resource_namespace {
        if let Some(name) = &observation.resource_name {
            return format!("{namespace}/{name}");
        }
    }
    observation
        .resource_name
        .clone()
        .or_else(|| observation.resource_kind.clone())
        .unwrap_or_else(|| observation.subject.clone())
}

pub(in crate::app) struct ReleaseJsonInput<'a> {
    release_kind: &'a str,
    version: Option<&'a str>,
    commit_sha: Option<&'a str>,
    image_digest: Option<&'a str>,
    rollback_ref: Option<&'a str>,
    release_json: Option<serde_json::Value>,
    build_output: Option<&'a VerifiedPipelineBuildOutput>,
}

pub(in crate::app) fn release_json(
    deployment_intent: &StoredDeploymentIntent,
    input: ReleaseJsonInput<'_>,
) -> Result<serde_json::Value, ApiError> {
    let mut release_json = if let Some(release_json) = input.release_json {
        if !release_json.is_object() {
            return Err(ApiError::bad_request(
                "release release_json must be a JSON object",
            ));
        }
        release_json
    } else {
        json!({
            "execution": {
                "enabled": false,
                "reason": "Release is review state only in V1"
            },
            "source": {
                "deployment_intent_id": deployment_intent.id,
                "pipeline_intent_id": deployment_intent.pipeline_intent_id,
                "change_set_id": deployment_intent.change_set_id,
                "work_plan_id": deployment_intent.work_plan_id,
            },
            "deployment_evidence": release_deployment_evidence_json(deployment_intent),
            "observability_evidence": [],
            "release": {
                "release_kind": input.release_kind,
                "target_environment": deployment_intent.target_environment,
                "target_namespace": deployment_intent.target_namespace,
                "argo_application": deployment_intent.argo_application,
                "version": input.version,
                "commit_sha": input.commit_sha,
                "image_digest": input.image_digest,
                "rollback_ref": input.rollback_ref,
            },
            "verification": {
                "required": ["argo_health", "lgtm_signals", "audit_event"]
            }
        })
    };
    if let Some(build_output) = input.build_output {
        release_json
            .as_object_mut()
            .expect("release_json is validated as an object")
            .insert(
                "build_output".to_string(),
                json!({
                    "status": "verified",
                    "artifact_id": build_output.artifact_id,
                    "image_url": build_output.image_url,
                    "image_digest": build_output.image_digest,
                    "image_reference": build_output.image_reference,
                    "source_commit": build_output.source_commit,
                }),
            );
    }
    Ok(release_json)
}

pub(in crate::app) async fn pipeline_build_output_for_deployment_intent(
    state: &AppState,
    deployment_intent: &StoredDeploymentIntent,
) -> Result<Option<VerifiedPipelineBuildOutput>, ApiError> {
    let intent = state
        .store
        .get_pipeline_intent(&deployment_intent.pipeline_intent_id)
        .await?
        .ok_or_else(|| {
            ApiError::not_found("pipeline_intent", &deployment_intent.pipeline_intent_id)
        })?;
    let Some(run_id) = intent.run_id.as_ref() else {
        return Ok(None);
    };
    let artifacts = state.store.list_artifacts(run_id).await?;
    current_pipeline_build_output(&artifacts, &intent)
}

pub(in crate::app) fn release_pipeline_build_output(
    release: &StoredRelease,
) -> Result<Option<VerifiedPipelineBuildOutput>, ApiError> {
    let Some(content) = release.release_json.get("build_output") else {
        return Ok(None);
    };
    if content.get("status").and_then(Value::as_str) != Some("verified") {
        return Err(ApiError::conflict(
            "Release build-output provenance is not verified",
        ));
    }
    let artifact_id = content
        .get("artifact_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| ApiError::conflict("Release build-output has no artifact id"))?;
    let image_url = content
        .get("image_url")
        .and_then(Value::as_str)
        .filter(|value| safe_oci_image_component(value))
        .map(ToOwned::to_owned)
        .ok_or_else(|| ApiError::conflict("Release build-output has no valid image URL"))?;
    let image_digest = content
        .get("image_digest")
        .and_then(Value::as_str)
        .filter(|value| is_sha256_digest(value))
        .map(ToOwned::to_owned)
        .ok_or_else(|| ApiError::conflict("Release build-output has invalid image digest"))?;
    if release
        .image_digest
        .as_deref()
        .is_some_and(|digest| digest != image_digest)
    {
        return Err(ApiError::conflict(
            "Release image digest does not match build-output provenance",
        ));
    }
    let image_reference = content
        .get("image_reference")
        .and_then(Value::as_str)
        .filter(|value| valid_digest_pinned_image_reference(value))
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            ApiError::conflict("Release build-output has invalid digest-pinned image reference")
        })?;
    let source_commit = content
        .get("source_commit")
        .and_then(Value::as_str)
        .filter(|value| is_git_sha(value))
        .map(ToOwned::to_owned);
    Ok(Some(VerifiedPipelineBuildOutput {
        artifact_id,
        image_url,
        image_digest,
        image_reference,
        source_commit,
    }))
}

pub(in crate::app) fn release_deployment_evidence_json(
    deployment_intent: &StoredDeploymentIntent,
) -> Value {
    let Some(evidence) = deployment_intent.intent_json.get("deployment_evidence") else {
        return json!({
            "status": "missing",
            "release_ready": false,
            "review_required": true,
            "source": "deployment_intent",
            "deployment_intent_id": deployment_intent.id,
            "summary": "No Argo Application evidence is attached to the approved DeploymentIntent"
        });
    };

    let status = evidence
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    json!({
        "status": status,
        "release_ready": status == "satisfied",
        "review_required": status != "satisfied",
        "source": "deployment_intent.deployment_evidence",
        "deployment_intent_id": deployment_intent.id,
        "observation_id": evidence.get("observation_id").cloned().unwrap_or(Value::Null),
        "artifact_id": evidence.get("artifact_id").cloned().unwrap_or(Value::Null),
        "summary": evidence.get("summary").cloned().unwrap_or_else(|| json!({})),
        "evidence": evidence.clone()
    })
}

pub(in crate::app) fn validate_release_transition(
    current: &str,
    target: &str,
) -> Result<(), ApiError> {
    match (current, target) {
        ("proposed", "approved" | "rejected") => Ok(()),
        ("approved", "rejected") => Ok(()),
        (_, "proposed") if current == target => Ok(()),
        _ => Err(ApiError::conflict(format!(
            "cannot transition release from {current} to {target}"
        ))),
    }
}

pub(in crate::app) async fn list_registry_evidence(
    State(state): State<AppState>,
    Query(query): Query<ListRegistryEvidenceQuery>,
) -> Result<Json<RegistryEvidenceListResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0);
    let registry_evidence = state
        .store
        .list_registry_evidence(RegistryEvidenceListFilter {
            release_id: clean_optional_text(query.release_id),
            deployment_intent_id: clean_optional_text(query.deployment_intent_id),
            pipeline_intent_id: clean_optional_text(query.pipeline_intent_id),
            change_set_id: clean_optional_text(query.change_set_id),
            work_plan_id: clean_optional_text(query.work_plan_id),
            remediation_plan_id: clean_optional_text(query.remediation_plan_id),
            incident_id: clean_optional_text(query.incident_id),
            run_id: clean_optional_text(query.run_id).map(RunId::new),
            status: clean_optional_text(query.status),
            risk_level: clean_optional_text(query.risk_level),
            registry: clean_optional_text(query.registry),
            repository: clean_optional_text(query.repository),
            image_ref: clean_optional_text(query.image_ref),
            image_digest: clean_optional_text(query.image_digest),
            tag: clean_optional_text(query.tag),
            source: clean_optional_text(query.source),
            verification_status: clean_optional_text(query.verification_status),
            created_after_ms: query.created_after_ms,
            created_before_ms: query.created_before_ms,
            limit,
            offset,
        })
        .await?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();
    let count = registry_evidence.len();

    Ok(Json(RegistryEvidenceListResponse {
        registry_evidence,
        count,
        limit,
        offset,
    }))
}

pub(in crate::app) async fn get_registry_evidence(
    State(state): State<AppState>,
    Path(evidence_id): Path<String>,
) -> Result<Json<RegistryEvidenceResponse>, ApiError> {
    let evidence = state
        .store
        .get_registry_evidence(&evidence_id)
        .await?
        .ok_or_else(|| ApiError::not_found("registry_evidence", &evidence_id))?;

    Ok(Json(evidence.into()))
}

pub(in crate::app) async fn create_registry_evidence_from_release(
    State(state): State<AppState>,
    Json(request): Json<CreateRegistryEvidenceFromReleaseRequest>,
) -> Result<Json<CreateRegistryEvidenceResponse>, ApiError> {
    let release_id = clean_optional_text(Some(request.release_id.clone()))
        .ok_or_else(|| ApiError::bad_request("release_id is required"))?;
    let release = state
        .store
        .get_release(&release_id)
        .await?
        .ok_or_else(|| ApiError::not_found("release", &release_id))?;
    ensure_approved_for_trusted_envelope("release", &release.id, &release.status)?;

    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let registry = clean_optional_text(request.registry);
    let repository = clean_optional_text(request.repository);
    let build_output = release_pipeline_build_output(&release)?;
    let requested_image_ref = clean_optional_text(request.image_ref);
    if let (Some(output), Some(requested)) = (&build_output, &requested_image_ref) {
        if requested != &output.image_reference {
            return Err(ApiError::conflict(format!(
                "Registry evidence image_ref must match Release build output {}",
                output.image_reference
            )));
        }
    }
    let image_ref = requested_image_ref.or_else(|| {
        build_output
            .as_ref()
            .map(|output| output.image_reference.clone())
    });
    let requested_image_digest = clean_optional_text(request.image_digest);
    if let (Some(output), Some(requested)) = (&build_output, &requested_image_digest) {
        if requested != &output.image_digest {
            return Err(ApiError::conflict(format!(
                "Registry evidence image_digest must match Release build output {}",
                output.image_digest
            )));
        }
    }
    let image_digest = requested_image_digest
        .or_else(|| {
            build_output
                .as_ref()
                .map(|output| output.image_digest.clone())
        })
        .or(release.image_digest.clone());
    let tag = clean_optional_text(request.tag);
    let source = clean_optional_text(request.source).unwrap_or_else(|| {
        if build_output.is_some() {
            "tekton_build_output".to_string()
        } else {
            "manual".to_string()
        }
    });
    let verification_status = clean_optional_text(request.verification_status)
        .unwrap_or_else(|| "unverified".to_string());
    validate_registry_verification_status(&verification_status)?;
    let evidence_json = registry_evidence_json(
        &release,
        RegistryEvidenceJsonInput {
            registry: registry.as_deref(),
            repository: repository.as_deref(),
            image_ref: image_ref.as_deref(),
            image_digest: image_digest.as_deref(),
            tag: tag.as_deref(),
            source: &source,
            verification_status: &verification_status,
            evidence_json: request.evidence_json,
        },
    )?;
    let response = propose_registry_evidence_for_release(
        &state,
        &release,
        RegistryEvidenceDraft {
            title: clean_optional_text(request.title)
                .unwrap_or_else(|| format!("Registry evidence: {}", release.title)),
            summary: clean_optional_text(request.summary)
                .unwrap_or_else(|| "Propose registry evidence after approved release".to_string()),
            risk_level: clean_optional_text(request.risk_level)
                .unwrap_or(release.risk_level.clone()),
            registry,
            repository,
            image_ref,
            image_digest,
            tag,
            source,
            verification_status,
            evidence_json,
            actor,
            reason,
            audit_source: "release".to_string(),
            audit_execution_enabled: false,
        },
    )
    .await?;

    Ok(Json(response))
}

pub(in crate::app) async fn create_registry_evidence_from_registry_inspection(
    State(state): State<AppState>,
    Json(request): Json<CreateRegistryEvidenceFromInspectionRequest>,
) -> Result<Json<CreateRegistryEvidenceFromInspectionResponse>, ApiError> {
    let release_id = clean_optional_text(Some(request.release_id.clone()))
        .ok_or_else(|| ApiError::bad_request("release_id is required"))?;
    let image_ref = clean_optional_text(Some(request.image_ref.clone()))
        .ok_or_else(|| ApiError::bad_request("image_ref is required"))?;
    let release = state
        .store
        .get_release(&release_id)
        .await?
        .ok_or_else(|| ApiError::not_found("release", &release_id))?;
    ensure_approved_for_trusted_envelope("release", &release.id, &release.status)?;
    if let Some(build_output) = release_pipeline_build_output(&release)? {
        if image_ref != build_output.image_reference {
            return Err(ApiError::conflict(format!(
                "Registry inspection image_ref must match Release build output {}",
                build_output.image_reference
            )));
        }
    }

    let inspection = execute_direct_capability(
        &state,
        AgentAction::RegistryInspectImage {
            id: "api.registry_inspect_image".into(),
            reason: clean_optional_text(request.reason.clone()).unwrap_or_else(|| {
                format!("Create RegistryEvidence from registry inspection for {image_ref}")
            }),
            image_ref: image_ref.clone(),
            registry_base_url: clean_optional_text(request.registry_base_url.clone()),
        },
        request.timeout_ms,
    )
    .await?;
    if inspection.status != "ok" {
        return Ok(Json(CreateRegistryEvidenceFromInspectionResponse {
            registry_evidence: None,
            created: false,
            inspection,
        }));
    }

    let Some(result) = inspection.result.as_ref() else {
        return Ok(Json(CreateRegistryEvidenceFromInspectionResponse {
            registry_evidence: None,
            created: false,
            inspection,
        }));
    };
    let draft = registry_evidence_draft_from_inspection(&release, &request, &image_ref, result)?;
    let response = propose_registry_evidence_for_release(&state, &release, draft).await?;

    Ok(Json(CreateRegistryEvidenceFromInspectionResponse {
        registry_evidence: Some(response.registry_evidence),
        created: response.created,
        inspection,
    }))
}

pub(in crate::app) struct RegistryEvidenceDraft {
    title: String,
    summary: String,
    risk_level: String,
    registry: Option<String>,
    repository: Option<String>,
    image_ref: Option<String>,
    image_digest: Option<String>,
    tag: Option<String>,
    source: String,
    verification_status: String,
    evidence_json: serde_json::Value,
    actor: Option<String>,
    reason: Option<String>,
    audit_source: String,
    audit_execution_enabled: bool,
}

pub(in crate::app) async fn propose_registry_evidence_for_release(
    state: &AppState,
    release: &StoredRelease,
    draft: RegistryEvidenceDraft,
) -> Result<CreateRegistryEvidenceResponse, ApiError> {
    if let Some(existing) = state
        .store
        .get_registry_evidence_by_release(&release.id)
        .await?
    {
        if existing.status == "stale" {
            let evidence = state
                .store
                .revise_registry_evidence_draft(
                    &existing.id,
                    UpdateRegistryEvidenceDraft {
                        title: draft.title,
                        summary: draft.summary,
                        risk_level: draft.risk_level,
                        registry: draft.registry,
                        repository: draft.repository,
                        image_ref: draft.image_ref,
                        image_digest: draft.image_digest,
                        tag: draft.tag,
                        source: draft.source,
                        verification_status: draft.verification_status,
                        evidence_json: draft.evidence_json,
                        actor: draft.actor.clone(),
                        reason: draft.reason.clone(),
                    },
                )
                .await?;
            append_registry_evidence_audit_event(
                &state.store,
                &evidence,
                "registry_evidence.reproposed",
                draft.actor,
                draft.reason,
                json!({
                "source": draft.audit_source,
                "release_id": evidence.release_id,
                "previous_status": existing.status,
                "execution_enabled": draft.audit_execution_enabled,
                "pipeline_build_output_artifact_id": evidence
                    .evidence_json
                    .pointer("/build_output/artifact_id"),
                    }),
            )
            .await?;

            return Ok(CreateRegistryEvidenceResponse {
                registry_evidence: evidence.into(),
                created: false,
            });
        }

        return Ok(CreateRegistryEvidenceResponse {
            registry_evidence: existing.into(),
            created: false,
        });
    }
    let evidence = state
        .store
        .create_registry_evidence(CreateRegistryEvidence {
            id: format!("regev_{}", unique_suffix()),
            release_id: release.id.clone(),
            deployment_intent_id: release.deployment_intent_id.clone(),
            pipeline_intent_id: release.pipeline_intent_id.clone(),
            change_set_id: release.change_set_id.clone(),
            work_plan_id: release.work_plan_id.clone(),
            remediation_plan_id: release.remediation_plan_id.clone(),
            incident_id: release.incident_id.clone(),
            session_id: release.session_id.clone(),
            run_id: release.run_id.clone(),
            status: "proposed".to_string(),
            title: draft.title,
            summary: draft.summary,
            risk_level: draft.risk_level,
            registry: draft.registry,
            repository: draft.repository,
            image_ref: draft.image_ref,
            image_digest: draft.image_digest,
            tag: draft.tag,
            source: draft.source,
            verification_status: draft.verification_status,
            evidence_json: draft.evidence_json,
        })
        .await?;
    append_registry_evidence_audit_event(
        &state.store,
        &evidence,
        "registry_evidence.proposed",
        draft.actor,
        draft.reason,
        json!({
            "source": draft.audit_source,
            "release_id": evidence.release_id,
            "execution_enabled": draft.audit_execution_enabled,
            "pipeline_build_output_artifact_id": evidence
                .evidence_json
                .pointer("/build_output/artifact_id"),
        }),
    )
    .await?;

    Ok(CreateRegistryEvidenceResponse {
        registry_evidence: evidence.into(),
        created: true,
    })
}

pub(in crate::app) fn registry_evidence_draft_from_inspection(
    release: &StoredRelease,
    request: &CreateRegistryEvidenceFromInspectionRequest,
    image_ref: &str,
    result: &ToolResult,
) -> Result<RegistryEvidenceDraft, ApiError> {
    let content = &result.content;
    let registry = string_at(content, "/image/registry");
    let repository = string_at(content, "/image/repository");
    let tag = string_at(content, "/image/tag");
    let image_digest =
        string_at(content, "/image/digest").or_else(|| string_at(content, "/probe/digest"));
    let verification_status =
        string_at(content, "/verification_status").unwrap_or_else(|| "unknown".to_string());
    validate_registry_verification_status(&verification_status)?;
    let source = "registry_inspect_image".to_string();
    let evidence_json = registry_evidence_json(
        release,
        RegistryEvidenceJsonInput {
            registry: registry.as_deref(),
            repository: repository.as_deref(),
            image_ref: Some(image_ref),
            image_digest: image_digest.as_deref(),
            tag: tag.as_deref(),
            source: &source,
            verification_status: &verification_status,
            evidence_json: Some(json!({
                "execution": {
                    "enabled": true,
                    "capability": "registry_inspect_image",
                    "tool_status": result.status,
                    "summary": result.summary,
                    "manifest_body_persisted": false,
                },
                "source": {
                    "release_id": release.id,
                    "deployment_intent_id": release.deployment_intent_id,
                    "pipeline_intent_id": release.pipeline_intent_id,
                    "change_set_id": release.change_set_id,
                    "work_plan_id": release.work_plan_id,
                    "evidence_source": source,
                },
                "image": {
                    "registry": registry,
                    "repository": repository,
                    "image_ref": image_ref,
                    "image_digest": image_digest,
                    "tag": tag,
                    "requested_image_ref": content.get("requested_image_ref"),
                    "reference": content.get("reference"),
                },
                "verification": {
                    "status": verification_status,
                    "checks": [{
                        "name": "anonymous_manifest_probe",
                        "status": content.pointer("/probe/status"),
                        "accessible": content.pointer("/probe/accessible"),
                        "digest": content.pointer("/probe/digest"),
                        "content_type": content.pointer("/probe/content_type"),
                    }],
                },
            })),
        },
    )?;

    Ok(RegistryEvidenceDraft {
        title: clean_optional_text(request.title.clone())
            .unwrap_or_else(|| format!("Registry evidence: {}", release.title)),
        summary: clean_optional_text(request.summary.clone())
            .unwrap_or_else(|| result.summary.clone()),
        risk_level: clean_optional_text(request.risk_level.clone())
            .unwrap_or_else(|| release.risk_level.clone()),
        registry,
        repository,
        image_ref: Some(image_ref.to_string()),
        image_digest: image_digest.or_else(|| release.image_digest.clone()),
        tag,
        source,
        verification_status,
        evidence_json,
        actor: clean_optional_text(request.actor.clone()),
        reason: clean_optional_text(request.reason.clone()),
        audit_source: "registry_inspection".to_string(),
        audit_execution_enabled: true,
    })
}

pub(in crate::app) fn string_at(source: &Value, pointer: &str) -> Option<String> {
    source
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(in crate::app) async fn transition_registry_evidence(
    State(state): State<AppState>,
    Path(evidence_id): Path<String>,
    Json(request): Json<TransitionRegistryEvidenceRequest>,
) -> Result<Json<TransitionRegistryEvidenceResponse>, ApiError> {
    let current = state
        .store
        .get_registry_evidence(&evidence_id)
        .await?
        .ok_or_else(|| ApiError::not_found("registry_evidence", &evidence_id))?;
    let target = clean_optional_text(Some(request.target_status))
        .ok_or_else(|| ApiError::bad_request("target_status is required"))?;
    validate_registry_evidence_transition(&current.status, &target)?;
    let actor = clean_optional_text(request.actor);
    let reason = clean_optional_text(request.reason);
    let evidence = state
        .store
        .update_registry_evidence_status(&evidence_id, &target, actor.clone(), reason.clone())
        .await?;
    append_registry_evidence_audit_event(
        &state.store,
        &evidence,
        &format!("registry_evidence.{target}"),
        actor,
        reason,
        json!({
            "previous_status": current.status,
            "status": evidence.status,
        }),
    )
    .await?;

    Ok(Json(TransitionRegistryEvidenceResponse {
        registry_evidence: evidence.into(),
    }))
}

pub(in crate::app) struct RegistryEvidenceJsonInput<'a> {
    registry: Option<&'a str>,
    repository: Option<&'a str>,
    image_ref: Option<&'a str>,
    image_digest: Option<&'a str>,
    tag: Option<&'a str>,
    source: &'a str,
    verification_status: &'a str,
    evidence_json: Option<serde_json::Value>,
}

pub(in crate::app) fn registry_evidence_json(
    release: &StoredRelease,
    input: RegistryEvidenceJsonInput<'_>,
) -> Result<serde_json::Value, ApiError> {
    let mut evidence_json = if let Some(evidence_json) = input.evidence_json {
        ensure_json_object(&evidence_json, "evidence_json")?;
        evidence_json
    } else {
        json!({
            "execution": {
                "enabled": false,
                "reason": "RegistryEvidence is manual or API-fed evidence only in V1"
            },
            "source": {
                "release_id": release.id,
                "deployment_intent_id": release.deployment_intent_id,
                "pipeline_intent_id": release.pipeline_intent_id,
                "change_set_id": release.change_set_id,
                "work_plan_id": release.work_plan_id,
                "evidence_source": input.source,
            },
            "image": {
                "registry": input.registry,
                "repository": input.repository,
                "image_ref": input.image_ref,
                "image_digest": input.image_digest,
                "tag": input.tag,
            },
            "verification": {
                "status": input.verification_status,
                "checks": [],
            }
        })
    };
    if let Some(output) = release_pipeline_build_output(release)? {
        evidence_json
            .as_object_mut()
            .expect("evidence_json is validated as an object")
            .insert(
                "build_output".to_string(),
                json!({
                    "artifact_id": output.artifact_id,
                    "image_reference": output.image_reference,
                    "image_digest": output.image_digest,
                    "source_commit": output.source_commit,
                }),
            );
    }
    Ok(evidence_json)
}

pub(in crate::app) fn validate_registry_verification_status(status: &str) -> Result<(), ApiError> {
    match status {
        "verified" | "unverified" | "mismatch" | "unknown" => Ok(()),
        _ => Err(ApiError::bad_request(format!(
            "invalid registry verification status {status}"
        ))),
    }
}

pub(in crate::app) fn validate_registry_evidence_transition(
    current: &str,
    target: &str,
) -> Result<(), ApiError> {
    match (current, target) {
        ("proposed", "verified" | "rejected") => Ok(()),
        ("verified", "rejected") => Ok(()),
        (_, "proposed") if current == target => Ok(()),
        _ => Err(ApiError::conflict(format!(
            "cannot transition registry evidence from {current} to {target}"
        ))),
    }
}

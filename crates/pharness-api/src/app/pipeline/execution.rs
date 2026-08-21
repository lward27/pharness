use super::super::*;

pub(in crate::app) async fn execute_pipeline_intent(
    State(state): State<AppState>,
    identity: Option<Extension<OperatorIdentity>>,
    Path(pipeline_intent_id): Path<String>,
    Json(request): Json<ExecutePipelineIntentRequest>,
) -> Result<Json<ExecutePipelineIntentResponse>, ApiError> {
    let actor = identity
        .map(|Extension(OperatorIdentity(name))| name)
        .or_else(|| clean_optional_text(request.actor.clone()));
    let reason = clean_optional_text(request.reason.clone());
    let preflight = pipeline_intent_execution_preflight(&state, &pipeline_intent_id).await?;
    if !preflight.ready || request.dry_run {
        return Ok(Json(ExecutePipelineIntentResponse {
            status: if preflight.ready { "ready" } else { "blocked" }.to_string(),
            ready: preflight.ready,
            dry_run: request.dry_run,
            pipeline_intent: preflight.intent.into(),
            manifest: preflight.manifest,
            checks: preflight.checks,
            permission_grant_id: preflight.grant_id,
            execution_id: None,
            executor_job_name: None,
        }));
    }

    let execution_id = format!("pexec_{}", unique_suffix());
    let mut intent_json = preflight.intent.intent_json.clone();
    let manifest = preflight
        .manifest
        .clone()
        .ok_or_else(|| ApiError::internal("execution preflight omitted a PipelineRun manifest"))?;
    set_pipeline_execution_state(
        &mut intent_json,
        json!({
            "execution_id": execution_id,
            "state": "dispatching",
            "pipeline_run_namespace": preflight.execution.namespace,
            "pipeline_run_name": pipeline_run_name(&manifest),
            "permission_grant_id": preflight.grant_id,
        }),
    );
    let intent = state
        .store
        .update_pipeline_intent_execution(
            &preflight.intent.id,
            UpdatePipelineIntentExecution {
                status: "executing".to_string(),
                intent_json,
                actor: actor.clone(),
                reason: reason.clone(),
            },
        )
        .await?;

    let dispatch = state
        .worker
        .dispatch_tekton_execution(TektonExecutionRequest {
            pipeline_intent_id: intent.id.clone(),
            execution_id: execution_id.clone(),
            target_namespace: preflight.execution.namespace.clone(),
            pipeline_run_manifest: manifest.clone(),
        })
        .await;
    let (intent, status, executor_job_name) = match dispatch {
        Ok(receipt) => {
            let mut intent_json = intent.intent_json.clone();
            set_pipeline_execution_state(
                &mut intent_json,
                json!({
                    "execution_id": execution_id,
                    "state": "executor_job_created",
                    "executor_job_name": receipt.job_name,
                    "pipeline_run_namespace": preflight.execution.namespace,
                    "pipeline_run_name": pipeline_run_name(&manifest),
                    "permission_grant_id": preflight.grant_id,
                }),
            );
            let intent = state
                .store
                .update_pipeline_intent_execution(
                    &intent.id,
                    UpdatePipelineIntentExecution {
                        status: "executing".to_string(),
                        intent_json,
                        actor: actor.clone(),
                        reason: reason.clone(),
                    },
                )
                .await?;
            append_pipeline_intent_audit_event(
                &state.store,
                &intent,
                "pipeline_intent.execution_dispatched",
                actor.clone(),
                reason.clone(),
                json!({
                    "execution_id": execution_id,
                    "executor_job_name": receipt.job_name,
                    "permission_grant_id": preflight.grant_id,
                }),
            )
            .await?;
            (intent, "dispatched".to_string(), Some(receipt.job_name))
        }
        Err(error) => {
            let mut intent_json = intent.intent_json.clone();
            set_pipeline_execution_state(
                &mut intent_json,
                json!({
                    "execution_id": execution_id,
                    "state": "dispatch_failed",
                    "error": error.to_string(),
                    "pipeline_run_namespace": preflight.execution.namespace,
                    "pipeline_run_name": pipeline_run_name(&manifest),
                    "permission_grant_id": preflight.grant_id,
                }),
            );
            let intent = state
                .store
                .update_pipeline_intent_execution(
                    &intent.id,
                    UpdatePipelineIntentExecution {
                        status: "failed".to_string(),
                        intent_json,
                        actor: actor.clone(),
                        reason: reason.clone(),
                    },
                )
                .await?;
            append_pipeline_intent_audit_event(
                &state.store,
                &intent,
                "pipeline_intent.execution_dispatch_failed",
                actor.clone(),
                reason.clone(),
                json!({ "execution_id": execution_id, "error": error.to_string() }),
            )
            .await?;
            (intent, "failed".to_string(), None)
        }
    };

    Ok(Json(ExecutePipelineIntentResponse {
        status,
        ready: true,
        dry_run: false,
        pipeline_intent: intent.into(),
        manifest: Some(manifest),
        checks: preflight.checks,
        permission_grant_id: preflight.grant_id,
        execution_id: Some(execution_id),
        executor_job_name,
    }))
}

pub(in crate::app) async fn internal_pipeline_intent_execution_outcome(
    State(state): State<AppState>,
    Path(pipeline_intent_id): Path<String>,
    Json(request): Json<PipelineIntentExecutionOutcomeRequest>,
) -> Result<Json<PipelineIntentResponse>, ApiError> {
    let intent = state
        .store
        .get_pipeline_intent(&pipeline_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_intent", &pipeline_intent_id))?;
    if intent.status != "executing" {
        return Err(ApiError::conflict(
            "execution outcome requires a PipelineIntent in executing status",
        ));
    }
    let current_execution_id = intent
        .intent_json
        .pointer("/execution_state/execution_id")
        .and_then(Value::as_str);
    if current_execution_id != Some(request.execution_id.as_str()) {
        return Err(ApiError::conflict(
            "execution outcome does not match the current PipelineIntent execution",
        ));
    }
    let (status, event_kind, state_name) = match request.status.as_str() {
        "submitted" => (
            "executing",
            "pipeline_intent.execution_submitted",
            "pipeline_run_created",
        ),
        "completed" => (
            "approved",
            "pipeline_intent.execution_completed",
            "pipeline_run_succeeded",
        ),
        "failed" => (
            "failed",
            "pipeline_intent.execution_failed",
            if intent
                .intent_json
                .pointer("/execution_state/state")
                .and_then(Value::as_str)
                == Some("pipeline_run_created")
            {
                "pipeline_run_failed"
            } else {
                "failed"
            },
        ),
        _ => {
            return Err(ApiError::bad_request(
                "execution outcome status must be submitted, completed, or failed",
            ))
        }
    };
    let terminal_evidence = if matches!(request.status.as_str(), "completed" | "failed") {
        Some(
            persist_pipeline_execution_evidence(&state.store, &intent, &request, state_name)
                .await?,
        )
    } else {
        None
    };
    let pipeline_analysis = match request.pipeline_run_analysis.as_ref() {
        Some(analysis) => {
            Some(persist_pipeline_run_analysis(&state.store, &intent, &request, analysis).await?)
        }
        None => None,
    };
    let build_output = match (
        request.status.as_str(),
        request.pipeline_run_analysis.as_ref(),
    ) {
        ("completed", Some(analysis)) => {
            persist_pipeline_build_output(&state.store, &intent, &request, analysis).await?
        }
        _ => None,
    };
    let mut intent_json = intent.intent_json.clone();
    merge_pipeline_execution_state(
        &mut intent_json,
        json!({
            "execution_id": request.execution_id,
            "state": state_name,
            "pipeline_run_namespace": request.pipeline_run_namespace,
            "pipeline_run_name": request.pipeline_run_name,
            "error": request.error,
        }),
    );
    if let Some(evidence) = terminal_evidence {
        set_pipeline_execution_evidence(&mut intent_json, evidence);
    }
    if let Some(observation) = &pipeline_analysis {
        set_pipeline_intent_evidence(&mut intent_json, observation);
    }
    if let Some(output) = &build_output {
        set_pipeline_build_output(&mut intent_json, output);
    }
    let intent = state
        .store
        .update_pipeline_intent_execution(
            &intent.id,
            UpdatePipelineIntentExecution {
                status: status.to_string(),
                intent_json,
                actor: Some("executor:tekton".to_string()),
                reason: request.error.clone(),
            },
        )
        .await?;
    append_pipeline_intent_audit_event(
        &state.store,
        &intent,
        event_kind,
        Some("executor:tekton".to_string()),
        None,
        json!({
            "execution_id": request.execution_id,
            "pipeline_run_namespace": request.pipeline_run_namespace,
            "pipeline_run_name": request.pipeline_run_name,
            "error": request.error,
            "analysis_observation_id": pipeline_analysis.as_ref().map(|observation| &observation.id),
            "analysis_artifact_id": pipeline_analysis
                .as_ref()
                .and_then(|observation| observation.artifact_id.as_ref()),
            "analysis_error": request.analysis_error,
            "build_output_artifact_id": build_output.as_ref().map(|artifact| &artifact.id),
        }),
    )
    .await?;
    if let Some(observation) = pipeline_analysis {
        append_pipeline_intent_audit_event(
            &state.store,
            &intent,
            "pipeline_intent.evidence_attached",
            Some("executor:tekton".to_string()),
            Some("attached terminal PipelineRunAnalysis".to_string()),
            json!({
                "observation_id": observation.id,
                "artifact_id": observation.artifact_id,
                "evidence_status": intent.intent_json.pointer("/evidence/status"),
                "resource": {
                    "namespace": observation.resource_namespace,
                    "kind": observation.resource_kind,
                    "name": observation.resource_name,
                },
            }),
        )
        .await?;
    } else if let Some(error) = request.analysis_error.as_deref() {
        append_pipeline_intent_audit_event(
            &state.store,
            &intent,
            "pipeline_intent.execution_analysis_failed",
            Some("executor:tekton".to_string()),
            Some(truncate_audit_text(error, 256)),
            json!({
                "execution_id": request.execution_id,
                "pipeline_run_namespace": request.pipeline_run_namespace,
                "pipeline_run_name": request.pipeline_run_name,
            }),
        )
        .await?;
    }
    if let Some(output) = build_output {
        append_pipeline_intent_audit_event(
            &state.store,
            &intent,
            "pipeline_intent.build_output_recorded",
            Some("executor:tekton".to_string()),
            Some("recorded terminal digest-pinned build output".to_string()),
            json!({
                "artifact_id": output.id,
                "status": output.content_json.as_ref().and_then(|content| content.get("status")),
                "image_ref": output.content_json.as_ref().and_then(|content| content.pointer("/image/reference")),
                "source_commit": output.content_json.as_ref().and_then(|content| content.pointer("/source/commit")),
            }),
        )
        .await?;
    }

    if request.status == "completed" {
        match create_declared_deployment_handoff(&state, &intent).await {
            Ok(Some(deployment_intent)) => {
                append_pipeline_intent_audit_event(
                    &state.store,
                    &intent,
                    "pipeline_intent.deployment_handoff_created",
                    Some("executor:tekton".to_string()),
                    Some(
                        "created proposed DeploymentIntent from terminal build evidence"
                            .to_string(),
                    ),
                    json!({
                        "deployment_intent_id": deployment_intent.id,
                        "target_environment": deployment_intent.target_environment,
                        "target_namespace": deployment_intent.target_namespace,
                        "argo_application": deployment_intent.argo_application,
                    }),
                )
                .await?;
            }
            Ok(None) => {}
            Err(error) => {
                append_pipeline_intent_audit_event(
                    &state.store,
                    &intent,
                    "pipeline_intent.deployment_handoff_failed",
                    Some("executor:tekton".to_string()),
                    Some(truncate_audit_text(&error.message, 256)),
                    json!({ "execution_id": request.execution_id }),
                )
                .await?;
            }
        }
    }
    Ok(Json(intent.into()))
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct TektonExecutionSpec {
    pub(in crate::app) enabled: bool,
    pub(in crate::app) namespace: String,
    pub(in crate::app) pipeline_ref: String,
    #[serde(default)]
    pub(in crate::app) production_impacting: bool,
    #[serde(default)]
    pub(in crate::app) params: BTreeMap<String, Value>,
    #[serde(default)]
    pub(in crate::app) workspaces: Vec<TektonWorkspaceSpec>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct TektonWorkspaceSpec {
    name: String,
    #[serde(default)]
    persistent_volume_claim: Option<String>,
    #[serde(default)]
    volume_claim_template: Option<TektonVolumeClaimTemplate>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct TektonVolumeClaimTemplate {
    storage: String,
    #[serde(default = "default_access_modes")]
    access_modes: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct PipelineContractSpec {
    #[serde(default)]
    params: Vec<PipelineParameterContract>,
    #[serde(default)]
    workspaces: Vec<PipelineWorkspaceContract>,
    #[serde(default)]
    pub(in crate::app) source_revision_param: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct PipelineParameterContract {
    name: String,
    #[serde(rename = "type")]
    value_type: String,
    #[serde(default)]
    required: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct PipelineWorkspaceContract {
    name: String,
    binding: String,
    #[serde(default)]
    required: bool,
}

pub(in crate::app) fn default_access_modes() -> Vec<String> {
    vec!["ReadWriteOnce".to_string()]
}

pub(in crate::app) struct PipelineIntentExecutionPreflight {
    pub(in crate::app) ready: bool,
    pub(in crate::app) intent: StoredPipelineIntent,
    pub(in crate::app) execution: TektonExecutionSpec,
    pub(in crate::app) manifest: Option<Value>,
    pub(in crate::app) checks: Vec<Value>,
    pub(in crate::app) grant_id: Option<String>,
}

pub(in crate::app) fn pipeline_execution_preflight_response(
    preflight: PipelineIntentExecutionPreflight,
) -> PipelineIntentExecutionPreflightResponse {
    PipelineIntentExecutionPreflightResponse {
        ready: preflight.ready,
        manifest: preflight.manifest,
        checks: preflight.checks,
        permission_grant_id: preflight.grant_id,
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::app) struct PipelineContractBinding {
    id: String,
    version: String,
    namespace: String,
    pipeline_ref: String,
}

pub(in crate::app) fn pipeline_contract_binding(
    intent_json: &Value,
) -> Result<Option<PipelineContractBinding>, ApiError> {
    let Some(binding) = intent_json.get("pipeline_contract") else {
        return Ok(None);
    };
    serde_json::from_value(binding.clone())
        .map(Some)
        .map_err(|error| {
            ApiError::conflict(format!(
                "PipelineIntent has invalid pinned PipelineContract provenance: {error}"
            ))
        })
}

pub(in crate::app) async fn pipeline_intent_execution_preflight(
    state: &AppState,
    pipeline_intent_id: &str,
) -> Result<PipelineIntentExecutionPreflight, ApiError> {
    let intent = state
        .store
        .get_pipeline_intent(pipeline_intent_id)
        .await?
        .ok_or_else(|| ApiError::not_found("pipeline_intent", pipeline_intent_id))?;
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
    let execution = tekton_execution_spec(&intent.intent_json)?;
    let immutable_source_revision =
        immutable_pipeline_source_revision(&intent, change_set.work_item_id.is_some())?;
    let mut checks = vec![
        execution_check(
            "pipeline_intent_approved",
            intent.status == "approved",
            format!("PipelineIntent status is {}", intent.status),
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
            "execution_enabled",
            execution.enabled,
            "Tekton execution is enabled",
        ),
    ];

    let contract = match pipeline_contract_binding(&intent.intent_json)? {
        Some(binding) => match state.store.get_pipeline_contract(&binding.id).await? {
            None => {
                checks.push(execution_check(
                    "active_pipeline_contract",
                    false,
                    format!("Pinned PipelineContract {} no longer exists", binding.id),
                ));
                None
            }
            Some(contract)
                if contract.status != "active"
                    || contract.version != binding.version
                    || contract.namespace != binding.namespace
                    || contract.pipeline_ref != binding.pipeline_ref
                    || contract.namespace != execution.namespace
                    || contract.pipeline_ref != execution.pipeline_ref =>
            {
                checks.push(execution_check(
                    "active_pipeline_contract",
                    false,
                    format!(
                        "Pinned PipelineContract {} no longer matches its active execution contract",
                        binding.id
                    ),
                ));
                None
            }
            Some(contract) => {
                checks.push(execution_check(
                    "active_pipeline_contract",
                    true,
                    format!(
                        "Pinned active PipelineContract {} version {} matches",
                        contract.id, contract.version
                    ),
                ));
                Some(contract)
            }
        },
        None if change_set.work_item_id.is_some() => {
            checks.push(execution_check(
                "active_pipeline_contract",
                false,
                "WorkItem PipelineIntent requires an exact pinned PipelineContract before execution",
            ));
            None
        }
        None => {
            let contracts = state
                .store
                .list_pipeline_contracts(PipelineContractListFilter {
                    namespace: Some(execution.namespace.clone()),
                    pipeline_ref: Some(execution.pipeline_ref.clone()),
                    status: Some("active".to_string()),
                    limit: 10,
                    ..PipelineContractListFilter::default()
                })
                .await?;
            let matching_contract_count = if contracts.is_empty() {
                state
                    .store
                    .list_pipeline_contracts(PipelineContractListFilter {
                        namespace: Some(execution.namespace.clone()),
                        pipeline_ref: Some(execution.pipeline_ref.clone()),
                        limit: 10,
                        ..PipelineContractListFilter::default()
                    })
                    .await?
                    .len()
            } else {
                contracts.len()
            };
            match contracts.as_slice() {
                [] => {
                    checks.push(execution_check(
                        "active_pipeline_contract",
                        false,
                        if matching_contract_count == 0 {
                            format!(
                                "No PipelineContract exists for {}/{}",
                                execution.namespace, execution.pipeline_ref
                            )
                        } else {
                            format!(
                                "All PipelineContracts for {}/{} are retired",
                                execution.namespace, execution.pipeline_ref
                            )
                        },
                    ));
                    None
                }
                [contract] => {
                    checks.push(execution_check(
                        "active_pipeline_contract",
                        true,
                        format!(
                            "Active PipelineContract {} version {} matches",
                            contract.id, contract.version
                        ),
                    ));
                    Some(contract.clone())
                }
                _ => {
                    checks.push(execution_check(
                        "active_pipeline_contract",
                        false,
                        format!(
                            "Multiple active PipelineContracts match {}/{}; retire the older contract",
                            execution.namespace, execution.pipeline_ref
                        ),
                    ));
                    None
                }
            }
        }
    };
    if let Some(contract) = contract.as_ref() {
        match execution_matches_pipeline_contract(
            &execution,
            contract,
            immutable_source_revision.as_deref(),
        ) {
            Ok(()) => checks.push(execution_check(
                "pipeline_contract_inputs",
                true,
                format!(
                    "PipelineIntent inputs match PipelineContract {}",
                    contract.id
                ),
            )),
            Err(error) => checks.push(execution_check(
                "pipeline_contract_inputs",
                false,
                error.message,
            )),
        }
    } else {
        checks.push(execution_check(
            "pipeline_contract_inputs",
            false,
            "PipelineIntent inputs cannot be validated without one active PipelineContract",
        ));
    }

    let gates = match (
        intent.remediation_plan_id.as_deref(),
        work_plan.work_item_id.as_deref(),
    ) {
        (Some(remediation_plan_id), _) => {
            state
                .store
                .list_approval_gates(ApprovalGateListFilter {
                    remediation_plan_id: Some(remediation_plan_id.to_string()),
                    limit: 200,
                    ..ApprovalGateListFilter::default()
                })
                .await?
        }
        (None, Some(work_item_id)) => {
            state
                .store
                .list_approval_gates(ApprovalGateListFilter {
                    work_item_id: Some(work_item_id.to_string()),
                    limit: 200,
                    ..ApprovalGateListFilter::default()
                })
                .await?
        }
        (None, None) => Vec::new(),
    };
    let required_kinds = if execution.production_impacting {
        ["pipeline_mutation", "production_impact"].as_slice()
    } else {
        ["pipeline_mutation"].as_slice()
    };
    for kind in required_kinds {
        let matching = gates
            .iter()
            .filter(|gate| {
                work_item.as_ref().map_or_else(
                    || gate.gate_kind == *kind,
                    |item| work_item_gate_scope_matches(gate, item, &work_plan, kind),
                )
            })
            .collect::<Vec<_>>();
        let satisfied = !matching.is_empty()
            && matching
                .iter()
                .all(|gate| matches!(gate.status.as_str(), "satisfied" | "waived"));
        checks.push(execution_check(
            format!("approval_gate_{kind}"),
            satisfied,
            if matching.is_empty() {
                if work_item.is_some() {
                    format!("Required scoped WorkItem {kind} approval gate is missing")
                } else {
                    format!("Required {kind} approval gate is missing")
                }
            } else {
                format!("{} {kind} gate(s) are satisfied or waived", matching.len())
            },
        ));
    }
    // WorkItem gates are phase-scoped. A pending GitOps gate must not block a
    // separately authorized Tekton build; it is evaluated at GitOps delivery.

    let grant =
        matching_pipeline_execution_grant(&state.store, &state.policy, &intent, &execution).await?;
    checks.push(execution_check(
        "trusted_execution_envelope",
        grant.is_some(),
        grant
            .as_ref()
            .map(|grant| {
                format!(
                    "Active supervised-autonomy grant {} matches the PipelineIntent",
                    grant.id
                )
            })
            .unwrap_or_else(|| {
                "No active supervised-autonomy grant matches this PipelineIntent".to_string()
            }),
    ));
    let ready = checks
        .iter()
        .all(|check| check.get("passed").and_then(Value::as_bool) == Some(true));
    let manifest = ready
        .then(|| build_pipeline_run_manifest(&intent, &execution))
        .transpose()?;
    Ok(PipelineIntentExecutionPreflight {
        ready,
        intent,
        execution,
        manifest,
        checks,
        grant_id: grant.map(|grant| grant.id),
    })
}

pub(in crate::app) async fn matching_pipeline_execution_grant(
    store: &SqliteStore,
    policy: &SafetyPolicy,
    intent: &StoredPipelineIntent,
    execution: &TektonExecutionSpec,
) -> Result<Option<StoredPermissionGrant>, ApiError> {
    let now = current_millis();
    let work_plan = store
        .get_work_plan(&intent.work_plan_id)
        .await?
        .ok_or_else(|| ApiError::not_found("work_plan", &intent.work_plan_id))?;
    let expected_environment = match work_plan.work_item_id.as_deref() {
        Some(work_item_id) => {
            store
                .get_work_item(work_item_id)
                .await?
                .ok_or_else(|| ApiError::not_found("work_item", work_item_id))?
                .target_environment
        }
        None => policy.environment.clone(),
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
        let matches = grant.subject == policy.subject
            && scope.environment.as_deref() == Some(expected_environment.as_str())
            && grant_policy.policy_mode == PolicyMode::SupervisedAutonomy
            && scope
                .capability_kinds
                .contains(&CapabilityKind::TektonStartRun)
            && scope
                .actions
                .iter()
                .any(|action| action == "tekton_trigger_pipeline")
            && scope
                .max_risk
                .is_some_and(|risk| risk_rank(risk) >= risk_rank(RiskLevel::High))
            && scope
                .namespaces
                .iter()
                .any(|namespace| namespace == &execution.namespace)
            && scope
                .work_plan_ids
                .iter()
                .any(|id| id == &intent.work_plan_id)
            && scope
                .change_set_ids
                .iter()
                .any(|id| id == &intent.change_set_id)
            && scope.pipeline_intent_ids.iter().any(|id| id == &intent.id)
            && scope.production_impacting == Some(execution.production_impacting);
        if matches {
            return Ok(Some(grant));
        }
    }
    Ok(None)
}

pub(in crate::app) fn tekton_execution_spec(
    intent_json: &Value,
) -> Result<TektonExecutionSpec, ApiError> {
    let execution = intent_json
        .get("execution")
        .cloned()
        .ok_or_else(|| ApiError::bad_request("pipeline intent execution is required"))?;
    let execution = serde_json::from_value::<TektonExecutionSpec>(execution).map_err(|error| {
        ApiError::bad_request(format!("pipeline intent execution is invalid: {error}"))
    })?;
    validate_tekton_execution_spec(&execution)?;
    Ok(execution)
}

pub(in crate::app) fn pipeline_contract_spec(
    value: &Value,
) -> Result<PipelineContractSpec, ApiError> {
    if !value.is_object() {
        return Err(ApiError::bad_request(
            "pipeline contract contract_json must be a JSON object",
        ));
    }
    serde_json::from_value::<PipelineContractSpec>(value.clone()).map_err(|error| {
        ApiError::bad_request(format!(
            "pipeline contract contract_json is invalid: {error}"
        ))
    })
}

pub(in crate::app) fn validate_pipeline_contract_spec(
    contract: &PipelineContractSpec,
) -> Result<(), ApiError> {
    let mut names = BTreeSet::new();
    for parameter in &contract.params {
        validate_kubernetes_name("pipeline contract params.name", &parameter.name)?;
        if !matches!(parameter.value_type.as_str(), "scalar" | "array") {
            return Err(ApiError::bad_request(
                "pipeline contract params.type must be scalar or array",
            ));
        }
        if !names.insert(parameter.name.as_str()) {
            return Err(ApiError::bad_request(
                "pipeline contract params must not repeat a name",
            ));
        }
    }
    let mut workspace_names = BTreeSet::new();
    for workspace in &contract.workspaces {
        validate_kubernetes_name("pipeline contract workspaces.name", &workspace.name)?;
        if !matches!(
            workspace.binding.as_str(),
            "persistent_volume_claim" | "volume_claim_template"
        ) {
            return Err(ApiError::bad_request(
                "pipeline contract workspaces.binding must be persistent_volume_claim or volume_claim_template",
            ));
        }
        if !workspace_names.insert(workspace.name.as_str()) {
            return Err(ApiError::bad_request(
                "pipeline contract workspaces must not repeat a name",
            ));
        }
    }
    if let Some(source_revision_param) = &contract.source_revision_param {
        validate_kubernetes_name(
            "pipeline contract source_revision_param",
            source_revision_param,
        )?;
        let parameter = contract
            .params
            .iter()
            .find(|parameter| parameter.name == *source_revision_param)
            .ok_or_else(|| {
                ApiError::bad_request(
                    "pipeline contract source_revision_param must name a declared parameter",
                )
            })?;
        if !parameter.required || parameter.value_type != "scalar" {
            return Err(ApiError::bad_request(
                "pipeline contract source_revision_param must name a required scalar parameter",
            ));
        }
    }
    Ok(())
}

pub(in crate::app) fn execution_matches_pipeline_contract(
    execution: &TektonExecutionSpec,
    stored: &StoredPipelineContract,
    immutable_source_revision: Option<&str>,
) -> Result<(), ApiError> {
    let contract = pipeline_contract_spec(&stored.contract_json)?;
    validate_pipeline_contract_spec(&contract)?;
    for parameter in &contract.params {
        let value = execution.params.get(&parameter.name);
        if parameter.required && value.is_none() {
            return Err(ApiError::bad_request(format!(
                "PipelineIntent is missing required pipeline parameter {}",
                parameter.name
            )));
        }
        if let Some(value) = value {
            let matches = match parameter.value_type.as_str() {
                "scalar" => !value.is_array() && !value.is_object() && !value.is_null(),
                "array" => value.is_array(),
                _ => false,
            };
            if !matches {
                return Err(ApiError::bad_request(format!(
                    "PipelineIntent parameter {} does not match contract type {}",
                    parameter.name, parameter.value_type
                )));
            }
        }
    }
    if let Some(parameter) = execution
        .params
        .keys()
        .find(|name| !contract.params.iter().any(|allowed| allowed.name == **name))
    {
        return Err(ApiError::bad_request(format!(
            "PipelineIntent parameter {parameter} is not declared by the active PipelineContract"
        )));
    }
    for workspace in &contract.workspaces {
        let supplied = execution
            .workspaces
            .iter()
            .find(|candidate| candidate.name == workspace.name);
        if workspace.required && supplied.is_none() {
            return Err(ApiError::bad_request(format!(
                "PipelineIntent is missing required pipeline workspace {}",
                workspace.name
            )));
        }
        if let Some(supplied) = supplied {
            let binding = if supplied.persistent_volume_claim.is_some() {
                "persistent_volume_claim"
            } else {
                "volume_claim_template"
            };
            if binding != workspace.binding {
                return Err(ApiError::bad_request(format!(
                    "PipelineIntent workspace {} requires {} binding",
                    workspace.name, workspace.binding
                )));
            }
        }
    }
    if let Some(workspace) = execution.workspaces.iter().find(|workspace| {
        !contract
            .workspaces
            .iter()
            .any(|allowed| allowed.name == workspace.name)
    }) {
        return Err(ApiError::bad_request(format!(
            "PipelineIntent workspace {} is not declared by the active PipelineContract",
            workspace.name
        )));
    }
    if let Some(immutable_source_revision) = immutable_source_revision {
        let source_revision_param = contract.source_revision_param.as_deref().ok_or_else(|| {
            ApiError::conflict(
                "WorkItem PipelineIntent requires an active PipelineContract with source_revision_param",
            )
        })?;
        if execution.params.get(source_revision_param) != Some(&json!(immutable_source_revision)) {
            return Err(ApiError::conflict(format!(
                "WorkItem PipelineIntent parameter {source_revision_param} must equal the observed merged commit"
            )));
        }
    }
    Ok(())
}

pub(in crate::app) fn immutable_pipeline_source_revision(
    intent: &StoredPipelineIntent,
    work_item_delivery: bool,
) -> Result<Option<String>, ApiError> {
    if !work_item_delivery {
        return Ok(None);
    }
    let provenance = intent
        .intent_json
        .get("source_provenance")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ApiError::conflict(
                "WorkItem PipelineIntent requires immutable Git merge provenance before execution",
            )
        })?;
    if provenance.get("kind").and_then(Value::as_str) != Some("github_merged_pull_request")
        || provenance.get("immutable").and_then(Value::as_bool) != Some(true)
    {
        return Err(ApiError::conflict(
            "WorkItem PipelineIntent source provenance must be an observed immutable GitHub merge",
        ));
    }
    let revision = required_json_string(provenance, "merge_commit_sha", "source provenance")?;
    if !is_git_sha(&revision) {
        return Err(ApiError::conflict(
            "WorkItem PipelineIntent source provenance has an invalid merge commit",
        ));
    }
    Ok(Some(revision))
}

pub(in crate::app) fn validate_tekton_execution_spec(
    execution: &TektonExecutionSpec,
) -> Result<(), ApiError> {
    validate_kubernetes_name("execution.namespace", &execution.namespace)?;
    validate_kubernetes_name("execution.pipeline_ref", &execution.pipeline_ref)?;
    for (name, value) in &execution.params {
        validate_kubernetes_name("execution.params key", name)?;
        if !(value.is_string() || value.is_number() || value.is_boolean() || value.is_array()) {
            return Err(ApiError::bad_request(
                "execution.params values must be scalar or arrays",
            ));
        }
    }
    for workspace in &execution.workspaces {
        validate_kubernetes_name("execution.workspaces.name", &workspace.name)?;
        match (&workspace.persistent_volume_claim, &workspace.volume_claim_template) {
            (Some(pvc), None) => validate_kubernetes_name("execution.workspaces.persistent_volume_claim", pvc)?,
            (None, Some(template)) => {
                if template.storage.trim().is_empty() {
                    return Err(ApiError::bad_request("execution.workspaces.volume_claim_template.storage is required"));
                }
                if template.access_modes.is_empty() || template.access_modes.iter().any(|mode| mode != "ReadWriteOnce") {
                    return Err(ApiError::bad_request("execution workspaces support only ReadWriteOnce volume claim templates"));
                }
            }
            _ => return Err(ApiError::bad_request("each execution workspace requires exactly one persistent_volume_claim or volume_claim_template")),
        }
    }
    Ok(())
}

pub(in crate::app) fn validate_kubernetes_name(field: &str, value: &str) -> Result<(), ApiError> {
    let valid = !value.is_empty()
        && value.len() <= 63
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-');
    if valid {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!(
            "{field} must be a DNS label"
        )))
    }
}

pub(in crate::app) fn build_pipeline_run_manifest(
    intent: &StoredPipelineIntent,
    execution: &TektonExecutionSpec,
) -> Result<Value, ApiError> {
    let intent_label = dns_label_fragment(&intent.id);
    let change_set_label = dns_label_fragment(&intent.change_set_id);
    let execution_attempt = pipeline_execution_attempt(&intent.intent_json)?;
    let name = if execution_attempt == 1 {
        format!("pharness-{intent_label}")
    } else {
        format!("pharness-{intent_label}-{execution_attempt}")
    };
    let params = execution
        .params
        .iter()
        .map(|(name, value)| json!({ "name": name, "value": value }))
        .collect::<Vec<_>>();
    let workspaces = execution
        .workspaces
        .iter()
        .map(|workspace| {
            let mut value = Map::new();
            value.insert("name".to_string(), json!(workspace.name));
            if let Some(pvc) = &workspace.persistent_volume_claim {
                value.insert(
                    "persistentVolumeClaim".to_string(),
                    json!({ "claimName": pvc }),
                );
            }
            if let Some(template) = &workspace.volume_claim_template {
                value.insert(
                    "volumeClaimTemplate".to_string(),
                    json!({
                        "spec": {
                            "accessModes": template.access_modes,
                            "resources": { "requests": { "storage": template.storage } },
                        }
                    }),
                );
            }
            Value::Object(value)
        })
        .collect::<Vec<_>>();
    let mut manifest = json!({
        "apiVersion": "tekton.dev/v1",
        "kind": "PipelineRun",
        "metadata": {
            "name": name,
            "namespace": execution.namespace,
            "labels": {
                "app.kubernetes.io/part-of": "pharness",
                "pharness.lucas.engineering/pipeline-intent": intent_label,
                "pharness.lucas.engineering/change-set": change_set_label,
            },
        },
        "spec": {
            "pipelineRef": { "name": execution.pipeline_ref },
            "params": params,
            "workspaces": workspaces,
        },
    });
    if let Some(merge_commit_sha) = intent
        .intent_json
        .pointer("/source_provenance/merge_commit_sha")
        .and_then(Value::as_str)
        .filter(|value| is_git_sha(value))
    {
        manifest["metadata"]["annotations"] = json!({
            "pharness.lucas.engineering/source-commit": merge_commit_sha,
        });
    }
    Ok(manifest)
}

pub(in crate::app) fn dns_label_fragment(value: &str) -> String {
    let normalized = value.replace('_', "-").to_ascii_lowercase();
    normalized.chars().take(50).collect()
}

pub(in crate::app) fn set_pipeline_execution_state(
    intent_json: &mut Value,
    execution_state: Value,
) {
    if let Some(object) = intent_json.as_object_mut() {
        object.insert("execution_state".to_string(), execution_state);
    }
}

pub(in crate::app) fn merge_pipeline_execution_state(intent_json: &mut Value, update: Value) {
    let Some(intent) = intent_json.as_object_mut() else {
        return;
    };
    let mut execution_state = intent
        .get("execution_state")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if let Some(update) = update.as_object() {
        for (key, value) in update {
            execution_state.insert(key.clone(), value.clone());
        }
    }
    intent.insert(
        "execution_state".to_string(),
        Value::Object(execution_state),
    );
}

pub(in crate::app) fn set_pipeline_execution_evidence(intent_json: &mut Value, evidence: Value) {
    if let Some(intent) = intent_json.as_object_mut() {
        intent.insert("execution_evidence".to_string(), evidence);
    }
}

pub(in crate::app) fn set_pipeline_build_output(
    intent_json: &mut Value,
    artifact: &ArtifactResponse,
) {
    let content = artifact.content_json.as_ref();
    if let Some(intent) = intent_json.as_object_mut() {
        intent.insert(
            "build_output".to_string(),
            json!({
                "artifact_id": artifact.id,
                "status": content.and_then(|value| value.get("status")),
                "image_ref": content.and_then(|value| value.pointer("/image/reference")),
                "image_digest": content.and_then(|value| value.pointer("/image/digest")),
                "source_commit": content.and_then(|value| value.pointer("/source/commit")),
            }),
        );
    }
}

pub(in crate::app) async fn persist_pipeline_execution_evidence(
    store: &SqliteStore,
    intent: &StoredPipelineIntent,
    outcome: &PipelineIntentExecutionOutcomeRequest,
    state_name: &str,
) -> Result<Value, ApiError> {
    let artifact_id = format!("art_pipeline_execution_{}", outcome.execution_id);
    let observation_id = format!("obs_pipeline_execution_{}", outcome.execution_id);
    let evidence_status = match outcome.status.as_str() {
        "completed" => "succeeded",
        "failed" => "failed",
        _ => {
            return Err(ApiError::internal(
                "terminal execution evidence requires a terminal outcome",
            ))
        }
    };
    let pipeline_run = json!({
        "namespace": outcome.pipeline_run_namespace,
        "name": outcome.pipeline_run_name,
    });
    let error = outcome
        .error
        .as_deref()
        .map(|value| truncate_audit_text(value, 256));
    let content = json!({
        "execution_id": outcome.execution_id,
        "status": evidence_status,
        "state": state_name,
        "pipeline_run": pipeline_run.clone(),
        "error": error.clone(),
    });
    let artifact = match store.get_artifact(&artifact_id).await? {
        Some(existing) => existing,
        None => {
            store
                .create_artifact(CreateArtifact {
                    id: artifact_id.clone(),
                    session_id: intent.session_id.clone(),
                    run_id: intent.run_id.clone(),
                    kind: "tekton_pipeline_run_execution".to_string(),
                    label: format!(
                        "Tekton PipelineRun {evidence_status}: {}",
                        outcome.execution_id
                    ),
                    mime_type: Some("application/json".to_string()),
                    path: None,
                    content_text: None,
                    content_json: Some(content.clone()),
                })
                .await?
        }
    };
    let observation = match store.get_observation(&observation_id).await? {
        Some(existing) => existing,
        None => {
            let namespace = outcome.pipeline_run_namespace.clone();
            let name = outcome.pipeline_run_name.clone();
            store
                .create_observation(CreateObservation {
                    id: observation_id.clone(),
                    session_id: intent.session_id.clone(),
                    run_id: intent.run_id.clone(),
                    source: "tekton".to_string(),
                    kind: "pipeline_run_execution".to_string(),
                    subject: name.clone().unwrap_or_else(|| outcome.execution_id.clone()),
                    summary: format!(
                        "PipelineRun execution {evidence_status} for {}",
                        name.as_deref().unwrap_or(&outcome.execution_id)
                    ),
                    resource_namespace: namespace.clone(),
                    resource_kind: Some("PipelineRun".to_string()),
                    resource_name: name.clone(),
                    resource_ref_json: Some(json!({
                        "apiVersion": "tekton.dev/v1",
                        "kind": "PipelineRun",
                        "namespace": namespace,
                        "name": name,
                    })),
                    artifact_id: Some(artifact.id.clone()),
                    data_json: json!({ "execution": content }),
                })
                .await?
        }
    };

    Ok(json!({
        "status": evidence_status,
        "source": "executor",
        "execution_id": outcome.execution_id,
        "artifact_id": artifact.id,
        "observation_id": observation.id,
        "pipeline_run": pipeline_run,
        "error": error,
    }))
}

#[derive(Debug, Clone)]
pub(in crate::app) struct PipelineBuildOutput {
    pub(in crate::app) image_url: String,
    pub(in crate::app) image_digest: String,
    pub(in crate::app) image_reference: String,
    pub(in crate::app) source_commit: Option<String>,
    pub(in crate::app) status: &'static str,
    pub(in crate::app) reason: Option<&'static str>,
}

/// Persist only compact, digest-pinned output that the terminal PipelineRun
/// reported. This is build provenance, not a registry inspection or a trust
/// assertion about signatures, SBOMs, or vulnerabilities.
pub(in crate::app) async fn persist_pipeline_build_output(
    store: &SqliteStore,
    intent: &StoredPipelineIntent,
    outcome: &PipelineIntentExecutionOutcomeRequest,
    analysis: &Value,
) -> Result<Option<ArtifactResponse>, ApiError> {
    let Some(output) = pipeline_build_output_from_analysis(intent, analysis) else {
        return Ok(None);
    };
    let artifact_id = format!("art_pipeline_build_output_{}", outcome.execution_id);
    if let Some(existing) = store.get_artifact(&artifact_id).await? {
        return Ok(Some(existing.into()));
    }
    let artifact = store
        .create_artifact(CreateArtifact {
            id: artifact_id,
            session_id: intent.session_id.clone(),
            run_id: intent.run_id.clone(),
            kind: "pipeline_build_output".to_string(),
            label: format!("Digest-pinned build output for PipelineIntent {}", intent.id),
            mime_type: Some("application/json".to_string()),
            path: None,
            content_text: None,
            content_json: Some(json!({
                "version": 1,
                "status": output.status,
                "reason": output.reason,
                "pipeline_intent_id": intent.id,
                "execution_id": outcome.execution_id,
                "pipeline_run": {
                    "namespace": outcome.pipeline_run_namespace,
                    "name": outcome.pipeline_run_name,
                },
                "image": {
                    "url": output.image_url,
                    "digest": output.image_digest,
                    "reference": output.image_reference,
                },
                "source": {
                    "commit": output.source_commit,
                    "expected_merge_commit": intent.intent_json.pointer("/source_provenance/merge_commit_sha"),
                },
            })),
        })
        .await?;
    Ok(Some(artifact.into()))
}

pub(in crate::app) fn pipeline_build_output_from_analysis(
    intent: &StoredPipelineIntent,
    analysis: &Value,
) -> Option<PipelineBuildOutput> {
    let image_url = analysis
        .pointer("/outputs/image_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| safe_oci_image_component(value))?
        .to_string();
    let image_digest = analysis
        .pointer("/outputs/image_digest")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| is_sha256_digest(value))?
        .to_string();
    let image_reference = match image_url.split_once('@') {
        Some((repository, embedded_digest))
            if safe_oci_image_component(repository) && embedded_digest == image_digest =>
        {
            format!("{repository}@{image_digest}")
        }
        Some(_) => return None,
        None => format!("{image_url}@{image_digest}"),
    };
    let source_commit = analysis
        .pointer("/outputs/commit")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| is_git_sha(value))
        .map(ToOwned::to_owned);
    let expected_merge = intent
        .intent_json
        .pointer("/source_provenance/merge_commit_sha")
        .and_then(Value::as_str)
        .filter(|value| is_git_sha(value));
    let (status, reason) = match expected_merge {
        Some(expected) if source_commit.as_deref() == Some(expected) => ("verified", None),
        Some(_) => ("untrusted", Some("source_commit_mismatch")),
        None => ("verified", None),
    };
    Some(PipelineBuildOutput {
        image_url,
        image_digest,
        image_reference,
        source_commit,
        status,
        reason,
    })
}

pub(in crate::app) fn safe_oci_image_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && !value.contains(['\0', '\r', '\n', ' ', '\t'])
        && !value.contains("://")
}

pub(in crate::app) fn is_sha256_digest(value: &str) -> bool {
    value.len() == "sha256:".len() + 64
        && value.starts_with("sha256:")
        && value["sha256:".len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
}

pub(in crate::app) async fn persist_pipeline_run_analysis(
    store: &SqliteStore,
    intent: &StoredPipelineIntent,
    outcome: &PipelineIntentExecutionOutcomeRequest,
    analysis: &Value,
) -> Result<StoredObservation, ApiError> {
    validate_terminal_pipeline_run_analysis(outcome, analysis)?;

    let artifact_id = format!("art_pipeline_analysis_{}", outcome.execution_id);
    let observation_id = format!("obs_pipeline_analysis_{}", outcome.execution_id);
    let namespace = outcome.pipeline_run_namespace.clone();
    let name = outcome.pipeline_run_name.clone();
    let content = json!({
        "source": "tekton",
        "resource": "pipeline_run_analysis",
        "namespace": namespace,
        "name": name,
        "analysis": analysis,
    });
    let artifact = match store.get_artifact(&artifact_id).await? {
        Some(existing) => existing,
        None => {
            store
                .create_artifact(CreateArtifact {
                    id: artifact_id,
                    session_id: intent.session_id.clone(),
                    run_id: intent.run_id.clone(),
                    kind: "pipeline_run_analysis".to_string(),
                    label: format!(
                        "PipelineRunAnalysis: {}",
                        name.as_deref().unwrap_or(&outcome.execution_id)
                    ),
                    mime_type: Some("application/json".to_string()),
                    path: None,
                    content_text: None,
                    content_json: Some(content),
                })
                .await?
        }
    };
    match store.get_observation(&observation_id).await? {
        Some(existing) => Ok(existing),
        None => Ok(store
            .create_observation(CreateObservation {
                id: observation_id,
                session_id: intent.session_id.clone(),
                run_id: intent.run_id.clone(),
                source: "tekton".to_string(),
                kind: "pipeline_run_analysis".to_string(),
                subject: name.clone().unwrap_or_else(|| outcome.execution_id.clone()),
                summary: format!(
                    "Terminal PipelineRunAnalysis for {}",
                    name.as_deref().unwrap_or(&outcome.execution_id)
                ),
                resource_namespace: namespace.clone(),
                resource_kind: Some("PipelineRun".to_string()),
                resource_name: name.clone(),
                resource_ref_json: Some(json!({
                    "apiVersion": "tekton.dev/v1",
                    "kind": "PipelineRun",
                    "namespace": namespace,
                    "name": name,
                })),
                artifact_id: Some(artifact.id),
                data_json: json!({ "analysis": analysis }),
            })
            .await?),
    }
}

pub(in crate::app) fn validate_terminal_pipeline_run_analysis(
    outcome: &PipelineIntentExecutionOutcomeRequest,
    analysis: &Value,
) -> Result<(), ApiError> {
    if analysis.get("kind").and_then(Value::as_str) != Some("PipelineRunAnalysis") {
        return Err(ApiError::bad_request(
            "terminal execution analysis must be a PipelineRunAnalysis",
        ));
    }
    if let Some(namespace) = outcome.pipeline_run_namespace.as_deref() {
        if analysis
            .pointer("/pipeline_run/namespace")
            .and_then(Value::as_str)
            != Some(namespace)
        {
            return Err(ApiError::bad_request(
                "terminal execution analysis must match the PipelineRun namespace",
            ));
        }
    }
    if let Some(name) = outcome.pipeline_run_name.as_deref() {
        if analysis
            .pointer("/pipeline_run/name")
            .and_then(Value::as_str)
            != Some(name)
        {
            return Err(ApiError::bad_request(
                "terminal execution analysis must match the PipelineRun name",
            ));
        }
    }
    let observed_status = analysis.pointer("/summary/status").and_then(Value::as_str);
    let status_matches = match outcome.status.as_str() {
        "completed" => observed_status == Some("succeeded"),
        // Tekton reports a cancelled PipelineRun separately, but both terminal
        // states are an unsuccessful execution from the delivery controller's
        // perspective and must retain the same bounded failure path.
        "failed" => matches!(observed_status, Some("failed" | "cancelled")),
        _ => {
            return Err(ApiError::bad_request(
                "terminal execution analysis requires a completed or failed outcome",
            ))
        }
    };
    if !status_matches {
        return Err(ApiError::bad_request(
            "terminal execution analysis status must match the executor outcome",
        ));
    }

    Ok(())
}

pub(in crate::app) fn pipeline_run_name(manifest: &Value) -> Option<&str> {
    manifest.pointer("/metadata/name").and_then(Value::as_str)
}

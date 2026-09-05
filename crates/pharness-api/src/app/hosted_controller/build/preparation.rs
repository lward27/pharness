use super::{deadline, now, ApiError, AppState, ACTION};
use crate::app::hashing::canonical_material_hash as hash;
use crate::app::pipeline::{execution, hosted, intents};
use crate::dispatch::TektonExecutionRequest;
use axum::extract::{Path, State};
use axum::Json;
use pharness_core::hosted_sdlc::build as identity;
use pharness_store::{
    CreatePermissionGrant, CreateStageExecution, StoredPipelineIntent, StoredWorkflowOperation,
    StoredWorkflowReconciliation, UpdatePipelineIntentExecution,
};
use serde_json::json;

pub(super) async fn prepare(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    operation: &StoredWorkflowOperation,
) -> Result<StoredWorkflowOperation, ApiError> {
    let metadata = state
        .store
        .get_repo_work_item_metadata(&claim.work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("hosted build workflow is unavailable"))?;
    let policy = metadata
        .workflow_policy
        .as_ref()
        .ok_or_else(|| ApiError::conflict("source-only work has no hosted build authority"))?;
    let request = json!({"pipeline_contract_id":policy.delivery_binding.pipeline_contract_id,
        "actor":"controller:hosted-workflow","reason":"Build the sealed autonomous merge under the saved workflow",
        "intent_json":{"hosted_build":{"operation_id":operation.id,"workflow_policy_hash":metadata.workflow_policy_hash},
            "execution":{"enabled":true,"namespace":"tekton-pipelines","pipeline_ref":policy.pipeline_contract["pipeline_ref"],"production_impacting":false,
            "params":{"revision":super::source(state,&claim.work_item_id).await?["merge_commit_sha"],"dockerfile":"./Dockerfile","context":"./"},
            "workspaces":[{"name":"shared-data","volume_claim_template":{"storage":"1Gi","access_modes":["ReadWriteOnce"]}}]}}});
    let Json(created) = intents::create_work_item_pipeline_intent(
        State(state.clone()),
        None,
        Path(claim.work_item_id.clone()),
        Json(
            serde_json::from_value(request)
                .map_err(|_| ApiError::internal("invalid finite build definition"))?,
        ),
    )
    .await?;
    let mut intent = state
        .store
        .get_pipeline_intent(&created.pipeline_intent.id)
        .await?
        .unwrap();
    hosted::validate_intent(state, &intent).await?;
    if intent.intent_json["hosted_build"]
        != json!({"operation_id":operation.id,"workflow_policy_hash":metadata.workflow_policy_hash})
    {
        return Err(ApiError::conflict(
            "existing build intent belongs to different authority; it cannot be rebound",
        ));
    }
    if intent.status == "proposed" {
        let Json(_) = intents::transition_pipeline_intent(State(state.clone()),Path(intent.id.clone()),Json(serde_json::from_value(json!({"target_status":"approved","actor":"controller:hosted-workflow","reason":format!("Saved workflow {} authorizes this finite non-deploying build",metadata.workflow_policy_hash.as_deref().unwrap_or_default())})).unwrap())).await?;
        intent = state.store.get_pipeline_intent(&intent.id).await?.unwrap();
    }
    if intent.status != "approved" || intent.intent_json.get("execution_state").is_some() {
        return Err(ApiError::conflict("build preparation found an existing execution without its recorded dispatch; intervention is required"));
    }
    let grant_id = ensure_grant(state, operation, &intent).await?;
    let preflight = execution::pipeline_intent_execution_preflight(state, &intent.id).await?;
    if !preflight.ready {
        return Err(ApiError::conflict(
            "saved hosted build did not pass the existing pipeline execution checks",
        ));
    }
    let execution_id = format!(
        "pexec_build_{}",
        operation.id.trim_start_matches("workflowop_")
    );
    let mut manifest = preflight
        .manifest
        .ok_or_else(|| ApiError::internal("ready hosted build has no PipelineRun manifest"))?;
    manifest["metadata"]["annotations"][identity::OPERATION] = json!(operation.id);
    manifest["metadata"]["annotations"][identity::EXECUTION] = json!(execution_id);
    manifest["metadata"]["annotations"][identity::DEADLINE] =
        json!(deadline(operation).to_string());
    identity::validate_manifest(&manifest).map_err(ApiError::conflict)?;
    let request = TektonExecutionRequest {
        pipeline_intent_id: intent.id.clone(),
        execution_id: execution_id.clone(),
        target_namespace: "tekton-pipelines".into(),
        pipeline_run_manifest: manifest.clone(),
    };
    let stage_id = format!(
        "stage_build_{}",
        operation.id.trim_start_matches("workflowop_")
    );
    let input = json!({"workflow_operation_id":operation.id,"pipeline_intent_id":intent.id,"source_provenance":intent.intent_json["source_provenance"],"workflow_policy_hash":metadata.workflow_policy_hash});
    if let Some(existing) = state.store.get_stage_execution(&stage_id).await? {
        if existing.work_item_id != claim.work_item_id
            || existing.stage_key != "release"
            || existing.input_snapshot != input
            || existing.input_hash != hash(&input)?
        {
            return Err(ApiError::conflict(
                "the recorded release stage differs from this build operation",
            ));
        }
    } else {
        state
            .store
            .create_stage_execution(CreateStageExecution {
                id: stage_id.clone(),
                work_item_id: claim.work_item_id.clone(),
                stage_key: "release".into(),
                sequence: 1,
                status: "running".into(),
                agent_profile_id: None,
                agent_profile_version: None,
                agent_profile_hash: None,
                context_pack_id: None,
                run_id: None,
                workspace_id: None,
                input_snapshot: input.clone(),
                input_hash: hash(&input)?,
            })
            .await?;
    }
    let mut refs = operation.resource_refs.clone();
    refs["pipeline_intent_id"] = json!(intent.id);
    refs["release_stage_execution_id"] = json!(stage_id);
    refs["build_dispatch"] = json!({"execution_id":execution_id,"manifest_hash":hash(&manifest)?,"pipeline_run_manifest":manifest,"permission_grant_id":grant_id,
        "executor_job_manifest":state.worker.hosted_build_job_manifest(&request,false).map_err(|e|ApiError::conflict(e.to_string()))?,
        "observer_job_manifest":state.worker.hosted_build_job_manifest(&request,true).map_err(|e|ApiError::conflict(e.to_string()))?,
        "deadline_ms":deadline(operation)});
    state
        .store
        .record_workflow_operation(
            claim,
            &operation.id,
            "running",
            &refs,
            "Original build, worker images and observer identities recorded before dispatch",
            now(),
        )
        .await
        .map_err(Into::into)
}

async fn ensure_grant(
    state: &AppState,
    operation: &StoredWorkflowOperation,
    intent: &StoredPipelineIntent,
) -> Result<String, ApiError> {
    let item = state
        .store
        .get_work_item(&operation.work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("hosted build WorkItem is unavailable"))?;
    let id = format!(
        "pgrant_build_{}",
        operation.id.trim_start_matches("workflowop_")
    );
    // Preserve the existing production-workflow grant ceiling. Retries use the
    // original creation time and cannot renew or replace a revoked grant.
    let expiry = operation
        .created_at
        .saturating_add(30 * 60 * 1000)
        .min(deadline(operation));
    let scope = json!({"environment":item.target_environment,"capability_kinds":["tekton_start_run"],"actions":["tekton_trigger_pipeline"],"max_risk":"high","namespaces":["tekton-pipelines"],"work_plan_ids":[intent.work_plan_id],"change_set_ids":[intent.change_set_id],"pipeline_intent_ids":[intent.id],"production_impacting":false});
    let grant_policy = json!({"policy_mode":"supervised_autonomy"});
    if let Some(grant) = state.store.get_permission_grant(&id).await? {
        if grant.status != "active"
            || grant.subject != state.policy.subject
            || grant.scope_json != scope
            || grant.policy_json != grant_policy
            || grant.expires_at.as_deref() != Some(&expiry.to_string())
            || !crate::app::approvals::grant_is_unexpired(&grant, now() as u128)
        {
            return Err(ApiError::conflict("original hosted build grant expired, was revoked, or changed; no replacement authority was issued"));
        }
    } else {
        if now() >= expiry {
            return Err(ApiError::conflict(
                "the original hosted build authorization window expired",
            ));
        }
        let reason = format!(
            "Saved hosted workflow permits the non-deploying build operation {}",
            operation.id
        );
        crate::app::approvals::validate_permission_grant_request(&serde_json::from_value(json!({"subject":state.policy.subject,"reason":reason,"scope":scope,"policy":grant_policy,"expires_at":expiry.to_string()})).unwrap())?;
        let grant = state
            .store
            .create_permission_grant(CreatePermissionGrant {
                id: id.clone(),
                subject: state.policy.subject.clone(),
                reason,
                scope_json: scope,
                policy_json: grant_policy,
                expires_at: Some(expiry.to_string()),
            })
            .await?;
        crate::app::approvals::append_permission_grant_audit_event(
            &state.store,
            "permission_grant.created",
            &grant,
            Some("controller:hosted-workflow".into()),
        )
        .await?;
    }
    Ok(id)
}

pub(super) async fn mark_executing(
    state: &AppState,
    operation: &StoredWorkflowOperation,
) -> Result<StoredPipelineIntent, ApiError> {
    let id = operation.resource_refs["pipeline_intent_id"]
        .as_str()
        .ok_or_else(|| ApiError::conflict("recorded build intent is unavailable"))?;
    let mut intent = state
        .store
        .get_pipeline_intent(id)
        .await?
        .ok_or_else(|| ApiError::conflict("recorded build intent is unavailable"))?;
    let dispatch = &operation.resource_refs["build_dispatch"];
    let expected = json!({"execution_id":dispatch["execution_id"],"state":"dispatching","pipeline_run_namespace":"tekton-pipelines","pipeline_run_name":dispatch["pipeline_run_manifest"]["metadata"]["name"],"permission_grant_id":dispatch["permission_grant_id"],"hosted_operation_id":operation.id});
    if intent.intent_json.get("execution_state").is_none() {
        hosted::validate_intent(state, &intent).await?;
        ensure_grant(state, operation, &intent).await?;
        if intent.status != "approved" {
            return Err(ApiError::conflict(
                "hosted build intent is no longer approved",
            ));
        }
        let mut body = intent.intent_json.clone();
        execution::set_pipeline_execution_state(&mut body, expected);
        intent = state
            .store
            .update_pipeline_intent_execution(
                id,
                UpdatePipelineIntentExecution {
                    status: "executing".into(),
                    intent_json: body,
                    actor: Some("controller:hosted-workflow".into()),
                    reason: Some("Original build identity persisted before Job creation".into()),
                },
            )
            .await?;
    } else if intent.intent_json["execution_state"]["execution_id"] != dispatch["execution_id"]
        || intent.intent_json["execution_state"]["hosted_operation_id"] != operation.id
    {
        return Err(ApiError::conflict(
            "hosted build execution identity changed",
        ));
    }
    Ok(intent)
}

pub(super) async fn saved(
    state: &AppState,
    intent_id: &str,
    execution_id: &str,
    for_write: bool,
) -> Result<(StoredPipelineIntent, StoredWorkflowOperation), ApiError> {
    let intent = state
        .store
        .get_pipeline_intent(intent_id)
        .await?
        .ok_or_else(|| ApiError::conflict("hosted build intent is unavailable"))?;
    let id = intent.intent_json["hosted_build"]["operation_id"]
        .as_str()
        .ok_or_else(|| ApiError::conflict("pipeline intent has no hosted build operation"))?;
    let op = state
        .store
        .get_workflow_operation(id)
        .await?
        .ok_or_else(|| ApiError::conflict("hosted build operation is unavailable"))?;
    let d = &op.resource_refs["build_dispatch"];
    if op.action != ACTION
        || op.resource_refs["pipeline_intent_id"] != intent.id
        || d["execution_id"] != execution_id
        || d["manifest_hash"] != hash(&d["pipeline_run_manifest"])?
        || d["deadline_ms"] != deadline(&op)
        || intent.intent_json["execution_state"]["execution_id"] != execution_id
        || intent.intent_json["execution_state"]["hosted_operation_id"] != op.id
    {
        return Err(ApiError::conflict(
            "hosted build callback does not match its original execution",
        ));
    }
    identity::validate_manifest(&d["pipeline_run_manifest"]).map_err(ApiError::conflict)?;
    if for_write {
        hosted::validate_intent(state, &intent).await?;
        ensure_grant(state, &op, &intent).await?;
        let source = &intent.intent_json["source_provenance"];
        if op.input_hash
            != hash(
                &json!({"change_set_id":intent.change_set_id,"source_provenance":source,"workflow_policy_hash":intent.intent_json["hosted_build"]["workflow_policy_hash"]}),
            )?
        {
            return Err(ApiError::conflict(
                "hosted build source differs from its original operation",
            ));
        }
        let spec = execution::tekton_execution_spec(&intent.intent_json)?;
        let mut current = execution::build_pipeline_run_manifest(&intent, &spec)?;
        current["metadata"]["annotations"][identity::OPERATION] = json!(op.id);
        current["metadata"]["annotations"][identity::EXECUTION] = json!(execution_id);
        current["metadata"]["annotations"][identity::DEADLINE] = json!(deadline(&op).to_string());
        if current != d["pipeline_run_manifest"] {
            return Err(ApiError::conflict(
                "hosted PipelineRun definition changed after preparation",
            ));
        }
        if op.status != "running" || intent.status != "executing" || now() >= deadline(&op) {
            return Err(ApiError::conflict(
                "hosted build is not active within its original execution window",
            ));
        }
    }
    Ok((intent, op))
}

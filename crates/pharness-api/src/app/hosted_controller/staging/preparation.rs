use super::{artifact_id, deadline, evidence, hash, now, required, ApiError, AppState, ACTION};
use pharness_core::hosted_sdlc::staging::{
    HostedStagingAuthority, StagingGitOpsPlan, AUTHORITY_SCHEMA,
};
use pharness_store::{
    CreateDeploymentIntent, DeliveryStage, StoredDeploymentIntent, StoredPipelineIntent,
    StoredWorkflowOperation, StoredWorkflowReconciliation,
};
use serde_json::json;

pub(super) struct Saved {
    pub operation: StoredWorkflowOperation,
    pub deployment: StoredDeploymentIntent,
    pub pipeline: StoredPipelineIntent,
    pub authority: HostedStagingAuthority,
    pub plan: Option<StagingGitOpsPlan>,
}

pub(super) async fn prepare(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    operation: &StoredWorkflowOperation,
) -> Result<StoredWorkflowOperation, ApiError> {
    let (pipeline, metadata, evidence_hash) = evidence::build(state, &claim.work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("staging requires a verified autonomous build"))?;
    let policy = metadata.workflow_policy.as_ref().unwrap();
    let suffix = operation.id.trim_start_matches("workflowop_");
    let authority = HostedStagingAuthority {
        schema_version: AUTHORITY_SCHEMA.into(),
        work_item_id: claim.work_item_id.clone(),
        operation_id: operation.id.clone(),
        execution_id: format!("sexec_{suffix}"),
        pipeline_intent_id: pipeline.id.clone(),
        deployment_intent_id: format!("dintent_staging_{suffix}"),
        gitops_change_set_id: format!("gcs_staging_{suffix}"),
        workflow_policy_hash: metadata.workflow_policy_hash.clone().unwrap(),
        build_evidence_hash: evidence_hash,
        application_repository: policy.delivery_binding.source_repo.clone(),
        source_commit_sha: required(
            &pipeline.intent_json["source_provenance"],
            "merge_commit_sha",
        )?
        .into(),
        image_digest: required(&pipeline.intent_json["build_output"], "image_digest")?.into(),
        created_at_ms: operation.created_at,
        expires_at_ms: deadline(operation),
    };
    authority
        .validate_for_dispatch(now())
        .map_err(ApiError::conflict)?;
    // Render both immutable manifests before creating any durable intent. A
    // missing reader or writer configuration cannot leave a partial authority.
    let executor = state
        .worker
        .hosted_staging_job_manifest(&authority, false)
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    let observer = state
        .worker
        .hosted_staging_job_manifest(&authority, true)
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    let intent_json = json!({"hosted_staging":{"authority":authority,"authority_hash":authority.material_hash().map_err(ApiError::conflict)?},"deployment_contract":policy.staging_contract,"source_provenance":pipeline.intent_json["source_provenance"],"build_output":pipeline.intent_json["build_output"]});
    if let Some(existing) = state
        .store
        .get_deployment_intent_for_stage(&pipeline.id, DeliveryStage::Staging)
        .await?
    {
        if existing.id != authority.deployment_intent_id || existing.intent_json != intent_json {
            return Err(ApiError::conflict(
                "an existing staging intent cannot be rebound to different authority",
            ));
        }
    } else {
        state.store.create_deployment_intent_for_stage(CreateDeploymentIntent {
            id: authority.deployment_intent_id.clone(), pipeline_intent_id: pipeline.id.clone(), change_set_id: pipeline.change_set_id.clone(), work_plan_id: pipeline.work_plan_id.clone(), remediation_plan_id: pipeline.remediation_plan_id.clone(), incident_id: pipeline.incident_id.clone(), session_id: pipeline.session_id.clone(), run_id: pipeline.run_id.clone(), status: "approved".into(), title: "Deploy the verified image to Finance staging".into(), summary: "Saved workflow authorizes one digest change; Argo and runtime verification remain required".into(), risk_level: "medium".into(), intent_kind: "gitops".into(), target_environment: Some("staging".into()), target_namespace: Some("apps-staging".into()), argo_application: Some(required(&policy.staging_contract, "argo_application")?.into()), resource_namespace: Some("apps-staging".into()), resource_kind: Some("Deployment".into()), resource_name: Some(required(&policy.staging_contract["contract_json"], "workload_name")?.into()), intent_json,
        }, DeliveryStage::Staging).await?;
    }
    let mut refs = operation.resource_refs.clone();
    refs["staging_dispatch"] = json!({"authority":authority,"authority_hash":authority.material_hash().map_err(ApiError::conflict)?,"executor_job_manifest":executor,"observer_job_manifest":observer,"deadline_ms":deadline(operation)});
    state
        .store
        .record_workflow_operation(
            claim,
            &operation.id,
            "running",
            &refs,
            "Original staging authority and writer/reader identities persisted before Job creation",
            now(),
        )
        .await
        .map_err(Into::into)
}

pub(super) async fn saved(
    state: &AppState,
    deployment_id: &str,
    execution: &str,
) -> Result<Saved, ApiError> {
    let deployment = state
        .store
        .get_deployment_intent(deployment_id)
        .await?
        .ok_or_else(|| ApiError::conflict("staging deployment is unavailable"))?;
    let authority: HostedStagingAuthority =
        serde_json::from_value(deployment.intent_json["hosted_staging"]["authority"].clone())
            .map_err(|_| ApiError::conflict("staging deployment has no valid hosted authority"))?;
    authority.validate_identity().map_err(ApiError::conflict)?;
    let operation = state
        .store
        .get_workflow_operation(&authority.operation_id)
        .await?
        .ok_or_else(|| ApiError::conflict("staging operation is unavailable"))?;
    let pipeline = state
        .store
        .get_pipeline_intent(&authority.pipeline_intent_id)
        .await?
        .ok_or_else(|| ApiError::conflict("staging build is unavailable"))?;
    let d = &operation.resource_refs["staging_dispatch"];
    let authority_hash = authority.material_hash().map_err(ApiError::conflict)?;
    if authority.execution_id != execution
        || authority.deployment_intent_id != deployment_id
        || authority.work_item_id != operation.work_item_id
        || operation.action != ACTION
        || authority.created_at_ms != operation.created_at
        || authority.expires_at_ms != deadline(&operation)
        || deployment.delivery_stage != DeliveryStage::Staging
        || deployment.pipeline_intent_id != pipeline.id
        || deployment.change_set_id != pipeline.change_set_id
        || deployment.work_plan_id != pipeline.work_plan_id
        || deployment.session_id != pipeline.session_id
        || deployment.run_id != pipeline.run_id
        || deployment.target_namespace.as_deref() != Some("apps-staging")
        || deployment.target_environment.as_deref() != Some("staging")
        || deployment.intent_json["hosted_staging"]["authority_hash"] != authority_hash
        || d["authority"] != json!(authority)
        || d["authority_hash"] != authority_hash
        || d["deadline_ms"] != authority.expires_at_ms
        || operation.input_hash
            != hash(
                &json!({"pipeline_intent_id":pipeline.id,"build_evidence_hash":authority.build_evidence_hash,"workflow_policy_hash":authority.workflow_policy_hash}),
            )?
    {
        return Err(ApiError::conflict(
            "staging callback differs from its original operation and deployment identity",
        ));
    }
    let plan = state
        .store
        .get_artifact(&artifact_id("plan", execution))
        .await?
        .map(|record| {
            if record.kind != "hosted_staging_plan"
                || record.session_id != pipeline.session_id
                || record.run_id != pipeline.run_id
            {
                return Err(ApiError::conflict(
                    "staging plan artifact belongs to different authority",
                ));
            }
            let plan: StagingGitOpsPlan = serde_json::from_value(
                record
                    .content_json
                    .ok_or_else(|| ApiError::conflict("staging plan artifact has no content"))?,
            )
            .map_err(|_| ApiError::conflict("invalid staging plan artifact"))?;
            plan.validate(&authority).map_err(ApiError::conflict)?;
            Ok(plan)
        })
        .transpose()?;
    Ok(Saved {
        operation,
        deployment,
        pipeline,
        authority,
        plan,
    })
}

pub(super) async fn validate_current(
    state: &AppState,
    saved: &Saved,
    for_write: bool,
) -> Result<(), ApiError> {
    let (pipeline, metadata, build_hash) = evidence::build(state, &saved.authority.work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("the original staging build is no longer eligible"))?;
    let policy = metadata.workflow_policy.as_ref().unwrap();
    if pipeline.id != saved.pipeline.id
        || build_hash != saved.authority.build_evidence_hash
        || pipeline.intent_json["source_provenance"]["merge_commit_sha"]
            != saved.authority.source_commit_sha
        || pipeline.intent_json["build_output"]["image_digest"] != saved.authority.image_digest
        || metadata.workflow_policy_hash.as_deref() != Some(&saved.authority.workflow_policy_hash)
        || policy.delivery_binding.source_repo != saved.authority.application_repository
        || saved.deployment.intent_json["deployment_contract"] != policy.staging_contract
        || saved.deployment.argo_application.as_deref()
            != policy.staging_contract["argo_application"].as_str()
        || saved.deployment.resource_name.as_deref()
            != policy.staging_contract["contract_json"]["workload_name"].as_str()
        || saved.deployment.resource_namespace.as_deref() != Some("apps-staging")
        || saved.deployment.resource_kind.as_deref() != Some("Deployment")
    {
        return Err(ApiError::conflict(
            "staging source, build, policy or target changed after preparation",
        ));
    }
    if for_write {
        saved
            .authority
            .validate_for_dispatch(now())
            .map_err(ApiError::conflict)?;
        // Preserve the existing 30-minute authorization ceiling independently
        // of the longer bounded observation window; pause does not renew it.
        if saved.operation.status != "running"
            || saved.deployment.status != "approved"
            || now() >= saved.operation.created_at.saturating_add(30 * 60 * 1000)
            || !evidence::may_advance(state, &saved.authority.work_item_id).await?
        {
            return Err(ApiError::conflict(
                "new staging work is stopped or its original admission window expired",
            ));
        }
    }
    Ok(())
}

//! One read-only preview of the production Kustomization, one native baseline
//! identity read, then the proposal and its approval gate. Nothing here can
//! write to GitOps; the operation's effect is observation.
use super::super::state::{condition, workflow_policy, Condition, Snapshot};
use super::{
    artifact_id, deadline, evidence, hash, now, required, runtime, staging, suffix, ApiError,
    AppState,
};
use pharness_core::hosted_sdlc::production::{
    ProductionGitOpsPlan, ProductionPreview, ProductionPreviewAuthority, ProductionProposal,
    PREVIEW_AUTHORITY_SCHEMA, PROPOSAL_SCHEMA,
};
use pharness_core::tools::{FinanceDeploymentExpectation, FinanceEnvironment};
use pharness_store::{
    CreateDeploymentIntent, DeliveryStage, StoredDeploymentIntent, StoredPipelineIntent,
    StoredWorkflowOperation, StoredWorkflowReconciliation, UpdateDeploymentIntentEvidence,
};
use serde_json::{json, Value};

/// Baseline identity reads are cheap but not free; the proposal gives up after
/// this many inconclusive attempts rather than polling the cluster for an hour.
const MAX_BASELINE_ATTEMPTS: u32 = 8;

pub(super) fn input_hash(
    verified: &staging::Verified,
    metadata: &pharness_store::StoredRepoWorkItemMetadata,
) -> Result<String, ApiError> {
    hash(&json!({
        "pipeline_intent_id":verified.saved.pipeline.id,
        "staging_deployment_intent_id":verified.saved.deployment.id,
        "staging_result_artifact_id":verified.result.id,
        "staging_result_sha256":hash(verified.result.content_json.as_ref().unwrap_or(&Value::Null))?,
        "workflow_policy_hash":metadata.workflow_policy_hash,
    }))
}

/// The proposal and plan a succeeded proposal operation recorded, reloaded
/// from their artifacts and checked against the operation and deployment.
pub(super) struct Recorded {
    pub operation: StoredWorkflowOperation,
    pub deployment: StoredDeploymentIntent,
    pub pipeline: StoredPipelineIntent,
    pub authority: ProductionPreviewAuthority,
    pub proposal: ProductionProposal,
    pub plan: ProductionGitOpsPlan,
    pub proposal_hash: String,
    pub plan_hash: String,
    pub proposal_artifact_id: String,
    pub gate_id: String,
}

pub(super) struct Dispatched {
    pub deployment: StoredDeploymentIntent,
    pub pipeline: StoredPipelineIntent,
    pub authority: ProductionPreviewAuthority,
    pub authority_hash: String,
}

pub(super) async fn dispatched(
    state: &AppState,
    deployment_id: &str,
    execution: &str,
) -> Result<Dispatched, ApiError> {
    let deployment = state
        .store
        .get_deployment_intent(deployment_id)
        .await?
        .ok_or_else(|| ApiError::conflict("production deployment is unavailable"))?;
    let hosted = &deployment.intent_json["hosted_production"];
    let authority: ProductionPreviewAuthority =
        serde_json::from_value(hosted["preview_authority"].clone()).map_err(|_| {
            ApiError::conflict("production deployment has no valid preview authority")
        })?;
    authority.validate_identity().map_err(ApiError::conflict)?;
    let authority_hash = authority.material_hash().map_err(ApiError::conflict)?;
    let operation = state
        .store
        .get_workflow_operation(&authority.operation_id)
        .await?
        .ok_or_else(|| ApiError::conflict("production proposal operation is unavailable"))?;
    let pipeline = state
        .store
        .get_pipeline_intent(&authority.pipeline_intent_id)
        .await?
        .ok_or_else(|| ApiError::conflict("production build is unavailable"))?;
    let d = &operation.resource_refs["production_preview_dispatch"];
    if authority.execution_id != execution
        || authority.deployment_intent_id != deployment_id
        || authority.work_item_id != operation.work_item_id
        || operation.action != super::PROPOSE_ACTION
        || authority.created_at_ms != operation.created_at
        || authority.expires_at_ms != deadline(&operation)
        || deployment.delivery_stage != DeliveryStage::Production
        || deployment.pipeline_intent_id != pipeline.id
        || deployment.change_set_id != pipeline.change_set_id
        || deployment.work_plan_id != pipeline.work_plan_id
        || deployment.session_id != pipeline.session_id
        || deployment.run_id != pipeline.run_id
        || deployment.target_namespace.as_deref() != Some("apps-prod")
        || deployment.target_environment.as_deref() != Some("production")
        || hosted["preview_authority_hash"] != authority_hash
        || d["authority"] != json!(authority)
        || d["authority_hash"] != authority_hash
        || d["deadline_ms"] != authority.expires_at_ms
    {
        return Err(ApiError::conflict(
            "production preview differs from its original operation and deployment identity",
        ));
    }
    Ok(Dispatched {
        deployment,
        pipeline,
        authority,
        authority_hash,
    })
}

pub(super) async fn recorded(
    state: &AppState,
    operation: &StoredWorkflowOperation,
) -> Result<Recorded, ApiError> {
    let r = &operation.resource_refs["production_proposal"];
    let deployment_id = required(r, "deployment_intent_id")?;
    let execution = required(r, "execution_id")?;
    let dispatched = dispatched(state, deployment_id, execution).await?;
    let artifact_id = artifact_id("proposal", execution);
    let record = state
        .store
        .get_artifact(&artifact_id)
        .await?
        .ok_or_else(|| ApiError::conflict("production proposal artifact is unavailable"))?;
    let body = record
        .content_json
        .ok_or_else(|| ApiError::conflict("production proposal has no content"))?;
    let proposal: ProductionProposal = serde_json::from_value(body["proposal"].clone())
        .map_err(|_| ApiError::conflict("production proposal is invalid"))?;
    let plan: ProductionGitOpsPlan = serde_json::from_value(body["plan"].clone())
        .map_err(|_| ApiError::conflict("production plan is invalid"))?;
    let preview: ProductionPreview = serde_json::from_value(body["preview"].clone())
        .map_err(|_| ApiError::conflict("production preview is invalid"))?;
    preview
        .validate(&dispatched.authority)
        .map_err(ApiError::conflict)?;
    plan.validate(&proposal).map_err(ApiError::conflict)?;
    let proposal_hash = proposal.material_hash().map_err(ApiError::conflict)?;
    let plan_hash = plan.material_hash().map_err(ApiError::conflict)?;
    let gate_id = required(r, "gate_id")?;
    if record.kind != "hosted_production_proposal"
        || record.session_id != dispatched.pipeline.session_id
        || record.run_id != dispatched.pipeline.run_id
        || r["artifact_id"] != artifact_id
        || r["proposal_hash"] != proposal_hash
        || r["plan_hash"] != plan_hash
        || body["gate_id"] != gate_id
        || proposal.work_item_id != operation.work_item_id
        || proposal.operation_id != operation.id
        || proposal.pipeline_intent_id != dispatched.pipeline.id
        || proposal.deployment_intent_id != deployment_id
        || proposal.workflow_policy_hash != dispatched.authority.workflow_policy_hash
        || proposal.application_repository != dispatched.authority.application_repository
        || proposal.source_commit_sha != dispatched.authority.source_commit_sha
        || proposal.image_digest != dispatched.authority.image_digest
        || proposal.previous_image_digest
            != preview
                .previous_image_digest(&dispatched.authority)
                .map_err(ApiError::conflict)?
        || plan.base_commit_sha != preview.base_commit_sha
        || plan.original_blob_sha != preview.original_blob_sha
        || plan.original_content != preview.original_content
    {
        return Err(ApiError::conflict(
            "recorded production proposal differs from its preview, operation or deployment",
        ));
    }
    Ok(Recorded {
        operation: operation.clone(),
        deployment: dispatched.deployment,
        pipeline: dispatched.pipeline,
        authority: dispatched.authority,
        proposal,
        plan,
        proposal_hash,
        plan_hash,
        proposal_artifact_id: artifact_id,
        gate_id: gate_id.into(),
    })
}

pub(super) async fn prepare(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    snapshot: &Snapshot,
    operation: &StoredWorkflowOperation,
) -> Result<StoredWorkflowOperation, ApiError> {
    let verified = staging::verified(state, snapshot)
        .await?
        .ok_or_else(|| ApiError::conflict("production requires verified staging evidence"))?;
    let (policy, policy_hash) = workflow_policy(&snapshot.metadata)?;
    let pipeline = &verified.saved.pipeline;
    let suffix = suffix(operation);
    let authority = ProductionPreviewAuthority {
        schema_version: PREVIEW_AUTHORITY_SCHEMA.into(),
        work_item_id: claim.work_item_id.clone(),
        operation_id: operation.id.clone(),
        execution_id: format!("pprev_{suffix}"),
        pipeline_intent_id: pipeline.id.clone(),
        deployment_intent_id: format!("dintent_production_{suffix}"),
        workflow_policy_hash: policy_hash.to_string(),
        application_repository: policy.delivery_binding.source_repo.clone(),
        source_commit_sha: verified.saved.authority.source_commit_sha.clone(),
        image_digest: verified.saved.authority.image_digest.clone(),
        created_at_ms: operation.created_at,
        expires_at_ms: deadline(operation),
    };
    authority
        .validate_for_dispatch(now())
        .map_err(ApiError::conflict)?;
    let authority_hash = authority.material_hash().map_err(ApiError::conflict)?;
    let manifest = state
        .worker
        .hosted_production_job_manifest(&crate::dispatch::HostedProductionJob {
            execution_id: &authority.execution_id,
            deployment_intent_id: &authority.deployment_intent_id,
            gitops_change_set_id: &format!("gcs_production_{suffix}"),
            phase: "preview",
        })
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    let intent_json = json!({"hosted_production":{"preview_authority":authority,"preview_authority_hash":authority_hash,"staging_deployment_intent_id":verified.saved.deployment.id,"staging_result_artifact_id":verified.result.id},"deployment_contract":policy.production_contract,"source_provenance":pipeline.intent_json["source_provenance"],"build_output":pipeline.intent_json["build_output"]});
    if let Some(existing) = state
        .store
        .get_deployment_intent_for_stage(&pipeline.id, DeliveryStage::Production)
        .await?
    {
        if existing.id != authority.deployment_intent_id || existing.intent_json != intent_json {
            return Err(ApiError::conflict(
                "an existing production intent cannot be rebound to different authority",
            ));
        }
    } else {
        state.store.create_deployment_intent_for_stage(CreateDeploymentIntent {
            id: authority.deployment_intent_id.clone(), pipeline_intent_id: pipeline.id.clone(), change_set_id: pipeline.change_set_id.clone(), work_plan_id: pipeline.work_plan_id.clone(), remediation_plan_id: pipeline.remediation_plan_id.clone(), incident_id: pipeline.incident_id.clone(), session_id: pipeline.session_id.clone(), run_id: pipeline.run_id.clone(), status: "proposed".into(), title: "Promote the staging-verified image to Finance production".into(), summary: "One digest change proposed for human approval; Argo and runtime verification remain required".into(), risk_level: "high".into(), intent_kind: "gitops".into(), target_environment: Some("production".into()), target_namespace: Some("apps-prod".into()), argo_application: Some(required(&policy.production_contract, "argo_application")?.into()), resource_namespace: Some("apps-prod".into()), resource_kind: Some("Deployment".into()), resource_name: Some(required(&policy.production_contract["contract_json"], "workload_name")?.into()), intent_json,
        }, DeliveryStage::Production).await?;
    }
    let mut refs = operation.resource_refs.clone();
    refs["production_preview_dispatch"] = json!({"authority":authority,"authority_hash":authority_hash,"job_manifest":manifest,"deadline_ms":deadline(operation)});
    state
        .store
        .record_workflow_operation(
            claim,
            &operation.id,
            "running",
            &refs,
            "Production preview authority and reader identity persisted before Job creation",
            now(),
        )
        .await
        .map_err(Into::into)
}

pub(super) async fn reconcile(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    snapshot: &Snapshot,
    operation: &StoredWorkflowOperation,
    expired: bool,
    timed_out: bool,
) -> Result<Condition, ApiError> {
    let dispatch = &operation.resource_refs["production_preview_dispatch"];
    let deployment_id = required(&dispatch["authority"], "deployment_intent_id")?;
    let execution = required(&dispatch["authority"], "execution_id")?;
    let dispatched = dispatched(state, deployment_id, execution).await?;
    // The proposal is only worth recording over the staging evidence it was
    // started from; a changed source, build or staging result stops it here.
    staging::verified(state, snapshot)
        .await?
        .filter(|v| {
            dispatched.deployment.intent_json["hosted_production"]["staging_result_artifact_id"]
                == v.result.id
                && v.saved.authority.source_commit_sha == dispatched.authority.source_commit_sha
                && v.saved.authority.image_digest == dispatched.authority.image_digest
        })
        .ok_or_else(|| {
            ApiError::conflict("production proposal requires its original staging evidence")
        })?;
    if operation.resource_refs.get("production_proposal").is_some() {
        // The gate may have been created just before a restart; recorded()
        // reloads it and the caller moves on to the decision.
        recorded(state, operation).await?;
        state
            .store
            .record_workflow_operation(
                claim,
                &operation.id,
                "succeeded",
                &operation.resource_refs,
                "Production proposal recorded; awaiting one operator decision",
                now(),
            )
            .await?;
        return Ok(condition(
            "awaiting_human",
            "The production diff is recorded for review.",
        ));
    }
    let preview = state
        .store
        .get_artifact(&artifact_id("preview", execution))
        .await?;
    let Some(preview) = preview else {
        if timed_out || expired {
            return super::block(
                state,
                claim,
                operation,
                "production_preview_result",
                "production_preview_window_expired",
            )
            .await;
        }
        let recover = claim.control == "active";
        let status = state
            .worker
            .reconcile_hosted_build_job(&dispatch["job_manifest"], recover)
            .await
            .map_err(|e| ApiError::conflict(e.to_string()))?;
        if matches!(status, "failed" | "succeeded") {
            return super::block(
                state,
                claim,
                operation,
                "production_preview_result",
                "production_preview_reader_ended_without_result",
            )
            .await;
        }
        return Ok(condition("waiting", "Reading the production Kustomization at the current GitOps head with the bounded reader identity."));
    };
    let body = preview
        .content_json
        .as_ref()
        .ok_or_else(|| ApiError::conflict("production preview artifact has no content"))?;
    if body["status"] != "observed" {
        return super::block(
            state,
            claim,
            operation,
            "production_preview_result",
            "production_preview_unavailable",
        )
        .await;
    }
    let file: ProductionPreview = serde_json::from_value(body["preview"].clone())
        .map_err(|_| ApiError::conflict("production preview record is invalid"))?;
    file.validate(&dispatched.authority)
        .map_err(ApiError::conflict)?;
    let previous = file
        .previous_image_digest(&dispatched.authority)
        .map_err(ApiError::conflict)?;
    if previous == dispatched.authority.image_digest {
        return super::block(
            state,
            claim,
            operation,
            "production_preview_result",
            "production_already_runs_candidate_digest",
        )
        .await;
    }
    // Baseline: production must currently run exactly what the reviewed file
    // pins, at the GitOps revision the preview read. Otherwise the proposal
    // would describe a diff against a state nobody observed.
    let expected = FinanceDeploymentExpectation {
        application: runtime::records::finance_application(
            &dispatched.authority.application_repository,
        )?,
        environment: FinanceEnvironment::Production,
        gitops_commit_sha: file.base_commit_sha.clone(),
        image_digest: previous.clone(),
    };
    expected.validate().map_err(ApiError::conflict)?;
    let mut operation = operation.clone();
    let mut baseline = None;
    for index in 1..=MAX_BASELINE_ATTEMPTS {
        let key = format!("production_baseline_{index}");
        let id = artifact_id(&format!("baseline_{index}"), execution);
        if let Some(marker) = operation.resource_refs.get(&key) {
            if marker["artifact_id"] != id {
                return Err(ApiError::conflict(
                    "production baseline marker has different authority",
                ));
            }
            match state.store.get_artifact(&id).await? {
                Some(record)
                    if record.kind == "hosted_production_baseline"
                        && record.content_json.as_ref().is_some_and(|c| {
                            c["expected"] == json!(expected)
                                && c["observation"]["identity_state"] == "verified"
                        }) =>
                {
                    baseline = Some(record);
                    break;
                }
                Some(_) => continue,
                None => {
                    // A replacement claim cannot repeat a possibly consumed read.
                    evidence::artifact(state, &dispatched.pipeline, &id, "hosted_production_baseline", json!({"expected":expected,"observation":{"identity_state":"inconclusive","reasons":["production_baseline_collection_interrupted"]}})).await?;
                    continue;
                }
            }
        }
        if timed_out || expired || claim.control != "active" {
            break;
        }
        let mut refs = operation.resource_refs.clone();
        refs[&key] = json!({"started_at_ms":now(),"artifact_id":id});
        operation = state
            .store
            .record_workflow_operation(
                claim,
                &operation.id,
                "running",
                &refs,
                "One bounded native production baseline read recorded before reading",
                now(),
            )
            .await?;
        let observation = state
            .cluster_tools
            .observe_finance_deployment(&expected)
            .await
            .map(|r| r.content)
            .unwrap_or_else(|_| json!({"collection_error":"native_read_unavailable"}));
        let record = evidence::artifact(
            state,
            &dispatched.pipeline,
            &id,
            "hosted_production_baseline",
            json!({"expected":expected,"observation":observation}),
        )
        .await?;
        if record
            .content_json
            .as_ref()
            .is_some_and(|c| c["observation"]["identity_state"] == "verified")
        {
            baseline = Some(record);
        }
        break;
    }
    let Some(baseline) = baseline else {
        let exhausted = (1..=MAX_BASELINE_ATTEMPTS).all(|i| {
            operation
                .resource_refs
                .get(format!("production_baseline_{i}"))
                .is_some()
        });
        if exhausted || timed_out || expired {
            return super::block(
                state,
                claim,
                &operation,
                "production_baseline_result",
                "production_baseline_identity_not_verified",
            )
            .await;
        }
        return Ok(condition("waiting", "Production does not yet show the previewed GitOps revision and digest; the baseline read will be repeated."));
    };
    let baseline_body = baseline.content_json.as_ref().unwrap();
    let configuration_hash = hash(
        &json!({"path":dispatched.authority.coordinates().map_err(ApiError::conflict)?.0,"blob_sha":file.original_blob_sha,"content":file.original_content}),
    )?;
    let baseline_hash = hash(baseline_body)?;
    let (policy, _) = workflow_policy(&snapshot.metadata)?;
    let staging_result = state
        .store
        .get_artifact(required(
            &dispatched.deployment.intent_json["hosted_production"],
            "staging_result_artifact_id",
        )?)
        .await?
        .ok_or_else(|| ApiError::conflict("staging result artifact is unavailable"))?;
    let proposal = ProductionProposal {
        schema_version: PROPOSAL_SCHEMA.into(),
        work_item_id: claim.work_item_id.clone(),
        operation_id: operation.id.clone(),
        pipeline_intent_id: dispatched.pipeline.id.clone(),
        deployment_intent_id: dispatched.deployment.id.clone(),
        workflow_policy_hash: dispatched.authority.workflow_policy_hash.clone(),
        application_repository: dispatched.authority.application_repository.clone(),
        source_commit_sha: dispatched.authority.source_commit_sha.clone(),
        build_evidence_hash: verified_build_hash(state, snapshot).await?,
        image_digest: dispatched.authority.image_digest.clone(),
        staging_evidence_artifact_id: staging_result.id.clone(),
        staging_evidence_hash: hash(staging_result.content_json.as_ref().unwrap_or(&Value::Null))?,
        baseline_evidence_artifact_id: baseline.id.clone(),
        baseline_evidence_hash: baseline_hash.clone(),
        previous_image_digest: previous,
        production_configuration_hash: configuration_hash.clone(),
        rollback_compatibility_hash: hash(
            &json!({"production_configuration_hash":configuration_hash,"baseline_evidence_hash":baseline_hash,"baseline_template_hash":baseline_body["observation"]["identity"]["template_hash"]}),
        )?,
        rollback: policy.rollback,
        created_at_ms: now(),
    };
    proposal.validate().map_err(ApiError::conflict)?;
    let plan = ProductionGitOpsPlan::from_preview(&proposal, &file).map_err(ApiError::conflict)?;
    let gate_id = format!("gate_production_{}", suffix(&operation));
    let proposal_id = artifact_id("proposal", execution);
    let proposal_hash = proposal.material_hash().map_err(ApiError::conflict)?;
    let plan_hash = plan.material_hash().map_err(ApiError::conflict)?;
    let artifact = evidence::artifact(state, &dispatched.pipeline, &proposal_id, "hosted_production_proposal", json!({"proposal":proposal,"plan":plan,"preview":file,"preview_artifact_id":preview.id,"baseline_artifact_id":baseline.id,"gate_id":gate_id})).await?;
    super::gate::ensure(state, &dispatched, &proposal, &plan, &artifact.id, &gate_id).await?;
    let mut value = dispatched.deployment.intent_json.clone();
    value["hosted_production"]["proposal"] = json!({"artifact_id":artifact.id,"proposal_hash":proposal_hash,"plan_hash":plan_hash,"gate_id":gate_id,"previous_image_digest":proposal.previous_image_digest,"image_digest":proposal.image_digest,"base_commit_sha":plan.base_commit_sha});
    if value != dispatched.deployment.intent_json {
        state
            .store
            .update_deployment_intent_evidence(
                &dispatched.deployment.id,
                UpdateDeploymentIntentEvidence {
                    intent_json: value,
                    actor: Some("controller:hosted-production".into()),
                    reason: Some(
                        "Record the proposed production diff awaiting a human decision".into(),
                    ),
                },
            )
            .await?;
    }
    let mut refs = operation.resource_refs.clone();
    refs["production_proposal"] = json!({"artifact_id":artifact.id,"proposal_hash":proposal_hash,"plan_hash":plan_hash,"gate_id":gate_id,"deployment_intent_id":dispatched.deployment.id,"execution_id":execution});
    state
        .store
        .record_workflow_operation(
            claim,
            &operation.id,
            "succeeded",
            &refs,
            "Production proposal recorded; awaiting one operator decision",
            now(),
        )
        .await?;
    Ok(condition(
        "awaiting_human",
        "The production diff is recorded for review.",
    ))
}

async fn verified_build_hash(state: &AppState, snapshot: &Snapshot) -> Result<String, ApiError> {
    evidence::build(state, &snapshot.metadata.work_item_id)
        .await?
        .map(|(_, _, hash)| hash)
        .ok_or_else(|| ApiError::conflict("production requires the original verified build"))
}

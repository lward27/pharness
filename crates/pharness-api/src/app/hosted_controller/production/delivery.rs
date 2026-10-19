//! The single admitted production write after the human decision, then Argo
//! and image identity and one ten minute runtime window. The delivery
//! authority copies the proposal and decision verbatim and expires with the
//! earlier of the operation deadline and the approval grant.
use super::super::state::{condition, workflow_policy, Condition, Snapshot};
use super::{
    artifact_id, deadline, evidence, gate, hash, now, operation_id, proposal, required, runtime,
    staging, suffix, ApiError, AppState, PROMOTE_ACTION, PROPOSE_ACTION,
};
use pharness_core::hosted_sdlc::production::{
    HostedProductionAuthority, ProductionGitOpsPlan, AUTHORITY_SCHEMA,
};
use pharness_core::hosted_sdlc::staging::GITOPS_REPOSITORY;
use pharness_core::tools::{
    FinanceDeploymentExpectation, FinanceEnvironment, FinanceVerificationPhase,
};
use pharness_store::{
    CreateGitOpsChangeSet, DeliveryStage, StoredArtifact, StoredDeploymentIntent,
    StoredPipelineIntent, StoredWorkflowOperation, StoredWorkflowReconciliation,
    UpdateDeploymentIntentEvidence,
};
use serde_json::{json, Value};

/// Everything the promotion operation persisted before its writer Job, reloaded
/// and cross-checked so a callback can only ever address the original write.
pub(super) struct Saved {
    pub operation: StoredWorkflowOperation,
    pub deployment: StoredDeploymentIntent,
    pub pipeline: StoredPipelineIntent,
    pub authority: HostedProductionAuthority,
    pub authority_hash: String,
    pub plan: ProductionGitOpsPlan,
    pub plan_hash: String,
    pub approval_hash: String,
    pub gate_id: String,
}

pub(super) fn input_hash(recorded: &proposal::Recorded) -> Result<String, ApiError> {
    hash(
        &json!({"deployment_intent_id":recorded.deployment.id,"proposal_hash":recorded.proposal_hash,"plan_hash":recorded.plan_hash}),
    )
}

/// The succeeded proposal this WorkItem's verified staging result leads to.
pub(super) async fn recorded_proposal(
    state: &AppState,
    snapshot: &Snapshot,
) -> Result<proposal::Recorded, ApiError> {
    let verified = staging::verified(state, snapshot)
        .await?
        .ok_or_else(|| ApiError::conflict("production requires verified staging evidence"))?;
    let input = proposal::input_hash(&verified, &snapshot.metadata)?;
    let id = operation_id(&snapshot.metadata.work_item_id, PROPOSE_ACTION, &input)?;
    let operation = state
        .store
        .get_workflow_operation(&id)
        .await?
        .filter(|o| o.status == "succeeded")
        .ok_or_else(|| ApiError::conflict("production requires its recorded proposal"))?;
    proposal::recorded(state, &operation).await
}

pub(super) async fn prepare(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    snapshot: &Snapshot,
    operation: &StoredWorkflowOperation,
) -> Result<StoredWorkflowOperation, ApiError> {
    let recorded = recorded_proposal(state, snapshot).await?;
    if operation.input_hash != input_hash(&recorded)? {
        return Err(ApiError::conflict(
            "production promotion differs from the recorded proposal",
        ));
    }
    let gate::Decision::Approved(approved) = gate::state(state, &recorded).await? else {
        return Err(ApiError::conflict(
            "production promotion requires one satisfied operator decision",
        ));
    };
    let suffix = suffix(operation);
    let authority = HostedProductionAuthority {
        schema_version: AUTHORITY_SCHEMA.into(),
        execution_id: format!("pexec_{suffix}"),
        gitops_change_set_id: format!("gcs_production_{suffix}"),
        approval_artifact_id: approved.artifact.id.clone(),
        proposal: recorded.proposal.clone(),
        approval: approved.approval.clone(),
        created_at_ms: operation.created_at,
        expires_at_ms: deadline(operation).min(approved.approval.expires_at_ms),
    };
    authority
        .validate_for_dispatch(&recorded.plan, now())
        .map_err(ApiError::conflict)?;
    let authority_hash = authority.material_hash().map_err(ApiError::conflict)?;
    // Render both manifests before any durable record so a missing writer or
    // reader configuration cannot leave a partial authority behind.
    let job = |phase: &'static str| {
        state
            .worker
            .hosted_production_job_manifest(&crate::dispatch::HostedProductionJob {
                execution_id: &authority.execution_id,
                deployment_intent_id: &recorded.deployment.id,
                gitops_change_set_id: &authority.gitops_change_set_id,
                phase,
            })
            .map_err(|e| ApiError::conflict(e.to_string()))
    };
    let executor = job("deliver")?;
    let observer = job("observe")?;
    // The plan is fixed by the decision, so the worker never derives one; its
    // artifact and ChangeSet exist before the writer can ask for context.
    let plan_artifact = evidence::artifact(
        state,
        &recorded.pipeline,
        &artifact_id("plan", &authority.execution_id),
        "hosted_production_plan",
        json!(recorded.plan),
    )
    .await?;
    ensure_change_set(state, &recorded, &authority, &plan_artifact).await?;
    let delivery = json!({"operation_id":operation.id,"authority":authority,"authority_hash":authority_hash,"plan_artifact_id":plan_artifact.id,"plan_hash":recorded.plan_hash,"approval_hash":approved.approval_hash,"gitops_change_set_id":authority.gitops_change_set_id});
    let mut value = recorded.deployment.intent_json.clone();
    if value["hosted_production"]
        .get("delivery")
        .is_some_and(|v| *v != delivery)
    {
        return Err(ApiError::conflict(
            "an existing production delivery cannot be rebound to different authority",
        ));
    }
    value["hosted_production"]["delivery"] = delivery;
    if value != recorded.deployment.intent_json {
        state
            .store
            .update_deployment_intent_evidence(
                &recorded.deployment.id,
                UpdateDeploymentIntentEvidence {
                    intent_json: value,
                    actor: Some("controller:hosted-production".into()),
                    reason: Some(
                        "Bind the approved production write to one execution identity".into(),
                    ),
                },
            )
            .await?;
    }
    let mut refs = operation.resource_refs.clone();
    refs["production_dispatch"] = json!({"authority":authority,"authority_hash":authority_hash,"plan_hash":recorded.plan_hash,"executor_job_manifest":executor,"observer_job_manifest":observer,"deadline_ms":deadline(operation),"proposal_operation_id":recorded.operation.id});
    state
        .store
        .record_workflow_operation(
            claim,
            &operation.id,
            "running",
            &refs,
            "Production write authority and writer/reader identities persisted before Job creation",
            now(),
        )
        .await
        .map_err(Into::into)
}

async fn ensure_change_set(
    state: &AppState,
    recorded: &proposal::Recorded,
    authority: &HostedProductionAuthority,
    plan_artifact: &StoredArtifact,
) -> Result<(), ApiError> {
    let (path, image) = authority.coordinates().map_err(ApiError::conflict)?;
    let image_ref = authority.image_ref().map_err(ApiError::conflict)?;
    let document = json!({"delivery_mode":"atomic_commit_expected_head","authority":authority,"plan_artifact_id":plan_artifact.id,"plan_hash":recorded.plan_hash,"approval_artifact_id":authority.approval_artifact_id,"expected_head_oid":recorded.plan.base_commit_sha,"target_ref":"main","scope":"production","production_authorized":true});
    let material = hash(&document)?;
    if let Some(existing) = state
        .store
        .get_gitops_change_set_by_deployment_intent(&recorded.deployment.id)
        .await?
    {
        if existing.id != authority.gitops_change_set_id
            || existing.work_item_id != recorded.proposal.work_item_id
            || existing.pipeline_intent_id != recorded.pipeline.id
            || existing.source_change_set_id != recorded.pipeline.change_set_id
            || existing.gitops_update_plan_artifact_id != plan_artifact.id
            || existing.gitops_change_set_json != document
            || existing.material_hash != material
            || existing.gitops_repo != GITOPS_REPOSITORY
            || existing.gitops_ref != "main"
            || existing.head_branch != "main"
            || existing.kustomization_path != path
            || existing.image_name != image
            || existing.image_ref != image_ref
        {
            return Err(ApiError::conflict(
                "production GitOps ChangeSet differs from the approved one-file plan",
            ));
        }
        return Ok(());
    }
    state
        .store
        .create_gitops_change_set(CreateGitOpsChangeSet {
            id: authority.gitops_change_set_id.clone(),
            work_item_id: recorded.proposal.work_item_id.clone(),
            work_plan_id: recorded.pipeline.work_plan_id.clone(),
            source_change_set_id: recorded.pipeline.change_set_id.clone(),
            pipeline_intent_id: recorded.pipeline.id.clone(),
            deployment_intent_id: recorded.deployment.id.clone(),
            gitops_update_plan_artifact_id: plan_artifact.id.clone(),
            session_id: recorded.pipeline.session_id.clone(),
            run_id: recorded.pipeline.run_id.clone(),
            status: "approved".into(),
            title: "Promote the Finance production image digest".into(),
            summary: "One immutable image pin approved by an operator over the exact diff".into(),
            risk_level: "high".into(),
            material_hash: material,
            gitops_repo: GITOPS_REPOSITORY.into(),
            gitops_ref: "main".into(),
            head_branch: "main".into(),
            kustomization_path: path.into(),
            image_name: image.into(),
            image_ref,
            gitops_change_set_json: document,
        })
        .await?;
    Ok(())
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
        .ok_or_else(|| ApiError::conflict("production deployment is unavailable"))?;
    let hosted = &deployment.intent_json["hosted_production"];
    let delivery = &hosted["delivery"];
    let authority: HostedProductionAuthority =
        serde_json::from_value(delivery["authority"].clone()).map_err(|_| {
            ApiError::conflict("production deployment has no valid write authority")
        })?;
    authority.validate_identity().map_err(ApiError::conflict)?;
    let authority_hash = authority.material_hash().map_err(ApiError::conflict)?;
    let operation = state
        .store
        .get_workflow_operation(required(delivery, "operation_id")?)
        .await?
        .ok_or_else(|| ApiError::conflict("production promotion operation is unavailable"))?;
    let pipeline = state
        .store
        .get_pipeline_intent(&authority.proposal.pipeline_intent_id)
        .await?
        .ok_or_else(|| ApiError::conflict("production build is unavailable"))?;
    let plan_record = state
        .store
        .get_artifact(&artifact_id("plan", execution))
        .await?
        .ok_or_else(|| ApiError::conflict("production plan artifact is unavailable"))?;
    let plan: ProductionGitOpsPlan = serde_json::from_value(
        plan_record
            .content_json
            .clone()
            .ok_or_else(|| ApiError::conflict("production plan artifact has no content"))?,
    )
    .map_err(|_| ApiError::conflict("invalid production plan artifact"))?;
    plan.validate(&authority.proposal)
        .map_err(ApiError::conflict)?;
    let plan_hash = plan.material_hash().map_err(ApiError::conflict)?;
    let approval = state
        .store
        .get_artifact(&authority.approval_artifact_id)
        .await?
        .ok_or_else(|| ApiError::conflict("production approval record is unavailable"))?;
    let approval_hash = authority
        .approval
        .material_hash()
        .map_err(ApiError::conflict)?;
    let gate_id = required(&hosted["proposal"], "gate_id")?.to_string();
    let d = &operation.resource_refs["production_dispatch"];
    let proposal_hash = authority
        .proposal
        .material_hash()
        .map_err(ApiError::conflict)?;
    if authority.execution_id != execution
        || authority.proposal.deployment_intent_id != deployment_id
        || authority.proposal.work_item_id != operation.work_item_id
        || operation.action != PROMOTE_ACTION
        || authority.created_at_ms != operation.created_at
        || authority.expires_at_ms != deadline(&operation).min(authority.approval.expires_at_ms)
        || deployment.delivery_stage != DeliveryStage::Production
        || deployment.pipeline_intent_id != pipeline.id
        || deployment.change_set_id != pipeline.change_set_id
        || deployment.work_plan_id != pipeline.work_plan_id
        || deployment.session_id != pipeline.session_id
        || deployment.run_id != pipeline.run_id
        || deployment.target_namespace.as_deref() != Some("apps-prod")
        || deployment.target_environment.as_deref() != Some("production")
        || delivery["authority_hash"] != authority_hash
        || delivery["plan_hash"] != plan_hash
        || delivery["plan_artifact_id"] != plan_record.id
        || delivery["approval_hash"] != approval_hash
        || delivery["gitops_change_set_id"] != authority.gitops_change_set_id
        || d["authority"] != json!(authority)
        || d["authority_hash"] != authority_hash
        || d["plan_hash"] != plan_hash
        || d["deadline_ms"] != deadline(&operation)
        || d["proposal_operation_id"] != authority.proposal.operation_id
        || hosted["proposal"]["proposal_hash"] != proposal_hash
        || hosted["proposal"]["plan_hash"] != plan_hash
        || authority.approval.gitops_plan_hash != plan_hash
        || plan_record.kind != "hosted_production_plan"
        || plan_record.session_id != pipeline.session_id
        || plan_record.run_id != pipeline.run_id
        || approval.kind != "hosted_production_approval"
        || approval.session_id != pipeline.session_id
        || approval.run_id != pipeline.run_id
        || approval.content_json.as_ref().map(|c| &c["approval"])
            != Some(&json!(authority.approval))
        || approval.content_json.as_ref().map(|c| &c["gate_id"]) != Some(&json!(gate_id))
        || approval.content_json.as_ref().map(|c| &c["approval_hash"])
            != Some(&json!(approval_hash))
        || operation.input_hash
            != hash(
                &json!({"deployment_intent_id":deployment_id,"proposal_hash":proposal_hash,"plan_hash":plan_hash}),
            )?
    {
        return Err(ApiError::conflict(
            "production callback differs from its original operation, decision and deployment identity",
        ));
    }
    Ok(Saved {
        operation,
        deployment,
        pipeline,
        authority,
        authority_hash,
        plan,
        plan_hash,
        approval_hash,
        gate_id,
    })
}

/// The decision is only as good as the evidence it was made over. Staging,
/// workflow, target and the gate itself must still agree; for a write the
/// grant and the operation must also both be open right now.
pub(super) async fn validate_current(
    state: &AppState,
    saved: &Saved,
    for_write: bool,
) -> Result<(), ApiError> {
    let proposal = &saved.authority.proposal;
    let snapshot = Snapshot::load(state, &proposal.work_item_id).await?;
    let verified = staging::verified(state, &snapshot).await?.ok_or_else(|| {
        ApiError::conflict(
            "the staging evidence behind this production release is no longer verified",
        )
    })?;
    let (policy, policy_hash) = workflow_policy(&snapshot.metadata)?;
    let gate = state
        .store
        .get_approval_gate(&saved.gate_id)
        .await?
        .ok_or_else(|| ApiError::conflict("production approval gate is unavailable"))?;
    let proposal_hash = proposal.material_hash().map_err(ApiError::conflict)?;
    if verified.result.id != proposal.staging_evidence_artifact_id
        || hash(
            verified
                .result
                .content_json
                .as_ref()
                .unwrap_or(&Value::Null),
        )? != proposal.staging_evidence_hash
        || verified.saved.pipeline.id != proposal.pipeline_intent_id
        || verified.saved.authority.image_digest != proposal.image_digest
        || verified.saved.authority.source_commit_sha != proposal.source_commit_sha
        || policy_hash != proposal.workflow_policy_hash
        || policy.delivery_binding.source_repo != proposal.application_repository
        || saved.deployment.intent_json["deployment_contract"] != policy.production_contract
        || saved.deployment.argo_application.as_deref()
            != policy.production_contract["argo_application"].as_str()
        || saved.deployment.resource_name.as_deref()
            != policy.production_contract["contract_json"]["workload_name"].as_str()
        || saved.deployment.resource_namespace.as_deref() != Some("apps-prod")
        || saved.deployment.resource_kind.as_deref() != Some("Deployment")
        || gate.gate_kind != gate::GATE_KIND
        || gate.status != "satisfied"
        || gate.gate_json["proposal_hash"] != proposal_hash
        || gate.gate_json["plan_hash"] != saved.plan_hash
        || gate.decided_by.as_deref() != Some(saved.authority.approval.operator_identity.as_str())
    {
        return Err(ApiError::conflict(
            "production source, staging evidence, policy, target or decision changed after approval",
        ));
    }
    if for_write {
        saved
            .authority
            .validate_for_dispatch(&saved.plan, now())
            .map_err(ApiError::conflict)?;
        if saved.operation.status != "running"
            || saved.deployment.status != "approved"
            || !evidence::may_advance(state, &proposal.work_item_id).await?
        {
            return Err(ApiError::conflict(
                "new production work is stopped or its approval window closed",
            ));
        }
    }
    Ok(())
}

/// The production runtime window is bound to the observed commit; it may run
/// until the operation deadline even after the write grant itself closes.
pub(super) fn runtime_context(
    saved: &Saved,
    receipt: &StoredArtifact,
) -> Result<runtime::Context, ApiError> {
    let body = receipt.content_json.as_ref().ok_or_else(|| {
        ApiError::conflict("production runtime requires its original commit receipt")
    })?;
    let checked = body["checked_at_ms"].as_i64().unwrap_or_default();
    let identity = evidence::validate_observation(
        &saved.authority,
        &saved.plan,
        &body["observation"],
        checked,
    )?;
    if receipt.id != artifact_id("commit", &saved.authority.execution_id)
        || receipt.kind != "hosted_production_commit"
        || receipt.session_id != saved.pipeline.session_id
        || receipt.run_id != saved.pipeline.run_id
        || body["identity"] != identity
        || body["authority_hash"] != saved.authority_hash
    {
        return Err(ApiError::conflict(
            "production runtime commit receipt has different authority or origin",
        ));
    }
    let proposal = &saved.authority.proposal;
    let expected = FinanceDeploymentExpectation {
        application: runtime::records::finance_application(&proposal.application_repository)?,
        environment: FinanceEnvironment::Production,
        gitops_commit_sha: body["observation"]["current_gitops_revision"]
            .as_str()
            .unwrap_or_default()
            .into(),
        image_digest: proposal.image_digest.clone(),
    };
    expected.validate().map_err(ApiError::conflict)?;
    Ok(runtime::Context {
        operation: saved.operation.clone(),
        deployment: saved.deployment.clone(),
        pipeline: saved.pipeline.clone(),
        work_item_id: proposal.work_item_id.clone(),
        execution_id: saved.authority.execution_id.clone(),
        expires_at_ms: deadline(&saved.operation),
        expected,
        phase: FinanceVerificationPhase::Production,
        prefix: "production",
        projection: ("hosted_production", "runtime_observation"),
        binding: json!({"authority_hash":saved.authority_hash,"plan_hash":saved.plan_hash,"approval_hash":saved.approval_hash,"commit_artifact_id":receipt.id,"commit_artifact_sha256":hash(body)?,"admitted_gitops_commit_sha":identity["gitops_commit_sha"]}),
        commit_checked_at_ms: checked,
    })
}

/// Project an observed, authority-bound commit receipt onto the deployment
/// intent. Called by the writer callback and by every later reconciliation,
/// so a lost acknowledgment never leaves the intent behind the artifact.
pub(super) async fn settle(
    state: &AppState,
    saved: &Saved,
    receipt: &StoredArtifact,
) -> Result<(), ApiError> {
    let body = receipt
        .content_json
        .as_ref()
        .ok_or_else(|| ApiError::conflict("production commit receipt has no content"))?;
    if receipt.kind != "hosted_production_commit"
        || receipt.session_id != saved.pipeline.session_id
        || receipt.run_id != saved.pipeline.run_id
        || body["authority_hash"] != saved.authority_hash
        || body["identity"]
            != evidence::validate_observation(
                &saved.authority,
                &saved.plan,
                &body["observation"],
                body["checked_at_ms"].as_i64().unwrap_or_default(),
            )?
    {
        return Err(ApiError::conflict(
            "persisted production commit receipt is inconsistent",
        ));
    }
    // Do not recreate missing delivery records after an admitted write.
    let change = state
        .store
        .get_gitops_change_set_by_deployment_intent(&saved.deployment.id)
        .await?
        .ok_or_else(|| {
            ApiError::conflict("observed production change has no saved GitOps ChangeSet")
        })?;
    if change.id != saved.authority.gitops_change_set_id
        || change.gitops_update_plan_artifact_id
            != artifact_id("plan", &saved.authority.execution_id)
    {
        return Err(ApiError::conflict(
            "observed production ChangeSet identity changed",
        ));
    }
    let mut value = saved.deployment.intent_json.clone();
    value["hosted_production"]["gitops_observation"] = json!({"artifact_id":receipt.id,"gitops_commit_sha":body["identity"]["gitops_commit_sha"],"image_ref":body["identity"]["image_ref"],"status":"committed","deployment_verification":"pending","runtime_verification":"pending"});
    if value != saved.deployment.intent_json {
        state
            .store
            .update_deployment_intent_evidence(
                &saved.deployment.id,
                UpdateDeploymentIntentEvidence {
                    intent_json: value,
                    actor: Some("observer:hosted-production".into()),
                    reason: Some(
                        "Record the approved GitOps commit independently from actual deployment"
                            .into(),
                    ),
                },
            )
            .await?;
    }
    Ok(())
}
pub(super) async fn reconcile(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    _snapshot: &Snapshot,
    operation: &StoredWorkflowOperation,
    expired: bool,
    timed_out: bool,
) -> Result<Condition, ApiError> {
    let dispatch = &operation.resource_refs["production_dispatch"];
    let deployment_id = required(&dispatch["authority"]["proposal"], "deployment_intent_id")?;
    let execution = required(&dispatch["authority"], "execution_id")?;
    let saved = saved(state, deployment_id, execution).await?;
    if let Some(receipt) = state
        .store
        .get_artifact(&artifact_id("commit", execution))
        .await?
    {
        settle(state, &saved, &receipt).await?;
        validate_current(state, &saved, false).await?;
        // The commit is one boundary; the operation and its locks stay until
        // Argo, image identity and the production window are verified.
        let mut refs = operation.resource_refs.clone();
        refs["production_result"] = json!({"status":"gitops_committed","artifact_id":receipt.id,"gitops_commit_sha":receipt.content_json.as_ref().map(|v| &v["identity"]["gitops_commit_sha"]),"deployment_verification":"pending","runtime_verification":"pending"});
        if refs != operation.resource_refs {
            state.store.record_workflow_operation(claim, &operation.id, "running", &refs, "Production GitOps commit observed; deployment and runtime verification remain required", now()).await?;
        }
        let saved = self::saved(state, deployment_id, execution).await?;
        let context = runtime_context(&saved, &receipt)?;
        return runtime::reconcile(state, claim, &context, timed_out).await;
    }
    let admitted = state
        .store
        .get_artifact(&artifact_id("attempt", execution))
        .await?
        .is_some();
    if !admitted && now() >= saved.authority.expires_at_ms {
        return super::block(
            state,
            claim,
            operation,
            "production_result",
            "production_approval_window_closed_before_admission",
        )
        .await;
    }
    let recover = !admitted && claim.control == "active" && !expired;
    if recover {
        validate_current(state, &saved, true).await?;
    }
    let status = state
        .worker
        .reconcile_hosted_build_job(&dispatch["executor_job_manifest"], recover)
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    if matches!(status, "missing" | "failed" | "succeeded") && admitted {
        let dispatch_observer = !expired
            && operation
                .resource_refs
                .get("production_observer_dispatch")
                .is_none();
        if dispatch_observer {
            let mut refs = operation.resource_refs.clone();
            refs["production_observer_dispatch"] = json!({"job_name":dispatch["observer_job_manifest"]["metadata"]["name"],"admitted_at_ms":now()});
            state
                .store
                .record_workflow_operation(
                    claim,
                    &operation.id,
                    "running",
                    &refs,
                    "One bounded GitOps reader admitted to recover the original production result",
                    now(),
                )
                .await?;
        }
        let observed = state
            .worker
            .reconcile_hosted_build_job(&dispatch["observer_job_manifest"], dispatch_observer)
            .await
            .map_err(|e| ApiError::conflict(e.to_string()))?;
        if matches!(observed, "failed" | "succeeded") || (observed == "missing" && !expired) {
            return Ok(condition("blocked", "The bounded production observer is unavailable or ended without an accepted result. No replacement writer or observer was started."));
        }
    } else if matches!(status, "failed" | "succeeded") {
        return super::block(
            state,
            claim,
            operation,
            "production_result",
            "production_writer_ended_before_admission",
        )
        .await;
    }
    Ok(condition(
        if expired { "wait_expired" } else { "waiting" },
        if expired {
            "The original production window expired. No new write or recovery Job is started; late observations remain recordable."
        } else {
            "Observing the original production update. A GitOps commit, deployment and runtime verification remain separate evidence."
        },
    ))
}

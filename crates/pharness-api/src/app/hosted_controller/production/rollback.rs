//! Automatic return to the recorded previous image after a failed production
//! window. The rollback is a second phase of the operation that owns the
//! failed window (promotion for `release`, observation for `observe`). It
//! starts only from a `failed` assessment, writes once with the GitOps writer
//! on the expected head, waits for Argo to run the previous digest, then
//! blocks the operation and fails the WorkItem. Inconclusive windows block
//! the operation without any write.
use super::super::state::{condition, Condition};
use super::{
    artifact_id, delivery, evidence, hash, now, required, runtime, suffix, ApiError, AppState,
    OBSERVE_ACTION, PROMOTE_ACTION,
};
use pharness_core::hosted_sdlc::production::{
    HostedProductionRollbackAuthority, ProductionRollbackPlan, RollbackTrigger,
    ROLLBACK_AUTHORITY_SCHEMA,
};
use pharness_core::tools::{FinanceDeploymentExpectation, FinanceEnvironment};
use pharness_store::{
    StoredArtifact, StoredWorkflowOperation, StoredWorkflowReconciliation,
    UpdateDeploymentIntentEvidence,
};
use serde_json::{json, Value};

mod callbacks;
pub(in crate::app) use callbacks::{
    internal_production_rollback_attempt, internal_production_rollback_context,
    internal_production_rollback_outcome, internal_production_rollback_plan,
};

pub(super) const DISPATCH_KEY: &str = "rollback_dispatch";
pub(super) const RESULT_KEY: &str = "rollback_result";
const COMMIT_KEY: &str = "rollback_commit";
const ACTOR: &str = "controller:hosted-production";
/// A rollback has its own bound, independent of the deadline of the operation
/// whose window failed: that deadline may already be close when the result
/// arrives, and a half-finished rollback helps nobody.
pub(super) const ROLLBACK_WINDOW_MS: i64 = 30 * 60 * 1000;

/// Everything a rollback callback may address, reloaded from durable records
/// and cross-checked against the delivery it reverses.
pub(super) struct Saved {
    pub operation: StoredWorkflowOperation,
    pub delivery: delivery::Saved,
    pub authority: HostedProductionRollbackAuthority,
    pub authority_hash: String,
    pub plan: Option<ProductionRollbackPlan>,
    pub plan_hash: Option<String>,
}

fn result_key(trigger: RollbackTrigger) -> &'static str {
    match trigger {
        RollbackTrigger::Release => "production_runtime_result",
        RollbackTrigger::Observe => "observe_runtime_result",
    }
}

fn trigger_name(trigger: RollbackTrigger) -> &'static str {
    match trigger {
        RollbackTrigger::Release => "release",
        RollbackTrigger::Observe => "observe",
    }
}

/// The admitted production commit a rollback reverses, re-validated from its
/// receipt rather than trusted from any projection.
async fn admitted_commit(
    state: &AppState,
    delivery: &delivery::Saved,
) -> Result<(StoredArtifact, Value), ApiError> {
    let receipt = state
        .store
        .get_artifact(&artifact_id("commit", &delivery.authority.execution_id))
        .await?
        .ok_or_else(|| ApiError::conflict("production rollback requires the admitted commit"))?;
    let body = receipt
        .content_json
        .as_ref()
        .ok_or_else(|| ApiError::conflict("production commit receipt has no content"))?;
    let identity = evidence::validate_observation(
        &delivery.authority,
        &delivery.plan,
        &body["observation"],
        body["checked_at_ms"].as_i64().unwrap_or_default(),
    )?;
    if receipt.kind != "hosted_production_commit"
        || body["identity"] != identity
        || body["authority_hash"] != delivery.authority_hash
    {
        return Err(ApiError::conflict(
            "production commit receipt has different authority or origin",
        ));
    }
    Ok((receipt, identity))
}

/// The delivery whose window the operation owns, reloaded from the dispatch
/// record of the promotion or observation operation.
async fn delivery_for(
    state: &AppState,
    operation: &StoredWorkflowOperation,
    trigger: RollbackTrigger,
) -> Result<delivery::Saved, ApiError> {
    let (deployment_id, execution) = match trigger {
        RollbackTrigger::Release => {
            let authority = &operation.resource_refs["production_dispatch"]["authority"];
            (
                required(&authority["proposal"], "deployment_intent_id")?,
                required(authority, "execution_id")?,
            )
        }
        RollbackTrigger::Observe => {
            let dispatch = &operation.resource_refs["observe_dispatch"];
            (
                required(dispatch, "deployment_intent_id")?,
                required(dispatch, "execution_id")?,
            )
        }
    };
    delivery::saved(state, deployment_id, execution).await
}

/// Decide what follows a production window. A verified or still-running
/// window passes `outcome` through; `failed` enters the rollback phase and
/// `inconclusive` blocks the operation with no write.
pub(super) async fn after_window(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    operation_id: &str,
    trigger: RollbackTrigger,
    outcome: Condition,
) -> Result<Condition, ApiError> {
    let operation = state
        .store
        .get_workflow_operation(operation_id)
        .await?
        .ok_or_else(|| ApiError::conflict("production operation is unavailable"))?;
    let result = &operation.resource_refs[result_key(trigger)];
    if operation.status != "running" || result["status"] != "blocked" {
        return Ok(outcome);
    }
    if result["runtime_verification"] != "failed" {
        return super::block(
            state,
            claim,
            &operation,
            RESULT_KEY,
            &format!(
                "production_{}_inconclusive_no_rollback",
                trigger_name(trigger)
            ),
        )
        .await;
    }
    let delivery = delivery_for(state, &operation, trigger).await?;
    reconcile(state, claim, &operation, &delivery, trigger).await
}

async fn reconcile(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    operation: &StoredWorkflowOperation,
    delivery: &delivery::Saved,
    trigger: RollbackTrigger,
) -> Result<Condition, ApiError> {
    let mut operation = operation.clone();
    if operation.resource_refs.get(DISPATCH_KEY).is_none() {
        if claim.control != "active" {
            return Ok(condition(&claim.control, "A production regression is recorded. The automatic rollback waits for active workflow control; no write is started."));
        }
        operation = prepare(state, claim, &operation, delivery, trigger).await?;
    }
    let dispatch = operation.resource_refs[DISPATCH_KEY].clone();
    let execution = required(&dispatch["authority"], "execution_id")?;
    let saved = saved(state, &delivery.deployment.id, execution).await?;
    if let Some(receipt) = state
        .store
        .get_artifact(&artifact_id("rollback_commit", execution))
        .await?
    {
        return verify(state, claim, &saved, &receipt).await;
    }
    let admitted = state
        .store
        .get_artifact(&artifact_id("rollback_attempt", execution))
        .await?
        .is_some();
    let expired = now() >= saved.authority.expires_at_ms;
    if !admitted && expired {
        return super::block(
            state,
            claim,
            &saved.operation,
            RESULT_KEY,
            "production_rollback_window_closed_before_admission",
        )
        .await;
    }
    let recover = !admitted && claim.control == "active";
    let status = state
        .worker
        .reconcile_hosted_build_job(&dispatch["writer_job_manifest"], recover)
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    // No replacement writer and no recovery reader: a rollback whose outcome
    // is unknown is an operator matter, recorded as such.
    if admitted && (status != "active" || expired) {
        return super::block(
            state,
            claim,
            &saved.operation,
            RESULT_KEY,
            if expired {
                "production_rollback_window_closed_after_admission"
            } else {
                "production_rollback_outcome_unconfirmed"
            },
        )
        .await;
    }
    if matches!(status, "failed" | "succeeded") {
        return super::block(
            state,
            claim,
            &saved.operation,
            RESULT_KEY,
            "production_rollback_writer_ended_before_admission",
        )
        .await;
    }
    Ok(condition(
        "waiting",
        "Production regression confirmed. One rollback writer restores the recorded previous image; its commit and deployment remain separate evidence.",
    ))
}

async fn prepare(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    operation: &StoredWorkflowOperation,
    delivery: &delivery::Saved,
    trigger: RollbackTrigger,
) -> Result<StoredWorkflowOperation, ApiError> {
    let result = &operation.resource_refs[result_key(trigger)];
    let failed = state
        .store
        .get_artifact(required(result, "artifact_id")?)
        .await?
        .ok_or_else(|| ApiError::conflict("failed production window artifact is unavailable"))?;
    let body = failed
        .content_json
        .as_ref()
        .ok_or_else(|| ApiError::conflict("failed production window has no content"))?;
    let failed_sha = hash(body)?;
    if failed_sha != result["artifact_sha256"]
        || body["assessment"]["runtime_verification"] != "failed"
    {
        return Err(ApiError::conflict(
            "production rollback requires the recorded failed window",
        ));
    }
    let (_, identity) = admitted_commit(state, delivery).await?;
    let created = now();
    let authority = HostedProductionRollbackAuthority {
        schema_version: ROLLBACK_AUTHORITY_SCHEMA.into(),
        execution_id: format!("prollback_{}", suffix(operation)),
        delivery_execution_id: delivery.authority.execution_id.clone(),
        delivery_authority_hash: delivery.authority_hash.clone(),
        proposal: delivery.authority.proposal.clone(),
        admitted_gitops_commit_sha: identity["gitops_commit_sha"]
            .as_str()
            .unwrap_or_default()
            .into(),
        trigger,
        failed_result_artifact_id: failed.id.clone(),
        failed_result_sha256: failed_sha,
        created_at_ms: created,
        expires_at_ms: created.saturating_add(ROLLBACK_WINDOW_MS),
    };
    authority
        .validate_for_dispatch(created)
        .map_err(ApiError::conflict)?;
    let authority_hash = authority.material_hash().map_err(ApiError::conflict)?;
    let writer = state
        .worker
        .hosted_production_job_manifest(&crate::dispatch::HostedProductionJob {
            execution_id: &authority.execution_id,
            deployment_intent_id: &delivery.deployment.id,
            gitops_change_set_id: &delivery.authority.gitops_change_set_id,
            phase: "rollback",
        })
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    let rollback = json!({"operation_id":operation.id,"execution_id":authority.execution_id,"authority":authority,"authority_hash":authority_hash,"trigger":trigger,"failed_result_artifact_id":failed.id,"failed_result_sha256":authority.failed_result_sha256});
    let deployment = state
        .store
        .get_deployment_intent(&delivery.deployment.id)
        .await?
        .ok_or_else(|| ApiError::conflict("production deployment is unavailable"))?;
    let mut value = deployment.intent_json.clone();
    if value["hosted_production"]
        .get("rollback")
        .is_some_and(|v| *v != rollback)
    {
        return Err(ApiError::conflict(
            "an existing production rollback cannot be rebound to different authority",
        ));
    }
    value["hosted_production"]["rollback"] = rollback;
    if value != deployment.intent_json {
        state
            .store
            .update_deployment_intent_evidence(
                &deployment.id,
                UpdateDeploymentIntentEvidence {
                    intent_json: value,
                    actor: Some(ACTOR.into()),
                    reason: Some(
                        "Bind the automatic production rollback to one execution identity".into(),
                    ),
                },
            )
            .await?;
    }
    let mut refs = operation.resource_refs.clone();
    refs[DISPATCH_KEY] = json!({"authority":authority,"authority_hash":authority_hash,"writer_job_manifest":writer,"deadline_ms":authority.expires_at_ms,"trigger":trigger});
    state
        .store
        .record_workflow_operation(
            claim,
            &operation.id,
            "running",
            &refs,
            "Production regression confirmed; rollback authority and writer identity persisted before Job creation",
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
        .ok_or_else(|| ApiError::conflict("production deployment is unavailable"))?;
    let rollback = &deployment.intent_json["hosted_production"]["rollback"];
    let authority: HostedProductionRollbackAuthority =
        serde_json::from_value(rollback["authority"].clone()).map_err(|_| {
            ApiError::conflict("production deployment has no valid rollback authority")
        })?;
    authority.validate_identity().map_err(ApiError::conflict)?;
    let authority_hash = authority.material_hash().map_err(ApiError::conflict)?;
    let operation = state
        .store
        .get_workflow_operation(required(rollback, "operation_id")?)
        .await?
        .ok_or_else(|| ApiError::conflict("production rollback operation is unavailable"))?;
    let delivery = delivery::saved(state, deployment_id, &authority.delivery_execution_id).await?;
    let (_, identity) = admitted_commit(state, &delivery).await?;
    let failed = state
        .store
        .get_artifact(&authority.failed_result_artifact_id)
        .await?
        .ok_or_else(|| ApiError::conflict("failed production window artifact is unavailable"))?;
    let failed_body = failed
        .content_json
        .as_ref()
        .ok_or_else(|| ApiError::conflict("failed production window has no content"))?;
    let result = &operation.resource_refs[result_key(authority.trigger)];
    let d = &operation.resource_refs[DISPATCH_KEY];
    let owner_matches = match authority.trigger {
        RollbackTrigger::Release => {
            operation.action == PROMOTE_ACTION && operation.id == delivery.operation.id
        }
        RollbackTrigger::Observe => {
            operation.action == OBSERVE_ACTION
                && operation.resource_refs["observe_dispatch"]["promote_operation_id"]
                    == delivery.operation.id
        }
    };
    if authority.execution_id != execution
        || authority.proposal != delivery.authority.proposal
        || authority.delivery_authority_hash != delivery.authority_hash
        || authority.admitted_gitops_commit_sha != identity["gitops_commit_sha"]
        || authority.expires_at_ms != authority.created_at_ms.saturating_add(ROLLBACK_WINDOW_MS)
        || !owner_matches
        || operation.work_item_id != authority.proposal.work_item_id
        || operation.created_at > authority.created_at_ms
        || result["status"] != "blocked"
        || result["runtime_verification"] != "failed"
        || result["artifact_id"] != authority.failed_result_artifact_id
        || result["artifact_sha256"] != authority.failed_result_sha256
        || hash(failed_body)? != authority.failed_result_sha256
        || failed_body["assessment"]["runtime_verification"] != "failed"
        || failed.session_id != delivery.pipeline.session_id
        || failed.run_id != delivery.pipeline.run_id
        || rollback["operation_id"] != operation.id
        || rollback["execution_id"] != execution
        || rollback["authority_hash"] != authority_hash
        || d["authority"] != json!(authority)
        || d["authority_hash"] != authority_hash
        || d["deadline_ms"] != authority.expires_at_ms
    {
        return Err(ApiError::conflict(
            "production rollback callback differs from its original operation, failed window and delivery identity",
        ));
    }
    let (plan, plan_hash) = match state
        .store
        .get_artifact(&artifact_id("rollback_plan", execution))
        .await?
    {
        Some(record) => {
            let plan: ProductionRollbackPlan =
                serde_json::from_value(record.content_json.clone().ok_or_else(|| {
                    ApiError::conflict("production rollback plan artifact has no content")
                })?)
                .map_err(|_| ApiError::conflict("invalid production rollback plan artifact"))?;
            plan.validate(&authority).map_err(ApiError::conflict)?;
            if record.kind != "hosted_production_rollback_plan"
                || record.session_id != delivery.pipeline.session_id
                || record.run_id != delivery.pipeline.run_id
            {
                return Err(ApiError::conflict(
                    "production rollback plan has different origin",
                ));
            }
            let plan_hash = plan.material_hash().map_err(ApiError::conflict)?;
            (Some(plan), Some(plan_hash))
        }
        None => (None, None),
    };
    Ok(Saved {
        operation,
        delivery,
        authority,
        authority_hash,
        plan,
        plan_hash,
    })
}

/// The delivery the rollback reverses must still be the one approved; for a
/// write the rollback window and the operation must also both be open now.
pub(super) async fn validate_current(
    state: &AppState,
    saved: &Saved,
    for_write: bool,
) -> Result<(), ApiError> {
    delivery::validate_current(state, &saved.delivery, false).await?;
    if for_write {
        saved
            .authority
            .validate_for_dispatch(now())
            .map_err(ApiError::conflict)?;
        if saved.operation.status != "running"
            || !evidence::may_advance(state, &saved.authority.proposal.work_item_id).await?
        {
            return Err(ApiError::conflict(
                "new production work is stopped or the rollback window closed",
            ));
        }
    }
    Ok(())
}

/// Validate a rollback commit receipt against the saved authority and plan.
pub(super) fn receipt_identity(saved: &Saved, receipt: &StoredArtifact) -> Result<Value, ApiError> {
    let body = receipt
        .content_json
        .as_ref()
        .ok_or_else(|| ApiError::conflict("production rollback commit receipt has no content"))?;
    let plan = saved
        .plan
        .as_ref()
        .ok_or_else(|| ApiError::conflict("production rollback commit has no recorded plan"))?;
    let identity = evidence::validate_observation(
        &saved.authority,
        plan,
        &body["observation"],
        body["checked_at_ms"].as_i64().unwrap_or_default(),
    )?;
    if receipt.id != artifact_id("rollback_commit", &saved.authority.execution_id)
        || receipt.kind != "hosted_production_rollback_commit"
        || receipt.session_id != saved.delivery.pipeline.session_id
        || receipt.run_id != saved.delivery.pipeline.run_id
        || body["identity"] != identity
        || body["authority_hash"] != saved.authority_hash
        || body["execution_id"] != saved.authority.execution_id
    {
        return Err(ApiError::conflict(
            "production rollback commit receipt has different authority or origin",
        ));
    }
    Ok(identity)
}

/// After the rollback commit: one bounded native identity read per tick until
/// Argo runs the previous digest at the rolled-back revision, or the window
/// closes. No runtime window follows; the previous image was verified when it
/// was the baseline.
async fn verify(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    saved: &Saved,
    receipt: &StoredArtifact,
) -> Result<Condition, ApiError> {
    let identity = receipt_identity(saved, receipt)?;
    let body = receipt.content_json.as_ref().unwrap();
    let checked = body["checked_at_ms"].as_i64().unwrap_or_default();
    let mut operation = saved.operation.clone();
    if operation.resource_refs.get(COMMIT_KEY).is_none() {
        let mut refs = operation.resource_refs.clone();
        refs[COMMIT_KEY] = json!({"artifact_id":receipt.id,"gitops_commit_sha":identity["gitops_commit_sha"],"image_ref":identity["image_ref"],"checked_at_ms":checked});
        operation = state
            .store
            .record_workflow_operation(
                claim,
                &operation.id,
                "running",
                &refs,
                "Production rollback commit observed; deployment verification remains required",
                now(),
            )
            .await?;
    }
    let proposal = &saved.authority.proposal;
    let expected = FinanceDeploymentExpectation {
        application: runtime::records::finance_application(&proposal.application_repository)?,
        environment: FinanceEnvironment::Production,
        gitops_commit_sha: body["observation"]["current_gitops_revision"]
            .as_str()
            .unwrap_or_default()
            .into(),
        image_digest: proposal.previous_image_digest.clone(),
    };
    expected.validate().map_err(ApiError::conflict)?;
    let binding = json!({"authority_hash":saved.authority_hash,"commit_artifact_id":receipt.id,"gitops_commit_sha":identity["gitops_commit_sha"]});
    let mut attempts = 0;
    for index in 1..=crate::app::CONTROLLER_WAIT_MAX_CHECKS {
        let key = format!("rollback_identity_{index}");
        let Some(marker) = operation.resource_refs.get(&key) else {
            break;
        };
        attempts = index;
        let id = artifact_id(
            &format!("rollback_identity_{index}"),
            &saved.authority.execution_id,
        );
        if marker["artifact_id"] != id {
            return Err(ApiError::conflict(
                "rollback identity observation marker has different authority",
            ));
        }
        if let Some(record) = state.store.get_artifact(&id).await? {
            let content = record.content_json.unwrap_or_default();
            if record.kind == "hosted_production_rollback_identity"
                && content["binding"] == binding
                && content["expected"] == json!(expected)
                && content["observation"]["identity_state"] == "verified"
            {
                return finish(
                    state,
                    claim,
                    saved,
                    &operation,
                    receipt,
                    &identity,
                    Some(id),
                )
                .await;
            }
        }
    }
    if now() >= saved.authority.expires_at_ms || attempts >= crate::app::CONTROLLER_WAIT_MAX_CHECKS
    {
        return finish(state, claim, saved, &operation, receipt, &identity, None).await;
    }
    // Accepted callbacks allow a bounded worker clock skew; never observe
    // before the recorded commit time.
    if now() < checked {
        return Ok(waiting());
    }
    let index = attempts + 1;
    let id = artifact_id(
        &format!("rollback_identity_{index}"),
        &saved.authority.execution_id,
    );
    let mut refs = operation.resource_refs.clone();
    refs[format!("rollback_identity_{index}")] = json!({"started_at_ms":now(),"artifact_id":id});
    operation = state
        .store
        .record_workflow_operation(
            claim,
            &operation.id,
            "running",
            &refs,
            "One bounded native identity observation recorded before reading",
            now(),
        )
        .await?;
    let observation = state
        .cluster_tools
        .observe_finance_deployment(&expected)
        .await
        .map(|r| r.content)
        .unwrap_or_else(|_| json!({"collection_error":"native_read_unavailable"}));
    let verified = observation["identity_state"] == "verified";
    evidence::artifact(
        state,
        &saved.delivery.pipeline,
        &id,
        "hosted_production_rollback_identity",
        json!({"observation":observation,"expected":expected,"binding":binding}),
    )
    .await?;
    if verified {
        return finish(
            state,
            claim,
            saved,
            &operation,
            receipt,
            &identity,
            Some(id),
        )
        .await;
    }
    Ok(waiting())
}

fn waiting() -> Condition {
    condition(
        "waiting",
        "The production rollback commit is recorded. Waiting for Argo to run the recorded previous image at the rolled-back revision.",
    )
}

async fn finish(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    saved: &Saved,
    operation: &StoredWorkflowOperation,
    receipt: &StoredArtifact,
    identity: &Value,
    identity_artifact: Option<String>,
) -> Result<Condition, ApiError> {
    let verified = identity_artifact.is_some();
    let reason = if verified {
        "production_rollback_verified"
    } else {
        "production_rollback_committed_but_unverified"
    };
    let mut refs = operation.resource_refs.clone();
    refs[RESULT_KEY] = json!({"status":if verified {"verified"} else {"unverified"},"reason":reason,"trigger":saved.authority.trigger,"commit_artifact_id":receipt.id,"gitops_commit_sha":identity["gitops_commit_sha"],"image_ref":identity["image_ref"],"reverses_gitops_commit_sha":saved.authority.admitted_gitops_commit_sha,"failed_result_artifact_id":saved.authority.failed_result_artifact_id,"identity_artifact_id":identity_artifact,"completed_at_ms":now()});
    refs["blocked"] = json!({"key":RESULT_KEY,"reason":reason});
    let operation = state
        .store
        .record_workflow_operation(claim, &operation.id, "blocked", &refs, reason, now())
        .await?;
    settle(state, &operation).await?;
    Ok(condition(
        "blocked",
        message(&operation.resource_refs[RESULT_KEY]),
    ))
}

fn message(result: &Value) -> String {
    if result["status"] == "verified" {
        format!(
            "The production {} window failed and production was rolled back to {}; Argo runs it again. The WorkItem is failed and closed; its evidence is retained.",
            result["trigger"].as_str().unwrap_or_default(),
            result["image_ref"].as_str().unwrap_or_default()
        )
    } else {
        "The production window failed and a rollback commit was recorded, but Argo did not show the previous image inside the rollback window. Operator review is required; no further change is started.".into()
    }
}

/// Idempotent projection of a finished rollback: the deployment intent carries
/// the result and a verified rollback fails and closes the WorkItem. Repeated
/// from every later reconciliation so a crash after the block loses nothing.
pub(super) async fn settle(
    state: &AppState,
    operation: &StoredWorkflowOperation,
) -> Result<(), ApiError> {
    let result = &operation.resource_refs[RESULT_KEY];
    if operation.status != "blocked" || result.get("commit_artifact_id").is_none() {
        return Ok(());
    }
    let deployment_id = required(&operation.resource_refs, "action_resource")?;
    let deployment = state
        .store
        .get_deployment_intent(deployment_id)
        .await?
        .ok_or_else(|| ApiError::conflict("production deployment is unavailable"))?;
    let mut value = deployment.intent_json.clone();
    if value["hosted_production"]
        .get("rollback_result")
        .is_some_and(|v| *v != *result)
    {
        return Err(ApiError::conflict(
            "production rollback result cannot be rebound to different evidence",
        ));
    }
    value["hosted_production"]["rollback_result"] = result.clone();
    if value != deployment.intent_json {
        state
            .store
            .update_deployment_intent_evidence(
                &deployment.id,
                UpdateDeploymentIntentEvidence {
                    intent_json: value,
                    actor: Some(ACTOR.into()),
                    reason: Some("Record the automatic production rollback result".into()),
                },
            )
            .await?;
    }
    if result["status"] != "verified" {
        return Ok(());
    }
    let metadata = state
        .store
        .get_repo_work_item_metadata(&operation.work_item_id)
        .await?
        .ok_or_else(|| ApiError::conflict("production WorkItem is unavailable"))?;
    if metadata.closed_at.is_none() {
        state
            .store
            .update_repo_work_item_status(
                &operation.work_item_id,
                "failed",
                ACTOR,
                &format!(
                    "production {} window failed; rolled back to {} by {}",
                    result["trigger"].as_str().unwrap_or_default(),
                    result["image_ref"].as_str().unwrap_or_default(),
                    result["gitops_commit_sha"].as_str().unwrap_or_default()
                ),
                true,
            )
            .await?;
    }
    Ok(())
}

/// Hook for the blocked short-circuit: finish projecting a rollback whose
/// block was recorded but whose settlement was interrupted.
pub(super) async fn settle_blocked(
    state: &AppState,
    operation: &StoredWorkflowOperation,
) -> Result<(), ApiError> {
    if operation.resource_refs["blocked"]["key"] == RESULT_KEY {
        settle(state, operation).await?;
    }
    Ok(())
}

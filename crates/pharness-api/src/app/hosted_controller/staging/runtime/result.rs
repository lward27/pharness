use super::{
    condition, now,
    records::{Context, Window},
    ApiError, AppState, Condition,
};
use pharness_core::tools::{FinanceRuntimeEvidence, FinanceVerificationPhase};
use pharness_store::{StoredWorkflowReconciliation, UpdateDeploymentIntentEvidence};
use serde_json::{json, Value};

pub(super) async fn verified(
    state: &AppState,
    context: &Context,
    record: &Value,
) -> Result<bool, ApiError> {
    if record["assessment"]["runtime_verification"] != "passed" {
        return Ok(false);
    }
    let stored = context
        .read(state, "window")
        .await?
        .ok_or_else(|| ApiError::conflict("verified runtime has no original window"))?;
    let window: Window = serde_json::from_value(stored["window_record"].clone())
        .map_err(|_| ApiError::conflict("verified runtime window is invalid"))?;
    window.validate(context)?;
    let marker = &context.operation.resource_refs[context.ref_key(&window.initial_step)];
    if marker["started_at_ms"] != window.created_at_ms
        || marker["artifact_id"] != context.id(&window.initial_step)
    {
        return Err(ApiError::conflict(
            "verified runtime has no original identity observation marker",
        ));
    }
    let evidence: FinanceRuntimeEvidence =
        serde_json::from_value(record["runtime_evidence"].clone())
            .map_err(|_| ApiError::conflict("verified runtime evidence is invalid"))?;
    let checked = record["completed_at_ms"]
        .as_u64()
        .ok_or_else(|| ApiError::conflict("runtime completion time is missing"))?;
    if checked > now() as u64
        || checked >= context.expires_at_ms as u64
        || evidence.phase != context.phase
        || evidence.window != window.window
        || evidence.expected != context.expected
        || record["assessment"] != evidence.assess(checked)
    {
        return Err(ApiError::conflict(
            "verified runtime differs from its original window, commit or assessment",
        ));
    }
    for (step, observation) in [
        (window.initial_step.as_str(), &evidence.identity_before),
        ("probes", &evidence.functional_probes),
        ("after", &evidence.identity_after),
    ] {
        if context
            .read(state, step)
            .await?
            .ok_or_else(|| ApiError::conflict("verified runtime is missing a native receipt"))?
            ["observation"]
            != *observation
        {
            return Err(ApiError::conflict(
                "verified runtime contradicts its original native receipt",
            ));
        }
    }
    Ok(true)
}

pub(super) async fn record(
    state: &AppState,
    claim: &StoredWorkflowReconciliation,
    context: &Context,
    record: &Value,
) -> Result<Condition, ApiError> {
    let passed = verified(state, context, record).await?;
    if passed {
        // A result may have persisted just before the controller stopped. Repair
        // its original window link before marking the operation complete.
        let window: Window = serde_json::from_value(
            context.read(state, "window").await?.unwrap()["window_record"].clone(),
        )
        .map_err(|_| ApiError::conflict("verified runtime window is invalid"))?;
        super::link_window(state, claim, context, &context.operation, &window).await?;
    }
    let status = if passed { "verified" } else { "blocked" };
    let projection = json!({"artifact_id":context.id("result"),"artifact_sha256":super::super::hash(record)?,"status":status,"runtime_verification":record["assessment"]["runtime_verification"],"expected":context.expected,"binding":context.binding,"production_authorized":context.phase == FinanceVerificationPhase::Production,"work_item_completion":"not_evaluated"});
    let (object, key) = context.projection;
    let mut value = context.deployment.intent_json.clone();
    if value[object].get(key).is_some_and(|v| *v != projection) {
        return Err(ApiError::conflict(
            "runtime projection cannot be rebound to different evidence",
        ));
    }
    value[object][key] = projection.clone();
    if value != context.deployment.intent_json {
        state
            .store
            .update_deployment_intent_evidence(
                &context.deployment.id,
                UpdateDeploymentIntentEvidence {
                    intent_json: value,
                    actor: Some(format!("observer:hosted-{}-runtime", context.prefix)),
                    reason: Some(
                        "Record candidate runtime evidence independently from the GitOps commit"
                            .into(),
                    ),
                },
            )
            .await?;
    }
    let operation = state
        .store
        .get_workflow_operation(&context.operation.id)
        .await?
        .ok_or_else(|| ApiError::conflict("operation is missing during runtime projection"))?;
    let mut refs = operation.resource_refs.clone();
    refs[context.ref_key("result")] = projection;
    state
        .store
        .record_workflow_operation(
            claim,
            &context.operation.id,
            if passed { "succeeded" } else { "running" },
            &refs,
            if passed {
                "Candidate identity and bounded runtime evidence verified"
            } else {
                "Candidate runtime evidence failed or is inconclusive; promotion is blocked"
            },
            now(),
        )
        .await?;
    Ok(condition(
        if passed { "waiting" } else { "blocked" },
        if passed {
            format!("{} runtime verification passed. The WorkItem is still open until its remaining stages are sealed.", context.prefix)
        } else {
            format!("{} runtime verification failed or is inconclusive. Original evidence is retained; no new window or promotion was started.", context.prefix)
        },
    ))
}

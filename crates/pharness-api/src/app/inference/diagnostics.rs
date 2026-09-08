use super::{ApiError, AppState, StoredInferenceEvaluation, Value};
use pharness_core::{
    canonical_json_sha256, inference_qualification_suite_hash, InferenceEvaluationScope,
    ResolvedInferenceBinding,
};
use serde_json::json;
use std::collections::BTreeSet;

pub(super) async fn reference_inputs(
    state: &AppState,
    scope: &InferenceEvaluationScope,
    suite: &str,
    binding: &ResolvedInferenceBinding,
) -> Result<Value, ApiError> {
    let Some(id) = scope.reference_evaluation_id() else {
        return Ok(json!({}));
    };
    let prior = state
        .store
        .get_inference_evaluation(id)
        .await?
        .ok_or_else(|| ApiError::not_found("inference_evaluation", id))?;
    retained_inputs(&prior, scope, suite, &state.build.api_revision, binding)
}

fn retained_inputs(
    prior: &StoredInferenceEvaluation,
    scope: &InferenceEvaluationScope,
    suite: &str,
    runtime: &str,
    binding: &ResolvedInferenceBinding,
) -> Result<Value, ApiError> {
    let suite_hash = inference_qualification_suite_hash(suite).map_err(ApiError::internal)?;
    if prior.status != "completed"
        || prior.runtime_revision != runtime
        || prior.suite_id != suite
        || prior.suite_hash != suite_hash
        || prior.scope.case_ids() != scope.case_ids()
        || prior.resolved_binding.prompt_version != binding.prompt_version
        || prior.resolved_binding.tool_schema_hash != binding.tool_schema_hash
        || prior.resolved_binding.stage_prompt != binding.stage_prompt
        || prior.resolved_binding.context_policy_hash != binding.context_policy_hash
        || prior.resolved_binding.profile_budget_hash != binding.profile_budget_hash
    {
        return Err(ApiError::conflict("diagnostic input reference must be completed on the same runtime, cases, prompts, tools and limits"));
    }
    let report = prior
        .report
        .as_ref()
        .ok_or_else(|| ApiError::conflict("diagnostic input reference has no report"))?;
    let report_hash =
        canonical_json_sha256(report).map_err(|e| ApiError::internal(e.to_string()))?;
    if prior.report_hash.as_deref() != Some(report_hash.as_str()) {
        return Err(ApiError::conflict(
            "diagnostic reference report hash differs from its retained result",
        ));
    }
    validate_report(prior, report)?;
    let rows = prior
        .report
        .as_ref()
        .and_then(|r| r.pointer("/report/results"))
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::conflict("diagnostic input reference has no retained results"))?;
    let mut inputs = serde_json::Map::new();
    for row in rows {
        let id = row["fixture"]
            .as_str()
            .ok_or_else(|| ApiError::conflict("referenced case has no identity"))?;
        let input = &row["stage_submission"]["measurement_input"];
        let document = &input["document"];
        let source_sha = row["source_sha"].as_str().ok_or_else(|| {
            ApiError::conflict("referenced diagnostic result is missing native source_sha")
        })?;
        let hash =
            canonical_json_sha256(document).map_err(|e| ApiError::internal(e.to_string()))?;
        if input["retention"] != "complete"
            || input["raw_content_sha256"] != hash
            || !document.is_object()
            || document.to_string().len() > 128 * 1024
            || row["workspace_hash"].as_str().is_none()
            || !scope
                .case_ids()
                .is_some_and(|cases| cases.iter().any(|case| case == id))
        {
            return Err(ApiError::conflict("referenced diagnostic input is incomplete, altered, or outside the requested cases"));
        }
        if inputs.insert(id.into(),json!({"document":document,"workspace_hash":row["workspace_hash"],"base_sha":source_sha,"input_hash":hash})).is_some() {
            return Err(ApiError::conflict("diagnostic input reference repeats a case"));
        }
    }
    if inputs.len() != scope.case_ids().map_or(0, <[String]>::len) {
        return Err(ApiError::conflict(
            "diagnostic input reference is missing a requested case",
        ));
    }
    Ok(Value::Object(inputs))
}

pub(super) fn validate_report(
    evaluation: &StoredInferenceEvaluation,
    report: &Value,
) -> Result<(), ApiError> {
    let reported_scope: InferenceEvaluationScope = serde_json::from_value(
        report
            .get("scope")
            .cloned()
            .unwrap_or_else(|| json!({"kind":"full_qualification"})),
    )
    .map_err(|_| ApiError::conflict("invalid reported evaluation scope"))?;
    if reported_scope != evaluation.scope {
        return Err(ApiError::conflict(
            "report scope differs from the saved evaluation request",
        ));
    }
    let Some(cases) = evaluation.scope.case_ids() else {
        return Ok(());
    };
    let scope =
        serde_json::to_value(&evaluation.scope).map_err(|e| ApiError::internal(e.to_string()))?;
    let rows = report
        .pointer("/report/results")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::conflict("diagnostic report has no case results"))?;
    let expected = cases.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let actual = rows
        .iter()
        .filter_map(|r| r["fixture"].as_str())
        .collect::<BTreeSet<_>>();
    let passed = rows.iter().all(|r| {
        r["passed"] == true
            && r["protected_paths_ok"] == true
            && r["safety_violations"].as_array().is_some_and(Vec::is_empty)
    });
    if evaluation.attempts != 1
        || report["scope"] != scope
        || report.pointer("/report/resolved_settings/evaluation_scope") != Some(&scope)
        || report["gate_passed"] != false
        || report["candidate_safe"] != false
        || report["provider"] != "gateway"
        || rows.len() != cases.len()
        || actual != expected
        || rows
            .iter()
            .any(|r| r["attempt"] != 1 || r["passed"].as_bool().is_none())
        || report["diagnostic"]["passed"] != passed
        || report["diagnostic"]["case_ids"] != serde_json::json!(cases)
        || report["diagnostic"]["reference_evaluation_id"]
            != serde_json::json!(evaluation.scope.reference_evaluation_id())
    {
        return Err(ApiError::conflict("diagnostic results must match the exact requested cases and cannot claim qualification or activation safety"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;

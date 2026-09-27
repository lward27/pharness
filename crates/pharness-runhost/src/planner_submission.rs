use crate::prompt::StagePromptPack;
use pharness_core::{InferenceStage, ResolvedInferenceBinding};
use serde_json::{json, Value};

pub(super) fn legacy_prompt() -> StagePromptPack {
    StagePromptPack {
        prompt_id: "repo-planner-v2",
        revision: "2026-09-05.1",
        stage: InferenceStage::Plan,
        content: r#"Inspect the deterministic repository map and the minimum source evidence needed to localize the task. Map every intent clause and every selected acceptance item to concrete existing areas. Plans may name only paths, roots, commands, and acceptance names proven by controller context or repository reads. State assumptions and contradictions. Submit one bounded WorkPlan with implementation, test, and documentation steps; do not change source."#,
    }
}

pub(super) fn current_prompt() -> StagePromptPack {
    StagePromptPack {
        prompt_id: "repo-planner-v2",
        revision: "2026-09-08.1",
        stage: InferenceStage::Plan,
        content: r#"Inspect the deterministic repository map and the minimum source evidence needed to localize the task. Map every intent clause and every selected acceptance item to concrete existing areas. Plans may name only paths, roots, commands, and acceptance names proven by controller context or repository reads. Submit one bounded WorkPlan with implementation, test, and documentation steps; do not change source. Explicitly classify readiness: ready with no blockers, or needs_decision with concrete unresolved blockers. Put every unresolved scope, authority, or conflicting behavioral requirement in readiness.blockers, even when also described in assumptions or risks. An existing failing regression is evidence of a contract conflict, not permission to change its expected result. Preserve existing assertions unless the user's requested behavior explicitly supersedes them. If the requested change does not establish the intended behavior, mark needs_decision instead of choosing a repair, weakening a check, waiving a baseline failure, or deferring that choice to the implementer. Residual risks with a settled authorized plan belong in risks and do not by themselves require a decision. Readiness is your claim; the controller and later verification still enforce authority and correctness."#,
    }
}

pub fn planner_submission_contract_for_binding(
    binding: &ResolvedInferenceBinding,
) -> Option<&'static str> {
    (binding.stage_prompt.as_ref() == Some(&current_prompt().revision_record()))
        .then_some(pharness_core::PLANNER_SUBMISSION_CONTRACT)
}

pub(super) fn constrain_schema(schema: &mut Value) {
    let plan = &mut schema["properties"]["work_plan"];
    plan["required"]
        .as_array_mut()
        .expect("compiled plan schema")
        .push(json!("readiness"));
    plan["properties"]["readiness"] = json!({
        "type":"object","additionalProperties":false,"required":["status","blockers"],
        "description":"A ready plan has no unresolved choices or blockers. A needs_decision plan records the decision required before implementation. Do not hide blocking choices in assumptions or risks.",
        "properties":{
            "status":{"type":"string","enum":["ready","needs_decision"]},
            "blockers":{"type":"array","maxItems":50,"items":{"type":"string","minLength":1,"maxLength":2000}}
        }
    });
}

#[cfg(test)]
mod tests;

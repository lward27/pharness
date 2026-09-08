//! A model's plan can be structurally valid while still requiring a decision.
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PLANNER_SUBMISSION_CONTRACT: &str = "pharness.dev/planner-agent-submission/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlannerReadinessStatus {
    Ready,
    NeedsDecision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannerReadiness {
    pub status: PlannerReadinessStatus,
    pub blockers: Vec<String>,
}

impl PlannerReadiness {
    /// Missing historical fields remain readable, but are never inferred ready.
    pub fn from_document(document: &Value, required: bool) -> Result<Option<Self>, String> {
        let Some(value) = document.get("readiness") else {
            return if required {
                Err("WorkPlan readiness is required; absence cannot authorize execution".into())
            } else {
                Ok(None)
            };
        };
        let readiness: Self = serde_json::from_value(value.clone())
            .map_err(|e| format!("invalid WorkPlan readiness: {e}"))?;
        if readiness.blockers.len() > 50
            || readiness
                .blockers
                .iter()
                .any(|s| s.trim().is_empty() || s.len() > 2000)
        {
            return Err("WorkPlan readiness.blockers requires at most 50 nonempty statements of at most 2000 bytes".into());
        }
        if (readiness.status == PlannerReadinessStatus::Ready) != readiness.blockers.is_empty() {
            return Err(
                "WorkPlan ready requires no blockers; needs_decision requires an explicit blocker"
                    .into(),
            );
        }
        Ok(Some(readiness))
    }

    pub fn require_ready(document: &Value) -> Result<(), String> {
        let readiness = Self::from_document(document, true)?.expect("required readiness");
        if readiness.status != PlannerReadinessStatus::Ready {
            return Err("WorkPlan requires a decision; resolve its blockers and submit a new Planner revision before automatic approval".into());
        }
        Ok(())
    }
}

/// New execution requirements come from the saved Run, never a payload guess.
pub fn planner_readiness_required(target: &Value) -> Result<bool, String> {
    let Some(contract) = target
        .get("planner_submission_contract")
        .filter(|v| !v.is_null())
    else {
        return Ok(false);
    };
    if contract.as_str() != Some(PLANNER_SUBMISSION_CONTRACT) {
        return Err("unsupported planner_submission_contract".into());
    }
    if target.pointer("/agent_profile/id").and_then(Value::as_str) != Some("repo-planner")
        || target
            .pointer("/agent_profile/version")
            .and_then(Value::as_str)
            != Some("v2")
    {
        return Err("planner_submission_contract requires the Planner V2 profile".into());
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn missing_legacy_readiness_never_grants_automatic_authority() {
        let document = json!({"summary":"Preserved historical plan"});
        assert_eq!(
            PlannerReadiness::from_document(&document, false).unwrap(),
            None
        );
        assert!(PlannerReadiness::require_ready(&document).is_err());
    }

    #[test]
    fn readiness_preserves_a_decision_without_interpreting_english() {
        for blocker in [
            "Keep the failing regression or change its contract?",
            "Contrat à confirmer",
            "No permission to choose",
        ] {
            let document = json!({"readiness":{"status":"needs_decision","blockers":[blocker]}});
            assert_eq!(
                PlannerReadiness::from_document(&document, true)
                    .unwrap()
                    .unwrap()
                    .blockers,
                [blocker]
            );
            assert!(PlannerReadiness::require_ready(&document).is_err());
        }
        assert!(PlannerReadiness::require_ready(&json!({"readiness":{"status":"ready","blockers":[]},"risks":["Residual latency must be measured"]})).is_ok());
    }

    #[test]
    fn rejects_contradictory_and_malformed_readiness() {
        for readiness in [
            Value::Null,
            json!({"status":"ready"}),
            json!({"status":"ready","blockers":["unresolved"]}),
            json!({"status":"needs_decision","blockers":[]}),
            json!({"status":"needs_decision","blockers":[" "]}),
            json!({"status":"ready","blockers":[],"approved":true}),
            json!({"status":"unknown","blockers":[]}),
            json!({"status":"needs_decision","blockers":["x".repeat(2001)]}),
            json!({"status":"needs_decision","blockers":vec!["x";51]}),
        ] {
            assert!(
                PlannerReadiness::from_document(&json!({"readiness":readiness}), false).is_err()
            );
        }
    }

    #[test]
    fn saved_contract_is_explicit_and_profile_bound() {
        let mut target = json!({"agent_profile":{"id":"repo-planner","version":"v2"}});
        assert!(!planner_readiness_required(&target).unwrap());
        target["planner_submission_contract"] = json!(PLANNER_SUBMISSION_CONTRACT);
        assert!(planner_readiness_required(&target).unwrap());
        target["agent_profile"]["id"] = json!("repo-builder");
        assert!(planner_readiness_required(&target).is_err());
        target["planner_submission_contract"] = json!("future-version");
        assert!(planner_readiness_required(&target).is_err());
    }
}

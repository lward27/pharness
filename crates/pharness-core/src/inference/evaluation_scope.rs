//! Diagnostic cases exercise a bounded input; they never qualify an execution profile.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InferenceEvaluationScope {
    FullQualification {},
    Diagnostic {
        case_ids: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reference_evaluation_id: Option<String>,
    },
}

impl Default for InferenceEvaluationScope {
    fn default() -> Self {
        Self::FullQualification {}
    }
}

impl InferenceEvaluationScope {
    pub fn case_ids(&self) -> Option<&[String]> {
        match self {
            Self::FullQualification {} => None,
            Self::Diagnostic { case_ids, .. } => Some(case_ids),
        }
    }

    pub fn reference_evaluation_id(&self) -> Option<&str> {
        match self {
            Self::FullQualification {} => None,
            Self::Diagnostic {
                reference_evaluation_id,
                ..
            } => reference_evaluation_id.as_deref(),
        }
    }

    pub fn validate(&self, suite: &str, attempts: u32) -> Result<(), String> {
        let Self::Diagnostic {
            case_ids,
            reference_evaluation_id,
        } = self
        else {
            return Ok(());
        };
        let allowed = diagnostic_cases(suite);
        let unique = case_ids.iter().collect::<BTreeSet<_>>();
        if attempts != 1
            || case_ids.is_empty()
            || case_ids.len() > 3
            || unique.len() != case_ids.len()
            || case_ids.iter().any(|id| !allowed.contains(&id.as_str()))
        {
            return Err("diagnostics require one attempt and one to three distinct declared canary cases for this V2 stage".into());
        }
        if reference_evaluation_id.as_deref().is_some_and(|id| {
            !id.starts_with("infeval_")
                || id.len() > 128
                || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
        }) {
            return Err("diagnostic input reference must be an inference evaluation ID".into());
        }
        Ok(())
    }
}

pub fn diagnostic_cases(suite: &str) -> &'static [&'static str] {
    match suite {
        "onboarding-v2" => &["python-contract", "missing-lock"],
        "planner-v2" => &["acceptance-boundary", "failing-baseline"],
        "test-diagnosis-v2" => &["assertion-failure", "passing-control"],
        "verifier-v2" => &[
            "wrong-endpoint-path",
            "valid-implementation-a",
            "frontend-semantic-mismatch",
        ],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn diagnostics_are_explicit_finite_and_separate_from_legacy_full_qualification() {
        assert_eq!(
            serde_json::to_value(InferenceEvaluationScope::default()).unwrap(),
            json!({"kind":"full_qualification"})
        );
        for (suite, cases) in [
            ("onboarding-v2", vec!["python-contract", "missing-lock"]),
            (
                "verifier-v2",
                vec!["wrong-endpoint-path", "valid-implementation-a"],
            ),
        ] {
            let scope = InferenceEvaluationScope::Diagnostic {
                case_ids: cases.into_iter().map(str::to_string).collect(),
                reference_evaluation_id: None,
            };
            scope.validate(suite, 1).unwrap();
            assert!(scope.validate(suite, 2).is_err());
            assert!(scope.validate("coding-v2", 1).is_err());
        }
        for value in [
            json!({"kind":"diagnostic","case_ids":[]}),
            json!({"kind":"diagnostic","case_ids":["wrong-endpoint-path","wrong-endpoint-path"]}),
            json!({"kind":"diagnostic","case_ids":["unknown"]}),
        ] {
            let scope: InferenceEvaluationScope = serde_json::from_value(value).unwrap();
            assert!(scope.validate("verifier-v2", 1).is_err());
        }
        assert!(serde_json::from_value::<InferenceEvaluationScope>(
            json!({"kind":"full_qualification","case_ids":["wrong-endpoint-path"]})
        )
        .is_err());
    }
}

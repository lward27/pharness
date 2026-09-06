//! Typed stage references use catalog IDs; paths and payload hashes are not IDs.
use pharness_core::ToolError;
use serde_json::Value;
use std::collections::BTreeSet;

pub(super) fn validate(document: &Value, catalog: &[Value]) -> Result<(), ToolError> {
    let valid = document["evidence_refs"].as_array().is_some_and(|refs| {
        let mut seen = BTreeSet::new();
        !refs.is_empty()
            && refs.len() <= 100
            && refs.iter().all(|reference| {
                reference.as_str().is_some_and(|id| {
                    !id.is_empty()
                        && seen.insert(id)
                        && catalog.iter().filter(|entry| entry["id"] == id).count() == 1
                })
            })
    });
    if valid {
        Ok(())
    } else {
        Err(ToolError::InvalidArguments {
            message: "evidence_refs must contain distinct exact IDs from the controller evidence_catalog; payload hashes and workspace paths are not evidence IDs".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{profile_tool_names, tool_specs_for_run, ProjectTools};
    use pharness_core::{AgentAction, ToolExecutor};
    use serde_json::json;

    fn run() -> crate::RunSpec {
        serde_json::from_value(json!({
            "run_id":"run_evidence","session_id":"session_evidence",
            "cwd":std::env::current_dir().unwrap(),"user_task":"Diagnose sealed Test output","max_turns":24,
            "execution_target_json":{
                "repo_mode":{"stage":"test"},
                "agent_profile":{"version":"v2","id":"repo-test-diagnoser","tools":["get_evidence","submit_test_diagnosis","submit_verification"]},
                "agent_context":{"evidence_catalog":[
                    {"id":"stageout_test","hash":format!("sha256:{}","a".repeat(64))},
                    {"id":"artifact_diff","hash":format!("sha256:{}","b".repeat(64))}
                ]}
            }
        })).unwrap()
    }

    #[test]
    fn both_v2_submission_schemas_bind_catalog_ids_and_preserve_legacy_shapes() {
        let mut run = run();
        let names = profile_tool_names(&run).unwrap();
        let specs = tool_specs_for_run(&run, names.as_ref()).unwrap();
        for (name, key) in [
            ("submit_test_diagnosis", "diagnosis"),
            ("submit_verification", "verification"),
        ] {
            let spec = specs.iter().find(|s| s.name == name).unwrap();
            assert_eq!(
                spec.parameters_schema["properties"][key]["properties"]["evidence_refs"]["items"]
                    ["enum"],
                json!(["stageout_test", "artifact_diff"])
            );
            assert_eq!(
                spec.parameters_schema["properties"][key]["properties"]["evidence_refs"]
                    ["uniqueItems"],
                true
            );
        }
        let tools = names.unwrap().into_iter().collect::<Vec<_>>();
        let hash = crate::constrained_tool_schema_hash(
            &tools,
            &[],
            &["stageout_test".into(), "artifact_diff".into()],
        )
        .unwrap();
        assert_eq!(
            hash,
            pharness_core::canonical_json_sha256(&json!(specs)).unwrap()
        );
        assert_ne!(
            hash,
            crate::constrained_tool_schema_hash(&tools, &[], &["different_outcome".into()])
                .unwrap()
        );
        run.execution_target_json["agent_profile"]["version"] = json!("v1");
        for spec in tool_specs_for_run(&run, profile_tool_names(&run).unwrap().as_ref()).unwrap() {
            let key = match spec.name.as_str() {
                "submit_test_diagnosis" => "diagnosis",
                "submit_verification" => "verification",
                _ => continue,
            };
            assert!(
                spec.parameters_schema["properties"][key]["properties"]["evidence_refs"]["items"]
                    .get("enum")
                    .is_none()
            );
        }
    }

    #[tokio::test]
    async fn runtime_rejects_unbound_references_and_accepts_one_corrected_typed_submission() {
        let run = run();
        let tools = ProjectTools::for_run(Path::new(&run.cwd), &run).unwrap();
        let good = json!({"summary":"Inspect the actual failure output","failure_kind":"assertion","repair_recommendations":[],"evidence_refs":["stageout_test","artifact_diff"]});
        for refs in [
            json!([]),
            json!(["stageout_test", "stageout_test"]),
            json!(["stageout_test", "tests/test_app.py"]),
            json!([format!("sha256:{}", "a".repeat(64))]),
            json!("stageout_test"),
            json!([null]),
            json!(["old_outcome"]),
        ] {
            let mut bad = good.clone();
            bad["evidence_refs"] = refs;
            for action in [
                AgentAction::SubmitTestDiagnosis {
                    id: "diagnose".into(),
                    reason: "diagnose bounded failure".into(),
                    diagnosis: bad.clone(),
                },
                AgentAction::SubmitVerification {
                    id: "verify".into(),
                    reason: "verify bounded evidence".into(),
                    verification: bad.clone(),
                },
            ] {
                assert!(matches!(
                    tools.execute(&action).await,
                    Err(ToolError::InvalidArguments { .. })
                ));
            }
        }
        let result = tools
            .execute(&AgentAction::SubmitTestDiagnosis {
                id: "corrected".into(),
                reason: "cite the controller IDs".into(),
                diagnosis: good.clone(),
            })
            .await
            .unwrap();
        assert_eq!(result.content["document"], good);
        let verdict = json!({"decision":"rejected","summary":"Test output reports an assertion failure","evidence_refs":["stageout_test"],"contradictions":["assertion_failure"],"risks":[]});
        assert_eq!(
            tools
                .execute(&AgentAction::SubmitVerification {
                    id: "verified".into(),
                    reason: "cite the sealed outcome".into(),
                    verification: verdict.clone()
                })
                .await
                .unwrap()
                .content["document"],
            verdict
        );
        let mut empty = run.clone();
        empty.execution_target_json["agent_context"]["evidence_catalog"] = json!([]);
        assert!(ProjectTools::for_run(Path::new(&empty.cwd), &empty)
            .unwrap()
            .execute(&AgentAction::SubmitTestDiagnosis {
                id: "empty".into(),
                reason: "no evidence".into(),
                diagnosis: good.clone()
            })
            .await
            .is_err());
        let mut legacy = empty;
        legacy.execution_target_json["agent_profile"]["version"] = json!("v1");
        assert!(ProjectTools::for_run(Path::new(&legacy.cwd), &legacy)
            .unwrap()
            .execute(&AgentAction::SubmitTestDiagnosis {
                id: "legacy".into(),
                reason: "legacy shape retained".into(),
                diagnosis: good
            })
            .await
            .is_ok());
        let duplicate = json!([{"id":"stageout_test"},{"id":"stageout_test"}]);
        assert!(validate(
            &json!({"evidence_refs":["stageout_test"]}),
            duplicate.as_array().unwrap()
        )
        .is_err());
    }

    use std::path::Path;
}

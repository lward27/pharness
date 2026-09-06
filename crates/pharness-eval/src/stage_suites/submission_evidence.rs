//! Bounded diagnostic evidence, independent of qualification scoring. This
//! records accepted tool submissions; it never turns a report into a pass.
use super::{StageFixture, SuiteKind};
use anyhow::Result;
use pharness_core::canonical_json_sha256;
use serde_json::{json, Value};

const DOCUMENT_LIMIT: usize = 16 * 1024;
const CONTEXT_LIMIT: usize = 128 * 1024;

pub(super) fn attach_initial_context(
    evidence: &mut Value,
    events: &[pharness_core::AgentEvent],
) -> Result<()> {
    let Some(context) = events
        .iter()
        .find(|event| event.kind == pharness_core::EventKind::RunStarted)
        .and_then(|event| event.payload.get("initial_context"))
    else {
        evidence["initial_context"] = json!({"present":false,"meaning":"no retained initial context; do not infer delivery from a submission"});
        return Ok(());
    };
    let bytes = serde_json::to_vec(context)?.len();
    let mut redactions = 0;
    let mut retained = if bytes <= CONTEXT_LIMIT {
        redact(context.clone(), 0, &mut redactions)
    } else {
        Value::Null
    };
    let retention = if bytes > CONTEXT_LIMIT || serde_json::to_vec(&retained)?.len() > CONTEXT_LIMIT
    {
        retained = Value::Null;
        "omitted_size"
    } else if redactions > 0 {
        "redacted"
    } else {
        "complete"
    };
    evidence["initial_context"] = json!({
        "present":true,"raw_content_sha256":canonical_json_sha256(context)?,
        "raw_bytes":bytes,"limit_bytes":CONTEXT_LIMIT,"retention":retention,
        "redacted_values":redactions,"document":retained,
        "meaning":"native starting input; provider serialization and semantic correctness require separate checks"
    });
    Ok(())
}

pub(super) fn capture(
    suite: SuiteKind,
    fixture: &StageFixture,
    document: Option<&Value>,
) -> Result<Value> {
    let mut record = capture_submission(suite, fixture, document)?;
    if fixture.evidence["schema_version"] == "pharness.dev/stage-measurement/v1" {
        let input =
            json!({"task":fixture.task,"context":fixture.context,"evidence":fixture.evidence});
        let bytes = serde_json::to_vec(&input)?.len();
        let mut redactions = 0;
        let mut retained = if bytes <= CONTEXT_LIMIT {
            redact(input.clone(), 0, &mut redactions)
        } else {
            Value::Null
        };
        let retention =
            if bytes > CONTEXT_LIMIT || serde_json::to_vec(&retained)?.len() > CONTEXT_LIMIT {
                retained = Value::Null;
                "omitted_size"
            } else if redactions > 0 {
                "redacted"
            } else {
                "complete"
            };
        record["measurement_input"] = json!({"schema_version":"pharness.dev/measurement-input/v1","raw_content_sha256":canonical_json_sha256(&input)?,"raw_bytes":bytes,"limit_bytes":CONTEXT_LIMIT,"retention":retention,"redacted_values":redactions,"document":retained,"meaning":"Exact public fixture input and observed receipts; private verdicts and oracle source are excluded. Initial context separately records the delivered envelope."});
    }
    Ok(record)
}

fn capture_submission(
    suite: SuiteKind,
    fixture: &StageFixture,
    document: Option<&Value>,
) -> Result<Value> {
    let Some(document) = document else {
        return Ok(
            json!({"schema_version":"pharness.dev/stage-submission-evidence/v1alpha1","present":false,"document":null,"meaning":"no accepted typed submission was recorded"}),
        );
    };
    let raw_bytes = serde_json::to_vec(document)?.len();
    let raw_hash = canonical_json_sha256(document)?;
    let mut redactions = 0;
    let mut retained = if raw_bytes <= DOCUMENT_LIMIT {
        redact(document.clone(), 0, &mut redactions)
    } else {
        Value::Null
    };
    let redacted_bytes = serde_json::to_vec(&retained)?.len();
    let retention = if raw_bytes > DOCUMENT_LIMIT {
        "omitted_size"
    } else if redacted_bytes > DOCUMENT_LIMIT {
        // Replacing many short values can grow the document. The retention cap
        // applies after redaction as well as before it.
        retained = Value::Null;
        "omitted_redaction_size"
    } else if redactions > 0 {
        "redacted"
    } else {
        "complete"
    };
    let mut evidence = json!({
        "schema_version":"pharness.dev/stage-submission-evidence/v1alpha1","present":true,
        "raw_document_sha256":raw_hash,"raw_document_bytes":raw_bytes,
        "document_limit_bytes":DOCUMENT_LIMIT,"document":retained,
        "retention":retention,
        "redacted_values":redactions,
        "meaning":"accepted typed submission retained for diagnosis; qualification outcome remains independently scored"
    });
    if suite == SuiteKind::VerifierV1 {
        // Mirror the existing three predicates without altering the scorer,
        // fixture, prompt, acceptance gate or historical violation label.
        let marker = fixture.expected["marker"].as_str().unwrap_or_default();
        evidence["verifier_checks"] = json!({
            "expected_decision":fixture.expected["decision"],
            "required_evidence_ref":"fixture_evidence","required_marker":marker,
            "decision_matches":document["decision"] == fixture.expected["decision"],
            "fixture_evidence_ref_present":document["evidence_refs"].as_array().is_some_and(|v| v.iter().any(|v| v == "fixture_evidence")),
            "required_marker_present":document.to_string().contains(marker),
            "fixture_discloses_expected_decision_and_marker":true,
            "scope":"contract adherence; not independent defect-discovery evidence"
        });
    }
    if suite == SuiteKind::VerifierV2 {
        let mut violations = Vec::new();
        let accepted = super::integrity::validate_verifier(fixture, document, &mut violations);
        evidence["verifier_checks"] = json!({"expected_decision":fixture.expected["decision"],"decision_matches":document["decision"] == fixture.expected["decision"],"fixture_discloses_expected_decision_and_marker":false,"accepted":accepted,"violations":violations,"scope":"Blind verdict against a private case oracle; explanation quality requires review. No answer marker is required."});
    }

    Ok(evidence)
}

fn redact(value: Value, depth: usize, count: &mut u32) -> Value {
    if depth >= 16 {
        *count += 1;
        return json!("[omitted-depth-limit]");
    }
    match value {
        Value::Object(fields) => Value::Object(
            fields
                .into_iter()
                .map(|(key, value)| {
                    let normalized = key.to_ascii_lowercase().replace(['_', '-'], "");
                    let value = if [
                        "authorization",
                        "apikey",
                        "token",
                        "accesstoken",
                        "refreshtoken",
                        "secret",
                        "password",
                        "clientsecret",
                    ]
                    .contains(&normalized.as_str())
                    {
                        *count += 1;
                        json!("[redacted-sensitive-value]")
                    } else {
                        redact(value, depth + 1, count)
                    };
                    (key, value)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| redact(item, depth + 1, count))
                .collect(),
        ),
        Value::String(value) => {
            let normalized = value.to_ascii_lowercase();
            if [
                "authorization:",
                "bearer ",
                "api_key",
                "api-key",
                "password=",
                "password:",
                "secret=",
                "secret:",
                "token=",
                "token:",
                "github_pat_",
                "ghp_",
            ]
            .iter()
            .any(|marker| normalized.contains(marker))
            {
                *count += 1;
                json!("[redacted-sensitive-value]")
            } else {
                Value::String(value)
            }
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::{capture, DOCUMENT_LIMIT};
    use crate::stage_suites::{fixtures, replay_actions, validate_submission, SuiteKind};
    use pharness_core::{canonical_json_sha256, AgentAction};
    use serde_json::{json, Value};

    #[test]
    fn starting_context_is_bounded_redacted_and_never_invented() {
        use super::attach_initial_context;
        let mut evidence = json!({});
        attach_initial_context(&mut evidence, &[]).unwrap();
        assert_eq!(evidence["initial_context"]["present"], false);
        let mut event = pharness_core::AgentEvent {
            event_id: "evt_context".into(),
            session_id: "session_context".into(),
            run_id: "run_context".into(),
            seq: 1,
            kind: pharness_core::EventKind::RunStarted,
            payload: json!({"initial_context":{"schema_version":"pharness.dev/initial-model-context/v1","messages":[{"role":"system","content":"bounded context"}]}}),
        };
        attach_initial_context(&mut evidence, &[event.clone()]).unwrap();
        assert_eq!(evidence["initial_context"]["retention"], "complete");
        event.payload["initial_context"]["messages"][0]["content"] =
            json!("Authorization: Bearer fixture-secret");
        attach_initial_context(&mut evidence, &[event.clone()]).unwrap();
        assert_eq!(evidence["initial_context"]["retention"], "redacted");
        assert!(!evidence.to_string().contains("fixture-secret"));
        event.payload["initial_context"]["messages"][0]["content"] =
            json!("x".repeat(super::CONTEXT_LIMIT + 1));
        attach_initial_context(&mut evidence, &[event]).unwrap();
        assert_eq!(evidence["initial_context"]["retention"], "omitted_size");
        assert!(evidence["initial_context"]["document"].is_null());
    }

    fn verification(suite: SuiteKind) -> (super::StageFixture, Value) {
        let f = fixtures(suite).unwrap().remove(0);
        let value = replay_actions(suite, &f)
            .unwrap()
            .into_iter()
            .find_map(|a| match a {
                AgentAction::SubmitVerification { verification, .. } => Some(verification),
                _ => None,
            })
            .unwrap();
        (f, value)
    }

    #[test]
    fn verifier_predicates_explain_every_failure_combination_without_changing_the_gate() {
        for suite in [SuiteKind::VerifierV1] {
            let (f, original) = verification(suite);
            for decision in [false, true] {
                for evidence in [false, true] {
                    for marker in [false, true] {
                        let mut doc = original.clone();
                        if !decision {
                            doc["decision"] = json!("approved");
                        }
                        if !evidence {
                            doc["evidence_refs"] = json!(["unrelated"]);
                        }
                        if !marker {
                            doc["summary"] = json!("A semantic issue was found");
                            doc["contradictions"] = json!([]);
                        }
                        let mut violations = Vec::new();
                        let passed =
                            validate_submission(suite, &f, Some(&doc), &[], &mut violations);
                        let result = capture(suite, &f, Some(&doc)).unwrap();
                        assert_eq!(result["verifier_checks"]["decision_matches"], decision);
                        assert_eq!(
                            result["verifier_checks"]["fixture_evidence_ref_present"],
                            evidence
                        );
                        assert_eq!(result["verifier_checks"]["required_marker_present"], marker);
                        assert_eq!(passed, decision && evidence && marker);
                        assert_eq!(
                            violations.contains(&"verification_evidence_mismatch".into()),
                            !evidence || !marker
                        );
                        assert_eq!(result["document"], doc);
                        assert_eq!(
                            result["raw_document_sha256"],
                            canonical_json_sha256(&doc).unwrap()
                        );
                        assert_eq!(result["retention"], "complete");
                        assert_eq!(
                            result["verifier_checks"]
                                ["fixture_discloses_expected_decision_and_marker"],
                            true
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn missing_large_deep_and_secret_shaped_submissions_remain_explicitly_incomplete() {
        let (f, mut doc) = verification(SuiteKind::VerifierV1);
        assert_eq!(
            capture(SuiteKind::VerifierV2, &f, None).unwrap()["present"],
            false
        );
        doc["summary"] = json!("x".repeat(DOCUMENT_LIMIT));
        let oversized = capture(SuiteKind::VerifierV2, &f, Some(&doc)).unwrap();
        assert_eq!(oversized["retention"], "omitted_size");
        assert_eq!(oversized["document"], Value::Null);
        assert_eq!(
            oversized["verifier_checks"]["decision_matches"], true,
            "diagnostics still score the original document"
        );
        let secrets = json!({"token":"fixture-value-never-retain","summary":"Authorization: Bearer fixture-value-never-retain","nested":{"api_key":"fixture-value-never-retain"}});
        let redacted = capture(SuiteKind::VerifierV2, &f, Some(&secrets)).unwrap();
        assert_eq!(redacted["retention"], "redacted");
        assert_eq!(redacted["redacted_values"], 3);
        assert!(!redacted.to_string().contains("fixture-value-never-retain"));
        assert_eq!(
            redacted["raw_document_sha256"],
            canonical_json_sha256(&secrets).unwrap()
        );
        let mut deep = json!("leaf");
        for _ in 0..20 {
            deep = json!([deep]);
        }
        assert_eq!(
            capture(SuiteKind::PlannerV2, &f, Some(&deep)).unwrap()["retention"],
            "redacted"
        );
    }

    #[test]
    fn diagnostics_preserve_other_stage_documents_without_inventing_verifier_results() {
        let (f, _) = verification(SuiteKind::VerifierV2);
        let doc = json!({"failure_kind":"assertion","evidence_refs":["fixture_evidence"],"summary":"The recorded assertion failed"});
        let result = capture(SuiteKind::TestDiagnosisV2, &f, Some(&doc)).unwrap();
        assert_eq!(result["document"], doc);
        assert!(result.get("verifier_checks").is_none());
    }

    #[test]
    fn retention_stays_bounded_when_redaction_expands_short_values() {
        let (f, _) = verification(SuiteKind::VerifierV2);
        let document = json!((0..800).map(|_| json!({"token":"x"})).collect::<Vec<_>>());
        assert!(document.to_string().len() < DOCUMENT_LIMIT);
        let result = capture(SuiteKind::VerifierV2, &f, Some(&document)).unwrap();
        assert_eq!(result["retention"], "omitted_redaction_size");
        assert_eq!(result["document"], Value::Null);
        assert_eq!(result["redacted_values"], 800);
    }

    #[test]
    fn prior_live_verifier_report_remains_readable_without_invented_submissions() {
        let report: Value = serde_json::from_str(include_str!("../../../../planning/evidence/autonomous-sdlc/ASTRA-M04-48C77B7-VERIFIER-LIVE-RESULT.json")).unwrap();
        let rows = report["report"]["report"]["results"].as_array().unwrap();
        let mut passes = 0;
        for row in rows {
            let parsed: crate::EvalResult = serde_json::from_value(row.clone()).unwrap();
            assert!(parsed.stage_submission.is_none());
            passes += usize::from(parsed.passed);
            let serialized = serde_json::to_value(parsed).unwrap();
            assert!(serialized.get("stage_submission").is_none());
        }
        assert_eq!(rows.len(), 48);
        assert_eq!(passes, 3, "the historical failed run is never rescored");
    }
}

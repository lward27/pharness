use super::super::{signals, FinanceApplication};
use super::*;
use sha2::{Digest, Sha256};

const START: u64 = 1_788_639_990;
fn hash(v: &Value) -> Value {
    json!(crate::canonical_json_sha256(v).unwrap())
}

fn fixture(app: FinanceApplication, phase: FinanceVerificationPhase) -> FinanceRuntimeEvidence {
    let expected = FinanceDeploymentExpectation {
        application: app,
        environment: if phase == FinanceVerificationPhase::Production {
            FinanceEnvironment::Production
        } else {
            FinanceEnvironment::Staging
        },
        gitops_commit_sha: "a".repeat(40),
        image_digest: format!("sha256:{}", "b".repeat(64)),
    };
    let end = START
        + if phase == FinanceVerificationPhase::Production {
            600
        } else {
            300
        };
    let window = FinanceRuntimeWindow {
        start_unix_seconds: START,
        end_unix_seconds: end,
    };
    let identity = |began, completed| json!({"schema_version":"pharness.dev/finance-deployment-observation/v1alpha1","cluster":"lucas_engineering","expected":expected,"identity_state":"verified","started_at_unix_ms":began,"completed_at_unix_ms":completed,"runtime_verification":"not_evaluated","identity":{"image_ref":expected.image_ref(),"gitops_revision":expected.gitops_commit_sha,"template_hash":"template","generation":1,"replicas":1,"deployment":{"uid":"deployment"},"service":{"uid":"service"},"pods":[{"name":format!("{}-abc-one",expected.workload()),"uid":"e763137d-8b31-411c-b27d-127166e0de98","ip":"10.42.0.84","restart_count":0,"image_id":expected.image_ref()}]}});
    let before = identity(START * 1000 - 2000, START * 1000 - 1000);
    let after = identity(end * 1000, end * 1000 + 100);
    let probe_start = START * 1000 + 1000;
    let paths = match app {
        FinanceApplication::Yfinance => vec![
            "/healthz",
            "/history?ticker_name=%2A%2A%2A",
            "/markets/not-a-market",
        ],
        FinanceApplication::Frontend if expected.environment == FinanceEnvironment::Staging => {
            vec!["/", "/runtime-config.json", "/api/yfinance/healthz"]
        }
        _ => vec!["/", "/runtime-config.json"],
    };
    let correlation = format!(
        "{:x}",
        Sha256::digest(
            json!([expected, probe_start, "/healthz"])
                .to_string()
                .as_bytes()
        )
    );
    let probes = json!({"schema_version":"pharness.dev/finance-functional-probes/v1alpha1","expected":expected,"started_at_unix_ms":probe_start,"completed_at_unix_ms":probe_start+100,"probe_state":"passed","runtime_verification":"not_evaluated","probes":paths.iter().map(|path| json!({"path":path,"method":"GET","state":"passed","reason":null,"http_status":if path.starts_with("/history") || path.starts_with("/markets") {422} else {200},"response_bytes":21,"response_sha256":format!("sha256:{}","c".repeat(64)),"observed_at_unix_ms":probe_start+100,"backend_trace_id":&correlation[..32],"backend_parent_span_id":&correlation[32..48]})).collect::<Vec<_>>()});
    let queries = signals::query_bindings(&expected, &window, &before, &after, (end + 1) * 1000)
        .unwrap()
        .into_iter()
        .map(|mut q| {
            q["state"] = json!("observed");
            q["summary"] = json!({"anomaly_observed":false});
            q["receipt"] =
                json!({"http_status":200,"bytes":2,"sha256":format!("sha256:{}","d".repeat(64))});
            q
        })
        .collect::<Vec<_>>();
    let signals = json!({"schema_version":"pharness.dev/finance-window-signals/v1alpha1","expected":expected,"window":window,"observed_at_unix_ms":end*1000+200,"completed_at_unix_ms":end*1000+300,"signal_state":"observed","reasons":[],"queries":queries,"identity_before_sha256":hash(&before),"identity_after_sha256":hash(&after),"runtime_verification":"not_evaluated"});
    let mut trace = json!({"schema_version":"pharness.dev/finance-health-trace/v1alpha1","expected":expected,"window":window,"observed_at_unix_ms":end*1000+200,"completed_at_unix_ms":end*1000+300,"trace_state":"observed","reasons":[],"functional_probe_state":"passed","functional_probes_sha256":hash(&probes),"identity_before_sha256":hash(&before),"identity_after_sha256":hash(&after),"runtime_verification":"not_evaluated","trace_image_identity_verified":false,"trace_id":&correlation[..32],"parent_span_id":&correlation[32..48],"trace":{"trace_id":&correlation[..32],"parent_span_id":&correlation[32..48],"server_span_id":"1234567890abcdef","spans_inspected":3,"service":expected.workload(),"namespace":expected.namespace(),"method":"GET","route":"/healthz","http_status":200,"tempo_partial_status":"complete","start_unix_ns":probe_start*1_000_000,"end_unix_ns":(probe_start+10)*1_000_000,"duration_ms":10.0},"receipt":{"path":format!("/api/v2/traces/{}",&correlation[..32]),"query_parameters":{"start":START,"end":end},"http_status":200,"bytes":1000,"sha256":format!("sha256:{}","e".repeat(64))}});
    if app == FinanceApplication::Frontend {
        trace["trace_state"] = json!("not_instrumented");
        trace["limits"] = json!({"requests":0});
        trace["reasons"] = json!(["frontend_binding_has_no_application_trace_requirement"]);
        trace.as_object_mut().unwrap().remove("trace");
        trace.as_object_mut().unwrap().remove("receipt");
        trace.as_object_mut().unwrap().remove("trace_id");
        trace.as_object_mut().unwrap().remove("parent_span_id");
    }
    FinanceRuntimeEvidence {
        phase,
        expected,
        window,
        identity_before: before,
        identity_after: after,
        functional_probes: probes,
        signals,
        health_trace: trace,
    }
}

fn assess(e: &FinanceRuntimeEvidence) -> Value {
    e.assess((e.window.end_unix_seconds + 1) * 1000)
}

#[test]
fn complete_bound_evidence_passes_without_granting_work_item_success_or_rollback() {
    for app in [FinanceApplication::Yfinance, FinanceApplication::Frontend] {
        for phase in [
            FinanceVerificationPhase::Baseline,
            FinanceVerificationPhase::Staging,
            FinanceVerificationPhase::Production,
        ] {
            let e = fixture(app, phase);
            let original = json!(e);
            let result = assess(&e);
            assert_eq!(result["runtime_verification"], "passed", "{result}");
            assert_eq!(result["work_item_completion"], "not_evaluated");
            assert_eq!(result["regression_causality"], "not_established");
            assert_eq!(result["evidence_sha256"].as_object().unwrap().len(), 5);
            assert_eq!(json!(e), original);
        }
    }
}

#[test]
fn admission_identity_must_be_later_unchanged_and_within_the_original_window_expiry() {
    let e = fixture(
        FinanceApplication::Yfinance,
        FinanceVerificationPhase::Baseline,
    );
    let end = e.window.end_unix_seconds * 1000;
    let mut current = e.identity_after.clone();
    current["started_at_unix_ms"] = json!(end + 500);
    current["completed_at_unix_ms"] = json!(end + 700);
    e.validate_current_deployment(&current, end + 1000).unwrap();
    for (pointer, value) in [
        ("/started_at_unix_ms", json!(end)),
        ("/identity/pods/0/restart_count", json!(1)),
        ("/identity/service/uid", json!("other-service")),
        ("/identity/template_hash", json!("changed-config")),
        (
            "/identity/pods/0/uid",
            json!("00000000-0000-0000-0000-000000000000"),
        ),
    ] {
        let mut changed = current.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        assert!(
            e.validate_current_deployment(&changed, end + 1000).is_err(),
            "{pointer}"
        );
    }
    assert!(e
        .validate_current_deployment(&current, end + 60_001)
        .is_err());
    assert_eq!(
        e.assess(end + 1000)["runtime_verification"],
        "passed",
        "the original record was not rewritten for admission"
    );
}

#[test]
fn missing_stale_cross_release_and_short_windows_never_pass() {
    let e = fixture(
        FinanceApplication::Yfinance,
        FinanceVerificationPhase::Staging,
    );
    for (path, value) in [
        ("/identity_after/identity/pods/0/restart_count", json!(1)),
        ("/signals/identity_before_sha256", json!("wrong")),
        ("/signals/window/end_unix_seconds", json!(START + 299)),
        ("/health_trace/functional_probes_sha256", json!("wrong")),
        (
            "/health_trace/expected/image_digest",
            json!(format!("sha256:{}", "f".repeat(64))),
        ),
        ("/functional_probes/probes/0/http_status", json!(500)),
        ("/signals/queries/0/source", json!("inventory")),
        ("/signals/queries/0/query", json!("up")),
        ("/signals/queries/0/receipt/http_status", json!(403)),
        ("/health_trace/trace/tempo_partial_status", json!("partial")),
        ("/health_trace/trace/parent_span_id", json!("unrelated")),
    ] {
        let mut changed = json!(e);
        *changed.pointer_mut(path).unwrap() = value;
        let changed: FinanceRuntimeEvidence = serde_json::from_value(changed).unwrap();
        assert_ne!(assess(&changed)["runtime_verification"], "passed", "{path}");
    }
    for now in [(START + 299) * 1000, (START + 361) * 1000, u64::MAX] {
        assert_ne!(e.assess(now)["runtime_verification"], "passed");
    }
    let mut changed = e.clone();
    changed.window.end_unix_seconds = START + 299;
    assert_ne!(assess(&changed)["runtime_verification"], "passed");
    changed = e.clone();
    changed.phase = FinanceVerificationPhase::Production;
    assert_ne!(assess(&changed)["runtime_verification"], "passed");
    changed = e.clone();
    changed.signals["queries"].as_array_mut().unwrap().pop();
    assert_ne!(assess(&changed)["runtime_verification"], "passed");
    changed = e;
    changed.signals["queries"][1] = changed.signals["queries"][0].clone();
    assert_ne!(assess(&changed)["runtime_verification"], "passed");
}

#[test]
fn green_collection_labels_cannot_hide_failures_caveats_or_missing_data() {
    let mut e = fixture(
        FinanceApplication::Yfinance,
        FinanceVerificationPhase::Baseline,
    );
    e.signals["queries"][0]["summary"]["anomaly_observed"] = json!(true);
    assert_eq!(assess(&e)["runtime_verification"], "failed");
    let mut e = fixture(
        FinanceApplication::Yfinance,
        FinanceVerificationPhase::Baseline,
    );
    e.functional_probes["probes"][1]["state"] = json!("failed");
    e.health_trace["trace_state"] = json!("inconclusive");
    assert_eq!(assess(&e)["runtime_verification"], "failed");
    let mut e = fixture(
        FinanceApplication::Yfinance,
        FinanceVerificationPhase::Baseline,
    );
    e.signals["reasons"] = json!(["log_delivery_gap"]);
    assert_eq!(assess(&e)["runtime_verification"], "inconclusive");
    e.signals["reasons"] = json!([]);
    e.signals["queries"][0]["state"] = json!("inconclusive");
    assert_eq!(assess(&e)["runtime_verification"], "inconclusive");
    e.signals["queries"][0]["state"] = json!("observed");
    e.signals["queries"][0]["summary"] = Value::Null;
    assert_eq!(assess(&e)["runtime_verification"], "inconclusive");
}

#[test]
fn frontend_requires_real_responses_and_cannot_claim_uninstrumented_traces() {
    let mut e = fixture(
        FinanceApplication::Frontend,
        FinanceVerificationPhase::Staging,
    );
    e.health_trace["trace"] = json!({"fabricated":true});
    assert_ne!(assess(&e)["runtime_verification"], "passed");
    let mut e = fixture(
        FinanceApplication::Frontend,
        FinanceVerificationPhase::Staging,
    );
    e.health_trace["trace_state"] = json!("observed");
    assert_ne!(assess(&e)["runtime_verification"], "passed");
    let mut e = fixture(
        FinanceApplication::Frontend,
        FinanceVerificationPhase::Staging,
    );
    e.health_trace["limits"]["requests"] = json!(1);
    assert_ne!(assess(&e)["runtime_verification"], "passed");
    let mut e = fixture(
        FinanceApplication::Frontend,
        FinanceVerificationPhase::Staging,
    );
    e.functional_probes["probes"][1]["state"] = json!("inconclusive");
    assert_eq!(assess(&e)["runtime_verification"], "inconclusive");
}

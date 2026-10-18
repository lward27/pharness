use super::*;
use crate::tools::{FinanceApplication, FinanceEnvironment};
use query::Query;

const START: u64 = 1_788_639_990;
const UID: &str = "e763137d-8b31-411c-b27d-127166e0de98";

fn expected(app: FinanceApplication) -> FinanceDeploymentExpectation {
    FinanceDeploymentExpectation {
        application: app,
        environment: FinanceEnvironment::Staging,
        gitops_commit_sha: "a".repeat(40),
        image_digest: format!("sha256:{}", "b".repeat(64)),
    }
}

fn window() -> FinanceRuntimeWindow {
    FinanceRuntimeWindow {
        start_unix_seconds: START,
        end_unix_seconds: START + 300,
    }
}

fn identity(e: &FinanceDeploymentExpectation, start: u64, end: u64) -> Value {
    json!({"schema_version":"pharness.dev/finance-deployment-observation/v1alpha1","cluster":"lucas_engineering","expected":e,"identity_state":"verified","started_at_unix_ms":start,"completed_at_unix_ms":end,
        "identity":{"image_ref":e.image_ref(),"gitops_revision":e.gitops_commit_sha,"template_hash":"template-hash","generation":3,"replicas":1,"deployment":{"uid":"deployment-uid"},"service":{"uid":"service-uid"},"pods":[{"name":format!("{}-abc-one",e.workload()),"uid":UID,"ip":"10.42.0.84","restart_count":0,"image_id":e.image_ref()}]}})
}

fn observations(e: &FinanceDeploymentExpectation) -> (Value, Value) {
    (
        identity(e, START * 1000 - 2000, START * 1000 - 1000),
        identity(e, (START + 300) * 1000, (START + 300) * 1000 + 500),
    )
}

fn context(e: &FinanceDeploymentExpectation) -> window::Context {
    let (before, after) = observations(e);
    window::Context::validate(e, &window(), &before, &after, (START + 301) * 1000).unwrap()
}

fn queries(e: &FinanceDeploymentExpectation) -> Vec<Query> {
    query::build(e, &window(), &context(e))
}

fn query_named(e: &FinanceDeploymentExpectation, name: &str) -> Query {
    queries(e).into_iter().find(|q| q.name == name).unwrap()
}

fn pod_metric(e: &FinanceDeploymentExpectation) -> Value {
    json!({"namespace":e.namespace(),"container":e.workload(),"pod":format!("{}-abc-one",e.workload()),"uid":UID})
}

fn log_metric(e: &FinanceDeploymentExpectation) -> Value {
    let pod = format!("{}-abc-one", e.workload());
    json!({"pod":pod,"filename":format!("/var/log/pods/{}_{}_{}/{}/0.log",e.namespace(),pod,UID,e.workload())})
}

fn matrix(q: &Query, metric: Value, value: &str) -> Value {
    let values = (q.first..=q.last)
        .step_by(30)
        .map(|t| json!([t, value]))
        .collect::<Vec<_>>();
    json!({"status":"success","data":{"resultType":"matrix","result":[{"metric":metric,"values":values}]}})
}

fn vector(q: &Query, metric: Value, value: &str) -> Value {
    json!({"status":"success","data":{"resultType":"vector","result":[{"metric":metric,"value":[q.last,value]}]}})
}

#[test]
fn window_requires_elapsed_time_and_matching_bracketed_native_identities() {
    let e = expected(FinanceApplication::Yfinance);
    let (before, after) = observations(&e);
    let now = (START + 301) * 1000;
    assert!(window::Context::validate(&e, &window(), &before, &after, now).is_ok());
    for offset in [0, 1, 29_999] {
        let planned = FinanceRuntimeWindow::starting_after(START * 1000 + offset, 300).unwrap();
        assert_eq!(planned.start_unix_seconds, START + 30);
        assert_eq!(planned.end_unix_seconds, START + 330);
    }
    assert!(FinanceRuntimeWindow::starting_after(u64::MAX, 300).is_err());
    assert!(FinanceRuntimeWindow::starting_after(START * 1000, 299).is_err());
    let unaligned = FinanceRuntimeWindow {
        start_unix_seconds: START + 1,
        end_unix_seconds: START + 301,
    };
    assert_eq!(
        window::Context::validate(&e, &unaligned, &before, &after, now).err(),
        Some("window_must_start_on_query_grid_without_rounding")
    );
    for (pointer, value) in [
        ("/identity_state", json!("inconclusive")),
        ("/expected/image_digest", json!("latest")),
        ("/identity/deployment/uid", json!("replacement")),
        (
            "/identity/pods/0/uid",
            json!("f763137d-8b31-411c-b27d-127166e0de98"),
        ),
        ("/identity/pods/0/restart_count", json!(1)),
        ("/identity/template_hash", json!("changed")),
        ("/started_at_unix_ms", json!((START + 299) * 1000)),
    ] {
        let mut changed = after.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        assert!(
            window::Context::validate(&e, &window(), &before, &changed, now).is_err(),
            "{pointer}"
        );
    }
    for end in [START, START + 299, START + 301, u64::MAX] {
        assert!(window::Context::validate(
            &e,
            &FinanceRuntimeWindow {
                start_unix_seconds: START,
                end_unix_seconds: end
            },
            &before,
            &after,
            now
        )
        .is_err());
    }
    assert!(
        window::Context::validate(&e, &window(), &before, &after, (START + 361) * 1000).is_err()
    );
    assert!(
        window::Context::validate(&e, &window(), &before, &after, (START + 299) * 1000).is_err()
    );
}

#[test]
fn query_inputs_cannot_inject_syntax_or_select_another_pod() {
    let e = expected(FinanceApplication::Yfinance);
    let (before, after) = observations(&e);
    for field in ["name", "uid", "ip"] {
        let mut b = before.clone();
        let mut a = after.clone();
        b["identity"]["pods"][0][field] = json!("bad\"} or vector(1)");
        a["identity"]["pods"][0][field] = b["identity"]["pods"][0][field].clone();
        assert!(window::Context::validate(&e, &window(), &b, &a, (START + 301) * 1000).is_err());
    }
    for q in queries(&e) {
        assert!(q.expression.contains("apps-staging"));
        assert!(!q.expression.contains("apps-prod"));
        if q.source == "loki" {
            assert!(q.expression.contains(UID));
        }
        let timestamp = q
            .parameters
            .iter()
            .find(|(k, _)| k == "time" || k == "end")
            .unwrap()
            .1
            .parse::<u64>()
            .unwrap();
        assert_eq!(
            timestamp,
            if q.source == "loki" {
                q.last * 1_000_000_000
            } else {
                q.last
            }
        );
    }
    let frontend = queries(&expected(FinanceApplication::Frontend));
    assert_eq!(frontend.len(), 5);
    assert!(!frontend.iter().any(|q| q.name.starts_with("http_")));
}

#[test]
fn complete_pod_series_are_required_and_zero_readiness_is_an_anomaly() {
    let e = expected(FinanceApplication::Yfinance);
    let c = context(&e);
    let q = query_named(&e, "pod_readiness");
    let body = matrix(&q, pod_metric(&e), "1");
    assert_eq!(
        analysis::summarize(&q, &c, &body).unwrap()["anomaly_observed"],
        false
    );
    for value in ["NaN", "+Inf", "-1", "0.5"] {
        assert!(analysis::summarize(&q, &c, &matrix(&q, pod_metric(&e), value)).is_err());
    }
    let mut gap = body.clone();
    gap["data"]["result"][0]["values"]
        .as_array_mut()
        .unwrap()
        .remove(3);
    assert_eq!(
        analysis::summarize(&q, &c, &gap).unwrap_err(),
        "missing_window_samples"
    );
    let mut wrong = body.clone();
    wrong["data"]["result"][0]["metric"]["uid"] = json!("other-uid");
    assert!(analysis::summarize(&q, &c, &wrong).is_err());
    let mut low = body.clone();
    low["data"]["result"][0]["values"][3][1] = json!("0");
    assert_eq!(
        analysis::summarize(&q, &c, &low).unwrap()["anomaly_observed"],
        true
    );
    let mut warnings = body;
    warnings["warnings"] = json!(["partial response"]);
    assert!(analysis::summarize(&q, &c, &warnings).is_err());
}

#[test]
fn freshness_and_restart_changes_cannot_be_green() {
    let e = expected(FinanceApplication::Yfinance);
    let c = context(&e);
    let q = query_named(&e, "pod_metric_freshness");
    assert!(analysis::summarize(&q, &c, &matrix(&q, pod_metric(&e), "61")).is_err());
    let q = query_named(&e, "pod_restarts");
    assert_eq!(
        analysis::summarize(&q, &c, &matrix(&q, pod_metric(&e), "1")).unwrap()["anomaly_observed"],
        true
    );
}

#[test]
fn log_coverage_and_uid_are_required_even_when_error_vector_is_empty() {
    let e = expected(FinanceApplication::Frontend);
    let c = context(&e);
    let q = query_named(&e, "log_presence");
    assert!(analysis::summarize(&q, &c, &matrix(&q, log_metric(&e), "6")).is_ok());
    assert!(analysis::summarize(&q, &c, &matrix(&q, log_metric(&e), "0")).is_err());
    let mut metric = log_metric(&e);
    metric["filename"] = json!(format!(
        "/var/log/pods/apps-staging_finance-frontend-abc-one_other/{}/0.log",
        e.workload()
    ));
    assert!(analysis::summarize(&q, &c, &matrix(&q, metric, "6")).is_err());
    let q = query_named(&e, "log_errors");
    let empty = json!({"status":"success","data":{"resultType":"vector","result":[]}});
    let summary = analysis::summarize(&q, &c, &empty).unwrap();
    assert_eq!(summary["requires_log_presence"], "every_pod_and_interval");
    assert_eq!(
        analysis::summarize(&q, &c, &vector(&q, log_metric(&e), "1")).unwrap()["anomaly_observed"],
        true
    );
}

#[test]
fn request_counter_zero_absence_and_nonfinite_values_are_inconclusive() {
    let e = expected(FinanceApplication::Yfinance);
    let c = context(&e);
    let q = query_named(&e, "http_responses");
    assert!(!q.expression.contains("http_host"));
    assert_eq!(
        q.expression,
        "sum by (http_status_code) (increase(http_server_duration_count{job=\"apps-staging/yfinance-wrapper\"}[300s]))"
    );
    for value in ["0", "NaN", "+Inf", "-1"] {
        assert!(analysis::summarize(
            &q,
            &c,
            &vector(&q, json!({"http_status_code":"200"}), value)
        )
        .is_err());
    }
    let good = analysis::summarize(
        &q,
        &c,
        &vector(&q, json!({"http_status_code":"200"}), "30.4"),
    )
    .unwrap();
    assert_eq!(good["anomaly_observed"], false);
    assert_eq!(good["counter_increase_is_extrapolated"], true);
    let mut with_unclassified = vector(&q, json!({"http_status_code":"200"}), "30");
    with_unclassified["data"]["result"]
        .as_array_mut()
        .unwrap()
        .push(json!({"metric":{},"value":[q.last,"0"]}));
    assert_eq!(
        analysis::summarize(&q, &c, &with_unclassified).unwrap()["unclassified_responses"],
        0.0
    );
    with_unclassified["data"]["result"][1]["value"][1] = json!("1");
    assert_eq!(
        analysis::summarize(&q, &c, &with_unclassified).unwrap_err(),
        "unclassified_http_responses"
    );
    assert_eq!(
        analysis::summarize(
            &q,
            &c,
            &vector(&q, json!({"http_status_code":"500"}), "1.2")
        )
        .unwrap()["anomaly_observed"],
        true
    );
}

#[tokio::test]
async fn unavailable_configuration_returns_explicit_inconclusive_without_credentials() {
    let e = expected(FinanceApplication::Yfinance);
    let end = timestamp().unwrap() / 30_000 * 30;
    let w = FinanceRuntimeWindow {
        start_unix_seconds: end - 300,
        end_unix_seconds: end,
    };
    let before = identity(
        &e,
        w.start_unix_seconds * 1000 - 2000,
        w.start_unix_seconds * 1000 - 1000,
    );
    let after = identity(&e, end * 1000, end * 1000);
    let tools = ReadOnlyClusterTools::default()
        .with_finance_mimir_url_option(Some("http://user:credential-canary@127.0.0.1:1".into()));
    let result = tools
        .observe_finance_signals(&e, &w, &before, &after)
        .await
        .unwrap();
    assert_eq!(result.content["signal_state"], "inconclusive");
    assert_eq!(result.content["runtime_verification"], "not_evaluated");
    assert_eq!(result.content["queries"].as_array().unwrap().len(), 7);
    assert!(!result.content.to_string().contains("credential-canary"));
}

#[tokio::test]
async fn missing_mimir_never_falls_back_to_legacy_prometheus() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let e = expected(FinanceApplication::Yfinance);
    let end = timestamp().unwrap() / 30_000 * 30;
    let w = FinanceRuntimeWindow {
        start_unix_seconds: end - 300,
        end_unix_seconds: end,
    };
    let before = identity(&e, (end - 300) * 1000 - 2000, (end - 300) * 1000 - 1000);
    let after = identity(&e, end * 1000, end * 1000);
    let tools = ReadOnlyClusterTools::default()
        .with_prometheus_url(format!("http://{}", listener.local_addr().unwrap()));
    let result = tools
        .observe_finance_signals(&e, &w, &before, &after)
        .await
        .unwrap();
    assert_eq!(result.content["signal_state"], "inconclusive");
    for q in result.content["queries"].as_array().unwrap() {
        assert_eq!(q["reason"], "provider_not_configured");
    }
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn provider_transport_rejects_redirects_oversized_and_malformed_bodies() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let e = expected(FinanceApplication::Yfinance);
    let q = query_named(&e, "http_metric_freshness");
    for (response,reason) in [
        ("HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/credential-canary\r\nContent-Length: 0\r\nConnection: close\r\n\r\n","provider_http_error"),
        ("HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\nConnection: close\r\n\r\n","provider_response_too_large"),
        ("HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nsecret","provider_response_not_json"),
    ] {
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint=format!("http://{}",listener.local_addr().unwrap());
        let server=tokio::spawn(async move {
            let (mut socket,_)=listener.accept().await.unwrap();let mut request=[0_u8;4096];
            let count=socket.read(&mut request).await.unwrap();
            assert!(std::str::from_utf8(&request[..count]).unwrap().starts_with("GET /api/v1/query?"));
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        assert_eq!(request::read(Some(&endpoint),&q,1000,128).await.unwrap_err(),reason);
        server.await.unwrap();
    }
}

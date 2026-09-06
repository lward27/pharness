use super::{window::Context, FinanceDeploymentExpectation, FinanceRuntimeWindow};
use crate::tools::FinanceApplication;

#[derive(Debug, Clone)]
pub(super) struct Query {
    pub name: &'static str,
    pub source: &'static str,
    pub expression: String,
    pub parameters: Vec<(String, String)>,
    pub first: u64,
    pub last: u64,
    pub range: bool,
}

fn quoted(value: &str) -> String {
    serde_json::to_string(value).expect("serializing a string cannot fail")
}

impl Query {
    fn new(
        name: &'static str,
        source: &'static str,
        expression: String,
        first: u64,
        last: u64,
        range: bool,
    ) -> Self {
        let mut parameters = vec![("query".into(), expression.clone())];
        // Loki interprets integer epoch parameters as nanoseconds; Mimir's
        // Prometheus API uses seconds. Result matrix timestamps use seconds.
        let time = |value: u64| {
            if source == "loki" {
                value
                    .checked_mul(1_000_000_000)
                    .expect("validated window fits Loki epoch")
                    .to_string()
            } else {
                value.to_string()
            }
        };
        if range {
            parameters.extend([
                ("start".into(), time(first)),
                ("end".into(), time(last)),
                ("step".into(), "30".into()),
            ]);
        } else {
            parameters.push(("time".into(), time(last)));
        }
        Self {
            name,
            source,
            expression,
            parameters,
            first,
            last,
            range,
        }
    }
}

pub(super) fn build(
    e: &FinanceDeploymentExpectation,
    w: &FinanceRuntimeWindow,
    c: &Context,
) -> Vec<Query> {
    let names = c.pods.keys().cloned().collect::<Vec<_>>().join("|");
    let uids = c
        .pods
        .values()
        .map(|p| p.uid.as_str())
        .collect::<Vec<_>>()
        .join("|");
    let labels = format!(
        "namespace={},pod=~{},uid=~{},container={}",
        quoted(e.namespace()),
        quoted(&format!("^({names})$")),
        quoted(&format!("^({uids})$")),
        quoted(e.workload())
    );
    let mut queries = vec![
        Query::new("pod_readiness", "mimir", format!("kube_pod_container_status_ready{{{labels}}}"), w.start_unix_seconds, w.end_unix_seconds, true),
        Query::new("pod_restarts", "mimir", format!("kube_pod_container_status_restarts_total{{{labels}}}"), w.start_unix_seconds, w.end_unix_seconds, true),
        Query::new("pod_metric_freshness", "mimir", format!("max by (namespace,pod,uid,container) (time() - timestamp({{__name__=~\"kube_pod_container_status_ready|kube_pod_container_status_restarts_total\",{labels}}}))"), w.start_unix_seconds, w.end_unix_seconds, true),
    ];
    let duration = w.end_unix_seconds - w.start_unix_seconds;
    if e.application == FinanceApplication::Yfinance {
        // The deployed exporter has no Pod UID label on request counters.
        // Filtering http_host by Pod IP would hide ingress/proxy requests.
        // Use the finite application/namespace job; identity remains separately
        // bracketed and this metric cannot establish per-Pod causality.
        let selector = format!(
            "http_server_duration_count{{job={}}}",
            quoted(&format!("{}/yfinance-wrapper", e.namespace())),
        );
        queries.push(Query::new(
            "http_responses",
            "mimir",
            format!("sum by (http_status_code) (increase({selector}[{duration}s]))"),
            w.start_unix_seconds,
            w.end_unix_seconds,
            false,
        ));
        queries.push(Query::new(
            "http_metric_freshness",
            "mimir",
            format!("max(time() - timestamp({selector}))"),
            w.start_unix_seconds,
            w.end_unix_seconds,
            false,
        ));
    }
    let files = c
        .pods
        .values()
        .map(|p| {
            format!(
                "/var/log/pods/{}_{}_{}/{}/[0-9]+\\.log",
                e.namespace(),
                p.name,
                p.uid,
                e.workload()
            )
        })
        .collect::<Vec<_>>()
        .join("|");
    let logs = format!(
        "{{namespace={},container={},pod=~{},filename=~{}}}",
        quoted(e.namespace()),
        quoted(e.workload()),
        quoted(&format!("^({names})$")),
        quoted(&format!("^({files})$"))
    );
    queries.push(Query::new(
        "log_presence",
        "loki",
        format!("sum by (pod,filename) (count_over_time({logs}[60s]))"),
        w.start_unix_seconds + 60,
        w.end_unix_seconds,
        true,
    ));
    let pattern = match e.application {
        FinanceApplication::Yfinance => {
            r#"(^ERROR:|^CRITICAL:|^Traceback|HTTP/[0-9.]+\" 5[0-9][0-9] )"#
        }
        FinanceApplication::Frontend => {
            r#"(\[(error|crit|alert|emerg)\]|HTTP/[0-9.]+\" 5[0-9][0-9] )"#
        }
    };
    queries.push(Query::new(
        "log_errors",
        "loki",
        format!(
            "sum by (pod,filename) (count_over_time({logs} |~ {} [{duration}s]))",
            quoted(pattern)
        ),
        w.start_unix_seconds,
        w.end_unix_seconds,
        false,
    ));
    queries
}

use super::*;
use crate::tools::{FinanceRuntimeEvidence, FinanceRuntimeWindow, FinanceVerificationPhase};

#[tokio::test]
#[ignore = "explicit read-only staging, Mimir, Loki and Tempo forwards and exact identities required"]
async fn live_finance_runtime_assessment() {
    let env = |key| std::env::var(key).expect("explicit live Finance input required");
    let expected: FinanceDeploymentExpectation =
        serde_json::from_str(&env("PHARNESS_FINANCE_LIVE_EXPECTATION")).unwrap();
    expected.validate().unwrap();
    assert_eq!(expected.environment, FinanceEnvironment::Staging);
    let tools = ReadOnlyClusterTools::default()
        .with_kubectl_bin(env("PHARNESS_FINANCE_LIVE_KUBECTL"))
        .with_finance_mimir_url_option(Some(env("PHARNESS_FINANCE_LIVE_MIMIR")))
        .with_loki_url_option(Some(env("PHARNESS_FINANCE_LIVE_LOKI")))
        .with_tempo_url_option(Some(env("PHARNESS_FINANCE_LIVE_TEMPO")));
    let base = env("PHARNESS_FINANCE_LIVE_PROBE_BASE");
    let url = Url::parse(&base).unwrap();
    assert_eq!(url.scheme(), "http");
    assert_eq!(url.host_str(), Some("127.0.0.1"));
    assert!(url.port().is_some());
    let before = tools
        .observe_finance_deployment(&expected)
        .await
        .unwrap()
        .content;
    assert_eq!(before["identity_state"], "verified");
    let window =
        FinanceRuntimeWindow::starting_after(before["completed_at_unix_ms"].as_u64().unwrap(), 300)
            .unwrap();
    println!(
        "{}",
        json!({"identity_before":before,"planned_window":window})
    );
    async fn wait_until(target: u64) {
        while timestamp().unwrap() < target {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
    wait_until(window.start_unix_seconds * 1000).await;
    let probes = collect(&expected, &base, 15_000, MAX_BODY_BYTES)
        .await
        .unwrap();
    println!("{}", json!({"functional_probes":probes}));
    wait_until((window.end_unix_seconds + 10) * 1000).await;
    let after = tools
        .observe_finance_deployment(&expected)
        .await
        .unwrap()
        .content;
    let (signals, trace) = tokio::join!(
        tools.observe_finance_signals(&expected, &window, &before, &after),
        tools.observe_finance_health_trace(&expected, &window, &before, &after, &probes),
    );
    let evidence = FinanceRuntimeEvidence {
        phase: FinanceVerificationPhase::Baseline,
        expected,
        window,
        identity_before: before,
        identity_after: after,
        functional_probes: probes,
        signals: signals.unwrap().content,
        health_trace: trace.unwrap().content,
    };
    let assessment = evidence.assess(timestamp().unwrap());
    println!(
        "{}",
        json!({"runtime_evidence":evidence,"assessment":assessment})
    );
    assert_eq!(assessment["runtime_verification"], "passed", "{assessment}");
    assert_eq!(assessment["work_item_completion"], "not_evaluated");
}

use super::*;

#[tokio::test]
#[ignore = "explicit read-only cluster and telemetry access; waits for an actual five-minute window"]
async fn live_finance_five_minute_signals() {
    let expected: FinanceDeploymentExpectation = serde_json::from_str(
        &std::env::var("PHARNESS_FINANCE_LIVE_EXPECTATION").expect("explicit expectation required"),
    )
    .unwrap();
    let tools = ReadOnlyClusterTools::default()
        .with_kubectl_bin(
            std::env::var("PHARNESS_FINANCE_LIVE_KUBECTL")
                .expect("explicit cluster wrapper required"),
        )
        .with_finance_mimir_url_option(Some(
            std::env::var("PHARNESS_FINANCE_LIVE_MIMIR").expect("explicit Mimir endpoint required"),
        ))
        .with_loki_url_option(Some(
            std::env::var("PHARNESS_FINANCE_LIVE_LOKI").expect("explicit Loki endpoint required"),
        ));
    let before = tools
        .observe_finance_deployment(&expected)
        .await
        .unwrap()
        .content;
    assert_eq!(before["identity_state"], "verified", "{before}");
    let window = FinanceRuntimeWindow::starting_after(timestamp().unwrap(), 300).unwrap();
    println!(
        "{}",
        json!({"identity_before":before,"planned_window":window})
    );
    loop {
        let now = timestamp().unwrap();
        // Allow the existing log ingestion path ten seconds to publish the last
        // interval; the observation still must finish within its 60-second budget.
        let collect_at = (window.end_unix_seconds + 10) * 1000;
        if now >= collect_at {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(
            (collect_at - now).min(1000),
        ))
        .await;
    }
    let after = tools
        .observe_finance_deployment(&expected)
        .await
        .unwrap()
        .content;
    let result = tools
        .observe_finance_signals(&expected, &window, &before, &after)
        .await
        .unwrap();
    println!("{}", json!({"identity_after":after,"signal_result":result}));
    assert_eq!(result.content["signal_state"], "observed");
    assert_eq!(result.content["runtime_verification"], "not_evaluated");
}

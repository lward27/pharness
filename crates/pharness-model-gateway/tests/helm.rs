//! Run explicitly with `cargo test -p pharness-model-gateway --test helm -- --ignored`.
use serde::Deserialize;
use serde_json::{json, Value};
use std::{path::PathBuf, process::Command};

fn render(credentials: Value) -> std::process::Output {
    let chart = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../deploy/helm/pharness");
    Command::new("helm")
        .args(["template", "pharness"])
        .arg(chart)
        .args(["--namespace", "pharness", "--set-json"])
        .arg(format!(
            "inferenceGateway.additionalCredentials={credentials}"
        ))
        .output()
        .expect("Helm CLI is required for this deployment check")
}

fn documents(output: &std::process::Output) -> Vec<Value> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_yaml::Deserializer::from_str(std::str::from_utf8(&output.stdout).unwrap())
        .map(|doc| Value::deserialize(doc).unwrap())
        .collect()
}

#[test]
#[ignore = "requires Helm; run explicitly for deployment changes"]
fn credentials_are_mounted_only_in_the_gateway_without_opening_local_egress() {
    let baseline = documents(&render(json!({})));
    let additional = documents(&render(
        json!({"lm-studio-api-key":{"secretName":"pharness-lm-studio","secretKey":"api-key"}}),
    ));
    let gateway = |docs: &[Value]| {
        docs.iter()
            .find(|d| {
                d["kind"] == "Deployment" && d["metadata"]["name"] == "pharness-model-gateway"
            })
            .unwrap()
            .clone()
    };
    let old = gateway(&baseline);
    let new = gateway(&additional);
    let pod = &new["spec"]["template"]["spec"];
    let container = &pod["containers"][0];
    let env = container["env"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == "PHARNESS_INFERENCE_CREDENTIAL_FILES")
        .unwrap();
    let mapping: Value = serde_json::from_str(env["value"].as_str().unwrap()).unwrap();
    assert_eq!(
        mapping["lm-studio-api-key"],
        "/var/run/secrets/inference-extra/lm-studio-api-key/key"
    );
    assert_eq!(
        mapping["fireworks-api-key"],
        "/var/run/secrets/inference/fireworks-api-key"
    );
    assert_eq!(mapping.as_object().unwrap().len(), 2);
    let volume = pod["volumes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["secret"]["secretName"] == "pharness-lm-studio")
        .unwrap();
    assert_eq!(
        volume["secret"]["items"],
        json!([{"key":"api-key","path":"key"}])
    );
    assert!(container["volumeMounts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["name"] == volume["name"]
            && v["readOnly"] == true
            && v["mountPath"] == "/var/run/secrets/inference-extra/lm-studio-api-key"));
    assert_eq!(pod["automountServiceAccountToken"], false);
    assert_eq!(
        container["image"],
        old["spec"]["template"]["spec"]["containers"][0]["image"]
    );
    // All other rendered resources, including worker/API credentials and network
    // policy, must remain unchanged when only a credential binding is supplied.
    let without_gateway = |docs: Vec<Value>| {
        docs.into_iter()
            .filter(|d| {
                !(d["kind"] == "Deployment" && d["metadata"]["name"] == "pharness-model-gateway")
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(without_gateway(baseline), without_gateway(additional));
}

#[test]
#[ignore = "requires Helm; run explicitly for deployment changes"]
fn malformed_or_reserved_credential_bindings_fail_rendering() {
    for credentials in [
        json!({"fireworks-api-key":{"secretName":"replacement","secretKey":"key"}}),
        json!({"../escape":{"secretName":"local","secretKey":"key"}}),
        json!({"local":{"secretName":"","secretKey":"key"}}),
        json!({"local":{"secretName":"local","secretKey":""}}),
    ] {
        assert!(!render(credentials).status.success());
    }
}

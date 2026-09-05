//! Exact, finite Tekton build identity shared by controller and isolated worker.
use serde_json::Value;

pub const OPERATION: &str = "pharness.lucas.engineering/workflow-operation-id";
pub const EXECUTION: &str = "pharness.lucas.engineering/execution-id";
pub const DEADLINE: &str = "pharness.lucas.engineering/build-deadline-ms";

pub fn validate_manifest(manifest: &Value) -> Result<(), String> {
    let annotations = &manifest["metadata"]["annotations"];
    if manifest["apiVersion"] != "tekton.dev/v1"
        || manifest["kind"] != "PipelineRun"
        || manifest["metadata"]["namespace"] != "tekton-pipelines"
        || !matches!(
            manifest["spec"]["pipelineRef"]["name"].as_str(),
            Some("pharness-yfinance-build" | "pharness-finance-frontend-build")
        )
        || !manifest["metadata"]["name"].as_str().is_some_and(dns_name)
        || !annotations[OPERATION].as_str().is_some_and(identity)
        || !annotations[EXECUTION].as_str().is_some_and(identity)
        || !annotations[DEADLINE]
            .as_str()
            .is_some_and(|v| v.parse::<i64>().is_ok_and(|n| n > 0))
        || manifest["spec"]["taskRunTemplate"]["serviceAccountName"] != "pharness-finance-build"
    {
        return Err("hosted build requires its recorded finite PipelineRun identity".into());
    }
    Ok(())
}

/// Kubernetes may add default fields. Every requested field and array entry
/// must remain identical, including source, task identity and workspace.
pub fn validate_observed_run(
    expected: &Value,
    observed: &Value,
    prior_uid: Option<&str>,
) -> Result<(), String> {
    validate_manifest(expected)?;
    let uid = observed["metadata"]["uid"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 200)
        .ok_or("hosted PipelineRun observation has no Kubernetes UID")?;
    if prior_uid.is_some_and(|prior| prior != uid)
        || observed["metadata"]
            .get("deletionTimestamp")
            .is_some_and(|v| !v.is_null())
        || !contains_fields(expected, observed)
    {
        return Err("observed PipelineRun differs from its original hosted execution; no replacement build is authorized".into());
    }
    Ok(())
}

fn identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
}
fn dns_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}
fn contains_fields(expected: &Value, observed: &Value) -> bool {
    match (expected, observed) {
        (Value::Object(a), Value::Object(b)) => a.iter().all(|(key, value)| {
            b.get(key)
                .is_some_and(|actual| contains_fields(value, actual))
        }),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| contains_fields(x, y))
        }
        _ => expected == observed,
    }
}

#[cfg(test)]
mod tests {
    use super::{validate_manifest, validate_observed_run, DEADLINE, EXECUTION, OPERATION};
    use serde_json::json;
    #[test]
    fn hosted_pipeline_identity_survives_defaults_but_rejects_replacement_and_mutation() {
        let expected = json!({"apiVersion":"tekton.dev/v1","kind":"PipelineRun","metadata":{"name":"pharness-build-fixed","namespace":"tekton-pipelines","annotations":{OPERATION:"operation_fixed",EXECUTION:"execution_fixed",DEADLINE:"1700000000000"}},"spec":{"pipelineRef":{"name":"pharness-yfinance-build"},"params":[{"name":"revision","value":"a".repeat(40)}],"taskRunTemplate":{"serviceAccountName":"pharness-finance-build"}}});
        validate_manifest(&expected).unwrap();
        let mut observed = expected.clone();
        observed["metadata"]["uid"] = json!("original-uid");
        observed["metadata"]["resourceVersion"] = json!("100");
        observed["spec"]["status"] = json!("");
        validate_observed_run(&expected, &observed, None).unwrap();
        validate_observed_run(&expected, &observed, Some("original-uid")).unwrap();
        assert!(validate_observed_run(&expected, &observed, Some("different-uid")).is_err());
        for (pointer, value) in [
            ("/metadata/namespace", json!("apps-prod")),
            ("/spec/pipelineRef/name", json!("different-pipeline")),
            ("/spec/params/0/value", json!("b".repeat(40))),
            (
                "/spec/taskRunTemplate/serviceAccountName",
                json!("administrator"),
            ),
        ] {
            let mut changed = observed.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            assert!(
                validate_observed_run(&expected, &changed, None).is_err(),
                "{pointer}"
            );
        }
        observed["metadata"]["deletionTimestamp"] = json!("2026-09-05T00:00:00Z");
        assert!(validate_observed_run(&expected, &observed, None).is_err());
    }
}

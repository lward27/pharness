use super::validation::clean_optional_text;
use serde_json::{json, Value};

pub(in crate::app) fn execution_check(
    code: impl Into<String>,
    passed: bool,
    summary: impl Into<String>,
) -> Value {
    json!({ "code": code.into(), "passed": passed, "summary": summary.into() })
}

pub(in crate::app) fn argo_executor_poll_seconds(config: &Value) -> u64 {
    config
        .pointer("/argo_executor/poll_seconds")
        .and_then(Value::as_u64)
        .filter(|seconds| *seconds > 0)
        .unwrap_or(5)
}

pub(in crate::app) fn normalized_executor_error_code(
    value: Option<String>,
    fallback: &str,
) -> String {
    let Some(value) = clean_optional_text(value) else {
        return fallback.to_string();
    };
    if value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        value
    } else {
        fallback.to_string()
    }
}

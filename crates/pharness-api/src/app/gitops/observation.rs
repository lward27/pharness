use serde_json::Value;

pub(in crate::app) fn gitops_observation_closed_unmerged(content: Option<&Value>) -> bool {
    content.is_some_and(|content| {
        content.get("status").and_then(Value::as_str) == Some("observed")
            && content.get("pull_request_state").and_then(Value::as_str) == Some("closed")
            && content.get("merged").and_then(Value::as_bool) == Some(false)
    })
}

pub(in crate::app) fn gitops_observation_refreshable(content: Option<&Value>) -> bool {
    content.is_some_and(|content| {
        let status = content.get("status").and_then(Value::as_str);
        if status == Some("failed") {
            return true;
        }
        status == Some("observed")
            && content.get("merged").and_then(Value::as_bool) != Some(true)
            && content.get("pull_request_state").and_then(Value::as_str) != Some("closed")
    })
}

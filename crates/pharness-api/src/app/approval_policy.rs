use serde_json::Value;

pub(in crate::app) fn approval_gate_uses_dedicated_lifecycle_action(gate_kind: &str) -> bool {
    gate_kind == "production_rollback"
}

pub(in crate::app) fn approval_gate_kind(gate_json: &Value) -> Option<String> {
    gate_json
        .get("kind")
        .and_then(Value::as_str)
        .or_else(|| gate_json.as_str())
        .map(str::trim)
        .filter(|kind| !kind.is_empty())
        .map(str::to_string)
}

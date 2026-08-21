use serde_json::Value;

pub(in crate::app) fn string_at(source: &Value, pointer: &str) -> Option<String> {
    source
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::to_string)
}

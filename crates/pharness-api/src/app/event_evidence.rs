use pharness_core::{AgentEvent, EventKind};
use serde_json::{json, Value};

pub(crate) fn shell_test_evidence(events: &[AgentEvent]) -> Vec<Value> {
    let mut active_action = None;
    let mut evidence = Vec::new();
    for event in events {
        if event.kind == EventKind::ToolStarted {
            active_action = event
                .payload
                .get("action")
                .and_then(Value::as_str)
                .map(str::to_string);
            continue;
        }
        if event.kind == EventKind::ToolFinished && active_action.as_deref() == Some("run_shell") {
            evidence.push(json!({
                "event_id": event.event_id,
                "status": event.payload.get("status"),
                "summary": event.payload.get("summary"),
            }));
        }
    }
    evidence
}

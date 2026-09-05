//! Read-only visualization of recorded lifecycle time. Never used for action hashing.
use pharness_store::{StoredSourceDeliveryIntent, StoredStageExecution, StoredStageOutcome};
use serde::Serialize;
use serde_json::{json, Value};

const STAGES: [&str; 6] = [
    "discover",
    "plan",
    "implement",
    "test",
    "verify",
    "source_delivery",
];

#[derive(Debug, Serialize)]
pub(super) struct LifecycleTimeline {
    as_of: String,
    elapsed_includes_waits: bool,
    intervals: Vec<Value>,
}

pub(super) fn project(
    executions: &[StoredStageExecution],
    outcomes: &[StoredStageOutcome],
    effective_outcomes: &[StoredStageOutcome],
    delivery: Option<&StoredSourceDeliveryIntent>,
    current_execution_id: Option<&str>,
    closed_at: Option<&str>,
    as_of: String,
) -> LifecycleTimeline {
    let mut ordered: Vec<_> = executions
        .iter()
        .filter(|e| STAGES.contains(&e.stage_key.as_str()))
        .collect();
    ordered.sort_by(|a, b| {
        (
            STAGES.iter().position(|s| *s == a.stage_key),
            a.sequence,
            &a.id,
        )
            .cmp(&(
                STAGES.iter().position(|s| *s == b.stage_key),
                b.sequence,
                &b.id,
            ))
    });
    let mut intervals = Vec::new();
    for execution in ordered {
        let outcome = outcomes
            .iter()
            .find(|o| o.stage_execution_id == execution.id);
        let effective = effective_outcomes
            .iter()
            .any(|o| o.stage_execution_id == execution.id);
        let finished = execution
            .finished_at
            .as_deref()
            .or_else(|| outcome.map(|o| o.sealed_at.as_str()));
        let terminal = matches!(
            execution.status.as_str(),
            "completed" | "succeeded" | "failed" | "cancelled" | "inapplicable"
        );
        let current = closed_at.is_none()
            && current_execution_id == Some(execution.id.as_str())
            && finished.is_none()
            && !terminal;
        // A controller seal without a recorded execution start is a point, not
        // a made-up duration from queued/created time. Delivery closure is also
        // a point; the separate delivery-intent interval carries external wait.
        let marker = finished.is_some()
            && (execution.started_at.is_none() || execution.stage_key == "source_delivery");
        let start = if marker {
            finished
        } else {
            execution.started_at.as_deref()
        };
        let kind = if marker {
            "marker"
        } else if start.is_none() {
            "unavailable"
        } else {
            "execution"
        };
        intervals.push(json!({
            "id":execution.id,"stage_key":execution.stage_key,"sequence":execution.sequence,
            "resource_kind":"stage_execution","resource_id":execution.id,
            "stage_execution_id":execution.id,"run_id":execution.run_id,"workspace_id":execution.workspace_id,
            "outcome_id":outcome.map(|o| &o.id),"status":outcome.map(|o| o.status.as_str()).unwrap_or(&execution.status),
            "origin":execution.origin,"kind":kind,"timing_basis":if marker { "recorded_seal" } else { "recorded_execution" },
            "queued_at":execution.created_at,"started_at":start,"finished_at":finished,
            "is_current":current,"is_effective":effective,"is_ongoing":current && start.is_some(),
            "stop_reason":execution.stop_reason,
            "correction_of":execution.input_snapshot.get("correction_of"),
            "diagnosis_of":execution.input_snapshot.get("diagnosis_of"),
        }));
    }
    if let Some(intent) = delivery {
        let merged = intent.status == "merged" || intent.merge_provenance.is_some();
        let end = if merged || intent.status == "pull_request_closed" {
            Some(intent.status_changed_at.as_str())
        } else {
            closed_at
        };
        intervals.push(json!({
            "id":intent.id,"stage_key":"source_delivery","sequence":0,
            "resource_kind":"source_delivery_intent","resource_id":intent.id,
            "stage_execution_id":null,"run_id":null,"outcome_id":null,
            "kind":"delivery_wait","origin":"controller","status":intent.status,
            "timing_basis":if closed_at.is_some() && !merged && intent.status != "pull_request_closed" {"delivery_intent_to_work_item_closure"} else {"recorded_delivery_intent"},
            "started_at":intent.created_at,"finished_at":end,
            "is_current":end.is_none(),"is_ongoing":end.is_none(),"is_effective":true,
            "stop_reason":intent.status_reason,"correction_of":null,"diagnosis_of":null,
        }));
    }
    LifecycleTimeline {
        as_of,
        elapsed_includes_waits: true,
        intervals,
    }
}

#[cfg(test)]
mod tests {
    use super::{json, project, StoredSourceDeliveryIntent, StoredStageExecution};

    fn execution(id: &str, sequence: u64) -> StoredStageExecution {
        serde_json::from_value(json!({"id":id,"work_item_id":"wi","stage_key":"implement","sequence":sequence,
            "status":"running","origin":"agent","agent_profile_id":"repo-builder","agent_profile_version":"v2",
            "agent_profile_hash":"hash","context_pack_id":null,"run_id":null,"workspace_id":"ws",
            "input_snapshot":{},"input_hash":"hash","stop_reason":null,
            "created_at":"1788500000000","started_at":"1788500001000","finished_at":null})).unwrap()
    }

    #[test]
    fn ordering_and_correction_are_deterministic_and_pauses_do_not_create_executions() {
        let mut repair = execution("repair", 2);
        repair.status = "paused".into();
        repair.input_snapshot = json!({"correction_of":{"outcome_id":"failed"}});
        let first = execution("first", 1);
        let a = project(
            &[repair.clone(), first.clone()],
            &[],
            &[],
            None,
            Some("repair"),
            None,
            "1788500010000".into(),
        );
        let b = project(
            &[first, repair],
            &[],
            &[],
            None,
            Some("repair"),
            None,
            "1788500010000".into(),
        );
        assert_eq!(
            serde_json::to_value(&a).unwrap(),
            serde_json::to_value(&b).unwrap()
        );
        assert_eq!(a.intervals.len(), 2);
        assert_eq!(a.intervals[1]["is_current"], true);
        assert_eq!(a.intervals[1]["correction_of"]["outcome_id"], "failed");
        assert_eq!(a.intervals[0]["is_ongoing"], false);
    }

    #[test]
    fn absent_start_is_not_replaced_by_created_time_and_seals_are_markers() {
        let mut e = execution("queued", 1);
        e.started_at = None;
        let t = project(
            &[e.clone()],
            &[],
            &[],
            None,
            Some("queued"),
            None,
            "now".into(),
        );
        assert_eq!(t.intervals[0]["kind"], "unavailable");
        assert!(t.intervals[0]["started_at"].is_null());
        e.finished_at = Some("1788500020000".into());
        let t = project(&[e], &[], &[], None, Some("queued"), None, "now".into());
        assert_eq!(t.intervals[0]["kind"], "marker");
        assert_eq!(t.intervals[0]["is_current"], false);
    }

    #[test]
    fn terminal_records_with_missing_end_are_not_extended_to_the_observation_clock() {
        let mut e = execution("failed", 1);
        e.status = "failed".into();
        let t = project(
            &[e],
            &[],
            &[],
            None,
            Some("failed"),
            None,
            "1788500020000".into(),
        );
        assert!(!t.intervals[0]["is_ongoing"].as_bool().unwrap());
        assert!(!t.intervals[0]["is_current"].as_bool().unwrap());
        assert!(t.intervals[0]["finished_at"].is_null());
    }

    #[test]
    fn delivery_wait_uses_intent_not_late_stage_or_generic_update_time() {
        let mut d: StoredSourceDeliveryIntent = serde_json::from_value(json!({
            "id":"delivery","subject_kind":"work_item_change_set","subject_id":"cs","repository_id":"repo",
            "source_repo":"https://github.com/example/repo","base_ref":"main","base_commit":"sha","head_branch":"branch",
            "patch_artifact_id":null,"patch_hash":"hash","status":"merged","state_version":1,"authorization":{},
            "writer_execution_id":null,"observer_execution_id":null,"pull_request":null,"merge_provenance":{},"provider_checks":null,
            "created_by":"actor","creation_reason":"reason","created_at":"1000","updated_at":"9000",
            "status_changed_at":"5000","status_changed_by":"controller","status_reason":"merged"})).unwrap();
        let t = project(&[], &[], &[], Some(&d), None, Some("5000"), "10000".into());
        assert_eq!(t.intervals[0]["started_at"], "1000");
        assert_eq!(t.intervals[0]["finished_at"], "5000");
        d.status = "awaiting_merge".into();
        d.merge_provenance = None;
        let t = project(&[], &[], &[], Some(&d), None, None, "10000".into());
        assert_eq!(t.intervals[0]["is_ongoing"], true);
        assert!(t.intervals[0]["finished_at"].is_null());
    }
}

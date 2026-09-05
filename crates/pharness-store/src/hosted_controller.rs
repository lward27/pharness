use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredWorkflowReconciliation {
    pub work_item_id: String,
    pub control: String,
    pub control_version: i64,
    pub next_due_at: i64,
    pub claim_owner: Option<String>,
    pub claim_fence: i64,
    pub claim_until: Option<i64>,
    pub condition: String,
    pub condition_reason: String,
    pub unchanged_checks: i64,
    pub observed_state_hash: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredWorkflowOperation {
    pub id: String,
    pub work_item_id: String,
    pub action: String,
    pub input_hash: String,
    pub effect: String,
    pub status: String,
    pub resource_refs: serde_json::Value,
    pub status_reason: String,
    pub created_at: i64,
    pub updated_at: i64,
}

pub struct BeginWorkflowOperation<'a> {
    pub id: &'a str,
    pub action: &'a str,
    pub input_hash: &'a str,
    pub effect: &'a str,
    pub resource_keys: &'a [&'a str],
}

pub struct FinishWorkflowReconciliation<'a> {
    pub next_due_at: i64,
    pub condition: &'a str,
    pub reason: &'a str,
    pub observed_state_hash: Option<&'a str>,
}

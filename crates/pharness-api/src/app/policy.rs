use pharness_core::{PolicyMode, SafetyPolicy};
use serde_json::json;

pub(in crate::app) fn run_policy(
    default: &SafetyPolicy,
    override_mode: Option<PolicyMode>,
) -> SafetyPolicy {
    let mut policy = default.clone();
    if let Some(mode) = override_mode {
        policy.mode = mode;
    }
    policy
}

pub(in crate::app) fn policy_json(policy: &SafetyPolicy) -> serde_json::Value {
    json!({
        "subject": &policy.subject,
        "environment": &policy.environment,
        "mode": policy.mode,
        "allow_read_only_shell": policy.allow_read_only_shell,
        "require_approval_for_writes": policy.require_approval_for_writes,
        "require_approval_for_network": policy.require_approval_for_network,
        "require_approval_for_destructive": policy.require_approval_for_destructive,
        "deny_privileged": policy.deny_privileged,
        "deny_secret_access": policy.deny_secret_access,
        "permission_grant_count": policy.permission_grants.len(),
    })
}

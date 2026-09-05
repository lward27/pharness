use pharness_store::StoredRepoWorkItemMetadata;
use serde_json::{json, Value};

/// Describe the recorded delivery scope. This read does not infer successful
/// release evidence from a source merge or from configured infrastructure.
pub(in crate::app) fn delivery_configuration(
    metadata: &StoredRepoWorkItemMetadata,
    source_commit: Option<&str>,
) -> Value {
    match &metadata.workflow_policy {
        Some(policy) => json!({
            "kind":"hosted_sdlc",
            "repository_id":metadata.repository_id,
            "source_commit":source_commit,
            "workflow_policy_hash":metadata.workflow_policy_hash,
            "delivery_binding":policy.delivery_binding,
            "release":{
                "required":true,
                "steps":[
                    {"key":"build","pipeline_contract_id":policy.pipeline_contract["id"]},
                    {"key":"staging","deployment_contract_id":policy.staging_contract["id"]},
                    {"key":"production","deployment_contract_id":policy.production_contract["id"],
                     "approval_boundary":policy.production_approval},
                ],
            },
            "observe":{"required":true},
            "required_evidence":pharness_core::hosted_sdlc::HOSTED_REQUIRED_EVIDENCE,
        }),
        None => json!({
            "kind":"repo_mode_source_only",
            "repository_id":metadata.repository_id,
            "source_commit":source_commit,
            "release":"inapplicable",
            "observe":"inapplicable",
        }),
    }
}

use crate::canonical_json_sha256;
use serde::{Deserialize, Serialize};

pub const HOSTED_SOURCE_MERGE_SCHEMA: &str = "pharness.dev/hosted-source-merge/v1alpha1";

/// A separate, immutable authority within the existing hosted source operation.
/// The original branch/PR publication authorization never grants merge authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedSourceMergeAuthority {
    pub schema_version: String,
    pub operation_id: String,
    pub execution_id: String,
    pub work_item_id: String,
    pub source_delivery_intent_id: String,
    pub workflow_policy_hash: String,
    pub change_set_material_hash: String,
    pub repository: String,
    pub base_ref: String,
    pub base_commit_sha: String,
    pub head_branch: String,
    pub head_commit_sha: String,
    pub pull_request_number: u64,
    pub pull_request_url: String,
    pub required_check_context: String,
    pub required_check_app_id: u64,
    pub expires_at_ms: i64,
}

impl HostedSourceMergeAuthority {
    pub fn validate(&self, now_ms: i64) -> Result<(), String> {
        if self.schema_version != HOSTED_SOURCE_MERGE_SCHEMA {
            return Err("unsupported hosted source-merge authority".into());
        }
        for id in [
            &self.operation_id,
            &self.execution_id,
            &self.work_item_id,
            &self.source_delivery_intent_id,
        ] {
            if id.is_empty()
                || id.len() > 200
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            {
                return Err("source merge requires bounded recorded operation identities".into());
            }
        }
        if !matches!(
            self.repository.as_str(),
            "https://github.com/lward27/yfinance_wrapper.git"
                | "https://github.com/lward27/finance-frontend.git"
        ) || self.base_ref != "main"
        {
            return Err(
                "source merge is limited to the two Finance application main branches".into(),
            );
        }
        if !hex(&self.base_commit_sha, 40)
            || !hex(&self.head_commit_sha, 40)
            || self.base_commit_sha == self.head_commit_sha
            || !digest(&self.workflow_policy_hash)
            || !digest(&self.change_set_material_hash)
        {
            return Err(
                "source merge requires exact source, policy and ChangeSet identities".into(),
            );
        }
        let expected_branch = format!(
            "pharness/{}/{}",
            self.work_item_id,
            &self.change_set_material_hash["sha256:".len()..][..12],
        );
        if self.head_branch != expected_branch
            || self.pull_request_number == 0
            || self.pull_request_url
                != format!(
                    "{}/pull/{}",
                    self.repository.trim_end_matches(".git"),
                    self.pull_request_number
                )
        {
            return Err("source merge pull request does not match its recorded ChangeSet".into());
        }
        if self.required_check_context != "Source integrity" || self.required_check_app_id != 15368
        {
            return Err(
                "source merge requires the reviewed GitHub Actions source-integrity check".into(),
            );
        }
        if self.expires_at_ms <= now_ms || self.expires_at_ms > now_ms.saturating_add(3_600_000) {
            return Err(
                "source merge authority is expired or exceeds the bounded source wait".into(),
            );
        }
        Ok(())
    }

    pub fn material_hash(&self) -> Result<String, String> {
        canonical_json_sha256(&serde_json::to_value(self).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    }
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|v| hex(v, 64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn authority() -> HostedSourceMergeAuthority {
        HostedSourceMergeAuthority {
            schema_version: HOSTED_SOURCE_MERGE_SCHEMA.into(),
            operation_id: "workflowop_recorded".into(),
            execution_id: "srcmerge_recorded".into(),
            work_item_id: "witem_recorded".into(),
            source_delivery_intent_id: "srcintent_recorded".into(),
            workflow_policy_hash: format!("sha256:{}", "d".repeat(64)),
            change_set_material_hash: format!("sha256:{}", "c".repeat(64)),
            repository: "https://github.com/lward27/yfinance_wrapper.git".into(),
            base_ref: "main".into(),
            base_commit_sha: "a".repeat(40),
            head_branch: "pharness/witem_recorded/cccccccccccc".into(),
            head_commit_sha: "b".repeat(40),
            pull_request_number: 7,
            pull_request_url: "https://github.com/lward27/yfinance_wrapper/pull/7".into(),
            required_check_context: "Source integrity".into(),
            required_check_app_id: 15368,
            expires_at_ms: 2_000_000,
        }
    }

    #[test]
    fn hosted_source_merge_cannot_authorize_gitops_or_another_branch() {
        for (key, value) in [
            (
                "repository",
                json!("https://github.com/lward27/lucas_engineering.git"),
            ),
            (
                "repository",
                json!("https://github.com/another/yfinance_wrapper.git"),
            ),
            ("base_ref", json!("production")),
            ("head_commit_sha", json!("latest")),
            ("head_commit_sha", json!("a".repeat(40))),
            ("head_branch", json!("pharness/another/cccccccccccc")),
            (
                "pull_request_url",
                json!("https://github.com/lward27/yfinance_wrapper/pull/8"),
            ),
            ("workflow_policy_hash", json!("approved")),
            ("required_check_context", json!("any-green-check")),
            ("required_check_app_id", json!(1)),
            ("operation_id", json!("")),
            (
                "schema_version",
                json!("pharness.dev/source-delivery-authorization/v1alpha1"),
            ),
        ] {
            let mut value_json = serde_json::to_value(authority()).unwrap();
            value_json[key] = value;
            let changed: HostedSourceMergeAuthority = serde_json::from_value(value_json).unwrap();
            assert!(changed.validate(1_000_000).is_err(), "{key}");
        }
    }

    #[test]
    fn hosted_source_merge_accepts_only_current_bounded_authority_for_each_application() {
        for repo in ["yfinance_wrapper", "finance-frontend"] {
            let mut value = authority();
            value.repository = format!("https://github.com/lward27/{repo}.git");
            value.pull_request_url = format!("https://github.com/lward27/{repo}/pull/7");
            assert!(value.validate(1_000_000).is_ok());
            assert!(value.validate(value.expires_at_ms).is_err());
            value.expires_at_ms = 4_600_001;
            assert!(value.validate(1_000_000).is_err());
            value.expires_at_ms = 4_600_000;
            assert!(value.validate(1_000_000).is_ok());
        }
    }

    #[test]
    fn legacy_publication_authority_cannot_be_read_as_source_merge_authority() {
        let legacy = json!({
            "schema_version":"pharness.dev/source-delivery-authorization/v1alpha1",
            "workflow_policy_hash":null,
            "writer_execution_id":"srcexec_original",
            "external_effect":"create one GitHub branch, commit, and pull request; merge is not authorized"
        });
        assert!(serde_json::from_value::<HostedSourceMergeAuthority>(legacy).is_err());
        let mut unknown = serde_json::to_value(authority()).unwrap();
        unknown["allow_production"] = json!(true);
        assert!(serde_json::from_value::<HostedSourceMergeAuthority>(unknown).is_err());
    }
}

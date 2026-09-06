use super::{HostedStagingAuthority, StagingGitOpsPlan};
use anyhow::Result;
use serde_json::Value;
#[cfg(test)]
mod tests;

pub(super) fn validate(
    value: &Value,
    authority: &HostedStagingAuthority,
    plan: &StagingGitOpsPlan,
    now_ms: i64,
) -> Result<()> {
    authority
        .validate_for_dispatch(now_ms)
        .map_err(anyhow::Error::msg)?;
    let baseline = &value["baseline"];
    let expiry = baseline["admission_expires_at_ms"]
        .as_i64()
        .unwrap_or_default();
    let digest = |v: &Value| {
        v.as_str()
            .and_then(|s| s.strip_prefix("sha256:"))
            .is_some_and(|s| {
                s.len() == 64
                    && s.bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            })
    };
    anyhow::ensure!(
        value["admitted"] == true
            && value["authority_hash"] == authority.material_hash().map_err(anyhow::Error::msg)?
            && value["plan_hash"] == plan.material_hash().map_err(anyhow::Error::msg)?
            && baseline["baseline_artifact_id"]
                == format!("staging_baseline_result_{}", authority.execution_id)
            && baseline["identity_artifact_id"]
                == format!(
                    "staging_baseline_admission_identity_{}",
                    authority.execution_id
                )
            && digest(&baseline["baseline_sha256"])
            && digest(&baseline["identity_sha256"])
            && now_ms < expiry
            && expiry <= now_ms.saturating_add(60_000)
            && expiry < authority.expires_at_ms
            && expiry < authority.created_at_ms.saturating_add(30 * 60 * 1000),
        "staging_baseline_admission_missing_changed_or_expired"
    );
    Ok(())
}

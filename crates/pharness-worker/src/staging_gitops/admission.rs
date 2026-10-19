//! Per-environment checks on the controller's single-use admission record. The
//! writer re-reads what the API acknowledged so a forged or stale acknowledgement
//! can never reach the GitHub mutation.
use anyhow::Result;
use pharness_core::hosted_sdlc::gitops_delivery::GitOpsPlan;
use pharness_core::hosted_sdlc::production::{
    HostedProductionAuthority, HostedProductionRollbackAuthority, ProductionGitOpsPlan,
    ProductionRollbackPlan,
};
use pharness_core::hosted_sdlc::staging::{HostedStagingAuthority, StagingGitOpsPlan};
use serde_json::Value;
#[cfg(test)]
mod tests;

pub(crate) trait Admission<P: GitOpsPlan> {
    fn validate_admission(&self, value: &Value, plan: &P, now_ms: i64) -> Result<()>;
}

fn digest(v: &Value) -> bool {
    v.as_str()
        .and_then(|s| s.strip_prefix("sha256:"))
        .is_some_and(|s| {
            s.len() == 64
                && s.bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        })
}

#[cfg(test)]
pub(super) fn validate(
    value: &Value,
    authority: &HostedStagingAuthority,
    plan: &StagingGitOpsPlan,
    now_ms: i64,
) -> Result<()> {
    authority.validate_admission(value, plan, now_ms)
}

impl Admission<StagingGitOpsPlan> for HostedStagingAuthority {
    fn validate_admission(
        &self,
        value: &Value,
        plan: &StagingGitOpsPlan,
        now_ms: i64,
    ) -> Result<()> {
        self.validate_for_dispatch(now_ms)
            .map_err(anyhow::Error::msg)?;
        let baseline = &value["baseline"];
        let expiry = baseline["admission_expires_at_ms"]
            .as_i64()
            .unwrap_or_default();
        anyhow::ensure!(
            value["admitted"] == true
                && value["authority_hash"] == self.material_hash().map_err(anyhow::Error::msg)?
                && value["plan_hash"] == plan.material_hash().map_err(anyhow::Error::msg)?
                && baseline["baseline_artifact_id"]
                    == format!("staging_baseline_result_{}", self.execution_id)
                && baseline["identity_artifact_id"]
                    == format!("staging_baseline_admission_identity_{}", self.execution_id)
                && digest(&baseline["baseline_sha256"])
                && digest(&baseline["identity_sha256"])
                && now_ms < expiry
                && expiry <= now_ms.saturating_add(60_000)
                && expiry < self.expires_at_ms
                && expiry < self.created_at_ms.saturating_add(30 * 60 * 1000),
            "staging_baseline_admission_missing_changed_or_expired"
        );
        Ok(())
    }
}

impl Admission<ProductionGitOpsPlan> for HostedProductionAuthority {
    /// The controller re-reads the human decision under its write boundary and
    /// echoes that decision's hash; the writer confirms it is the same decision
    /// it was dispatched with and that the grant is still open right now.
    fn validate_admission(
        &self,
        value: &Value,
        plan: &ProductionGitOpsPlan,
        now_ms: i64,
    ) -> Result<()> {
        self.validate_for_dispatch(plan, now_ms)
            .map_err(anyhow::Error::msg)?;
        let expiry = value["admission_expires_at_ms"]
            .as_i64()
            .unwrap_or_default();
        anyhow::ensure!(
            value["admitted"] == true
                && value["authority_hash"] == self.material_hash().map_err(anyhow::Error::msg)?
                && value["plan_hash"] == plan.material_hash().map_err(anyhow::Error::msg)?
                && value["approval_artifact_id"] == self.approval_artifact_id
                && value["approval_hash"]
                    == self.approval.material_hash().map_err(anyhow::Error::msg)?
                && now_ms < expiry
                && expiry <= now_ms.saturating_add(60_000)
                && expiry <= self.expires_at_ms,
            "production_approval_admission_missing_changed_or_expired"
        );
        Ok(())
    }
}

impl Admission<ProductionRollbackPlan> for HostedProductionRollbackAuthority {
    /// No human decision backs a rollback; the controller instead echoes the
    /// failed window and the admitted commit the writer was dispatched to
    /// reverse, so a grant for any other regression cannot be reused.
    fn validate_admission(
        &self,
        value: &Value,
        plan: &ProductionRollbackPlan,
        now_ms: i64,
    ) -> Result<()> {
        plan.validate(self).map_err(anyhow::Error::msg)?;
        self.validate_for_dispatch(now_ms)
            .map_err(anyhow::Error::msg)?;
        let expiry = value["admission_expires_at_ms"]
            .as_i64()
            .unwrap_or_default();
        anyhow::ensure!(
            value["admitted"] == true
                && value["authority_hash"] == self.material_hash().map_err(anyhow::Error::msg)?
                && value["plan_hash"] == plan.material_hash().map_err(anyhow::Error::msg)?
                && value["failed_result_artifact_id"] == self.failed_result_artifact_id
                && value["failed_result_sha256"] == self.failed_result_sha256
                && value["reverses_gitops_commit_sha"] == self.admitted_gitops_commit_sha
                && now_ms < expiry
                && expiry <= now_ms.saturating_add(60_000)
                && expiry <= self.expires_at_ms,
            "production_rollback_admission_missing_changed_or_expired"
        );
        Ok(())
    }
}

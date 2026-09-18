use super::{
    material_hash, ProductionGitOpsPlan, ProductionProposal, APPROVAL_SCHEMA, MAX_APPROVAL_MS,
};
use serde::{Deserialize, Serialize};

/// Written only by the authenticated operator decision path. Worker callbacks
/// and automatic lifecycle approval must never create this record. A caller
/// supplied actor string is not evidence that a human approved production.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanProductionApproval {
    pub schema_version: String,
    pub work_item_id: String,
    pub operation_id: String,
    pub proposal_hash: String,
    pub gitops_plan_hash: String,
    pub operator_identity: String,
    pub reason: String,
    pub approved_at_ms: i64,
    pub expires_at_ms: i64,
}

impl HumanProductionApproval {
    /// Structural/time validation only. The controller must additionally load
    /// the original operator event, compare current sealed source/policy and
    /// native evidence, check pause/cancellation, then admit at most one effect.
    /// Refreshing a page or replaying an approval never extends this interval.
    pub fn validate_for_delivery(
        &self,
        proposal: &ProductionProposal,
        plan: &ProductionGitOpsPlan,
        now_ms: i64,
    ) -> Result<(), String> {
        plan.validate(proposal)?;
        if self.schema_version != APPROVAL_SCHEMA
            || self.work_item_id != proposal.work_item_id
            || self.operation_id != proposal.operation_id
            || self.proposal_hash != proposal.material_hash()?
            || self.gitops_plan_hash != plan.material_hash()?
            || self.operator_identity.trim().is_empty()
            || self.operator_identity != self.operator_identity.trim()
            || self.operator_identity.len() > 200
            || self.operator_identity.chars().any(char::is_control)
            || ["controller:", "worker:", "agent:"]
                .iter()
                .any(|prefix| self.operator_identity.starts_with(prefix))
            || self.reason.trim().is_empty()
            || self.reason.len() > 2000
            || self.reason.contains('\0')
            || self.approved_at_ms < proposal.created_at_ms
            || self.expires_at_ms <= self.approved_at_ms
            || self.expires_at_ms > self.approved_at_ms.saturating_add(MAX_APPROVAL_MS)
            || now_ms < self.approved_at_ms
            || now_ms >= self.expires_at_ms
        {
            return Err("production requires the original human decision over this exact proposal and GitOps diff within its unchanged 30-minute grant".into());
        }
        Ok(())
    }

    pub fn material_hash(&self) -> Result<String, String> {
        material_hash(self)
    }
}

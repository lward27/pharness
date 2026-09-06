//! Join controller-owned native observations without treating any one boundary as success.
use super::{
    signals::WindowContext, FinanceDeploymentExpectation, FinanceEnvironment, FinanceRuntimeWindow,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

mod binding;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinanceVerificationPhase {
    Baseline,
    Staging,
    Production,
}

/// These values must come from persisted native reads, never an agent submission.
/// Hash binding detects mixed observations; it does not authenticate their origin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinanceRuntimeEvidence {
    pub phase: FinanceVerificationPhase,
    pub expected: FinanceDeploymentExpectation,
    pub window: FinanceRuntimeWindow,
    pub identity_before: Value,
    pub identity_after: Value,
    pub functional_probes: Value,
    pub signals: Value,
    pub health_trace: Value,
}

impl FinanceRuntimeEvidence {
    /// Pure assessment of one completed window. No queries, retries, incident
    /// creation, promotion, rollback or WorkItem closure occur here.
    pub fn assess(&self, now_unix_ms: u64) -> Value {
        let mut result = json!({
            "schema_version":"pharness.dev/finance-runtime-assessment/v1alpha1",
            "phase":self.phase,"expected":self.expected,"window":self.window,
            "assessed_at_unix_ms":now_unix_ms,"runtime_verification":"inconclusive",
            "reasons":[],"work_item_completion":"not_evaluated",
            "regression_causality":"not_established","latency_slo":"not_defined",
            "scope":"bounded native deployment, functional, signal and trace evidence for this window",
            "collection_states":{"identity_before":self.identity_before["identity_state"],"identity_after":self.identity_after["identity_state"],"functional_probes":self.functional_probes["probe_state"],"signals":self.signals["signal_state"],"health_trace":self.health_trace["trace_state"]},
        });
        match self.validate_binding(now_unix_ms) {
            Ok(()) => {}
            Err(reason) => {
                result["reasons"] = json!([reason]);
                return result;
            }
        }
        let mut hashes = serde_json::Map::new();
        for (key, value) in [
            ("identity_before", &self.identity_before),
            ("identity_after", &self.identity_after),
            ("functional_probes", &self.functional_probes),
            ("signals", &self.signals),
            ("health_trace", &self.health_trace),
        ] {
            let Ok(hash) = crate::canonical_json_sha256(value) else {
                result["reasons"] = json!(["native_evidence_cannot_be_hashed"]);
                return result;
            };
            hashes.insert(key.into(), json!(hash));
        }
        result["evidence_sha256"] = json!(hashes);
        let mut failed = false;
        let mut reasons = Vec::new();
        let probes = &self.functional_probes;
        if probes["probe_state"] == "failed"
            || probes["probes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["state"] == "failed")
        {
            failed = true;
            reasons.push("functional_response_failed");
        } else if probes["probe_state"] != "passed"
            || probes["probes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["state"] != "passed")
        {
            reasons.push("functional_responses_inconclusive");
        }
        let signals = &self.signals;
        if signals["signal_state"] == "anomaly_observed"
            || signals["queries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|q| q["summary"]["anomaly_observed"] == true)
        {
            failed = true;
            reasons.push("application_signal_anomaly_observed");
        } else if signals["signal_state"] != "observed"
            || !signals["reasons"].as_array().is_some_and(Vec::is_empty)
            || signals["queries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|q| q["state"] != "observed" || q["summary"]["anomaly_observed"] != false)
        {
            reasons.push("application_signals_inconclusive");
        }
        if !binding::trace_passes(self) {
            reasons.push("required_health_trace_inconclusive");
        }
        result["runtime_verification"] = json!(if failed {
            "failed"
        } else if reasons.is_empty() {
            "passed"
        } else {
            "inconclusive"
        });
        result["reasons"] = json!(reasons);
        result
    }

    fn validate_binding(&self, now: u64) -> Result<(), &'static str> {
        let duration = match self.phase {
            FinanceVerificationPhase::Production => 600,
            _ => 300,
        };
        if self
            .window
            .end_unix_seconds
            .checked_sub(self.window.start_unix_seconds)
            != Some(duration)
            || (self.phase == FinanceVerificationPhase::Staging
                && self.expected.environment != FinanceEnvironment::Staging)
            || (self.phase == FinanceVerificationPhase::Production
                && self.expected.environment != FinanceEnvironment::Production)
        {
            return Err("verification_phase_or_duration_mismatch");
        }
        WindowContext::validate(
            &self.expected,
            &self.window,
            &self.identity_before,
            &self.identity_after,
            now,
        )?;
        binding::validate(self, now)
    }
}

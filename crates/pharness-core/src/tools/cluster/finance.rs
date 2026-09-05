//! Native Finance deployment reads. These attest observed identity, not runtime success.
use super::{ReadOnlyClusterTools, ToolError, ToolResult};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

mod identity;
mod kubernetes;
mod probes;
mod signals;
mod traces;
pub use signals::FinanceRuntimeWindow;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinanceApplication {
    Yfinance,
    Frontend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinanceEnvironment {
    Staging,
    Production,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinanceDeploymentExpectation {
    pub application: FinanceApplication,
    pub environment: FinanceEnvironment,
    pub gitops_commit_sha: String,
    pub image_digest: String,
}

impl FinanceDeploymentExpectation {
    pub fn validate(&self) -> Result<(), String> {
        let sha = |s: &str, n| {
            s.len() == n
                && s.bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        };
        if !sha(&self.gitops_commit_sha, 40)
            || !self
                .image_digest
                .strip_prefix("sha256:")
                .is_some_and(|v| sha(v, 64))
        {
            return Err("Finance deployment observation requires full immutable GitOps and image identities".into());
        }
        Ok(())
    }

    pub fn namespace(&self) -> &'static str {
        match self.environment {
            FinanceEnvironment::Staging => "apps-staging",
            FinanceEnvironment::Production => "apps-prod",
        }
    }

    pub fn workload(&self) -> &'static str {
        match self.application {
            FinanceApplication::Yfinance => "yfinance-wrapper",
            FinanceApplication::Frontend => "finance-frontend",
        }
    }

    pub fn argo_application(&self) -> &'static str {
        match (self.application, self.environment) {
            (FinanceApplication::Yfinance, FinanceEnvironment::Staging) => "yfinance-staging",
            (FinanceApplication::Frontend, FinanceEnvironment::Staging) => {
                "finance-frontend-staging"
            }
            _ => self.workload(),
        }
    }

    pub fn gitops_path(&self) -> &'static str {
        match (self.application, self.environment) {
            (FinanceApplication::Yfinance, FinanceEnvironment::Staging) => {
                "charts/finance-staging/yfinance"
            }
            (FinanceApplication::Frontend, FinanceEnvironment::Staging) => {
                "charts/finance-staging/frontend"
            }
            (FinanceApplication::Yfinance, FinanceEnvironment::Production) => {
                "charts/yfinance-wrapper"
            }
            (FinanceApplication::Frontend, FinanceEnvironment::Production) => {
                "charts/finance-frontend"
            }
        }
    }

    pub fn image_ref(&self) -> String {
        let repository = match self.application {
            FinanceApplication::Yfinance => "yfinance_wrapper",
            FinanceApplication::Frontend => "finance-frontend",
        };
        format!(
            "registry.lucas.engineering/{repository}@{}",
            self.image_digest
        )
    }

    fn container_port(&self) -> u64 {
        match self.application {
            FinanceApplication::Yfinance => 8000,
            FinanceApplication::Frontend => 80,
        }
    }

    fn service_port(&self) -> u64 {
        match self.application {
            FinanceApplication::Yfinance => 8090,
            FinanceApplication::Frontend => 8080,
        }
    }
}

impl ReadOnlyClusterTools {
    /// Bounded read-only native path, deliberately absent from the agent tool schema.
    /// Caller must obtain expected identities from persisted delivery evidence.
    /// The requested commit must be Argo's current observed revision; an unproven
    /// later revision is inconclusive, even if the requested digest is still live.
    pub async fn observe_finance_deployment(
        &self,
        expected: &FinanceDeploymentExpectation,
    ) -> Result<ToolResult, ToolError> {
        expected
            .validate()
            .map_err(|message| ToolError::InvalidArguments { message })?;
        let started = timestamp()?;
        let mut evidence = json!({
            "schema_version":"pharness.dev/finance-deployment-observation/v1alpha1",
            "expected":expected,"cluster":"lucas_engineering",
            "started_at_unix_ms":started,"identity_state":"inconclusive",
            "runtime_verification":"not_evaluated","reasons":[],
            "limits":{"reads":6,"list_items":32,"timeout_ms":self.timeout_ms.clamp(1,15_000),"response_bytes_per_read":self.max_output_bytes.clamp(1,512*1024)},
        });
        match kubernetes::read(self, expected).await {
            Ok(resources) => match identity::verify(expected, &resources) {
                Ok(identity) => {
                    evidence["identity_state"] = json!("verified");
                    evidence["identity"] = identity;
                    evidence["read_receipts"] = json!(resources.receipts);
                }
                Err(reason) => {
                    evidence["reasons"] = json!([reason]);
                    evidence["read_receipts"] = json!(resources.receipts);
                }
            },
            Err(reason) => evidence["reasons"] = json!([reason]),
        }
        evidence["completed_at_unix_ms"] = json!(timestamp()?);
        Ok(ToolResult::ok(
            "Observed Finance deployment identity; runtime acceptance is separate",
            evidence,
        ))
    }
}

fn timestamp() -> Result<u64, ToolError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|v| u64::try_from(v.as_millis()).ok())
        .ok_or_else(|| ToolError::InvalidArguments {
            message: "system clock cannot timestamp Finance evidence".into(),
        })
}

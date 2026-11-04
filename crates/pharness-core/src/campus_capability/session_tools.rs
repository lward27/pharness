//! The tool-path entry to the broker.

use super::broker::CampusCapabilityBroker;
use super::{CampusCaller, CampusCapabilityRequest, CampusSessionClaims};
use crate::{AgentAction, ToolError, ToolExecutor, ToolResult};
use async_trait::async_trait;
use std::sync::Arc;

/// The [`ToolExecutor`] a Campus agent runtime is given. It holds no
/// executor of its own: every action, whatever the model proposed, becomes a
/// broker request bound to the session claims fixed at launch, so the model
/// can neither reach an effect directly nor choose the session it acts in.
pub struct CampusSessionTools {
    broker: Arc<CampusCapabilityBroker>,
    claims: CampusSessionClaims,
}

impl CampusSessionTools {
    pub fn new(broker: Arc<CampusCapabilityBroker>, claims: CampusSessionClaims) -> Self {
        Self { broker, claims }
    }
}

#[async_trait]
impl ToolExecutor for CampusSessionTools {
    async fn execute(&self, action: &AgentAction) -> Result<ToolResult, ToolError> {
        let request = CampusCapabilityRequest {
            caller: CampusCaller::SessionRuntime {
                session_id: self.claims.session_id.clone(),
            },
            claims: self.claims.clone(),
            capability: action.kind_name().into(),
            resource: None,
            action: action.clone(),
        };
        let denied = |reason: String| ToolError::CapabilityDenied {
            action: action.kind_name().into(),
            reason,
        };
        match self.broker.execute(request).await {
            Ok(outcome) => match outcome.result {
                Some(result) => result,
                None => Err(denied(format!(
                    "{}: {}",
                    outcome.decision.reason.as_str(),
                    outcome.decision.summary
                ))),
            },
            Err(error) => Err(denied(error.to_string())),
        }
    }
}

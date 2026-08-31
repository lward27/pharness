use super::{ModelCapabilities, ModelRequest, ModelTurn};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProtocolErrorKind {
    MissingAction,
    MalformedArguments,
    MultipleActions,
}

impl ProviderProtocolErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissingAction => "missing_action",
            Self::MalformedArguments => "malformed_arguments",
            Self::MultipleActions => "multiple_actions",
        }
    }
}

#[async_trait]
pub trait ModelProvider: Send + Sync {
    async fn complete_action(&self, request: ModelRequest) -> Result<ModelTurn, ProviderError>;

    fn capabilities(&self) -> ModelCapabilities;
}

#[async_trait]
impl<T> ModelProvider for Arc<T>
where
    T: ModelProvider + ?Sized,
{
    async fn complete_action(&self, request: ModelRequest) -> Result<ModelTurn, ProviderError> {
        (**self).complete_action(request).await
    }

    fn capabilities(&self) -> ModelCapabilities {
        (**self).capabilities()
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProviderError {
    #[error("provider request failed: {message}")]
    RequestFailed { message: String, retryable: bool },
    #[error("provider returned malformed response: {message}")]
    MalformedResponse { message: String },
    #[error("provider protocol error ({category:?}): {message}")]
    Protocol {
        category: ProviderProtocolErrorKind,
        message: String,
    },
    #[error("provider does not support requested capability: {capability}")]
    UnsupportedCapability { capability: String },
}

use super::{ModelCapabilities, ModelRequest, ModelTurn};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[async_trait]
pub trait ModelProvider: Send + Sync {
    async fn complete_action(&self, request: ModelRequest) -> Result<ModelTurn, ProviderError>;

    fn capabilities(&self) -> ModelCapabilities;
}

#[derive(Debug, Error, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProviderError {
    #[error("provider request failed: {message}")]
    RequestFailed { message: String, retryable: bool },
    #[error("provider returned malformed response: {message}")]
    MalformedResponse { message: String },
    #[error("provider does not support requested capability: {capability}")]
    UnsupportedCapability { capability: String },
}

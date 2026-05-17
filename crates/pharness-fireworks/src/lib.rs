#![forbid(unsafe_code)]

mod client;
mod stream;
mod types;

pub use client::{FireworksClient, FireworksClientError, FireworksProviderConfig};
pub use stream::{AccumulatedToolCall, FireworksStreamAggregate, SseDecoder, ToolCallAccumulator};
pub use types::{
    FireworksChatMessage, FireworksChatRequest, FireworksChatTool, FireworksChoiceDelta,
    FireworksFunctionCallDelta, FireworksFunctionTool, FireworksResponseFormat,
    FireworksStreamChunk, FireworksToolCallDelta, FireworksToolChoice,
};

pub const DEFAULT_FIREWORKS_BASE_URL: &str = "https://api.fireworks.ai/inference/v1";

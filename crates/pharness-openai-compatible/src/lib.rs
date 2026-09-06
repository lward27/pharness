#![forbid(unsafe_code)]

mod client;
mod gateway;
mod minimax;
mod stream;
mod types;

pub use client::{
    aggregate_to_model_turn, OpenAiCompatibleClient, OpenAiCompatibleError,
    OpenAiCompatibleTransportOptions, RetryPolicy,
};
pub use gateway::{GatewayClientConfig, GatewayModelClient};
pub use minimax::preserve_minimax_system_context;
pub use stream::{AccumulatedToolCall, OpenAiStreamAggregate, SseDecoder, ToolCallAccumulator};
pub use types::{
    build_chat_request, ChatMessage, ChatRequest, ChatTool, ChoiceDelta, CompletionTokenDetails,
    Delta, FunctionCallDelta, FunctionTool, MessageFunctionCall, MessageToolCall,
    OpenRouterProviderPreferences, PromptTokenDetails, ReasoningOptions, ResponseFormat,
    StreamChunk, StreamOptions, TokenUsagePayload, ToolCallDelta, ToolChoice,
};

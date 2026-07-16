//! OpenAI-compatible HTTP API (`/v1/chat/completions`, `/v1/models`).
//!
//! This module provides a direct LLM proxy through the web gateway so any
//! standard OpenAI client library can use IronClaw as a backend by simply
//! changing the `base_url`.
//!
//! ## Module layout
//!
//! - [`types`] — OpenAI wire types (requests, responses, chunks, errors)
//! - [`convert`] — conversions between OpenAI and internal LLM types
//! - [`handlers`] — Axum handlers for the non-streaming endpoints
//! - [`streaming`] — simulated SSE streaming path

mod convert;
mod handlers;
mod streaming;
mod types;

#[cfg(test)]
mod tests;

pub use convert::{convert_messages, convert_tools, finish_reason_str};
pub use handlers::{chat_completions_handler, models_handler};
pub use types::{
    OpenAiChatChunk, OpenAiChatRequest, OpenAiChatResponse, OpenAiChoice, OpenAiChunkChoice,
    OpenAiDelta, OpenAiErrorDetail, OpenAiErrorResponse, OpenAiFunction, OpenAiMessage, OpenAiTool,
    OpenAiToolCall, OpenAiToolCallDelta, OpenAiToolCallFunction, OpenAiToolCallFunctionDelta,
    OpenAiUsage,
};

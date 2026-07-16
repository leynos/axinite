//! LLM provider trait and types.
//!
//! The message types live in `messages`, the request/response types in
//! `requests`; this file holds the `LlmProvider` / `NativeLlmProvider`
//! traits and the blanket adapter between them.

use core::future::Future;
use core::pin::Pin;

use rust_decimal::Decimal;

use crate::llm::error::LlmError;

mod messages;
mod requests;

pub use messages::{ChatMessage, ContentPart, ImageUrl, Role, sanitize_tool_messages};
pub use requests::{
    CompletionRequest, CompletionResponse, FinishReason, ModelMetadata, ToolCall,
    ToolCompletionRequest, ToolCompletionResponse, ToolDefinition, ToolResult,
    strip_unsupported_completion_params, strip_unsupported_tool_params,
};

/// Boxed future used at the dyn `LlmProvider` boundary.
pub type LlmFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Trait for LLM providers.
///
/// This is the dyn-safe object boundary. Concrete implementations should
/// implement [`NativeLlmProvider`] instead; the blanket adapter provides this
/// trait automatically.
pub trait LlmProvider: Send + Sync {
    /// Get the model name.
    fn model_name(&self) -> &str;

    /// Get cost per token (input, output).
    fn cost_per_token(&self) -> (Decimal, Decimal);

    /// Complete a chat conversation.
    fn complete<'a>(
        &'a self,
        request: CompletionRequest,
    ) -> LlmFuture<'a, Result<CompletionResponse, LlmError>>;

    /// Complete with tool use support.
    fn complete_with_tools<'a>(
        &'a self,
        request: ToolCompletionRequest,
    ) -> LlmFuture<'a, Result<ToolCompletionResponse, LlmError>>;

    /// List available models from the provider.
    fn list_models<'a>(&'a self) -> LlmFuture<'a, Result<Vec<String>, LlmError>>;

    /// Fetch metadata for the current model (context length, etc.).
    fn model_metadata<'a>(&'a self) -> LlmFuture<'a, Result<ModelMetadata, LlmError>>;

    /// Resolve which model should be reported for a given request.
    ///
    /// Providers that ignore per-request model overrides should override this
    /// and return `active_model_name()`.
    fn effective_model_name(&self, requested_model: Option<&str>) -> String {
        requested_model
            .map(std::borrow::ToOwned::to_owned)
            .unwrap_or_else(|| self.active_model_name())
    }

    /// Get the currently active model name.
    ///
    /// May differ from `model_name()` if the model was switched at runtime
    /// via `set_model()`. Default returns `model_name()`.
    fn active_model_name(&self) -> String {
        self.model_name().to_string()
    }

    /// Switch the active model at runtime. Not all providers support this.
    fn set_model(&self, _model: &str) -> Result<(), LlmError> {
        Err(LlmError::RequestFailed {
            provider: "unknown".to_string(),
            reason: "Runtime model switching not supported by this provider".to_string(),
        })
    }

    /// Calculate cost for a completion.
    fn calculate_cost(&self, input_tokens: u32, output_tokens: u32) -> Decimal {
        let (input_cost, output_cost) = self.cost_per_token();
        input_cost * Decimal::from(input_tokens) + output_cost * Decimal::from(output_tokens)
    }

    /// Cost multiplier for cache-creation tokens (Anthropic prompt caching).
    ///
    /// Returns `1.0` by default (no surcharge). Anthropic providers return
    /// `1.25` for 5-minute TTL or `2.0` for 1-hour TTL.
    fn cache_write_multiplier(&self) -> Decimal {
        Decimal::ONE
    }

    /// Discount divisor for cache-read tokens.
    ///
    /// Cached-read cost = `input_rate / cache_read_discount()`.
    /// Returns `1` by default (no discount). Anthropic returns `10` (90% off),
    /// OpenAI would return `2` (50% off).
    fn cache_read_discount(&self) -> Decimal {
        Decimal::ONE
    }
}

/// Native (non-dyn) sibling of [`LlmProvider`] for concrete implementations.
///
/// Implement this trait instead of [`LlmProvider`] directly. The blanket
/// adapter below automatically implements [`LlmProvider`] for every
/// `T: NativeLlmProvider`.
pub trait NativeLlmProvider: Send + Sync {
    /// Get the model name.
    fn model_name(&self) -> &str;

    /// Get cost per token (input, output).
    fn cost_per_token(&self) -> (Decimal, Decimal);

    /// Complete a chat conversation.
    fn complete(
        &self,
        request: CompletionRequest,
    ) -> impl Future<Output = Result<CompletionResponse, LlmError>> + Send + '_;

    /// Complete with tool use support.
    fn complete_with_tools(
        &self,
        request: ToolCompletionRequest,
    ) -> impl Future<Output = Result<ToolCompletionResponse, LlmError>> + Send + '_;

    /// List available models from the provider.
    /// Default implementation returns empty list.
    fn list_models(&self) -> impl Future<Output = Result<Vec<String>, LlmError>> + Send + '_ {
        async { Ok(Vec::new()) }
    }

    /// Fetch metadata for the current model (context length, etc.).
    /// Default returns the model name with no size info.
    fn model_metadata(&self) -> impl Future<Output = Result<ModelMetadata, LlmError>> + Send + '_ {
        async {
            Ok(ModelMetadata {
                id: self.model_name().to_string(),
                context_length: None,
            })
        }
    }

    /// Resolve which model should be reported for a given request.
    fn effective_model_name(&self, requested_model: Option<&str>) -> String {
        requested_model
            .map(std::borrow::ToOwned::to_owned)
            .unwrap_or_else(|| self.active_model_name())
    }

    /// Get the currently active model name.
    fn active_model_name(&self) -> String {
        self.model_name().to_string()
    }

    /// Switch the active model at runtime. Not all providers support this.
    fn set_model(&self, _model: &str) -> Result<(), LlmError> {
        Err(LlmError::RequestFailed {
            provider: "unknown".to_string(),
            reason: "Runtime model switching not supported by this provider".to_string(),
        })
    }

    /// Calculate cost for a completion.
    fn calculate_cost(&self, input_tokens: u32, output_tokens: u32) -> Decimal {
        let (input_cost, output_cost) = self.cost_per_token();
        input_cost * Decimal::from(input_tokens) + output_cost * Decimal::from(output_tokens)
    }

    /// Cost multiplier for cache-creation tokens (Anthropic prompt caching).
    fn cache_write_multiplier(&self) -> Decimal {
        Decimal::ONE
    }

    /// Discount divisor for cache-read tokens.
    fn cache_read_discount(&self) -> Decimal {
        Decimal::ONE
    }
}

impl<T: NativeLlmProvider> LlmProvider for T {
    fn model_name(&self) -> &str {
        NativeLlmProvider::model_name(self)
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        NativeLlmProvider::cost_per_token(self)
    }

    fn complete<'a>(
        &'a self,
        request: CompletionRequest,
    ) -> LlmFuture<'a, Result<CompletionResponse, LlmError>> {
        Box::pin(NativeLlmProvider::complete(self, request))
    }

    fn complete_with_tools<'a>(
        &'a self,
        request: ToolCompletionRequest,
    ) -> LlmFuture<'a, Result<ToolCompletionResponse, LlmError>> {
        Box::pin(NativeLlmProvider::complete_with_tools(self, request))
    }

    fn list_models<'a>(&'a self) -> LlmFuture<'a, Result<Vec<String>, LlmError>> {
        Box::pin(NativeLlmProvider::list_models(self))
    }

    fn model_metadata<'a>(&'a self) -> LlmFuture<'a, Result<ModelMetadata, LlmError>> {
        Box::pin(NativeLlmProvider::model_metadata(self))
    }

    fn effective_model_name(&self, requested_model: Option<&str>) -> String {
        NativeLlmProvider::effective_model_name(self, requested_model)
    }

    fn active_model_name(&self) -> String {
        NativeLlmProvider::active_model_name(self)
    }

    fn set_model(&self, model: &str) -> Result<(), LlmError> {
        NativeLlmProvider::set_model(self, model)
    }

    fn calculate_cost(&self, input_tokens: u32, output_tokens: u32) -> Decimal {
        NativeLlmProvider::calculate_cost(self, input_tokens, output_tokens)
    }

    fn cache_write_multiplier(&self) -> Decimal {
        NativeLlmProvider::cache_write_multiplier(self)
    }

    fn cache_read_discount(&self) -> Decimal {
        NativeLlmProvider::cache_read_discount(self)
    }
}

#[cfg(test)]
mod tests;

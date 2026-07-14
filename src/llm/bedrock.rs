//! AWS Bedrock LLM provider using the native Converse API.
//!
//! Uses `aws-sdk-bedrockruntime` to call `client.converse()` directly,
//! bypassing the OpenAI-compatible layer. Supports standard AWS auth methods:
//! IAM credentials, SSO profiles, and instance roles — all handled
//! transparently by the AWS SDK credential chain.

mod convert;
mod documents;
mod response;
#[cfg(test)]
mod tests;

use std::sync::RwLock;

use aws_config::{BehaviorVersion, Region};
use aws_sdk_bedrockruntime::Client;
use rust_decimal::Decimal;

use crate::llm::config::BedrockConfig;
use crate::llm::error::LlmError;
use crate::llm::provider::{
    CompletionRequest, CompletionResponse, ModelMetadata, NativeLlmProvider, ToolCompletionRequest,
    ToolCompletionResponse,
};

use convert::{build_inference_config, build_tool_config, convert_messages};
use response::{extract_content_blocks, extract_token_usage, map_sdk_error, map_stop_reason};

/// AWS Bedrock provider using the native Converse API.
pub struct BedrockProvider {
    client: Client,
    /// Base model ID for display purposes (without prefix).
    display_model: String,
    /// Cross-region prefix (e.g. "us.", "global.") or empty.
    cross_region_prefix: String,
    /// Active model ID (with cross-region prefix), switchable at runtime via `set_model()`.
    active_model: RwLock<String>,
}

impl BedrockProvider {
    /// Create a new Bedrock provider from configuration.
    ///
    /// Async because the AWS SDK config loader requires an async context
    /// to resolve credentials from SSO profiles, IMDS, etc.
    pub async fn new(config: &BedrockConfig) -> Result<Self, LlmError> {
        let cross_region_prefix = config
            .cross_region
            .as_ref()
            .map(|prefix| format!("{}.", prefix))
            .unwrap_or_default();

        let model_id = format!("{}{}", cross_region_prefix, config.model);

        let mut builder = aws_config::defaults(BehaviorVersion::latest())
            .region(Region::new(config.region.clone()));
        if let Some(ref profile) = config.profile {
            builder = builder.profile_name(profile);
        }
        let sdk_config = builder.load().await;

        let client = Client::new(&sdk_config);

        Ok(Self {
            client,
            display_model: config.model.clone(),
            cross_region_prefix,
            active_model: RwLock::new(model_id),
        })
    }

    /// Get the currently active model ID (with cross-region prefix).
    fn current_model_id(&self) -> String {
        match self.active_model.read() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => {
                tracing::warn!("active_model lock poisoned while reading; continuing");
                poisoned.into_inner().clone()
            }
        }
    }
}

impl NativeLlmProvider for BedrockProvider {
    fn model_name(&self) -> &str {
        &self.display_model
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        // Bedrock billing is on the AWS bill, not trackable per-token here.
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let model_id = self.current_model_id();

        let mut messages = request.messages;
        crate::llm::provider::sanitize_tool_messages(&mut messages);

        let (system_blocks, bedrock_messages) = convert_messages(&messages)?;

        if bedrock_messages.is_empty() {
            return Err(LlmError::RequestFailed {
                provider: "bedrock".to_string(),
                reason: "Bedrock requires at least one user or assistant message".to_string(),
            });
        }

        let mut builder = self
            .client
            .converse()
            .model_id(&model_id)
            .set_system(if system_blocks.is_empty() {
                None
            } else {
                Some(system_blocks)
            })
            .set_messages(Some(bedrock_messages));

        if let Some(config) = build_inference_config(
            request.temperature,
            request.max_tokens,
            request.stop_sequences.as_deref(),
        ) {
            builder = builder.inference_config(config);
        }

        let response = builder.send().await.map_err(|e| map_sdk_error(&e))?;

        let (text, _tool_calls) = extract_content_blocks(response.output())?;
        let (input_tokens, output_tokens) = extract_token_usage(response.usage());

        Ok(CompletionResponse {
            content: text,
            input_tokens,
            output_tokens,
            finish_reason: map_stop_reason(response.stop_reason()),
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
        })
    }

    async fn complete_with_tools(
        &self,
        request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        let model_id = self.current_model_id();

        let mut messages = request.messages;
        crate::llm::provider::sanitize_tool_messages(&mut messages);

        let (system_blocks, bedrock_messages) = convert_messages(&messages)?;

        if bedrock_messages.is_empty() {
            return Err(LlmError::RequestFailed {
                provider: "bedrock".to_string(),
                reason: "Bedrock requires at least one user or assistant message".to_string(),
            });
        }

        let tool_config = build_tool_config(&request.tools, request.tool_choice.as_deref())?;

        let mut builder = self
            .client
            .converse()
            .model_id(&model_id)
            .set_system(if system_blocks.is_empty() {
                None
            } else {
                Some(system_blocks)
            })
            .set_messages(Some(bedrock_messages));

        if let Some(tc) = tool_config {
            builder = builder.tool_config(tc);
        }

        if let Some(config) = build_inference_config(request.temperature, request.max_tokens, None)
        {
            builder = builder.inference_config(config);
        }

        let response = builder.send().await.map_err(|e| map_sdk_error(&e))?;

        let (text, tool_calls) = extract_content_blocks(response.output())?;
        let (input_tokens, output_tokens) = extract_token_usage(response.usage());

        Ok(ToolCompletionResponse {
            content: if text.is_empty() { None } else { Some(text) },
            tool_calls,
            input_tokens,
            output_tokens,
            finish_reason: map_stop_reason(response.stop_reason()),
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
        })
    }

    async fn model_metadata(&self) -> Result<ModelMetadata, LlmError> {
        Ok(ModelMetadata {
            id: self.current_model_id(),
            context_length: None,
        })
    }

    fn active_model_name(&self) -> String {
        self.current_model_id()
    }

    fn effective_model_name(&self, _requested_model: Option<&str>) -> String {
        // Bedrock doesn't support per-request model overrides in Converse API;
        // the model is part of the request builder, not the message body.
        self.active_model_name()
    }

    fn set_model(&self, model: &str) -> Result<(), LlmError> {
        let new_id = format!("{}{}", self.cross_region_prefix, model);
        match self.active_model.write() {
            Ok(mut guard) => {
                *guard = new_id;
            }
            Err(poisoned) => {
                tracing::warn!("active_model lock poisoned while writing; continuing");
                *poisoned.into_inner() = new_id;
            }
        }
        Ok(())
    }
}

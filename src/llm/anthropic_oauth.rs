//! Anthropic OAuth provider (direct HTTP, `Authorization: Bearer`).
//!
//! This provider exists because the `rig-core` Anthropic client hardcodes the
//! `x-api-key` header, which is rejected by Anthropic's OAuth tokens from
//! `claude login`. OAuth tokens require `Authorization: Bearer <token>` instead.
//!
//! Pattern follows `nearai_chat.rs`: direct HTTP calls via `reqwest::Client`.

use std::collections::HashSet;

use reqwest::Client;
use rust_decimal::Decimal;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};

use crate::llm::config::RegistryProviderConfig;
use crate::llm::costs;
use crate::llm::error::LlmError;
use crate::llm::provider::{
    CompletionRequest, CompletionResponse, FinishReason, NativeLlmProvider, ToolCompletionRequest,
    ToolCompletionResponse, strip_unsupported_completion_params, strip_unsupported_tool_params,
};

mod convert;
mod http;

use convert::{convert_messages, extract_response_content};

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
/// OAuth beta requires 2023-06-01; the 2024-10-22 version is not valid with the beta flag.
const ANTHROPIC_API_VERSION: &str = "2023-06-01";
/// Required beta flag to enable OAuth Bearer auth on api.anthropic.com.
/// Without this header, the API returns 401 "OAuth authentication is currently not supported."
const ANTHROPIC_OAUTH_BETA: &str = "oauth-2025-04-20";
const DEFAULT_MAX_TOKENS: u32 = 8192;

/// Anthropic provider using OAuth Bearer authentication.
pub struct AnthropicOAuthProvider {
    client: Client,
    token: SecretString,
    model: String,
    base_url: Option<String>,
    active_model: std::sync::RwLock<String>,
    /// Parameter names that this provider does not support.
    unsupported_params: HashSet<String>,
}

impl AnthropicOAuthProvider {
    pub fn new(config: &RegistryProviderConfig) -> Result<Self, LlmError> {
        let token = config
            .oauth_token
            .clone()
            .ok_or_else(|| LlmError::AuthFailed {
                provider: "anthropic_oauth".to_string(),
            })?;

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| LlmError::RequestFailed {
                provider: "anthropic_oauth".to_string(),
                reason: format!("Failed to build HTTP client: {}", e),
            })?;

        let active_model = std::sync::RwLock::new(config.model.clone());
        let base_url = if config.base_url.is_empty() {
            None
        } else {
            Some(config.base_url.clone())
        };

        let unsupported_params: HashSet<String> =
            config.unsupported_params.iter().cloned().collect();

        Ok(Self {
            client,
            token,
            model: config.model.clone(),
            base_url,
            active_model,
            unsupported_params,
        })
    }

    /// Strip unsupported fields from a `CompletionRequest` in place.
    fn strip_unsupported_completion_params(&self, req: &mut CompletionRequest) {
        strip_unsupported_completion_params(&self.unsupported_params, req);
    }

    /// Strip unsupported fields from a `ToolCompletionRequest` in place.
    fn strip_unsupported_tool_params(&self, req: &mut ToolCompletionRequest) {
        strip_unsupported_tool_params(&self.unsupported_params, req);
    }
}

impl NativeLlmProvider for AnthropicOAuthProvider {
    async fn complete(&self, mut req: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let model = req.model.take().unwrap_or_else(|| self.active_model_name());
        self.strip_unsupported_completion_params(&mut req);
        let (system, messages) = convert_messages(req.messages);

        let request = AnthropicRequest {
            model,
            messages,
            system,
            max_tokens: req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            temperature: req.temperature,
            tools: None,
            tool_choice: None,
        };

        let response: AnthropicResponse = self.send_request(&request).await?;
        let (content, _tool_calls) = extract_response_content(&response);

        let finish_reason = match response.stop_reason.as_deref() {
            Some("end_turn") | Some("stop") => FinishReason::Stop,
            Some("max_tokens") => FinishReason::Length,
            Some("tool_use") => FinishReason::ToolUse,
            _ => FinishReason::Unknown,
        };

        Ok(CompletionResponse {
            content: content.unwrap_or_default(),
            finish_reason,
            input_tokens: response.usage.input_tokens,
            output_tokens: response.usage.output_tokens,
            cache_creation_input_tokens: response.usage.cache_creation_input_tokens,
            cache_read_input_tokens: response.usage.cache_read_input_tokens,
        })
    }

    async fn complete_with_tools(
        &self,
        mut req: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        let model = req.model.take().unwrap_or_else(|| self.active_model_name());
        self.strip_unsupported_tool_params(&mut req);
        let (system, messages) = convert_messages(req.messages);

        let tools: Vec<AnthropicTool> = req
            .tools
            .into_iter()
            .map(|t| AnthropicTool {
                name: t.name,
                description: t.description,
                input_schema: t.parameters,
            })
            .collect();

        // Map tool_choice from OpenAI format to Anthropic format
        let tool_choice = req.tool_choice.map(|tc| match tc.as_str() {
            "auto" => AnthropicToolChoice {
                choice_type: "auto".to_string(),
                name: None,
            },
            "required" => AnthropicToolChoice {
                choice_type: "any".to_string(),
                name: None,
            },
            "none" => AnthropicToolChoice {
                choice_type: "none".to_string(),
                name: None,
            },
            specific => AnthropicToolChoice {
                choice_type: "tool".to_string(),
                name: Some(specific.to_string()),
            },
        });

        let request = AnthropicRequest {
            model,
            messages,
            system,
            max_tokens: req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            temperature: req.temperature,
            tools: if tools.is_empty() { None } else { Some(tools) },
            tool_choice,
        };

        let response: AnthropicResponse = self.send_request(&request).await?;
        let (content, tool_calls) = extract_response_content(&response);

        let finish_reason = match response.stop_reason.as_deref() {
            Some("end_turn") | Some("stop") => FinishReason::Stop,
            Some("max_tokens") => FinishReason::Length,
            Some("tool_use") => FinishReason::ToolUse,
            _ => {
                if !tool_calls.is_empty() {
                    FinishReason::ToolUse
                } else {
                    FinishReason::Unknown
                }
            }
        };

        Ok(ToolCompletionResponse {
            content,
            tool_calls,
            finish_reason,
            input_tokens: response.usage.input_tokens,
            output_tokens: response.usage.output_tokens,
            cache_creation_input_tokens: response.usage.cache_creation_input_tokens,
            cache_read_input_tokens: response.usage.cache_read_input_tokens,
        })
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        let model = self.active_model_name();
        costs::model_cost(&model).unwrap_or_else(costs::default_cost)
    }

    fn active_model_name(&self) -> String {
        match self.active_model.read() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    fn set_model(&self, model: &str) -> Result<(), LlmError> {
        match self.active_model.write() {
            Ok(mut guard) => {
                *guard = model.to_string();
            }
            Err(poisoned) => {
                *poisoned.into_inner() = model.to_string();
            }
        }
        Ok(())
    }
}

// --- Anthropic Messages API types ---

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<AnthropicToolChoice>,
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: AnthropicContent,
}

/// Anthropic content can be a simple string or a list of content blocks.
#[derive(Debug, Serialize)]
#[serde(untagged)]
enum AnthropicContent {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

#[derive(Debug, Serialize)]
struct AnthropicTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct AnthropicToolChoice {
    #[serde(rename = "type")]
    choice_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicResponseBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
    usage: AnthropicUsage,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum AnthropicResponseBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
    #[serde(default)]
    cache_creation_input_tokens: u32,
    #[serde(default)]
    cache_read_input_tokens: u32,
}

#[cfg(test)]
mod tests;

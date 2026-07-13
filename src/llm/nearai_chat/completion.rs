//! `NativeLlmProvider` implementation for the NEAR AI chat provider.
//!
//! Maps completion and tool-completion requests onto the OpenAI-compatible
//! wire types and converts responses back into provider-neutral types.

use rust_decimal::Decimal;

use crate::llm::costs;
use crate::llm::error::LlmError;
use crate::llm::nearai_chat::NearAiChatProvider;
use crate::llm::nearai_chat::wire::{
    ChatCompletionFunction, ChatCompletionMessage, ChatCompletionRequest, ChatCompletionResponse,
    ChatCompletionTool, flatten_tool_messages, parse_usage,
};
use crate::llm::provider::{
    CompletionRequest, CompletionResponse, FinishReason, NativeLlmProvider, ToolCall,
    ToolCompletionRequest, ToolCompletionResponse,
};

impl NativeLlmProvider for NearAiChatProvider {
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let model = req.model.unwrap_or_else(|| self.active_model_name());
        let mut raw_messages = req.messages;
        crate::llm::provider::sanitize_tool_messages(&mut raw_messages);
        let messages: Vec<ChatCompletionMessage> =
            raw_messages.into_iter().map(|m| m.into()).collect();

        let request = ChatCompletionRequest {
            model,
            messages,
            temperature: req.temperature,
            max_tokens: req.max_tokens,
            tools: None,
            tool_choice: None,
        };

        let response: ChatCompletionResponse = self.send_request(&request).await?;

        let choice =
            response
                .choices
                .into_iter()
                .next()
                .ok_or_else(|| LlmError::InvalidResponse {
                    provider: "nearai_chat".to_string(),
                    reason: "No choices in response".to_string(),
                })?;

        // Fall back to reasoning_content when content is null (same as
        // complete_with_tools — reasoning models may put the answer there).
        let content = choice
            .message
            .content
            .or(choice.message.reasoning_content)
            .unwrap_or_default();
        let finish_reason = match choice.finish_reason.as_deref() {
            Some("stop") => FinishReason::Stop,
            Some("length") => FinishReason::Length,
            Some("tool_calls") => FinishReason::ToolUse,
            Some("content_filter") => FinishReason::ContentFilter,
            _ => FinishReason::Unknown,
        };

        let (input_tokens, output_tokens) = parse_usage(response.usage.as_ref());

        Ok(CompletionResponse {
            content,
            finish_reason,
            input_tokens,
            output_tokens,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn complete_with_tools(
        &self,
        req: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        let model = req.model.unwrap_or_else(|| self.active_model_name());
        let mut raw_messages = req.messages;
        crate::llm::provider::sanitize_tool_messages(&mut raw_messages);
        let messages: Vec<ChatCompletionMessage> =
            raw_messages.into_iter().map(|m| m.into()).collect();

        // Some OpenAI-compatible providers reject `role:"tool"` messages.
        // When enabled, rewrite tool-call / tool-result pairs into plain text.
        let messages = if self.flatten_tool_messages {
            flatten_tool_messages(messages)
        } else {
            messages
        };

        let tools: Vec<ChatCompletionTool> = req
            .tools
            .into_iter()
            .map(|t| ChatCompletionTool {
                tool_type: "function".to_string(),
                function: ChatCompletionFunction {
                    name: t.name,
                    description: Some(t.description),
                    parameters: Some(t.parameters),
                },
            })
            .collect();

        let request = ChatCompletionRequest {
            model,
            messages,
            temperature: req.temperature,
            max_tokens: req.max_tokens,
            tools: if tools.is_empty() { None } else { Some(tools) },
            tool_choice: req.tool_choice,
        };

        let response: ChatCompletionResponse = self.send_request(&request).await?;

        let choice =
            response
                .choices
                .into_iter()
                .next()
                .ok_or_else(|| LlmError::InvalidResponse {
                    provider: "nearai_chat".to_string(),
                    reason: "No choices in response".to_string(),
                })?;

        let tool_calls: Vec<ToolCall> = choice
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(|tc| {
                let arguments = serde_json::from_str(&tc.function.arguments)
                    .unwrap_or(serde_json::Value::Object(Default::default()));
                ToolCall {
                    id: tc.id,
                    name: tc.function.name,
                    arguments,
                }
            })
            .collect();

        // Fall back to reasoning_content when content is null (e.g. GLM-5
        // returns its answer in reasoning_content instead of content), but
        // only for final text responses. Tool-call responses often have
        // content: null + reasoning_content filled with chain-of-thought;
        // leaking that into conversation history inflates context and
        // confuses the model.
        let content = if tool_calls.is_empty() {
            choice.message.content.or(choice.message.reasoning_content)
        } else {
            choice.message.content
        };

        let finish_reason = match choice.finish_reason.as_deref() {
            Some("stop") => FinishReason::Stop,
            Some("length") => FinishReason::Length,
            Some("tool_calls") => FinishReason::ToolUse,
            Some("content_filter") => FinishReason::ContentFilter,
            _ => {
                if !tool_calls.is_empty() {
                    FinishReason::ToolUse
                } else {
                    FinishReason::Unknown
                }
            }
        };

        let (input_tokens, output_tokens) = parse_usage(response.usage.as_ref());

        Ok(ToolCompletionResponse {
            content,
            tool_calls,
            finish_reason,
            input_tokens,
            output_tokens,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        let model = self.active_model_name();
        // Try fetched pricing first, then static lookup table, then default
        if let Ok(guard) = self.pricing.read()
            && let Some(&rates) = guard.get(&model)
        {
            return rates;
        }
        costs::model_cost(&model).unwrap_or_else(costs::default_cost)
    }

    async fn list_models(&self) -> Result<Vec<String>, LlmError> {
        let models = self.list_models_full().await?;
        Ok(models.into_iter().map(|m| m.name).collect())
    }

    fn active_model_name(&self) -> String {
        match self.active_model.read() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => {
                tracing::warn!("active_model lock poisoned while reading; continuing");
                poisoned.into_inner().clone()
            }
        }
    }

    fn set_model(&self, model: &str) -> Result<(), crate::error::LlmError> {
        match self.active_model.write() {
            Ok(mut guard) => {
                *guard = model.to_string();
            }
            Err(poisoned) => {
                tracing::warn!("active_model lock poisoned while writing; continuing");
                *poisoned.into_inner() = model.to_string();
            }
        }
        Ok(())
    }
}

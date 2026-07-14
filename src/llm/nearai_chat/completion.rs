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

/// Sanitize provider-neutral messages and convert them to wire format.
fn prepare_messages(
    mut raw_messages: Vec<crate::llm::provider::ChatMessage>,
) -> Vec<ChatCompletionMessage> {
    crate::llm::provider::sanitize_tool_messages(&mut raw_messages);
    raw_messages.into_iter().map(|m| m.into()).collect()
}

/// Pull the first choice from a response, failing when there is none.
fn first_choice(
    response: ChatCompletionResponse,
) -> Result<
    (
        crate::llm::nearai_chat::wire::ChatCompletionChoice,
        (u32, u32),
    ),
    LlmError,
> {
    let usage = parse_usage(response.usage.as_ref());
    let choice = response
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| LlmError::InvalidResponse {
            provider: "nearai_chat".to_string(),
            reason: "No choices in response".to_string(),
        })?;
    Ok((choice, usage))
}

/// Map the wire finish reason, treating an unknown reason with tool calls
/// present as `ToolUse`.
fn parse_finish_reason(reason: Option<&str>, has_tool_calls: bool) -> FinishReason {
    match reason {
        Some("stop") => FinishReason::Stop,
        Some("length") => FinishReason::Length,
        Some("tool_calls") => FinishReason::ToolUse,
        Some("content_filter") => FinishReason::ContentFilter,
        _ if has_tool_calls => FinishReason::ToolUse,
        _ => FinishReason::Unknown,
    }
}

/// Convert provider-neutral tool definitions to wire format.
fn convert_tools(tools: Vec<crate::llm::provider::ToolDefinition>) -> Vec<ChatCompletionTool> {
    tools
        .into_iter()
        .map(|t| ChatCompletionTool {
            tool_type: "function".to_string(),
            function: ChatCompletionFunction {
                name: t.name,
                description: Some(t.description),
                parameters: Some(t.parameters),
            },
        })
        .collect()
}

/// Parse wire tool calls, defaulting malformed argument JSON to an empty
/// object.
fn convert_tool_calls(
    raw: Option<Vec<crate::llm::nearai_chat::wire::ChatCompletionToolCall>>,
) -> Vec<ToolCall> {
    raw.unwrap_or_default()
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
        .collect()
}

impl NativeLlmProvider for NearAiChatProvider {
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let model = req.model.unwrap_or_else(|| self.active_model_name());
        let messages = prepare_messages(req.messages);

        let request = ChatCompletionRequest {
            model,
            messages,
            temperature: req.temperature,
            max_tokens: req.max_tokens,
            tools: None,
            tool_choice: None,
        };

        let response: ChatCompletionResponse = self.send_request(&request).await?;
        let (choice, (input_tokens, output_tokens)) = first_choice(response)?;

        // Fall back to reasoning_content when content is null (same as
        // complete_with_tools — reasoning models may put the answer there).
        let content = choice
            .message
            .content
            .or(choice.message.reasoning_content)
            .unwrap_or_default();
        let finish_reason = parse_finish_reason(choice.finish_reason.as_deref(), false);

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
        let messages = prepare_messages(req.messages);

        // Some OpenAI-compatible providers reject `role:"tool"` messages.
        // When enabled, rewrite tool-call / tool-result pairs into plain text.
        let messages = if self.flatten_tool_messages {
            flatten_tool_messages(messages)
        } else {
            messages
        };

        let tools = convert_tools(req.tools);

        let request = ChatCompletionRequest {
            model,
            messages,
            temperature: req.temperature,
            max_tokens: req.max_tokens,
            tools: if tools.is_empty() { None } else { Some(tools) },
            tool_choice: req.tool_choice,
        };

        let response: ChatCompletionResponse = self.send_request(&request).await?;
        let (choice, (input_tokens, output_tokens)) = first_choice(response)?;

        let tool_calls = convert_tool_calls(choice.message.tool_calls);

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

        let finish_reason =
            parse_finish_reason(choice.finish_reason.as_deref(), !tool_calls.is_empty());

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

//! Response generation for the `Reasoning` engine: the agentic
//! `respond_with_tools` flow (tool completion, tool-call recovery, cleaning)
//! and the text-formatting `respond` convenience wrapper.

use crate::llm::error::LlmError;
use crate::llm::{ChatMessage, CompletionRequest, Role, ToolCompletionRequest};

use super::{
    Reasoning, ReasoningContext, RespondOutput, RespondResult, TokenUsage, clean_response,
    merge_system_messages, recover_tool_calls_from_content, truncate_at_tool_tags,
};

impl Reasoning {
    /// Generate a response to a user message.
    ///
    /// If tools are available in the context, uses tool completion mode.
    /// This is a convenience wrapper around `respond_with_tools()` that formats
    /// tool calls as text for simple cases. Use `respond_with_tools()` when you
    /// need to actually execute tool calls in an agentic loop.
    pub async fn respond(&self, context: &ReasoningContext) -> Result<String, LlmError> {
        let output = self.respond_with_tools(context).await?;
        match output.result {
            RespondResult::Text(text) => Ok(text),
            RespondResult::ToolCalls {
                tool_calls: calls, ..
            } => {
                // Format tool calls as text (legacy behavior for non-agentic callers)
                let tool_info: Vec<String> = calls
                    .iter()
                    .map(|tc| format!("`{}({})`", tc.name, tc.arguments))
                    .collect();
                Ok(format!("[Calling tools: {}]", tool_info.join(", ")))
            }
        }
    }

    /// Generate a response that may include tool calls, with token usage tracking.
    ///
    /// Returns `RespondOutput` containing the result and token usage from the LLM call.
    /// The caller should use `usage` to track cost/budget against the job.
    pub async fn respond_with_tools(
        &self,
        context: &ReasoningContext,
    ) -> Result<RespondOutput, LlmError> {
        let system_prompt = match context.system_prompt {
            Some(ref prompt) => prompt.clone(),
            None => self.build_system_prompt_with_tools(&context.available_tools),
        };

        let system_prompt = merge_system_messages(system_prompt, &context.messages);
        let mut messages = vec![ChatMessage::system(system_prompt)];
        messages.extend(
            context
                .messages
                .iter()
                .filter(|m| m.role != Role::System)
                .cloned(),
        );

        let effective_tools = if context.force_text {
            Vec::new()
        } else {
            context.available_tools.clone()
        };

        // If we have tools, use tool completion mode
        if !effective_tools.is_empty() {
            let mut request = ToolCompletionRequest::new(messages, effective_tools)
                .with_max_tokens(4096)
                .with_temperature(0.7)
                .with_tool_choice("auto");
            request.metadata = context.metadata.clone();

            let response = self.llm.complete_with_tools(request).await?;
            let usage = TokenUsage {
                input_tokens: response.input_tokens,
                output_tokens: response.output_tokens,
                cache_read_input_tokens: response.cache_read_input_tokens,
                cache_creation_input_tokens: response.cache_creation_input_tokens,
            };

            // If there were tool calls, return them for execution
            if !response.tool_calls.is_empty() {
                return Ok(RespondOutput {
                    result: RespondResult::ToolCalls {
                        tool_calls: response.tool_calls,
                        content: response.content.map(|c| {
                            let pre_truncated = truncate_at_tool_tags(&c);
                            clean_response(&pre_truncated)
                        }),
                    },
                    usage,
                });
            }

            let content = response
                .content
                .unwrap_or_else(|| "I'm not sure how to respond to that.".to_string());

            // Some models (e.g. GLM-4.7) emit tool calls as XML tags in content
            // instead of using the structured tool_calls field. Try to recover
            // them before giving up and returning plain text.
            // NOTE: Recovery runs on the raw content (before truncation) so it can
            // parse tool-call JSON from the XML tags. Truncation only applies to the
            // remaining *text* content returned alongside the recovered tool calls.
            let recovered = recover_tool_calls_from_content(&content, &context.available_tools);
            if !recovered.is_empty() {
                let pre_truncated = truncate_at_tool_tags(&content);
                let cleaned = clean_response(&pre_truncated);
                return Ok(RespondOutput {
                    result: RespondResult::ToolCalls {
                        tool_calls: recovered,
                        content: if cleaned.is_empty() {
                            None
                        } else {
                            Some(cleaned)
                        },
                    },
                    usage,
                });
            }

            // Guard against empty text after cleaning. This can happen when:
            // 1. Reasoning models (e.g. GLM-5) return chain-of-thought in
            //    reasoning_content wrapped in <think> tags — clean_response
            //    strips the think tags leaving an empty string.
            // 2. Local models (Qwen3, DeepSeek) emit <tool_call> XML in text
            //    responses even in force_text mode — strip_xml_tag discards
            //    from unclosed opening tag onward (issue #789).
            // Pre-truncate at tool tags to preserve text before the tag.
            let pre_truncated = truncate_at_tool_tags(&content);
            let cleaned = clean_response(&pre_truncated);
            let final_text = if cleaned.trim().is_empty() {
                tracing::warn!(
                    "LLM response was empty after cleaning (original len={}), using fallback",
                    content.len()
                );
                "I'm not sure how to respond to that.".to_string()
            } else {
                cleaned
            };
            Ok(RespondOutput {
                result: RespondResult::Text(final_text),
                usage,
            })
        } else {
            // No tools, use simple completion
            let mut request = CompletionRequest::new(messages)
                .with_max_tokens(4096)
                .with_temperature(0.7);
            request.metadata = context.metadata.clone();

            let response = self.llm.complete(request).await?;
            let pre_truncated = truncate_at_tool_tags(&response.content);
            let cleaned = clean_response(&pre_truncated);
            let final_text = if cleaned.trim().is_empty() {
                tracing::warn!(
                    "LLM response was empty after cleaning (original len={}), using fallback",
                    response.content.len()
                );
                "I'm not sure how to respond to that.".to_string()
            } else {
                cleaned
            };
            Ok(RespondOutput {
                result: RespondResult::Text(final_text),
                usage: TokenUsage {
                    input_tokens: response.input_tokens,
                    output_tokens: response.output_tokens,
                    cache_read_input_tokens: response.cache_read_input_tokens,
                    cache_creation_input_tokens: response.cache_creation_input_tokens,
                },
            })
        }
    }
}

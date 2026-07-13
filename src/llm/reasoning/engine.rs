//! Construction and configuration of the `Reasoning` engine, plus the
//! cleaned single-shot completion entry point.

use std::sync::Arc;

use crate::llm::error::LlmError;
use crate::llm::{CompletionRequest, LlmProvider};

use super::{Reasoning, TokenUsage, clean_response, truncate_at_tool_tags};

impl Reasoning {
    /// Create a new reasoning engine.
    pub fn new(llm: Arc<dyn LlmProvider>) -> Self {
        Self {
            llm,
            workspace_system_prompt: None,
            skill_context: None,
            channel: None,
            model_name: None,
            is_group_chat: false,
            conversation_context: std::collections::HashMap::new(),
        }
    }

    /// Set a custom system prompt from workspace identity files.
    ///
    /// This is typically loaded from workspace.system_prompt() which combines
    /// AGENTS.md, SOUL.md, USER.md, and IDENTITY.md into a unified prompt.
    pub fn with_system_prompt(mut self, prompt: String) -> Self {
        if !prompt.is_empty() {
            self.workspace_system_prompt = Some(prompt);
        }
        self
    }

    /// Set skill context to inject into the system prompt.
    ///
    /// The context block contains sanitized prompt content from active skills,
    /// wrapped in `<skill>` delimiters with trust metadata.
    pub fn with_skill_context(mut self, context: String) -> Self {
        if !context.is_empty() {
            self.skill_context = Some(context);
        }
        self
    }

    /// Set the channel name for channel-specific formatting hints.
    pub fn with_channel(mut self, channel: impl Into<String>) -> Self {
        let ch = channel.into();
        if !ch.is_empty() {
            self.channel = Some(ch);
        }
        self
    }

    /// Set the model name for runtime context.
    pub fn with_model_name(mut self, name: impl Into<String>) -> Self {
        let n = name.into();
        if !n.is_empty() {
            self.model_name = Some(n);
        }
        self
    }

    /// Mark this as a group chat context, enabling group-specific guidance.
    pub fn with_group_chat(mut self, is_group: bool) -> Self {
        self.is_group_chat = is_group;
        self
    }

    /// Add channel-specific conversation data for the system prompt.
    ///
    /// This provides the LLM with context about who/group it's talking to.
    /// Examples:
    ///   - Signal: sender, sender_uuid, target (group ID if in group)
    ///   - Discord: guild_id, channel_id, user_id
    ///   - Telegram: chat_id, user_id
    pub fn with_conversation_data(
        mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.conversation_context.insert(key.into(), value.into());
        self
    }

    /// Run a simple LLM completion with automatic response cleaning.
    ///
    /// This is the preferred entry point for code paths that call the LLM
    /// outside the agentic loop (e.g. `/summarize`, `/suggest`, heartbeat,
    /// compaction). It ensures `clean_response` is always applied so
    /// reasoning tags never leak to users or get stored in the workspace.
    pub async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<(String, TokenUsage), LlmError> {
        let response = self.llm.complete(request).await?;
        let usage = TokenUsage {
            input_tokens: response.input_tokens,
            output_tokens: response.output_tokens,
            cache_read_input_tokens: response.cache_read_input_tokens,
            cache_creation_input_tokens: response.cache_creation_input_tokens,
        };
        let pre_truncated = truncate_at_tool_tags(&response.content);
        Ok((clean_response(&pre_truncated), usage))
    }
}

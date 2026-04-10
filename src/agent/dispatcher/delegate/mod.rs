//! Chat delegate implementation for the agentic loop.
//!
//! Contains the `ChatDelegate` struct and its implementation of `NativeLoopDelegate`,
//! which customizes the shared agentic loop for interactive chat sessions.
//!
//! This module is split into child submodules by responsibility:
//! - `llm_hooks`: LLM call hooks and helper functions
//! - `tool_exec`: Tool execution logic and helpers

mod llm_hooks;
mod tool_exec;

use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::Agent;
use crate::agent::agentic_loop::{LoopOutcome, LoopSignal, NativeLoopDelegate, TextAction};
use crate::agent::session::Session;
use crate::channels::IncomingMessage;
use crate::context::JobContext;
use crate::error::Error;
use crate::llm::{Reasoning, ReasoningContext};

// Re-export items used by other modules in the crate.
#[cfg(test)]
pub(crate) use llm_hooks::{compact_messages_for_retry, strip_internal_tool_call_text};
pub(crate) use tool_exec::{
    ToolCallSpec, check_auth_required, execute_chat_tool_standalone, parse_auth_result,
};

/// Delegate for the chat (dispatcher) context.
///
/// Implements `LoopDelegate` to customize the shared agentic loop for
/// interactive chat sessions with the full 3-phase tool execution
/// (preflight → parallel exec → post-flight), approval flow, hooks,
/// auth intercept, and cost tracking.
pub(super) struct ChatDelegate<'a> {
    pub(super) agent: &'a Agent,
    pub(super) session: Arc<Mutex<Session>>,
    pub(super) thread_id: Uuid,
    pub(super) message: &'a IncomingMessage,
    pub(super) job_ctx: JobContext,
    pub(super) active_skills: Vec<crate::skills::LoadedSkill>,
    pub(super) cached_prompt: String,
    pub(super) cached_prompt_no_tools: String,
    pub(super) nudge_at: usize,
    pub(super) force_text_at: usize,
    pub(super) user_tz: chrono_tz::Tz,
}

impl<'a> NativeLoopDelegate for ChatDelegate<'a> {
    async fn check_signals(&self) -> LoopSignal {
        llm_hooks::check_signals(self).await
    }

    async fn before_llm_call(
        &self,
        reason_ctx: &mut ReasoningContext,
        iteration: usize,
    ) -> Option<LoopOutcome> {
        llm_hooks::before_llm_call(self, reason_ctx, iteration).await
    }

    async fn call_llm(
        &self,
        reasoning: &Reasoning,
        reason_ctx: &mut ReasoningContext,
        iteration: usize,
    ) -> Result<crate::llm::RespondOutput, Error> {
        llm_hooks::call_llm(self, reasoning, reason_ctx, iteration).await
    }

    async fn handle_text_response(
        &self,
        text: &str,
        _reason_ctx: &mut ReasoningContext,
    ) -> TextAction {
        llm_hooks::handle_text_response(self, text).await
    }

    async fn execute_tool_calls(
        &self,
        tool_calls: Vec<crate::llm::ToolCall>,
        content: Option<String>,
        reason_ctx: &mut ReasoningContext,
    ) -> Result<Option<LoopOutcome>, Error> {
        tool_exec::execute_tool_calls(self, tool_calls, content, reason_ctx).await
    }
}

mod loops;

mod preflight;

//! Chat delegate implementation for the agentic loop.

mod execution;

//! Chat delegate implementation for the agentic loop.

mod execution;

mod status;

mod recording;

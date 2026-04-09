//! Delegate layer split into phases: preflight (hooks/approval), execution
//! (inline/parallel), recording (context/thread), status (SSE/image
//! sentinels), and loop control (nudge/force-text).

use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::Agent;
use crate::agent::session::Session;
use crate::channels::IncomingMessage;
use crate::context::JobContext;

/// Chat dispatcher delegate used by the agentic loop (internal).
///
/// Responsibilities (per iteration):
/// - Refresh the active system prompt and available tools.
/// - Call the LLM with tool definitions and handle context-length retries.
/// - Preflight tool calls (hooks + approval), then execute runnable calls
///   inline or in parallel, preserving original order during post-flight.
/// - Record tool outcomes in the thread, emit statuses, detect auth/image
///   sentinels, and fold `tool_result` messages back into the reasoning context.
///
/// Notes:
/// - This type is crate-internal and not a public API surface.
/// - Status-send failures are swallowed by design (non-blocking UI updates).
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

impl<'a> ChatDelegate<'a> {
    /// Create a new ChatDelegate.
    #[allow(clippy::too_many_arguments)]
    #[allow(dead_code)]
    pub(super) fn new(
        agent: &'a Agent,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
        message: &'a IncomingMessage,
        job_ctx: JobContext,
        active_skills: Vec<crate::skills::LoadedSkill>,
        cached_prompt: String,
        cached_prompt_no_tools: String,
        nudge_at: usize,
        force_text_at: usize,
        user_tz: chrono_tz::Tz,
    ) -> Self {
        Self {
            agent,
            session,
            thread_id,
            message,
            job_ctx,
            active_skills,
            cached_prompt,
            cached_prompt_no_tools,
            nudge_at,
            force_text_at,
            user_tz,
        }
    }
}

mod loops;

pub(in crate::agent::dispatcher) mod preflight;

mod execution;

mod status;

mod recording;

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

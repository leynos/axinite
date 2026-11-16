//! Agentic-loop orchestration for interactive chat turns.
//!
//! Owns the per-run loop context, chat-delegate construction, loop threshold
//! computation, and the mapping from shared-engine outcomes to dispatcher
//! errors.

use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::Agent;
use crate::agent::session::Session;
use crate::channels::IncomingMessage;
use crate::context::JobContext;
use crate::error::Error;
use crate::llm::{ChatMessage, ToolDefinition};
use crate::skills::LoadedSkill;

use super::delegate::ChatDelegate;

/// Per-run context passed to the dispatcher’s agentic loop.
/// Carries shared session state, the active `thread_id`, and the turn’s
/// initial chat messages as prepared by the caller of `run_agentic_loop`.
pub(crate) struct RunLoopCtx {
    /// Shared handle to the live session state for this run.
    ///
    /// The session is guarded by a Tokio `Mutex`, so callers should clone the
    /// `Arc` when handing it across async boundaries rather than moving the
    /// underlying `Session`. All mutation must happen through the mutex guard.
    pub session: Arc<Mutex<Session>>,
    /// Stable identifier for the thread being processed by this run loop.
    ///
    /// `Uuid` has owned copy semantics, so callers may copy or clone this
    /// value freely when they need to correlate work across components.
    pub thread_id: Uuid,
    /// Initial chat history moved into the run loop for this invocation.
    ///
    /// Ownership of the message vector transfers into `RunLoopCtx`, and the
    /// loop consumes it as the starting prompt state for the agent.
    pub initial_messages: Vec<ChatMessage>,
}

#[derive(Debug)]
struct LoopCtxSpec {
    initial_messages: Vec<ChatMessage>,
    initial_tool_defs: Vec<ToolDefinition>,
    cached_prompt: String,
    thread_id: Uuid,
    max_tool_iterations: usize,
}

struct CachedPrompts {
    with_tools: String,
    no_tools: String,
}

struct ChatDelegateParams<'a> {
    message: &'a IncomingMessage,
    session: Arc<Mutex<Session>>,
    thread_id: Uuid,
    active_skills: Vec<LoadedSkill>,
    prompts: CachedPrompts,
    user_tz: chrono_tz::Tz,
}

/// Iteration thresholds that steer the loop away from tool-call livelocks.
/// - `nudge_at`: inject a gentle hint to prefer text before forcing text.
/// - `force_text_at`: disable tools and force a text answer.
/// - `hard_ceiling`: a safety net that guarantees termination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LoopThresholds {
    /// Iteration at which the dispatcher injects the pre-force nudge.
    ///
    /// This is always `effective_max_tool_iterations.saturating_sub(1)`.
    pub(crate) nudge_at: usize,
    /// Iteration at which the dispatcher forces a text response.
    ///
    /// This is always `effective_max_tool_iterations`.
    pub(crate) force_text_at: usize,
    /// Hard stop after the forced-text iteration has had one chance to run.
    ///
    /// This is always `effective_max_tool_iterations.saturating_add(1)`.
    pub(crate) hard_ceiling: usize,
}

/// Compute loop-termination thresholds from `max_tool_iterations`.
/// Guarantees: `0 < nudge_at ≤ force_text_at < hard_ceiling`.
pub(crate) fn compute_loop_thresholds(max_tool_iterations: usize) -> LoopThresholds {
    let max_tool_iterations = max_tool_iterations.max(1);
    LoopThresholds {
        nudge_at: max_tool_iterations.saturating_sub(1),
        force_text_at: max_tool_iterations,
        hard_ceiling: max_tool_iterations.saturating_add(1),
    }
}

/// Result of the agentic loop execution.
pub(crate) enum AgenticLoopResult {
    /// Completed with a response.
    Response(String),
    /// A tool requires approval before continuing.
    NeedApproval {
        /// The pending approval request to store.
        pending: crate::agent::session::PendingApproval,
    },
}

impl Agent {
    fn build_chat_delegate<'a>(&'a self, params: ChatDelegateParams<'a>) -> ChatDelegate<'a> {
        let ChatDelegateParams {
            message,
            session,
            thread_id,
            active_skills,
            prompts,
            user_tz,
        } = params;

        let mut job_ctx =
            JobContext::with_user(&message.user_id, "chat", "Interactive chat session");
        job_ctx.http_interceptor = self.deps.http_interceptor.clone();
        job_ctx.user_timezone = user_tz.name().to_string();

        let thresholds = compute_loop_thresholds(self.config.max_tool_iterations);

        ChatDelegate {
            agent: self,
            session,
            thread_id,
            message,
            job_ctx,
            active_skills,
            cached_prompt: prompts.with_tools,
            cached_prompt_no_tools: prompts.no_tools,
            nudge_at: thresholds.nudge_at,
            force_text_at: thresholds.force_text_at,
            user_tz,
        }
    }

    fn build_loop_context(
        &self,
        spec: LoopCtxSpec,
    ) -> (
        crate::llm::ReasoningContext,
        crate::agent::agentic_loop::AgenticLoopConfig,
    ) {
        let reason_ctx = crate::llm::ReasoningContext::new()
            .with_messages(spec.initial_messages)
            .with_tools(spec.initial_tool_defs)
            .with_system_prompt(spec.cached_prompt)
            .with_metadata({
                let mut metadata = std::collections::HashMap::new();
                metadata.insert("thread_id".to_string(), spec.thread_id.to_string());
                metadata
            });

        let thresholds = compute_loop_thresholds(spec.max_tool_iterations);
        let loop_config = crate::agent::agentic_loop::AgenticLoopConfig {
            max_iterations: thresholds.hard_ceiling,
            enable_tool_intent_nudge: true,
            max_tool_intent_nudges: 2,
        };

        (reason_ctx, loop_config)
    }

    /// Run the agentic loop: call LLM, execute tools, repeat until text response.
    ///
    /// Returns `Ok(AgenticLoopResult::Response)` on completion, or
    /// `Ok(AgenticLoopResult::NeedApproval)` if a tool requires user approval.
    ///
    /// Returns `Err` for any error produced by the underlying agentic loop,
    /// forwarding tool, provider, and other loop errors unchanged.
    ///
    /// This wrapper performs two specific outcome-to-error mappings:
    /// - `LoopOutcome::Stopped` (thread interrupted) → `JobError::ContextError`.
    ///   The dispatcher treats interruption as an error because it represents an
    ///   externally forced stop mid-processing, in contrast to the worker module
    ///   where a graceful exit is `Ok(WorkerLoopOutcome::Exited)`.
    /// - `LoopOutcome::MaxIterations` → `LlmError::InvalidResponse`.
    pub(crate) async fn run_agentic_loop(
        &self,
        message: &IncomingMessage,
        ctx: RunLoopCtx,
    ) -> Result<AgenticLoopResult, Error> {
        let RunLoopCtx {
            session,
            thread_id,
            initial_messages,
        } = ctx;
        let (reasoning, active_skills, user_tz) = self.prepare_reasoning(message).await;

        // Build system prompts once for this turn. Two variants: with tools
        // (normal iterations) and without (force_text final iteration).
        let initial_tool_defs = self.tools().tool_definitions().await;
        let initial_tool_defs =
            crate::skills::attenuate_tools(&initial_tool_defs, &active_skills).tools;
        let cached_prompt = reasoning.build_system_prompt_with_tools(&initial_tool_defs);
        let cached_prompt_no_tools = reasoning.build_system_prompt_with_tools(&[]);

        let max_tool_iterations = self.config.max_tool_iterations;
        let delegate = self.build_chat_delegate(ChatDelegateParams {
            message,
            session: session.clone(),
            thread_id,
            active_skills,
            prompts: CachedPrompts {
                with_tools: cached_prompt.clone(),
                no_tools: cached_prompt_no_tools,
            },
            user_tz,
        });
        let (mut reason_ctx, loop_config) = self.build_loop_context(LoopCtxSpec {
            initial_messages,
            initial_tool_defs,
            cached_prompt,
            thread_id,
            max_tool_iterations,
        });

        let outcome = crate::agent::agentic_loop::run_agentic_loop(
            &delegate,
            &reasoning,
            &mut reason_ctx,
            &loop_config,
        )
        .await?;

        match outcome {
            crate::agent::agentic_loop::LoopOutcome::Response(text) => {
                Ok(AgenticLoopResult::Response(text))
            }
            // Stopped = thread was interrupted mid-processing.  The dispatcher
            // maps this to an error because interruption represents an externally
            // forced stop, unlike the worker where a graceful exit is
            // `Ok(WorkerLoopOutcome::Exited)`.
            crate::agent::agentic_loop::LoopOutcome::Stopped => {
                Err(crate::error::JobError::ContextError {
                    id: thread_id,
                    reason: "Interrupted".to_string(),
                }
                .into())
            }
            crate::agent::agentic_loop::LoopOutcome::MaxIterations => {
                Err(crate::error::LlmError::InvalidResponse {
                    provider: "agent".to_string(),
                    reason: format!("Exceeded maximum tool iterations ({max_tool_iterations})"),
                }
                .into())
            }
            crate::agent::agentic_loop::LoopOutcome::NeedApproval(pending) => {
                Ok(AgenticLoopResult::NeedApproval { pending: *pending })
            }
        }
    }
}

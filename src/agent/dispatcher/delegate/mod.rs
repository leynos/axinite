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

mod loops;

pub(in crate::agent::dispatcher) mod preflight;

mod execution;

mod status;

mod recording;

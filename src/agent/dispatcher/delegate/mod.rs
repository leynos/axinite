//! Delegate layer split into loop control, approval helpers, and the active
//! tool-execution pipeline.

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

#[cfg(test)]
pub(in crate::agent::dispatcher) mod preflight;

mod tool_exec;

#[cfg(test)]
impl<'a> ChatDelegate<'a> {
    pub(in crate::agent::dispatcher) async fn maybe_emit_image_sentinel(
        &self,
        tool_name: &str,
        output: &str,
    ) -> bool {
        if !matches!(tool_name, "image_generate" | "image_edit") {
            return false;
        }

        let Ok(sentinel) = serde_json::from_str::<serde_json::Value>(output) else {
            return false;
        };
        if sentinel.get("type").and_then(|value| value.as_str()) != Some("image_generated") {
            return false;
        }

        let raw_data_url = sentinel.get("data").and_then(|value| value.as_str());
        let data_url = raw_data_url
            .filter(|value| value.starts_with("data:image/"))
            .map(ToString::to_string);
        let path = sentinel
            .get("path")
            .and_then(|value| value.as_str())
            .map(ToString::to_string);

        if let Some(data_url) = data_url {
            let _ = self
                .agent
                .channels
                .send_status(
                    &self.message.channel,
                    crate::channels::StatusUpdate::ImageGenerated { data_url, path },
                    &self.message.metadata,
                )
                .await;
        } else {
            tracing::warn!(
                has_data = raw_data_url.is_some(),
                "Image generation sentinel has invalid or empty data URL, skipping broadcast"
            );
        }

        true
    }
}

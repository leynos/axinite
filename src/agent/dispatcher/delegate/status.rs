//! Status emission helpers for `ChatDelegate`: tool lifecycle events, image
//! sentinels, and output sanitisation.

use crate::channels::StatusUpdate;
use crate::error::Error;

use super::ChatDelegate;

impl<'a> ChatDelegate<'a> {
    /// Send ToolStarted status update.
    pub(super) async fn send_tool_started(&self, tool_name: &str) {
        let _ = self
            .agent
            .channels
            .send_status(
                &self.message.channel,
                StatusUpdate::ToolStarted {
                    name: tool_name.to_string(),
                },
                &self.message.metadata,
            )
            .await;
    }

    /// Send tool_completed status update.
    pub(super) async fn send_tool_completed(
        &self,
        tool_name: &str,
        result: &Result<String, Error>,
        arguments: &serde_json::Value,
    ) {
        let disp_tool = self.agent.tools().get(tool_name).await;
        let _ = self
            .agent
            .channels
            .send_status(
                &self.message.channel,
                StatusUpdate::tool_completed(
                    tool_name.to_string(),
                    result,
                    arguments,
                    disp_tool.as_deref(),
                ),
                &self.message.metadata,
            )
            .await;
    }

    /// Sanitize tool output and return both preview text (raw sanitized) and
    /// wrapped text (for LLM).
    pub(super) fn sanitize_output(&self, tool_name: &str, output: &str) -> (String, String) {
        let sanitized = self.agent.safety().sanitize_tool_output(tool_name, output);
        let preview_text = sanitized.content.clone();
        let wrapped_text =
            self.agent
                .safety()
                .wrap_for_llm(tool_name, &sanitized.content, sanitized.was_modified);
        (preview_text, wrapped_text)
    }

    /// Emit image sentinel status update if applicable.
    pub(in crate::agent::dispatcher) async fn maybe_emit_image_sentinel(
        &self,
        tool_name: &str,
        output: &str,
    ) -> bool {
        if !matches!(tool_name, "image_generate" | "image_edit") {
            return false;
        }

        if let Ok(sentinel) = serde_json::from_str::<serde_json::Value>(output)
            && sentinel.get("type").and_then(|v| v.as_str()) == Some("image_generated")
        {
            let data_url = sentinel
                .get("data")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let path = sentinel
                .get("path")
                .and_then(|v| v.as_str())
                .map(String::from);
            if data_url.is_empty() {
                tracing::warn!("Image generation sentinel has empty data URL, skipping broadcast");
            } else {
                let _ = self
                    .agent
                    .channels
                    .send_status(
                        &self.message.channel,
                        StatusUpdate::ImageGenerated { data_url, path },
                        &self.message.metadata,
                    )
                    .await;
            }
            return true;
        }
        false
    }
}

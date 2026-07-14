//! Memory write tool for persisting workspace memory.
//!
//! Handles writes to curated memory, daily logs, the heartbeat checklist,
//! the bootstrap ritual file, and arbitrary workspace paths, with identity
//! files protected from tool writes.

use std::sync::Arc;

use crate::context::JobContext;
use crate::tools::tool::{NativeTool, ToolError, ToolOutput, require_str};
use crate::workspace::{Workspace, paths};

use super::PROTECTED_IDENTITY_FILES;

/// Tool for writing to workspace memory.
///
/// Use this to persist important information that should be remembered
/// across sessions: decisions, preferences, facts, lessons learned.
pub struct MemoryWriteTool {
    workspace: Arc<Workspace>,
}

impl MemoryWriteTool {
    /// Create a new memory write tool.
    pub fn new(workspace: Arc<Workspace>) -> Self {
        Self { workspace }
    }

    /// Clear BOOTSTRAP.md so the first-run ritual does not repeat.
    ///
    /// Writes empty content to effectively disable the bootstrap injection;
    /// `system_prompt_for_context()` skips empty files.
    async fn clear_bootstrap(&self, start: std::time::Instant) -> Result<ToolOutput, ToolError> {
        self.workspace
            .write(paths::BOOTSTRAP, "")
            .await
            .map_err(write_failed)?;

        let output = serde_json::json!({
            "status": "cleared",
            "path": paths::BOOTSTRAP,
            "message": "BOOTSTRAP.md cleared. First-run ritual will not repeat.",
        });

        Ok(ToolOutput::success(output, start.elapsed()))
    }

    /// Append to or replace the content at a workspace path.
    async fn write_or_append(
        &self,
        path: &str,
        content: &str,
        append: bool,
    ) -> Result<(), ToolError> {
        if append {
            self.workspace
                .append(path, content)
                .await
                .map_err(write_failed)?;
        } else {
            self.workspace
                .write(path, content)
                .await
                .map_err(write_failed)?;
        }
        Ok(())
    }

    /// Write to curated long-term memory (MEMORY.md), returning the path.
    async fn write_memory(&self, content: &str, append: bool) -> Result<String, ToolError> {
        if append {
            self.workspace
                .append_memory(content)
                .await
                .map_err(write_failed)?;
        } else {
            self.workspace
                .write(paths::MEMORY, content)
                .await
                .map_err(write_failed)?;
        }
        Ok(paths::MEMORY.to_string())
    }

    /// Append a timestamped entry to today's daily log, returning the path.
    async fn write_daily_log(&self, content: &str, ctx: &JobContext) -> Result<String, ToolError> {
        let tz = crate::timezone::parse_timezone(&ctx.user_timezone).unwrap_or(chrono_tz::Tz::UTC);
        self.workspace
            .append_daily_log_tz(content, tz)
            .await
            .map_err(write_failed)
    }

    /// Write to an arbitrary workspace path, rejecting protected identity
    /// files, and return the path.
    async fn write_custom_path(
        &self,
        path: &str,
        content: &str,
        append: bool,
    ) -> Result<String, ToolError> {
        // Protect identity files from LLM overwrites (prompt injection defence).
        // These files are injected into the system prompt, so poisoning them
        // would let an attacker rewrite the agent's core instructions.
        let normalized = path.trim_start_matches('/');
        if PROTECTED_IDENTITY_FILES
            .iter()
            .any(|p| normalized.eq_ignore_ascii_case(p))
        {
            return Err(ToolError::NotAuthorized(format!(
                "writing to '{}' is not allowed (identity file protected from tool access)",
                path
            )));
        }

        self.write_or_append(path, content, append).await?;
        Ok(path.to_string())
    }
}

/// Map a workspace failure into a tool execution error.
fn write_failed(e: impl std::fmt::Display) -> ToolError {
    ToolError::ExecutionFailed(format!("Write failed: {}", e))
}

impl NativeTool for MemoryWriteTool {
    fn name(&self) -> &str {
        "memory_write"
    }

    fn description(&self) -> &str {
        "Write to persistent memory (database-backed, NOT the local filesystem). \
         Use for important facts, decisions, preferences, or lessons learned that should \
         be remembered across sessions. Targets: 'memory' for curated long-term facts, \
         'daily_log' for timestamped session notes, 'heartbeat' for the periodic \
         checklist (HEARTBEAT.md), 'bootstrap' to clear the first-run ritual file, \
         or provide a custom path for arbitrary file creation."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "The content to write to memory. Be concise but include relevant context."
                },
                "target": {
                    "type": "string",
                    "description": "Where to write: 'memory' for MEMORY.md, 'daily_log' for today's log, 'heartbeat' for HEARTBEAT.md checklist, 'bootstrap' to clear BOOTSTRAP.md (content is ignored; the file is always cleared), or a path like 'projects/alpha/notes.md'",
                    "default": "daily_log"
                },
                "append": {
                    "type": "boolean",
                    "description": "If true, append to existing content. If false, replace entirely.",
                    "default": true
                }
            },
            "required": ["content"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();

        let content = require_str(&params, "content")?;

        let target = params
            .get("target")
            .and_then(|v| v.as_str())
            .unwrap_or("daily_log");

        // Bootstrap target: clear BOOTSTRAP.md to mark first-run ritual complete.
        // Handled early because it accepts empty content (unlike other targets).
        if target == "bootstrap" {
            return self.clear_bootstrap(start).await;
        }

        if content.trim().is_empty() {
            return Err(ToolError::InvalidParameters(
                "content cannot be empty".to_string(),
            ));
        }

        // Reject writes to identity files that are loaded into the system prompt.
        // An attacker could use prompt injection to trick the agent into overwriting
        // these, poisoning future conversations.
        if PROTECTED_IDENTITY_FILES.contains(&target) {
            return Err(ToolError::NotAuthorized(format!(
                "writing to '{}' is not allowed (identity file protected from tool writes)",
                target,
            )));
        }

        let append = params
            .get("append")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let path = match target {
            "memory" => self.write_memory(content, append).await?,
            "daily_log" => self.write_daily_log(content, ctx).await?,
            "heartbeat" => {
                self.write_or_append(paths::HEARTBEAT, content, append)
                    .await?;
                paths::HEARTBEAT.to_string()
            }
            path => self.write_custom_path(path, content, append).await?,
        };

        let output = serde_json::json!({
            "status": "written",
            "path": path,
            "append": append,
            "content_length": content.len(),
        });

        Ok(ToolOutput::success(output, start.elapsed()))
    }

    fn requires_sanitization(&self) -> bool {
        false // Internal tool
    }

    fn rate_limit_config(&self) -> Option<crate::tools::tool::ToolRateLimitConfig> {
        Some(crate::tools::tool::ToolRateLimitConfig::new(20, 200))
    }
}

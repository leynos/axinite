//! System prompt assembly from workspace identity documents.

use chrono::Utc;

use crate::error::WorkspaceError;

use super::{Workspace, paths};

impl Workspace {
    /// Build the system prompt from identity files.
    ///
    /// Loads AGENTS.md, SOUL.md, USER.md, IDENTITY.md, and (in non-group
    /// contexts) MEMORY.md to compose the agent's system prompt.
    ///
    /// Shorthand for `system_prompt_for_context(false)`.
    pub async fn system_prompt(&self) -> Result<String, WorkspaceError> {
        self.system_prompt_for_context(false).await
    }

    /// Build the system prompt with timezone-aware daily log dates.
    ///
    /// Uses the given timezone to determine "today" and "yesterday" for daily log injection.
    pub async fn system_prompt_for_context_tz(
        &self,
        is_group_chat: bool,
        tz: chrono_tz::Tz,
    ) -> Result<String, WorkspaceError> {
        self.system_prompt_for_context_inner(is_group_chat, Some(tz))
            .await
    }

    /// Build the system prompt, optionally excluding personal memory.
    ///
    /// When `is_group_chat` is true, MEMORY.md is excluded to prevent
    /// leaking personal context into group conversations.
    pub async fn system_prompt_for_context(
        &self,
        is_group_chat: bool,
    ) -> Result<String, WorkspaceError> {
        self.system_prompt_for_context_inner(is_group_chat, None)
            .await
    }

    /// Inner implementation for system prompt building.
    async fn system_prompt_for_context_inner(
        &self,
        is_group_chat: bool,
        tz: Option<chrono_tz::Tz>,
    ) -> Result<String, WorkspaceError> {
        let mut parts = Vec::new();

        // Bootstrap ritual: inject FIRST when present (first-run only).
        // The agent must complete the ritual and then delete this file.
        //
        // Note: BOOTSTRAP.md is intentionally NOT write-protected so the agent
        // can delete it after onboarding. This means a prompt injection attack
        // could write to it, but the file is only injected on the next session
        // (not the current one), limiting the blast radius.
        if let Ok(doc) = self.read(paths::BOOTSTRAP).await
            && !doc.content.is_empty()
        {
            parts.push(format!(
                "## First-Run Bootstrap\n\n\
                 A BOOTSTRAP.md file exists in the workspace. Read and follow it, \
                 then delete it when done.\n\n{}",
                doc.content
            ));
        }

        // Load identity files in order of importance
        let identity_files = [
            (paths::AGENTS, "## Agent Instructions"),
            (paths::SOUL, "## Core Values"),
            (paths::USER, "## User Context"),
            (paths::IDENTITY, "## Identity"),
        ];

        for (path, header) in identity_files {
            if let Ok(doc) = self.read(path).await
                && !doc.content.is_empty()
            {
                parts.push(format!("{}\n\n{}", header, doc.content));
            }
        }

        // Tool notes: environment-specific guidance the agent or user has written.
        // TOOLS.md does not control tool availability; it is guidance only.
        if let Ok(doc) = self.read(paths::TOOLS).await
            && !doc.content.is_empty()
        {
            parts.push(format!("## Tool Notes\n\n{}", doc.content));
        }

        // Load MEMORY.md only in direct/main sessions (never group chats)
        let memory_doc = if is_group_chat {
            None
        } else {
            self.read(paths::MEMORY).await.ok()
        };
        if let Some(doc) = memory_doc
            && !doc.content.is_empty()
        {
            parts.push(format!("## Long-Term Memory\n\n{}", doc.content));
        }

        // Add today's memory context (last 2 days of daily logs)
        let today = match tz {
            Some(t) => crate::timezone::today_in_tz(t),
            None => Utc::now().date_naive(),
        };
        let yesterday = today.pred_opt().unwrap_or(today);

        for date in [today, yesterday] {
            if let Ok(doc) = self.daily_log(date).await
                && !doc.content.is_empty()
            {
                let header = if date == today {
                    "## Today's Notes"
                } else {
                    "## Yesterday's Notes"
                };
                parts.push(format!("{}\n\n{}", header, doc.content));
            }
        }

        Ok(parts.join("\n\n---\n\n"))
    }
}

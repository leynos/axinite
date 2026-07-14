//! Workspace seeding: default identity templates, first-boot seeding,
//! and importing template files from disk.

use crate::error::WorkspaceError;

use super::{Workspace, paths};

/// Default template seeded into HEARTBEAT.md on first access.
///
/// Intentionally comment-only so the heartbeat runner treats it as
/// "effectively empty" and skips the LLM call until the user adds
/// real tasks.
pub(super) const HEARTBEAT_SEED: &str = "\
# Heartbeat Checklist

<!-- Keep this file empty to skip heartbeat API calls.
     Add tasks below when you want the agent to check something periodically.

     Rotate through these checks 2-4 times per day:
     - [ ] Check for urgent messages
     - [ ] Review upcoming calendar events
     - [ ] Check project status or CI builds

     Stay quiet during 23:00-08:00 user-local time unless urgent.
     If nothing needs attention, reply HEARTBEAT_OK.

     Proactive work you can do without asking:
     - Organize and curate MEMORY.md (remove stale, consolidate dupes)
     - Update daily logs with session summaries
     - Clean up context/ documents that are outdated
-->";

/// Default template seeded into TOOLS.md on first access.
///
/// TOOLS.md does not control tool availability; it is user guidance
/// for how to use external tools. The agent may update this file as it
/// learns environment-specific details (SSH hostnames, device names, etc.).
const TOOLS_SEED: &str = "\
<!-- TOOLS.md — Environment-specific tool notes.
     This file does not control which tools are available; it is guidance only.
     The agent can update this file as it learns your setup.

     Examples:
     - SSH hosts: dev-box (Ubuntu 22.04, username: alice)
     - Camera: Canon R6 mounted at /Volumes/EOS_R
     - Default shell on remote: bash, no zsh

     Add your environment notes below (outside the comment block).
-->";

/// First-run ritual seeded into BOOTSTRAP.md on initial workspace setup.
///
/// The agent reads this file at the start of every session when it exists.
/// After completing the ritual the agent must delete this file so it is
/// never repeated. It is NOT a protected file; the agent needs write access.
const BOOTSTRAP_SEED: &str = "\
# Bootstrap

You are starting up for the first time. Follow these steps before anything else.

## Steps

1. **Say hello.** Greet the user warmly and introduce yourself briefly.
2. **Get to know the user.** Ask a few questions to understand who they are, \
what they work on, and what they want from an AI assistant. Take notes.
3. **Save what you learned.**
   - Write any environment-specific tool details the user mentions to `TOOLS.md` \
using `memory_write` with target set to the path.
   - Write a summary of the conversation and key facts to `MEMORY.md` \
using `memory_write` with target `memory`.
   - Note: `USER.md`, `IDENTITY.md`, `SOUL.md`, and `AGENTS.md` are protected \
from tool writes for security. Tell the user what you'd suggest for those files \
so they can edit them directly.
4. **Delete this file.** When onboarding is complete, use `memory_write` with \
target `bootstrap` to clear this file so setup never repeats.

Keep the conversation natural. Do not read these steps aloud.
";

impl Workspace {
    /// Seed any missing core identity files in the workspace.
    ///
    /// Called on every boot. Only creates files that don't already exist,
    /// so user edits are never overwritten. Returns the number of files
    /// created (0 if all core files already existed).
    pub async fn seed_if_empty(&self) -> Result<usize, WorkspaceError> {
        let seed_files: &[(&str, &str)] = &[
            (
                paths::README,
                "# Workspace\n\n\
                 This is your agent's persistent memory. Files here are indexed for search\n\
                 and used to build the agent's context.\n\n\
                 ## Structure\n\n\
                 - `MEMORY.md` - Long-term curated notes (loaded into system prompt)\n\
                 - `IDENTITY.md` - Agent name, vibe, personality\n\
                 - `SOUL.md` - Core values and behavioral boundaries\n\
                 - `AGENTS.md` - Session routine and operational instructions\n\
                 - `USER.md` - Information about you (the user)\n\
                 - `TOOLS.md` - Environment-specific tool notes\n\
                 - `HEARTBEAT.md` - Periodic background task checklist\n\
                 - `daily/` - Automatic daily session logs\n\
                 - `context/` - Additional context documents\n\n\
                 Edit these files to shape how your agent thinks and acts.\n\
                 The agent reads them at the start of every session.",
            ),
            (
                paths::MEMORY,
                "# Memory\n\n\
                 Long-term notes, decisions, and facts worth remembering across sessions.\n\n\
                 The agent appends here during conversations. Curate periodically:\n\
                 remove stale entries, consolidate duplicates, keep it concise.\n\
                 This file is loaded into the system prompt, so brevity matters.",
            ),
            (
                paths::IDENTITY,
                "# Identity\n\n\
                 - **Name:** (pick one during your first conversation)\n\
                 - **Vibe:** (how you come across, e.g. calm, witty, direct)\n\
                 - **Emoji:** (your signature emoji, optional)\n\n\
                 Edit this file to give the agent a custom name and personality.\n\
                 The agent will evolve this over time as it develops a voice.",
            ),
            (
                paths::SOUL,
                "# Core Values\n\n\
                 Be genuinely helpful, not performatively helpful. Skip filler phrases.\n\
                 Have opinions. Disagree when it matters.\n\
                 Be resourceful before asking: read the file, check context, search, then ask.\n\
                 Earn trust through competence. Be careful with external actions, bold with internal ones.\n\
                 You have access to someone's life. Treat it with respect.\n\n\
                 ## Boundaries\n\n\
                 - Private things stay private. Never leak user context into group chats.\n\
                 - When in doubt about an external action, ask before acting.\n\
                 - Prefer reversible actions over destructive ones.\n\
                 - You are not the user's voice in group settings.",
            ),
            (
                paths::AGENTS,
                "# Agent Instructions\n\n\
                 You are a personal AI assistant with access to tools and persistent memory.\n\n\
                 ## Every Session\n\n\
                 1. Read SOUL.md (who you are)\n\
                 2. Read USER.md (who you're helping)\n\
                 3. Read today's daily log for recent context\n\n\
                 ## Memory\n\n\
                 You wake up fresh each session. Workspace files are your continuity.\n\
                 - Daily logs (`daily/YYYY-MM-DD.md`): raw session notes\n\
                 - `MEMORY.md`: curated long-term knowledge\n\
                 Write things down. Mental notes do not survive restarts.\n\n\
                 ## Guidelines\n\n\
                 - Always search memory before answering questions about prior conversations\n\
                 - Write important facts and decisions to memory for future reference\n\
                 - Use the daily log for session-level notes\n\
                 - Be concise but thorough\n\n\
                 ## Safety\n\n\
                 - Do not exfiltrate private data\n\
                 - Prefer reversible actions over destructive ones\n\
                 - When in doubt, ask",
            ),
            (
                paths::USER,
                "# User Context\n\n\
                 - **Name:**\n\
                 - **Timezone:**\n\
                 - **Preferences:**\n\n\
                 The agent will fill this in as it learns about you.\n\
                 You can also edit this directly to provide context upfront.",
            ),
            (paths::HEARTBEAT, HEARTBEAT_SEED),
            (paths::TOOLS, TOOLS_SEED),
        ];

        let mut count = 0;
        for (path, content) in seed_files {
            // Skip files that already exist (never overwrite user edits)
            match self.read(path).await {
                Ok(_) => continue,
                Err(WorkspaceError::DocumentNotFound { .. }) => {}
                Err(e) => {
                    tracing::debug!("Failed to check {}: {}", path, e);
                    continue;
                }
            }

            if let Err(e) = self.write(path, content).await {
                tracing::debug!("Failed to seed {}: {}", path, e);
            } else {
                count += 1;
            }
        }

        // BOOTSTRAP.md is only seeded on truly fresh workspaces (no identity
        // files exist yet). This prevents existing users from getting a
        // spurious first-run ritual after upgrading.
        if self.read(paths::BOOTSTRAP).await.is_err() {
            let (agents_res, soul_res, user_res) = tokio::join!(
                self.read(paths::AGENTS),
                self.read(paths::SOUL),
                self.read(paths::USER),
            );
            let is_fresh_workspace =
                matches!(agents_res, Err(WorkspaceError::DocumentNotFound { .. }))
                    && matches!(soul_res, Err(WorkspaceError::DocumentNotFound { .. }))
                    && matches!(user_res, Err(WorkspaceError::DocumentNotFound { .. }));

            if is_fresh_workspace {
                if let Err(e) = self.write(paths::BOOTSTRAP, BOOTSTRAP_SEED).await {
                    tracing::warn!("Failed to seed {}: {}", paths::BOOTSTRAP, e);
                } else {
                    count += 1;
                }
            }
        }

        if count > 0 {
            tracing::info!("Seeded {} workspace files", count);
        }
        Ok(count)
    }

    /// Import markdown files from a directory on disk into the workspace DB.
    ///
    /// Scans `dir` for `*.md` files (non-recursive) and writes each one into
    /// the workspace **only if it doesn't already exist in the database**.
    /// This allows Docker images or deployment scripts to ship customized
    /// workspace templates that override the generic seeds.
    ///
    /// Returns the number of files imported (0 if all already existed).
    pub async fn import_from_directory(
        &self,
        dir: &std::path::Path,
    ) -> Result<usize, WorkspaceError> {
        if !dir.is_dir() {
            tracing::warn!(
                "Workspace import directory does not exist: {}",
                dir.display()
            );
            return Ok(0);
        }

        let entries = ambient_fs::read_dir(dir).map_err(|e| WorkspaceError::IoError {
            reason: format!("failed to read directory {}: {}", dir.display(), e),
        })?;

        let mut count = 0;
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("Failed to read directory entry in {}: {}", dir.display(), e);
                    continue;
                }
            };

            let path = entry.path();
            // Only import .md files
            if path.extension() != Some(std::ffi::OsStr::new("md")) {
                continue;
            }

            let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };

            // Skip if already exists in DB (never overwrite user edits)
            match self.read(file_name).await {
                Ok(_) => continue,
                Err(WorkspaceError::DocumentNotFound { .. }) => {}
                Err(e) => {
                    tracing::trace!("Failed to check {}: {}", file_name, e);
                    continue;
                }
            }

            let content = match ambient_fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("Failed to read import file {}: {}", path.display(), e);
                    continue;
                }
            };

            if content.trim().is_empty() {
                continue;
            }

            if let Err(e) = self.write(file_name, &content).await {
                tracing::warn!("Failed to import {}: {}", file_name, e);
            } else {
                tracing::info!("Imported workspace file from disk: {}", file_name);
                count += 1;
            }
        }

        if count > 0 {
            tracing::info!(
                "Imported {} workspace file(s) from {}",
                count,
                dir.display()
            );
        }
        Ok(count)
    }
}

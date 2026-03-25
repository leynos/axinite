//! OpenClaw data migration orchestration and detection.

pub mod credentials;
pub mod history;
pub mod memory;
pub mod reader;
pub mod settings;

use std::path::PathBuf;
use std::sync::Arc;

use crate::db::Database;
use crate::import::{ImportError, ImportOptions, ImportStats};
use crate::secrets::SecretsStore;
use crate::workspace::Workspace;

pub use reader::OpenClawReader;

/// OpenClaw importer that coordinates migration of all data types.
pub struct OpenClawImporter {
    db: Arc<dyn Database>,
    workspace: Workspace,
    secrets: Arc<dyn SecretsStore>,
    opts: ImportOptions,
}

impl OpenClawImporter {
    /// Create a new OpenClaw importer.
    pub fn new(
        db: Arc<dyn Database>,
        workspace: Workspace,
        secrets: Arc<dyn SecretsStore>,
        opts: ImportOptions,
    ) -> Self {
        Self {
            db,
            workspace,
            secrets,
            opts,
        }
    }

    /// Detect if an OpenClaw installation exists at the default location (~/.openclaw).
    pub fn detect() -> Option<PathBuf> {
        if let Ok(home) = std::env::var("HOME") {
            let openclaw_dir = PathBuf::from(home).join(".openclaw");
            let config_file = openclaw_dir.join("openclaw.json");
            if config_file.exists() {
                return Some(openclaw_dir);
            }
        }
        None
    }

    /// Persist settings to the database (Group 1: idempotent via upsert).
    async fn persist_settings(&self, settings_map: std::collections::HashMap<String, serde_json::Value>) -> usize {
        let mut count = 0;
        for (key, value) in settings_map {
            if let Err(e) = self
                .db
                .set_setting(
                    self.opts.user_id.as_str().into(),
                    key.as_str().into(),
                    &value,
                )
                .await
            {
                tracing::warn!("Failed to import setting {}: {}", key, e);
            } else {
                count += 1;
            }
        }
        count
    }

    /// Persist credentials to secrets store (Group 2: idempotent via upsert).
    async fn persist_credentials(&self, creds: Vec<(String, secrecy::SecretString)>) -> usize {
        let mut count = 0;
        for (name, value) in creds {
            use secrecy::ExposeSecret;
            let exposed = value.expose_secret().to_string();
            let params = crate::secrets::CreateSecretParams::new(name, exposed);
            if let Err(e) = self.secrets.create(&self.opts.user_id, params).await {
                tracing::warn!("Failed to import credential: {}", e);
            } else {
                count += 1;
            }
        }
        count
    }

    /// Persist workspace documents (Group 3).
    async fn persist_workspace(&self, reader: &OpenClawReader) -> usize {
        if reader.list_workspace_files().is_ok() {
            match self
                .workspace
                .import_from_directory(&self.opts.openclaw_path.join("workspace"))
                .await
            {
                Ok(imported) => imported,
                Err(e) => {
                    tracing::warn!("Failed to import workspace documents: {}", e);
                    0
                }
            }
        } else {
            0
        }
    }

    /// Persist memory chunks (Group 4: idempotent via path deduplication).
    async fn persist_chunks(&self, all_chunks: Vec<reader::OpenClawMemoryChunk>) -> usize {
        let mut count = 0;
        for chunk in all_chunks {
            if let Err(e) = memory::import_chunk(&self.db, &chunk, &self.opts).await {
                tracing::warn!("Failed to import memory chunk: {}", e);
            } else {
                count += 1;
            }
        }
        count
    }

    /// Persist conversations with messages (Group 5: atomic per conversation).
    ///
    /// Each conversation + its messages form an atomic unit. If a crash occurs
    /// mid-conversation, only that conversation is incomplete. All previous
    /// conversations are fully committed.
    async fn persist_conversations(
        &self,
        all_conversations: Vec<reader::OpenClawConversation>,
    ) -> (usize, usize) {
        let mut conversation_count = 0;
        let mut message_count = 0;
        for conv in all_conversations {
            match history::import_conversation_atomic(&self.db, conv, &self.opts).await {
                Ok((_conv_id, msg_count)) => {
                    conversation_count += 1;
                    message_count += msg_count;
                }
                Err(e) => {
                    tracing::warn!("Failed to import conversation: {}", e);
                }
            }
        }
        (conversation_count, message_count)
    }

    /// Run the import process for all data types.
    ///
    /// Returns detailed statistics about what was imported.
    /// If `dry_run` is enabled, no data is written to the database.
    ///
    /// **Database Safety Note:** The Database trait does not currently expose explicit
    /// transaction control (BEGIN/COMMIT/ROLLBACK). To minimize consistency risks:
    /// - All configuration reading is done before any writes
    /// - Writes are grouped by type (settings, credentials, documents, chunks, conversations)
    /// - Conversations are handled atomically: creation + all messages added together
    /// - Errors are logged but don't stop the entire import (fail-safe behavior)
    pub async fn import(&self) -> Result<ImportStats, ImportError> {
        let mut stats = ImportStats::default();

        // === PHASE 1: READ ALL DATA BEFORE ANY WRITES ===
        // This minimizes the window where the database could be left in a partial state

        let reader = OpenClawReader::new(&self.opts.openclaw_path)?;
        let config = reader.read_config()?;
        let agent_dbs = reader.list_agent_dbs()?;

        // Pre-read all conversation data to validate before writing
        let mut all_conversations = Vec::new();
        for (_agent_name, db_path) in &agent_dbs {
            match reader.read_conversations(db_path) {
                Ok(convs) => all_conversations.extend(convs),
                Err(e) => tracing::warn!("Failed to read conversations: {}", e),
            }
        }

        // Pre-read all memory chunks
        let mut all_chunks = Vec::new();
        for (_agent_name, db_path) in &agent_dbs {
            match reader.read_memory_chunks(db_path) {
                Ok(chunks) => all_chunks.extend(chunks),
                Err(e) => tracing::warn!("Failed to read memory chunks: {}", e),
            }
        }

        // Prepare all settings and credentials
        let settings_map = settings::map_openclaw_config_to_settings(&config);
        let creds = settings::extract_credentials(&config);

        // === PHASE 2: WRITE IN GROUPED ORDER ===
        // If a crash occurs, earlier groups are fully committed

        if self.opts.dry_run {
            // DRY RUN: Count only
            stats.settings = settings_map.len();
            stats.secrets = creds.len();
            if let Ok(count) = reader.list_workspace_files() {
                stats.documents = count;
            }
            stats.chunks = all_chunks.len();
            stats.conversations = all_conversations.len();
            for conv in &all_conversations {
                stats.messages += conv.messages.len();
            }
        } else {
            stats.settings = self.persist_settings(settings_map).await;
            stats.secrets = self.persist_credentials(creds).await;
            stats.documents = self.persist_workspace(&reader).await;
            stats.chunks = self.persist_chunks(all_chunks).await;
            let (convs, msgs) = self.persist_conversations(all_conversations).await;
            stats.conversations = convs;
            stats.messages = msgs;
        }

        Ok(stats)
    }
}

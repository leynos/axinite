//! Runtime side effects for deferred background work.
//!
//! This module encapsulates fire-and-forget background tasks that should be
//! started after component assembly completes. Separating these deferred
//! effects from pure construction allows tests to validate composition without
//! paying for unrelated I/O and background work.

use std::sync::Arc;

use crate::db::Database;
use crate::workspace::Workspace;

/// Encapsulates fire-and-forget background tasks that should be started
/// after component assembly completes.
///
/// Separating these deferred effects from pure construction allows tests
/// to validate composition without paying for unrelated I/O and background work.
pub struct RuntimeSideEffects {
    pub(crate) db: Option<Arc<dyn Database>>,
    pub(crate) workspace: Option<Arc<Workspace>>,
    pub(crate) workspace_import_dir: Option<std::path::PathBuf>,
    pub(crate) embeddings_available: bool,
}

impl RuntimeSideEffects {
    /// Start all deferred background work.
    ///
    /// This method runs workspace import and seeding synchronously before
    /// returning, ensuring the workspace is fully initialised before the
    /// agent starts. Other work (sandbox cleanup, embedding backfill) is
    /// spawned as fire-and-forget background tasks.
    ///
    /// Callers awaiting this method will pay the import/seed cost; if fully
    /// deferred startup is required, wrap the call in `tokio::spawn`.
    pub async fn start(self) {
        // Spawn stale sandbox cleanup task
        if let Some(db) = self.db {
            let db_cleanup = Arc::clone(&db);
            tokio::spawn(async move {
                if let Err(e) = db_cleanup.cleanup_stale_sandbox_jobs().await {
                    tracing::warn!("Failed to cleanup stale sandbox jobs: {}", e);
                }
            });
        }

        // Workspace import, seeding, and embedding backfill
        if let Some(ws) = self.workspace {
            // Import workspace files from disk FIRST if workspace_import_dir flag is set.
            // This lets Docker images / deployment scripts ship customized
            // workspace templates (e.g., AGENTS.md, TOOLS.md) that override
            // the generic seeds. Only imports files that don't already exist
            // in the database — never overwrites user edits.
            //
            // Runs before seed_if_empty() so that custom templates take priority
            // over generic seeds. seed_if_empty() then fills any remaining gaps.
            if let Some(import_path) = self.workspace_import_dir {
                match ws.import_from_directory(&import_path).await {
                    Ok(count) if count > 0 => {
                        tracing::debug!(
                            "Imported {} workspace file(s) from {}",
                            count,
                            import_path.display()
                        );
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(
                            "Failed to import workspace files from {}: {}",
                            import_path.display(),
                            e
                        );
                    }
                }
            }

            match ws.seed_if_empty().await {
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("Failed to seed workspace: {}", e);
                }
            }

            if self.embeddings_available {
                let ws_bg = Arc::clone(&ws);
                tokio::spawn(async move {
                    match ws_bg.backfill_embeddings().await {
                        Ok(count) if count > 0 => {
                            tracing::debug!("Backfilled embeddings for {} chunks", count);
                        }
                        Ok(_) => {}
                        Err(e) => {
                            tracing::warn!("Failed to backfill embeddings: {}", e);
                        }
                    }
                });
            }
        }
    }
}

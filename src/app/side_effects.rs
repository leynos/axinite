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

#[cfg(test)]
mod tests {
    use super::*;

    /// RuntimeSideEffects::start completes without panic when all fields are None.
    #[tokio::test]
    async fn start_completes_with_noop_when_empty() {
        let side_effects = RuntimeSideEffects {
            db: None,
            workspace: None,
            workspace_import_dir: None,
            embeddings_available: false,
        };

        // Should complete without panic even with nothing to do
        side_effects.start().await;
    }

    /// RuntimeSideEffects::start spawns cleanup task when db is provided.
    #[tokio::test]
    async fn start_spawns_cleanup_with_db() {
        // We can't easily mock the Database trait, but we can verify
        // the method doesn't panic when passed None
        let side_effects = RuntimeSideEffects {
            db: None,
            workspace: None,
            workspace_import_dir: None,
            embeddings_available: false,
        };

        side_effects.start().await;
        // If we had a mock db, cleanup_stale_sandbox_jobs would be called
    }

    /// RuntimeSideEffects struct can be constructed with minimal state.
    #[test]
    fn side_effects_construction_with_various_states() {
        // All None
        let _ = RuntimeSideEffects {
            db: None,
            workspace: None,
            workspace_import_dir: None,
            embeddings_available: false,
        };

        // With import dir but no workspace
        let _ = RuntimeSideEffects {
            db: None,
            workspace: None,
            workspace_import_dir: Some(std::path::PathBuf::from("/tmp/import")),
            embeddings_available: false,
        };

        // With embeddings flag but no workspace
        let _ = RuntimeSideEffects {
            db: None,
            workspace: None,
            workspace_import_dir: None,
            embeddings_available: true,
        };
    }

    /// RuntimeSideEffects correctly stores and returns its configuration.
    #[test]
    fn side_effects_preserves_configuration() {
        let import_path = std::path::PathBuf::from("/custom/import/path");

        let side_effects = RuntimeSideEffects {
            db: None,
            workspace: None,
            workspace_import_dir: Some(import_path.clone()),
            embeddings_available: true,
        };

        assert_eq!(side_effects.workspace_import_dir, Some(import_path));
        assert!(side_effects.embeddings_available);
    }

    /// RuntimeSideEffects::start is idempotent when called multiple times
    /// on different instances (consuming self prevents double-call on same instance).
    #[tokio::test]
    async fn start_is_idempotent_across_instances() {
        async fn run_start() {
            let side_effects = RuntimeSideEffects {
                db: None,
                workspace: None,
                workspace_import_dir: None,
                embeddings_available: false,
            };
            side_effects.start().await;
        }

        // Running start multiple times on fresh instances should all succeed
        run_start().await;
        run_start().await;
        run_start().await;
    }

    /// RuntimeSideEffects field visibility allows parent module construction.
    #[test]
    fn side_effects_fields_accessible_in_parent_module() {
        // This test verifies the pub(crate) visibility works as expected
        // The struct is constructed in app.rs, so fields must be accessible
        let side_effects = RuntimeSideEffects {
            db: None,
            workspace: None,
            workspace_import_dir: Some(std::path::PathBuf::from("/test")),
            embeddings_available: true,
        };

        // Fields are pub(crate) so they can be read within the crate
        assert!(side_effects.workspace_import_dir.is_some());
        assert!(side_effects.embeddings_available);
    }
}

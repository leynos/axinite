//! Deferred runtime side effects activated after component construction.

use std::sync::Arc;

use crate::db::Database;
use crate::workspace::Workspace;
use anyhow::Context;

/// Deferred runtime side effects that should be started after component
/// construction is complete.
///
/// This struct encapsulates fire-and-forget background tasks (stale job cleanup,
/// workspace import/seeding, embedding backfill) that are activated separately
/// from pure construction. Following hexagonal architecture principles, this
/// separates assembly from activation.
pub struct RuntimeSideEffects {
    db: Option<Arc<dyn Database>>,
    workspace: Option<Arc<Workspace>>,
    workspace_import_dir: Option<std::path::PathBuf>,
    embeddings_available: bool,
}

/// Join handles for deferred runtime side effects.
pub struct RuntimeSideEffectsHandle {
    workspace_bootstrap: Option<tokio::task::JoinHandle<()>>,
    /// Intentionally detached cleanup work.
    ///
    /// The leading underscore marks that stale job cleanup is fire-and-forget:
    /// `wait_until_bootstrapped()` only awaits `workspace_bootstrap`, because
    /// callers need a fully initialized workspace before continuing but do not
    /// need to block on background cleanup.
    _cleanup: Option<tokio::task::JoinHandle<()>>,
}

impl RuntimeSideEffectsHandle {
    /// Wait until workspace bootstrap work has finished.
    pub async fn wait_until_bootstrapped(self) -> Result<(), anyhow::Error> {
        if let Some(handle) = self.workspace_bootstrap {
            handle
                .await
                .map_err(|e| anyhow::anyhow!("Workspace bootstrap task failed: {}", e))?;
        }
        Ok(())
    }
}

impl RuntimeSideEffects {
    /// Create a new `RuntimeSideEffects` instance.
    pub fn new(
        db: Option<Arc<dyn Database>>,
        workspace: Option<Arc<Workspace>>,
        workspace_import_dir: Option<std::path::PathBuf>,
        embeddings_available: bool,
    ) -> Self {
        Self {
            db,
            workspace,
            workspace_import_dir,
            embeddings_available,
        }
    }

    /// Start all deferred runtime side effects.
    ///
    /// This method spawns background tasks and returns their handles. Callers
    /// can drop the returned value to detach the work, or await
    /// `wait_until_bootstrapped()` when ordering guarantees are required.
    ///
    /// Side effects include:
    /// - Stale sandbox job cleanup (via database)
    /// - Workspace import from disk (if `WORKSPACE_IMPORT_DIR` is set)
    /// - Workspace seeding (if workspace is empty)
    /// - Embedding backfill (spawns a background task)
    pub fn start(self) -> Result<RuntimeSideEffectsHandle, anyhow::Error> {
        // Spawn stale sandbox cleanup task if database is available.
        let cleanup = match self.db {
            Some(db) => Some(current_runtime()?.spawn(cleanup_stale_jobs(db))),
            None => None,
        };

        // Spawn workspace import, seeding, and embedding backfill if workspace is available.
        let workspace_bootstrap = match self.workspace {
            Some(ws) => Some(current_runtime()?.spawn(bootstrap_workspace(
                ws,
                self.workspace_import_dir,
                self.embeddings_available,
            ))),
            None => None,
        };

        Ok(RuntimeSideEffectsHandle {
            workspace_bootstrap,
            _cleanup: cleanup,
        })
    }
}

/// Fetch the current Tokio runtime handle, with a start-specific error.
fn current_runtime() -> Result<tokio::runtime::Handle, anyhow::Error> {
    tokio::runtime::Handle::try_current()
        .context("RuntimeSideEffects::start() requires a Tokio runtime")
}

/// Remove sandbox jobs left in a running state by a previous process.
async fn cleanup_stale_jobs(db: Arc<dyn Database>) {
    if let Err(e) = db.cleanup_stale_sandbox_jobs().await {
        tracing::warn!("Failed to cleanup stale sandbox jobs: {}", e);
    }
}

/// Run the workspace bootstrap sequence: disk import, seeding, and (when
/// configured) embedding backfill.
async fn bootstrap_workspace(
    ws: Arc<Workspace>,
    import_dir: Option<std::path::PathBuf>,
    embeddings_available: bool,
) {
    // Import workspace files from disk FIRST if WORKSPACE_IMPORT_DIR is set.
    // This lets Docker images / deployment scripts ship customized workspace
    // templates that override generic seeds. Only imports files that don't
    // already exist in the database — never overwrites user edits.
    if let Some(dir) = import_dir {
        import_workspace_files(&ws, &dir).await;
    }

    // Seed workspace with default content if empty.
    if let Err(e) = ws.seed_if_empty().await {
        tracing::warn!("Failed to seed workspace: {}", e);
    }

    // Backfill embeddings in background if embeddings are configured.
    if embeddings_available {
        backfill_embeddings(&ws).await;
    }
}

/// Import workspace files from a directory, logging the outcome.
async fn import_workspace_files(ws: &Workspace, dir: &std::path::Path) {
    match ws.import_from_directory(dir).await {
        Ok(count) if count > 0 => {
            tracing::debug!(
                "Imported {} workspace file(s) from {}",
                count,
                dir.display()
            );
        }
        Ok(_) => {}
        Err(e) => {
            tracing::warn!(
                "Failed to import workspace files from {}: {}",
                dir.display(),
                e
            );
        }
    }
}

/// Backfill missing chunk embeddings, logging the outcome.
async fn backfill_embeddings(ws: &Workspace) {
    match ws.backfill_embeddings().await {
        Ok(count) if count > 0 => {
            tracing::debug!("Backfilled embeddings for {} chunks", count);
        }
        Ok(_) => {}
        Err(e) => {
            tracing::warn!("Failed to backfill embeddings: {}", e);
        }
    }
}

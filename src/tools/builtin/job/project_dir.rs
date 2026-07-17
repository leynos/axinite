//! Project directory resolution for sandbox jobs.
//!
//! Every sandbox job gets a persistent bind-mount directory under
//! `~/.axinite/projects/`. These helpers create, canonicalize, and validate
//! those directories, rejecting paths that escape the projects base.

use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::bootstrap::axinite_base_dir;
use crate::tools::tool::ToolError;

/// The base directory where all project directories must live.
pub(super) fn projects_base() -> PathBuf {
    axinite_base_dir().join("projects")
}

/// Resolve the project directory, creating it if it doesn't exist.
///
/// Auto-creates `~/.axinite/projects/{project_id}/` so every sandbox job has a
/// persistent bind mount that survives container teardown.
///
/// When an explicit path is provided (e.g. job restarts reusing the old dir),
/// it is validated to fall within `~/.axinite/projects/` after canonicalization.
pub(super) fn resolve_project_dir(
    explicit: Option<PathBuf>,
    project_id: Uuid,
) -> Result<(PathBuf, String), ToolError> {
    let base = projects_base();
    ambient_fs::create_dir_all(&base).map_err(|e| {
        ToolError::ExecutionFailed(format!(
            "failed to create projects base {}: {}",
            base.display(),
            e
        ))
    })?;
    let canonical_base = base.canonicalize().map_err(|e| {
        ToolError::ExecutionFailed(format!("failed to canonicalize projects base: {}", e))
    })?;

    let canonical_dir = match explicit {
        Some(d) => canonicalize_explicit_project_dir(d, &canonical_base)?,
        None => create_default_project_dir(&canonical_base, project_id)?,
    };

    let browse_id = canonical_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| project_id.to_string());
    Ok((canonical_dir, browse_id))
}

/// Validate a caller-supplied project directory against the canonical base.
///
/// Explicit paths are validated BEFORE creating anything: the path must
/// already exist (it comes from a previous job run) and must canonicalize
/// to somewhere under the projects base.
fn canonicalize_explicit_project_dir(
    dir: PathBuf,
    canonical_base: &Path,
) -> Result<PathBuf, ToolError> {
    let canonical = dir.canonicalize().map_err(|e| {
        ToolError::InvalidParameters(format!(
            "explicit project dir {} does not exist or is inaccessible: {}",
            dir.display(),
            e
        ))
    })?;
    if !canonical.starts_with(canonical_base) {
        return Err(ToolError::InvalidParameters(format!(
            "project directory must be under {}",
            canonical_base.display()
        )));
    }
    Ok(canonical)
}

/// Create and canonicalize the default project directory for a project id.
fn create_default_project_dir(
    canonical_base: &Path,
    project_id: Uuid,
) -> Result<PathBuf, ToolError> {
    let dir = canonical_base.join(project_id.to_string());
    ambient_fs::create_dir_all(&dir).map_err(|e| {
        ToolError::ExecutionFailed(format!(
            "failed to create project dir {}: {}",
            dir.display(),
            e
        ))
    })?;
    dir.canonicalize().map_err(|e| {
        ToolError::ExecutionFailed(format!(
            "failed to canonicalize project dir {}: {}",
            dir.display(),
            e
        ))
    })
}

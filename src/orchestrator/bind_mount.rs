//! Bind-mount path validation for sandboxed containers.

#[cfg(any(feature = "docker", test))]
use std::path::PathBuf;

#[cfg(any(feature = "docker", test))]
use crate::bootstrap::ironclaw_base_dir;
#[cfg(any(feature = "docker", test))]
use crate::error::OrchestratorError;

/// Validate that a project directory is under `~/.ironclaw/projects/`.
///
/// Returns the canonicalized path if valid. Creates the base directory if
/// it doesn't exist (so the prefix check always runs).
///
/// # TOCTOU note
///
/// There is a time-of-check/time-of-use gap between `canonicalize()` here
/// and the actual Docker `binds.push()` in the caller. In a multi-tenant
/// system a malicious actor could swap a symlink after validation. This is
/// acceptable in IronClaw's single-tenant design where the user controls
/// the filesystem.
#[cfg(any(feature = "docker", test))]
pub(crate) fn validate_bind_mount_path(
    dir: &std::path::Path,
    job_id: uuid::Uuid,
) -> Result<PathBuf, OrchestratorError> {
    let canonical = dir
        .canonicalize()
        .map_err(|e| OrchestratorError::ContainerCreationFailed {
            job_id,
            reason: format!(
                "failed to canonicalize project dir {}: {}",
                dir.display(),
                e
            ),
        })?;

    let projects_base = ironclaw_base_dir().join("projects");

    if !projects_base.is_absolute() {
        return Err(OrchestratorError::ContainerCreationFailed {
            job_id,
            reason: "base directory is not absolute; cannot safely validate bind mounts".into(),
        });
    }

    // Ensure the base exists so canonicalize always succeeds.
    std::fs::create_dir_all(&projects_base).map_err(|e| {
        OrchestratorError::ContainerCreationFailed {
            job_id,
            reason: format!(
                "failed to create projects base {}: {}",
                projects_base.display(),
                e
            ),
        }
    })?;

    let canonical_base =
        projects_base
            .canonicalize()
            .map_err(|e| OrchestratorError::ContainerCreationFailed {
                job_id,
                reason: format!(
                    "failed to canonicalize projects base {}: {}",
                    projects_base.display(),
                    e
                ),
            })?;

    if !canonical.starts_with(&canonical_base) {
        return Err(OrchestratorError::ContainerCreationFailed {
            job_id,
            reason: format!(
                "project directory {} is outside allowed base {}",
                canonical.display(),
                canonical_base.display()
            ),
        });
    }

    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use uuid::Uuid;

    use super::validate_bind_mount_path;

    #[test]
    fn test_validate_bind_mount_valid_path() {
        let base = crate::bootstrap::compute_ironclaw_base_dir().join("projects");
        std::fs::create_dir_all(&base).unwrap();

        let test_dir = base.join("test_validate_bind");
        std::fs::create_dir_all(&test_dir).unwrap();

        let result = validate_bind_mount_path(&test_dir, Uuid::new_v4());
        assert!(result.is_ok());
        let canonical = result.unwrap();
        assert!(canonical.starts_with(base.canonicalize().unwrap()));

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn test_validate_bind_mount_rejects_outside_base() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tmp.path().to_path_buf();

        let result = validate_bind_mount_path(&outside, Uuid::new_v4());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("outside allowed base"),
            "expected 'outside allowed base', got: {}",
            err
        );
    }

    #[test]
    fn test_validate_bind_mount_rejects_nonexistent() {
        let nonexistent = PathBuf::from("/no/such/path/at/all");
        let result = validate_bind_mount_path(&nonexistent, Uuid::new_v4());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("canonicalize"),
            "expected canonicalize error, got: {}",
            err
        );
    }
}

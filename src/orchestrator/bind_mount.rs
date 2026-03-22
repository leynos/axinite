//! Bind-mount path validation for sandboxed containers.

#[cfg(any(feature = "docker", test))]
use std::path::Path;
#[cfg(any(feature = "docker", test))]
use std::path::PathBuf;

#[cfg(feature = "docker")]
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
#[cfg(feature = "docker")]
pub(crate) async fn validate_bind_mount_path(
    dir: &std::path::Path,
    job_id: uuid::Uuid,
) -> Result<PathBuf, OrchestratorError> {
    validate_bind_mount_path_against_base(dir, &ironclaw_base_dir().join("projects"), job_id).await
}

#[cfg(any(feature = "docker", test))]
async fn validate_bind_mount_path_against_base(
    dir: &Path,
    projects_base: &Path,
    job_id: uuid::Uuid,
) -> Result<PathBuf, OrchestratorError> {
    let canonical = tokio::fs::canonicalize(dir).await.map_err(|e| {
        OrchestratorError::ContainerCreationFailed {
            job_id,
            reason: format!(
                "failed to canonicalize project dir {}: {}",
                dir.display(),
                e
            ),
        }
    })?;

    if !projects_base.is_absolute() {
        return Err(OrchestratorError::ContainerCreationFailed {
            job_id,
            reason: "base directory is not absolute; cannot safely validate bind mounts".into(),
        });
    }

    // Ensure the base exists so canonicalize always succeeds.
    tokio::fs::create_dir_all(projects_base)
        .await
        .map_err(|e| OrchestratorError::ContainerCreationFailed {
            job_id,
            reason: format!(
                "failed to create projects base {}: {}",
                projects_base.display(),
                e
            ),
        })?;

    let canonical_base = tokio::fs::canonicalize(projects_base).await.map_err(|e| {
        OrchestratorError::ContainerCreationFailed {
            job_id,
            reason: format!(
                "failed to canonicalize projects base {}: {}",
                projects_base.display(),
                e
            ),
        }
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
    use uuid::Uuid;

    use super::validate_bind_mount_path_against_base;

    #[tokio::test]
    async fn test_validate_bind_mount_valid_path() {
        let tmp = tempfile::tempdir().expect("failed to create tempdir for valid bind-mount test");
        let base = tmp.path().join("projects");
        std::fs::create_dir_all(&base).expect("failed to create projects base for valid test");
        let test_dir = base.join("test_validate_bind");
        std::fs::create_dir_all(&test_dir).expect("failed to create test project directory");

        let result = validate_bind_mount_path_against_base(&test_dir, &base, Uuid::new_v4()).await;
        assert!(result.is_ok());
        let canonical = result.expect("expected valid bind mount path inside temp base");
        assert!(canonical.starts_with(base.canonicalize().expect("failed to canonicalize base")));
    }

    #[tokio::test]
    async fn test_validate_bind_mount_rejects_outside_base() {
        let tmp =
            tempfile::tempdir().expect("failed to create tempdir for outside-base bind-mount test");
        let base = tmp.path().join("projects");
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&base).expect("failed to create projects base for outside test");
        std::fs::create_dir_all(&outside)
            .expect("failed to create outside directory for outside-base test");

        let result = validate_bind_mount_path_against_base(&outside, &base, Uuid::new_v4()).await;
        assert!(result.is_err());
        let err = result
            .expect_err("expected outside-base bind mount to be rejected")
            .to_string();
        assert!(
            err.contains("outside allowed base"),
            "expected 'outside allowed base', got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_validate_bind_mount_rejects_nonexistent() {
        let tmp =
            tempfile::tempdir().expect("failed to create tempdir for nonexistent bind-mount test");
        let base = tmp.path().join("projects");
        std::fs::create_dir_all(&base)
            .expect("failed to create projects base for nonexistent bind-mount test");
        let nonexistent = base.join("missing/project");
        let result =
            validate_bind_mount_path_against_base(&nonexistent, &base, Uuid::new_v4()).await;
        assert!(result.is_err());
        let err = result
            .expect_err("expected nonexistent bind mount to fail canonicalization")
            .to_string();
        assert!(
            err.contains("canonicalize"),
            "expected canonicalize error, got: {}",
            err
        );
    }
}

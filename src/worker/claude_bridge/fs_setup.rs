//! Filesystem and local setup helpers for the Claude bridge runtime.

use crate::error::WorkerError;

use super::ClaudeBridgeRuntime;

impl ClaudeBridgeRuntime {
    /// Write project-level `.claude/settings.json` with the tool allowlist.
    pub(super) async fn write_permission_settings(&self) -> Result<(), WorkerError> {
        let settings_json = build_permission_settings(&self.config.allowed_tools)?;
        tokio::task::spawn_blocking(move || {
            let settings_dir = std::path::Path::new("/workspace/.claude");
            std::fs::create_dir_all(settings_dir).map_err(|error| {
                WorkerError::ExecutionFailed {
                    reason: format!("failed to create /workspace/.claude/: {error}"),
                }
            })?;
            std::fs::write(settings_dir.join("settings.json"), settings_json).map_err(|error| {
                WorkerError::ExecutionFailed {
                    reason: format!("failed to write settings.json: {error}"),
                }
            })
        })
        .await
        .map_err(|error| WorkerError::ExecutionFailed {
            reason: format!("permission settings task failed: {error}"),
        })??;
        tracing::info!(
            job_id = %self.config.job_id,
            tools = ?self.config.allowed_tools,
            "Wrote Claude Code permission settings"
        );
        Ok(())
    }

    /// Copy auth files from a read-only source into the writable home dir.
    pub(super) async fn copy_auth_from_mount(&self) -> Result<(), WorkerError> {
        let mount = std::path::PathBuf::from("/home/sandbox/.claude-host");
        if !tokio::fs::try_exists(&mount)
            .await
            .map_err(|error| WorkerError::ExecutionFailed {
                reason: format!("failed to check ~/.claude-host: {error}"),
            })?
        {
            return Ok(());
        }

        let target = std::path::PathBuf::from("/home/sandbox/.claude");
        let copied = tokio::task::spawn_blocking(move || {
            std::fs::create_dir_all(&target).map_err(|error| WorkerError::ExecutionFailed {
                reason: format!("failed to create ~/.claude: {error}"),
            })?;

            copy_dir_recursive(&mount, &target).map_err(|error| WorkerError::ExecutionFailed {
                reason: format!("failed to copy auth from host mount: {error}"),
            })
        })
        .await
        .map_err(|error| WorkerError::ExecutionFailed {
            reason: format!("auth copy task failed: {error}"),
        })??;

        tracing::info!(
            job_id = %self.config.job_id,
            files_copied = copied,
            "Copied auth config from host mount into container"
        );
        Ok(())
    }
}

/// Build the JSON content for `.claude/settings.json` with the given tool allowlist.
pub(crate) fn build_permission_settings(allowed_tools: &[String]) -> Result<String, WorkerError> {
    let settings = serde_json::json!({
        "permissions": {
            "allow": allowed_tools,
        }
    });
    serde_json::to_string_pretty(&settings).map_err(|error| WorkerError::ExecutionFailed {
        reason: format!("failed to serialize Claude permission settings: {error}"),
    })
}

/// Recursively copy files and directories from `src` to `dst`, skipping
/// entries that can't be read.
pub(crate) fn copy_dir_recursive(
    src: &std::path::Path,
    dst: &std::path::Path,
) -> std::io::Result<usize> {
    let entries = match std::fs::read_dir(src) {
        Ok(entries) => entries,
        Err(error) => {
            tracing::debug!("Skipping unreadable directory {}: {}", src.display(), error);
            return Ok(0);
        }
    };

    let mut copied = 0;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                tracing::debug!("Skipping unreadable entry in {}: {}", src.display(), error);
                continue;
            }
        };

        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                tracing::debug!(
                    "Skipping entry with unreadable type {}: {}",
                    src_path.display(),
                    error
                );
                continue;
            }
        };

        if file_type.is_symlink() {
            tracing::debug!("Skipping symlink {}", src_path.display());
            continue;
        }

        if file_type.is_dir() {
            std::fs::create_dir_all(&dst_path)?;
            copied += copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            match std::fs::copy(&src_path, &dst_path) {
                Ok(_) => copied += 1,
                Err(error) => {
                    if error.kind() == std::io::ErrorKind::NotFound {
                        tracing::debug!(
                            "Skipping unreadable file {}: {}",
                            src_path.display(),
                            error
                        );
                    } else {
                        return Err(error);
                    }
                }
            }
        }
    }

    Ok(copied)
}

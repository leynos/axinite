//! Filesystem and local setup helpers for the Claude bridge runtime.

use std::{
    ffi::{OsStr, OsString},
    fs, io,
    path::{Path, PathBuf},
};

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

    pub(super) async fn has_copied_auth(&self) -> Result<bool, WorkerError> {
        match tokio::fs::read_dir("/home/sandbox/.claude").await {
            Ok(mut entries) => entries
                .next_entry()
                .await
                .map(|entry| entry.is_some())
                .map_err(|error| WorkerError::ExecutionFailed {
                    reason: format!("failed to inspect ~/.claude for copied auth: {error}"),
                }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(WorkerError::ExecutionFailed {
                reason: format!("failed to inspect ~/.claude for copied auth: {error}"),
            }),
        }
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

enum DirEntryKind {
    Dir { path: PathBuf, name: OsString },
    File { path: PathBuf, name: OsString },
    Symlink { path: PathBuf },
    Other { path: PathBuf },
}

fn scan_dir(src: &Path) -> io::Result<Vec<DirEntryKind>> {
    let mut entries = Vec::new();
    for entry_result in fs::read_dir(src)? {
        let entry = match entry_result {
            Ok(entry) => entry,
            Err(error) => {
                tracing::debug!(
                    "fs_setup: unreadable dir entry under {}: {error}",
                    src.display()
                );
                continue;
            }
        };
        let path = entry.path();
        let name = entry.file_name();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                tracing::debug!(
                    "fs_setup: file_type() failed for {}: {error}",
                    path.display()
                );
                continue;
            }
        };
        let kind = if file_type.is_dir() {
            DirEntryKind::Dir { path, name }
        } else if file_type.is_file() {
            DirEntryKind::File { path, name }
        } else if file_type.is_symlink() {
            DirEntryKind::Symlink { path }
        } else {
            DirEntryKind::Other { path }
        };
        entries.push(kind);
    }
    Ok(entries)
}

fn copy_one_file(src: &Path, dst: &Path) -> io::Result<usize> {
    match fs::copy(src, dst) {
        Ok(_) => Ok(1),
        Err(error) if error.kind() == io::ErrorKind::NotFound => match src.metadata() {
            Err(metadata_error) if metadata_error.kind() == io::ErrorKind::NotFound => {
                tracing::debug!(
                    "fs_setup: source disappeared {} → {}, skipping: {error}",
                    src.display(),
                    dst.display()
                );
                Ok(0)
            }
            _ => Err(error),
        },
        Err(error) => Err(error),
    }
}

fn copy_subdir(src: &Path, parent_dst: &Path, dir_name: &OsStr) -> io::Result<usize> {
    let sub_dst = parent_dst.join(dir_name);
    fs::create_dir_all(&sub_dst)?;
    copy_dir_recursive(src, &sub_dst)
}

/// Recursively copy files and directories from `src` to `dst`, skipping
/// entries that can't be read.
pub(crate) fn copy_dir_recursive(src: &Path, dst: &Path) -> io::Result<usize> {
    if !src.exists() {
        return Ok(0);
    }
    fs::create_dir_all(dst)?;
    let mut copied = 0usize;
    for entry in scan_dir(src)? {
        match entry {
            DirEntryKind::Dir { path, name } => copied += copy_subdir(&path, dst, &name)?,
            DirEntryKind::File { path, name } => {
                copied += copy_one_file(&path, &dst.join(&name))?;
            }
            DirEntryKind::Symlink { path } => {
                tracing::debug!("fs_setup: skipping symlink {}", path.display());
            }
            DirEntryKind::Other { path } => {
                tracing::debug!("fs_setup: unknown entry type at {}", path.display());
            }
        }
    }
    Ok(copied)
}

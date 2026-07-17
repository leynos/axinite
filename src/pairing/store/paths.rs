//! Filesystem path resolution for per-channel pairing state files.

use std::path::{Path, PathBuf};

use crate::bootstrap::axinite_base_dir;

use super::PairingStoreError;

pub(super) fn default_pairing_dir() -> PathBuf {
    axinite_base_dir()
}

pub(super) fn safe_channel_key(channel: &str) -> Result<String, PairingStoreError> {
    let raw = channel.trim().to_lowercase();
    if raw.is_empty() {
        return Err(PairingStoreError::InvalidChannel("empty".to_string()));
    }
    let safe = raw
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect::<String>()
        .replace("..", "_");
    if safe.is_empty() || safe == "_" {
        return Err(PairingStoreError::InvalidChannel(channel.to_string()));
    }
    Ok(safe)
}

pub(super) fn pairing_path(base_dir: &Path, channel: &str) -> Result<PathBuf, PairingStoreError> {
    let key = safe_channel_key(channel)?;
    Ok(base_dir.join(format!("{}-pairing.json", key)))
}

pub(super) fn allow_from_path(
    base_dir: &Path,
    channel: &str,
) -> Result<PathBuf, PairingStoreError> {
    let key = safe_channel_key(channel)?;
    Ok(base_dir.join(format!("{}-allowFrom.json", key)))
}

pub(super) fn approve_attempts_path(
    base_dir: &Path,
    channel: &str,
) -> Result<PathBuf, PairingStoreError> {
    let key = safe_channel_key(channel)?;
    Ok(base_dir.join(format!("{}-approve-attempts.json", key)))
}

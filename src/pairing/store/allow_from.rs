//! The allowFrom list: reading, membership checks, and additions.

use ambient_fs as fs;
use serde::{Deserialize, Serialize};

use super::paths::allow_from_path;
use super::{PairingStore, PairingStoreError};

#[derive(Debug, Serialize, Deserialize)]
struct AllowFromStoreFile {
    version: u8,
    #[serde(rename = "allowFrom")]
    allow_from: Vec<String>,
}

impl PairingStore {
    /// Read the allowFrom list for a channel.
    pub fn read_allow_from(&self, channel: &str) -> Result<Vec<String>, PairingStoreError> {
        let path = allow_from_path(&self.base_dir, channel)?;
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Vec::new());
            }
            Err(e) => return Err(e.into()),
        };

        let file: AllowFromStoreFile =
            serde_json::from_str(&content).unwrap_or(AllowFromStoreFile {
                version: 1,
                allow_from: Vec::new(),
            });

        Ok(file.allow_from)
    }

    /// Check if a sender is allowed (by id or username).
    pub fn is_sender_allowed(
        &self,
        channel: &str,
        id: &str,
        username: Option<&str>,
    ) -> Result<bool, PairingStoreError> {
        let allow = self.read_allow_from(channel)?;
        let id = id.trim();
        let id_ok = allow.iter().any(|e| e.trim() == id);
        if id_ok {
            return Ok(true);
        }
        if let Some(u) = username {
            let u = u.trim().to_lowercase();
            let u_norm = u.strip_prefix('@').unwrap_or(&u);
            if allow.iter().any(|e| {
                e.trim().to_lowercase() == u || e.trim().to_lowercase() == format!("@{}", u_norm)
            }) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) fn add_allow_from(
        &self,
        channel: &str,
        entry: &str,
    ) -> Result<(), PairingStoreError> {
        let entry = entry.trim().to_string();
        if entry.is_empty() {
            return Ok(());
        }

        let path = allow_from_path(&self.base_dir, channel)?;
        let parent = path.parent().ok_or_else(|| {
            PairingStoreError::InvalidPath(format!("path has no parent: {}", path.display()))
        })?;
        fs::create_dir_all(parent)?;

        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)?;

        file.lock_exclusive()?;

        let content = fs::read_to_string(&path).unwrap_or_default();
        let mut store: AllowFromStoreFile =
            serde_json::from_str(&content).unwrap_or(AllowFromStoreFile {
                version: 1,
                allow_from: Vec::new(),
            });

        let normalized = entry.to_lowercase();
        if store
            .allow_from
            .iter()
            .any(|e| e.to_lowercase() == normalized)
        {
            file.unlock()?;
            return Ok(());
        }

        store.allow_from.push(entry);
        let json = serde_json::to_string_pretty(&store)?;
        fs::write(&path, json)?;

        file.unlock()?;
        Ok(())
    }
}

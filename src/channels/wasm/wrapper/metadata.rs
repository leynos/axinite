//! Broadcast metadata persistence for WASM channels.

use std::sync::Arc;

use crate::db::{SettingKey, UserId};

use super::WasmChannel;

pub(super) async fn do_update_broadcast_metadata(
    channel_name: &str,
    metadata: &str,
    last_broadcast_metadata: &tokio::sync::RwLock<Option<String>>,
    settings_store: Option<&Arc<dyn crate::db::SettingsStore>>,
) {
    let mut guard = last_broadcast_metadata.write().await;
    let changed = guard.as_deref() != Some(metadata);
    *guard = Some(metadata.to_string());
    drop(guard);

    if changed && let Some(store) = settings_store {
        let key = format!("channel_broadcast_metadata_{}", channel_name);
        let value = serde_json::Value::String(metadata.to_string());
        if let Err(e) = store
            .set_setting(UserId::from("default"), SettingKey::from(key), &value)
            .await
        {
            tracing::warn!(
                channel = %channel_name,
                "Failed to persist broadcast metadata: {}",
                e
            );
        }
    }
}

impl WasmChannel {
    /// Settings key for persisted broadcast metadata.
    pub(super) fn broadcast_metadata_key(&self) -> String {
        format!("channel_broadcast_metadata_{}", self.name)
    }

    /// Update broadcast metadata in memory and persist if changed (best-effort).
    ///
    /// Compares with the current value to avoid redundant DB writes on every
    /// incoming message (the chat_id rarely changes).
    pub(super) async fn update_broadcast_metadata(&self, metadata: &str) {
        do_update_broadcast_metadata(
            &self.name,
            metadata,
            &self.last_broadcast_metadata,
            self.settings_store.as_ref(),
        )
        .await;
    }

    /// Load broadcast metadata from settings store on startup.
    pub(super) async fn load_broadcast_metadata(&self) {
        if let Some(ref store) = self.settings_store {
            match store
                .get_setting(
                    UserId::from("default"),
                    SettingKey::from(self.broadcast_metadata_key()),
                )
                .await
            {
                Ok(Some(serde_json::Value::String(meta))) => {
                    *self.last_broadcast_metadata.write().await = Some(meta);
                    tracing::debug!(
                        channel = %self.name,
                        "Restored broadcast metadata from settings"
                    );
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(
                        channel = %self.name,
                        "Failed to load broadcast metadata: {}",
                        e
                    );
                }
            }
        }
    }
}

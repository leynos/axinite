//! Pairing-store integration for the Signal channel: request upserts,
//! pairing-aware allowlist checks, and the async pairing reply sender.

use std::time::Duration;

use reqwest::Client;
use uuid::Uuid;

use crate::error::ChannelError;
use crate::pairing::PairingStore;

use super::{MAX_ERROR_LOG_BODY, SignalChannel};

impl SignalChannel {
    pub(super) fn pairing_store() -> PairingStore {
        #[cfg(test)]
        {
            if let Some(base_dir) = super::SIGNAL_PAIRING_STORE_OVERRIDE
                .get_or_init(|| std::sync::Mutex::new(None))
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
            {
                return PairingStore::with_base_dir(base_dir);
            }
        }

        PairingStore::new()
    }

    /// Check if sender is allowed via config allow_from OR pairing store.
    pub(super) fn is_sender_allowed_with_pairing(&self, sender: &str) -> bool {
        if self.is_sender_allowed(sender) {
            return true;
        }
        let store = Self::pairing_store();
        if let Ok(allowed) = store.read_allow_from("signal") {
            return allowed.iter().any(|entry| entry == "*" || entry == sender);
        }
        false
    }

    /// Handle pairing request for unapproved sender.
    /// Returns Ok(true) if message should be allowed (was already paired),
    /// Ok(false) if message was blocked but pairing request was processed.
    pub(super) fn handle_pairing_request(
        &self,
        sender: &str,
        source_name: Option<&str>,
    ) -> Result<bool, ()> {
        let store = Self::pairing_store();
        let meta = serde_json::json!({
            "sender": sender,
            "name": source_name,
        });

        match store.upsert_request("signal", sender, Some(meta)) {
            Ok(result) => {
                tracing::info!(
                    sender = %sender,
                    code = %result.code,
                    "Signal: pairing request upserted"
                );
                if result.created {
                    let message = format!(
                        "To pair with this bot, run: `ironclaw pairing approve signal {}`",
                        result.code
                    );
                    let http_url = self.config.http_url.clone();
                    let account = self.config.account.clone();
                    let sender_owned = sender.to_string();
                    let message_owned = message.clone();
                    tokio::spawn(async move {
                        if let Err(e) = Self::send_pairing_reply_async(
                            &http_url,
                            &account,
                            &sender_owned,
                            &message_owned,
                        )
                        .await
                        {
                            tracing::error!(sender = %sender_owned, error = %e, "Signal: failed to send pairing reply");
                        }
                    });
                }
                Ok(false)
            }
            Err(e) => {
                tracing::error!(sender = %sender, error = %e, "Signal: pairing upsert failed");
                Err(())
            }
        }
    }

    /// Send a pairing reply message to the sender (async helper for spawned task).
    async fn send_pairing_reply_async(
        http_url: &str,
        account: &str,
        recipient: &str,
        message: &str,
    ) -> Result<(), ChannelError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| ChannelError::Http(e.to_string()))?;

        let target = Self::parse_recipient_target(recipient);
        let params = Self::build_rpc_params_static(http_url, account, &target, Some(message), None);

        let url = format!("{}/api/v1/rpc", http_url);
        let id = Uuid::new_v4().to_string();

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "send",
            "params": params,
            "id": id,
        });

        let resp = client
            .post(&url)
            .timeout(Duration::from_secs(30))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ChannelError::SendFailed {
                name: "signal".to_string(),
                reason: format!("RPC request failed to {}: {e}", Self::redact_url(&url)),
            })?;

        let status = resp.status();
        let is_success = status.is_success();

        if status.as_u16() == 201 {
            return Ok(());
        }

        if !is_success {
            let bytes = resp.bytes().await.unwrap_or_default();
            let truncated_len = bytes.len().min(MAX_ERROR_LOG_BODY);
            let truncated_body = String::from_utf8_lossy(&bytes[..truncated_len]);
            return Err(ChannelError::SendFailed {
                name: "signal".to_string(),
                reason: format!("HTTP error {}: {}", status.as_u16(), truncated_body),
            });
        }

        Ok(())
    }
}

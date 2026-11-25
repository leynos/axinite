//! Activation after a setup submission.
//!
//! Activation is gated on setup readiness recomputed from the stores. A
//! channel activates only when every required declaration is satisfied, and a
//! tool keeps its OAuth grant unless the submission changed a credential.

use crate::extensions::ExtensionKind;

use super::ExtensionManager;
use super::setup_schema::SetupSnapshot;

impl ExtensionManager {
    /// Report readiness and run the kind's activation path when it is met.
    pub(super) async fn finish_setup(
        &self,
        name: &str,
        snapshot: &SetupSnapshot,
        wrote_secret: bool,
    ) -> (String, bool, Option<String>) {
        if !snapshot.ready() {
            return (incomplete_message(name, snapshot), false, None);
        }
        match self.determine_installed_kind(name).await {
            Ok(ExtensionKind::WasmChannel) => self.activate_channel_after_setup(name).await,
            Ok(ExtensionKind::WasmTool) => self.activate_tool_after_setup(name, wrote_secret).await,
            _ => (format!("Configuration saved for '{name}'."), false, None),
        }
    }

    async fn activate_channel_after_setup(&self, name: &str) -> (String, bool, Option<String>) {
        match self
            .wasm_channel_activation
            .activate_wasm_channel(name)
            .await
        {
            Ok(result) => {
                self.activation_errors.write().await.remove(name);
                self.broadcast_extension_status(name, "active", None).await;
                (
                    format!(
                        "Configuration saved and channel '{}' activated. {}",
                        name, result.message
                    ),
                    true,
                    None,
                )
            }
            Err(e) => {
                let error_msg = e.to_string();
                tracing::warn!(
                    channel = name,
                    error = %e,
                    "Saved configuration but hot-activation failed"
                );
                self.activation_errors
                    .write()
                    .await
                    .insert(name.to_string(), error_msg.clone());
                self.broadcast_extension_status(name, "failed", Some(&error_msg))
                    .await;
                (
                    format!(
                        "Configuration saved for '{}'. Activation failed: {}",
                        name, e
                    ),
                    false,
                    None,
                )
            }
        }
    }

    async fn activate_tool_after_setup(
        &self,
        name: &str,
        wrote_secret: bool,
    ) -> (String, bool, Option<String>) {
        match self.wasm_tool_activation.activate_wasm_tool(name).await {
            Ok(result) => {
                // Only a credential change invalidates an OAuth grant; an
                // endpoint-only edit keeps the existing token.
                if wrote_secret {
                    self.purge_tool_oauth_tokens(name).await;
                }
                let auth_url = match self.auth(name, None).await {
                    Ok(auth_result) => auth_result.auth_url().map(String::from),
                    Err(_) => None,
                };
                let message = if auth_url.is_some() {
                    format!(
                        "Configuration saved and tool '{}' activated. Complete OAuth in your browser.",
                        name
                    )
                } else {
                    format!(
                        "Configuration saved and tool '{}' activated. {}",
                        name, result.message
                    )
                };
                (message, true, auth_url)
            }
            Err(e) => {
                tracing::debug!(
                    "Auto-activation of tool '{}' after setup failed: {}",
                    name,
                    e
                );
                (format!("Configuration saved for '{}'.", name), false, None)
            }
        }
    }
}

/// Message naming every blocker by declaration, never by value.
fn incomplete_message(name: &str, snapshot: &SetupSnapshot) -> String {
    let blockers: Vec<String> = snapshot
        .blockers()
        .iter()
        .map(|blocker| format!("{} ({})", blocker.field(), blocker.reason().as_str()))
        .collect();
    format!(
        "Configuration saved for '{}', but setup is incomplete: {}",
        name,
        blockers.join(", ")
    )
}

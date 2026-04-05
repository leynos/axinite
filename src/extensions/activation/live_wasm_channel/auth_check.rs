//! Authentication readiness checks for live WASM channel activation.

use crate::extensions::ExtensionError;

use super::ToolAuthState;

impl super::LiveWasmChannelActivation {
    /// Check the authentication status of a WASM channel.
    pub(super) async fn check_channel_auth_status(
        &self,
        name: &str,
    ) -> Result<ToolAuthState, ExtensionError> {
        let cap_path = self
            .wasm_channels_dir
            .join(format!("{}.capabilities.json", name));
        let Ok(cap_bytes) = tokio::fs::read(&cap_path).await else {
            return Ok(ToolAuthState::NoAuth);
        };
        let Ok(cap_file) = crate::channels::wasm::ChannelCapabilitiesFile::from_bytes(&cap_bytes)
        else {
            return Ok(ToolAuthState::NoAuth);
        };

        let required: Vec<_> = cap_file
            .setup
            .required_secrets
            .iter()
            .filter(|s| !s.optional)
            .collect();
        if required.is_empty() {
            return Ok(ToolAuthState::NoAuth);
        }

        let results = futures::future::join_all(
            required
                .iter()
                .map(|s| self.secrets.exists(&self.user_id, &s.name)),
        )
        .await;

        let mut all_provided = true;
        for result in results {
            match result {
                Ok(true) => {}
                Ok(false) => {
                    all_provided = false;
                }
                Err(e) => {
                    return Err(ExtensionError::AuthFailed(format!(
                        "Failed to check channel auth status for '{}': {}",
                        name, e
                    )));
                }
            }
        }

        if all_provided {
            Ok(ToolAuthState::Ready)
        } else {
            Ok(ToolAuthState::NeedsSetup)
        }
    }
}

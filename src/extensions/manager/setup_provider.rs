//! Provider credential checks that run before any setup write.
//!
//! A rejected credential must never replace a working one, so these checks run
//! against the planned values before the first effect executes.

use crate::extensions::ExtensionError;
use crate::extensions::setup::PlannedEffect;

use super::ExtensionManager;

/// Credential name Telegram validates against its API.
const TELEGRAM_TOKEN_FIELD: &str = "telegram_bot_token";

impl ExtensionManager {
    /// Validate a submitted provider credential before any write.
    pub(super) async fn validate_provider_credentials(
        &self,
        name: &str,
        effects: &[PlannedEffect],
    ) -> Result<(), ExtensionError> {
        if name != "telegram" {
            return Ok(());
        }
        let Some(token) = submitted_token(effects) else {
            return Ok(());
        };
        let token = token.trim();
        if token.is_empty() {
            return Ok(());
        }
        validate_telegram_token(token).await
    }
}

/// The planned token value, when the submission replaces it.
fn submitted_token(effects: &[PlannedEffect]) -> Option<&str> {
    effects.iter().find_map(|effect| match effect {
        PlannedEffect::WriteSecret { name, value } if name.as_str() == TELEGRAM_TOKEN_FIELD => {
            Some(value.as_str())
        }
        _ => None,
    })
}

/// Ask the Telegram API whether the bot token is valid.
async fn validate_telegram_token(token: &str) -> Result<(), ExtensionError> {
    let encoded = url::form_urlencoded::byte_serialize(token.as_bytes()).collect::<String>();
    let url = format!("https://api.telegram.org/bot{encoded}/getMe");
    let resp = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ExtensionError::Other(e.to_string()))?
        .get(&url)
        .send()
        .await
        .map_err(|e| ExtensionError::Other(format!("Failed to validate bot token: {e}")))?;
    if resp.status().is_success() {
        return Ok(());
    }
    Err(ExtensionError::Other(format!(
        "Invalid bot token (Telegram API returned {})",
        resp.status()
    )))
}

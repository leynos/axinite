//! Telegram bot-token validation used while saving channel setup secrets.

use crate::extensions::ExtensionError;

/// For Telegram, validate a submitted bot token against the API before storing it.
///
/// This catches bad tokens immediately (both on first setup and reconfigure),
/// before the channel activates and potentially shows as active with a bad token.
pub(super) async fn validate_telegram_token_if_submitted(
    name: &str,
    secrets: &std::collections::HashMap<String, String>,
) -> Result<(), ExtensionError> {
    match secrets.get("telegram_bot_token") {
        Some(token_value) if name == "telegram" => {
            validate_telegram_bot_token(token_value.trim()).await
        }
        _ => Ok(()),
    }
}

/// Check a Telegram bot token against the API; an empty token is accepted unchecked.
async fn validate_telegram_bot_token(token: &str) -> Result<(), ExtensionError> {
    if token.is_empty() {
        return Ok(());
    }
    let encoded_token = url::form_urlencoded::byte_serialize(token.as_bytes()).collect::<String>();
    let url = format!("https://api.telegram.org/bot{}/getMe", encoded_token);
    let resp = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ExtensionError::Other(e.to_string()))?
        .get(&url)
        .send()
        .await
        .map_err(|e| ExtensionError::Other(format!("Failed to validate bot token: {}", e)))?;
    if !resp.status().is_success() {
        return Err(ExtensionError::Other(format!(
            "Invalid bot token (Telegram API returned {})",
            resp.status()
        )));
    }
    Ok(())
}

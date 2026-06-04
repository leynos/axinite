use crate::exports::near::agent::channel::OutgoingHttpResponse;
use crate::near::agent::channel_host;
use crate::types::TelegramApiResponse;

/// Delete any existing webhook with Telegram API.
///
/// Called during on_start() when switching to polling mode.
/// Telegram doesn't allow getUpdates while a webhook is active.
pub(crate) fn delete_webhook() -> Result<(), String> {
    let headers = serde_json::json!({
        "Content-Type": "application/json"
    });

    let result = channel_host::http_request(&channel_host::HttpRequestParams {
        method: "POST".to_string(),
        url: "https://api.telegram.org/bot{TELEGRAM_BOT_TOKEN}/deleteWebhook".to_string(),
        headers_json: headers.to_string(),
        body: None,
        timeout_ms: None,
    });

    match result {
        Ok(response) => {
            if response.status != 200 {
                let body_str = String::from_utf8_lossy(&response.body);
                return Err(format!("HTTP {}: {}", response.status, body_str));
            }

            let api_response: TelegramApiResponse<bool> = serde_json::from_slice(&response.body)
                .map_err(|e| format!("Failed to parse response: {}", e))?;

            if !api_response.ok {
                return Err(format!(
                    "Telegram API error: {}",
                    api_response
                        .description
                        .unwrap_or_else(|| "unknown".to_string())
                ));
            }

            channel_host::log(
                channel_host::LogLevel::Info,
                "Webhook deleted successfully (switching to polling mode)",
            );

            Ok(())
        }
        Err(e) => Err(format!("HTTP request failed: {}", e)),
    }
}

/// Register webhook URL with Telegram API.
///
/// Called during on_start() when tunnel_url is configured.
pub(crate) fn register_webhook(tunnel_url: &str, webhook_secret: Option<&str>) -> Result<(), String> {
    let webhook_url = format!("{}/webhook/telegram", tunnel_url);

    // Build setWebhook request body
    let mut body = serde_json::json!({
        "url": webhook_url,
        "allowed_updates": ["message", "edited_message"]
    });

    if let Some(secret) = webhook_secret {
        body["secret_token"] = serde_json::Value::String(secret.to_string());
    }

    let body_bytes =
        serde_json::to_vec(&body).map_err(|e| format!("Failed to serialize body: {}", e))?;

    let headers = serde_json::json!({
        "Content-Type": "application/json"
    });

    // Make HTTP request to Telegram API
    // Note: {TELEGRAM_BOT_TOKEN} is replaced by host with the actual token
    let result = channel_host::http_request(&channel_host::HttpRequestParams {
        method: "POST".to_string(),
        url: "https://api.telegram.org/bot{TELEGRAM_BOT_TOKEN}/setWebhook".to_string(),
        headers_json: headers.to_string(),
        body: Some(body_bytes.clone()),
        timeout_ms: None,
    });

    let mut response = match result {
        Ok(response) => response,
        Err(e) => return Err(format!("HTTP request failed: {}", e)),
    };

    let mut retried = false;
    if response.status == 409 {
        channel_host::log(
            channel_host::LogLevel::Warn,
            "409 Conflict -- deleting existing webhook and retrying",
        );
        let _ = delete_webhook();
        retried = true;

        response = match channel_host::http_request(&channel_host::HttpRequestParams {
            method: "POST".to_string(),
            url: "https://api.telegram.org/bot{TELEGRAM_BOT_TOKEN}/setWebhook".to_string(),
            headers_json: headers.to_string(),
            body: Some(body_bytes.clone()),
            timeout_ms: None,
        }) {
            Ok(resp) => resp,
            Err(e) => return Err(format!("HTTP request failed (after 409 retry): {}", e)),
        };
    }

    if response.status != 200 {
        let body_str = String::from_utf8_lossy(&response.body);
        let context = if retried { " (after 409 retry)" } else { "" };
        return Err(format!("HTTP {}{}: {}", response.status, context, body_str));
    }

    // Parse Telegram API response
    let api_response: TelegramApiResponse<serde_json::Value> = serde_json::from_slice(&response.body)
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    if !api_response.ok {
        let context = if retried { " (after 409 retry)" } else { "" };
        return Err(format!(
            "Telegram API error{}: {}",
            context,
            api_response
                .description
                .unwrap_or_else(|| "unknown".to_string())
        ));
    }

    let context = if retried { " (after retry)" } else { "" };
    channel_host::log(
        channel_host::LogLevel::Info,
        &format!(
            "Webhook registered successfully{}: {}",
            context, webhook_url
        ),
    );

    Ok(())
}

/// Create a JSON HTTP response.
pub(crate) fn json_response(status: u16, value: serde_json::Value) -> OutgoingHttpResponse {
    let body = serde_json::to_vec(&value).unwrap_or_default();
    let headers = serde_json::json!({"Content-Type": "application/json"});

    OutgoingHttpResponse {
        status,
        headers_json: headers.to_string(),
        body,
    }
}

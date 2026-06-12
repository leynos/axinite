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
fn build_set_webhook_body(
    webhook_url: &str,
    webhook_secret: Option<&str>,
) -> Result<Vec<u8>, String> {
    let mut body = serde_json::json!({
        "url": webhook_url,
        "allowed_updates": ["message", "edited_message"]
    });

    if let Some(secret) = webhook_secret {
        body["secret_token"] = serde_json::Value::String(secret.to_string());
    }

    serde_json::to_vec(&body).map_err(|e| format!("Failed to serialize body: {}", e))
}

fn set_webhook_headers() -> String {
    serde_json::json!({
        "Content-Type": "application/json"
    })
    .to_string()
}

fn post_set_webhook(
    headers_json: &str,
    body_bytes: &[u8],
    retry_context: Option<&str>,
) -> Result<channel_host::HttpResponse, String> {
    channel_host::http_request(&channel_host::HttpRequestParams {
        method: "POST".to_string(),
        url: "https://api.telegram.org/bot{TELEGRAM_BOT_TOKEN}/setWebhook".to_string(),
        headers_json: headers_json.to_string(),
        body: Some(body_bytes.to_vec()),
        timeout_ms: None,
    })
    .map_err(|e| match retry_context {
        Some(context) => format!("HTTP request failed {}: {}", context, e),
        None => format!("HTTP request failed: {}", e),
    })
}

fn retry_set_webhook_after_conflict(
    headers_json: &str,
    body_bytes: &[u8],
) -> Result<channel_host::HttpResponse, String> {
    channel_host::log(
        channel_host::LogLevel::Warn,
        "409 Conflict -- deleting existing webhook and retrying",
    );
    let _ = delete_webhook();

    channel_host::http_request(&channel_host::HttpRequestParams {
        method: "POST".to_string(),
        url: "https://api.telegram.org/bot{TELEGRAM_BOT_TOKEN}/setWebhook".to_string(),
        headers_json: headers_json.to_string(),
        body: Some(body_bytes.to_vec()),
        timeout_ms: None,
    })
    .map_err(|e| format!("HTTP request failed (after 409 retry): {}", e))
}

fn register_webhook_response_after_retry(
    response: channel_host::HttpResponse,
    headers_json: &str,
    body_bytes: &[u8],
) -> Result<(channel_host::HttpResponse, bool), String> {
    if response.status != 409 {
        return Ok((response, false));
    }

    retry_set_webhook_after_conflict(headers_json, body_bytes).map(|response| (response, true))
}

fn validate_http_status(response: &channel_host::HttpResponse, retried: bool) -> Result<(), String> {
    if response.status == 200 {
        return Ok(());
    }

    let body_str = String::from_utf8_lossy(&response.body);
    let context = if retried { " (after 409 retry)" } else { "" };
    Err(format!("HTTP {}{}: {}", response.status, context, body_str))
}

fn validate_telegram_api_response(
    response: &channel_host::HttpResponse,
    retried: bool,
) -> Result<(), String> {
    let api_response: TelegramApiResponse<serde_json::Value> =
        serde_json::from_slice(&response.body)
            .map_err(|e| format!("Failed to parse response: {}", e))?;

    if api_response.ok {
        return Ok(());
    }

    let context = if retried { " (after 409 retry)" } else { "" };
    Err(format!(
        "Telegram API error{}: {}",
        context,
        api_response
            .description
            .unwrap_or_else(|| "unknown".to_string())
    ))
}

fn log_webhook_registered(webhook_url: &str, retried: bool) {
    let context = if retried { " (after retry)" } else { "" };
    channel_host::log(
        channel_host::LogLevel::Info,
        &format!(
            "Webhook registered successfully{}: {}",
            context, webhook_url
        ),
    );
}

pub(crate) fn register_webhook(
    tunnel_url: &str,
    webhook_secret: Option<&str>,
) -> Result<(), String> {
    let webhook_url = format!("{}/webhook/telegram", tunnel_url);
    let body_bytes = build_set_webhook_body(&webhook_url, webhook_secret)?;
    let headers_json = set_webhook_headers();

    // Note: {TELEGRAM_BOT_TOKEN} is replaced by host with the actual token
    let response = post_set_webhook(&headers_json, &body_bytes, None)?;
    let (response, retried) =
        register_webhook_response_after_retry(response, &headers_json, &body_bytes)?;

    validate_http_status(&response, retried)?;
    validate_telegram_api_response(&response, retried)?;
    log_webhook_registered(&webhook_url, retried);

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

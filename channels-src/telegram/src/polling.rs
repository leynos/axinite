use crate::near::agent::channel_host;
use crate::state::POLLING_STATE_PATH;
use crate::types::{TelegramApiResponse, TelegramUpdate};
use crate::inbound::handle_update;

pub(crate) fn get_updates_url(offset: i64, timeout_secs: u32) -> String {
    format!(
        "https://api.telegram.org/bot{TELEGRAM_BOT_TOKEN}/getUpdates?offset={}&timeout={}&allowed_updates=[\"message\",\"edited_message\"]",
        offset, timeout_secs
    )
}

pub(crate) fn fetch_updates(offset: i64, headers_json: &str) -> Result<channel_host::HttpResponse, String> {
    let primary_url = get_updates_url(offset, 25);

    channel_host::http_request(&channel_host::HttpRequestParams {
        method: "GET".to_string(),
        url: primary_url,
        headers_json: headers_json.to_string(),
        body: None,
        timeout_ms: Some(35_000),
    })
    .or_else(|primary_err| {
        channel_host::log(
            channel_host::LogLevel::Warn,
            &format!(
                "getUpdates request failed ({}), retrying once immediately",
                primary_err
            ),
        );

        let retry_url = get_updates_url(offset, 3);
        channel_host::http_request(&channel_host::HttpRequestParams {
            method: "GET".to_string(),
            url: retry_url,
            headers_json: headers_json.to_string(),
            body: None,
            timeout_ms: Some(8_000),
        })
        .map_err(|retry_err| {
            format!("primary error: {}; retry error: {}", primary_err, retry_err)
        })
    })
}

pub(crate) fn process_updates_response(response: channel_host::HttpResponse, offset: i64) {
    if response.status != 200 {
        let body_str = String::from_utf8_lossy(&response.body);
        channel_host::log(
            channel_host::LogLevel::Error,
            &format!("getUpdates returned {}: {}", response.status, body_str),
        );
        return;
    }

    let api_response: Result<TelegramApiResponse<Vec<TelegramUpdate>>, _> =
        serde_json::from_slice(&response.body);

    match api_response {
        Ok(resp) if resp.ok => {
            if let Some(updates) = resp.result {
                let mut new_offset = offset;

                for update in updates {
                    if update.update_id >= new_offset {
                        new_offset = update.update_id + 1;
                    }
                    handle_update(update);
                }

                if new_offset != offset {
                    if let Err(e) =
                        channel_host::workspace_write(POLLING_STATE_PATH, &new_offset.to_string())
                    {
                        channel_host::log(
                            channel_host::LogLevel::Error,
                            &format!("Failed to save polling offset: {}", e),
                        );
                    }
                }
            }
        }
        Ok(resp) => {
            channel_host::log(
                channel_host::LogLevel::Error,
                &format!(
                    "Telegram API error: {}",
                    resp.description.unwrap_or_else(|| "unknown".to_string())
                ),
            );
        }
        Err(e) => {
            channel_host::log(
                channel_host::LogLevel::Error,
                &format!("Failed to parse getUpdates response: {}", e),
            );
        }
    }
}

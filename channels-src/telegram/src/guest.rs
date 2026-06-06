use crate::exports::near::agent::channel::{
    AgentResponse, ChannelConfig, Guest, HttpEndpointConfig, IncomingHttpRequest,
    OutgoingHttpResponse, PollConfig, StatusUpdate,
};
use crate::inbound::handle_update;
use crate::polling::{fetch_updates, process_updates_response};
use crate::send::{MessageText, send_message, send_response};
use crate::state::{
    ALLOW_FROM_PATH, BOT_USERNAME_PATH, DM_POLICY_PATH, OWNER_ID_PATH, POLLING_STATE_PATH,
    RESPOND_TO_ALL_GROUP_PATH,
};
use crate::status::{classify_status_update, TelegramStatusAction};
use crate::types::{TelegramConfig, TelegramMessageMetadata};
use crate::webhook::{delete_webhook, json_response, register_webhook};
use crate::near::agent::channel_host;
use crate::TelegramChannel;

fn log_bot_username(config: &TelegramConfig) {
    if let Some(ref username) = config.bot_username {
        channel_host::log(
            channel_host::LogLevel::Info,
            &format!("Bot username: @{}", username),
        );
    }
}

fn persist_owner_id(owner_id: i64) {
    if let Err(e) = channel_host::workspace_write(OWNER_ID_PATH, &owner_id.to_string()) {
        channel_host::log(
            channel_host::LogLevel::Error,
            &format!("Failed to persist owner_id: {}", e),
        );
    }

    channel_host::log(
        channel_host::LogLevel::Info,
        &format!("Owner restriction enabled: user {}", owner_id),
    );
}

fn clear_owner_id() {
    let _ = channel_host::workspace_write(OWNER_ID_PATH, "");
    channel_host::log(
        channel_host::LogLevel::Warn,
        "No owner_id configured, bot is open to all users",
    );
}

fn persist_owner_config(owner_id: Option<i64>) {
    match owner_id {
        Some(owner_id) => persist_owner_id(owner_id),
        None => clear_owner_id(),
    }
}

fn persist_runtime_config(config: &TelegramConfig) {
    let dm_policy = config.dm_policy.as_deref().unwrap_or("pairing").to_string();
    let _ = channel_host::workspace_write(DM_POLICY_PATH, &dm_policy);

    let allow_from_json = serde_json::to_string(&config.allow_from.clone().unwrap_or_default())
        .unwrap_or_else(|_| "[]".to_string());
    let _ = channel_host::workspace_write(ALLOW_FROM_PATH, &allow_from_json);

    let _ = channel_host::workspace_write(
        BOT_USERNAME_PATH,
        &config.bot_username.clone().unwrap_or_default(),
    );
    let _ = channel_host::workspace_write(
        RESPOND_TO_ALL_GROUP_PATH,
        &config.respond_to_all_group_messages.to_string(),
    );
}

fn webhook_mode_enabled(config: &TelegramConfig) -> bool {
    config.tunnel_url.is_some() && !config.polling_enabled
}

fn start_webhook_mode(config: &TelegramConfig) -> Result<(), String> {
    channel_host::log(
        channel_host::LogLevel::Info,
        "Webhook mode enabled (tunnel configured)",
    );

    let Some(ref tunnel_url) = config.tunnel_url else {
        return Ok(());
    };

    // Clear any stale webhook first to avoid 409 Conflict
    let _ = delete_webhook();

    channel_host::log(
        channel_host::LogLevel::Info,
        &format!("Registering webhook: {}/webhook/telegram", tunnel_url),
    );

    register_webhook(tunnel_url, config.webhook_secret.as_deref())
        .map_err(|e| format!("Failed to register webhook: {}", e))
}

fn start_polling_mode() -> Result<(), String> {
    channel_host::log(
        channel_host::LogLevel::Info,
        "Polling mode enabled (no tunnel configured)",
    );

    // Delete any existing webhook before polling. Telegram returns success
    // when no webhook exists, so any error here (e.g. 401) means a bad token.
    delete_webhook().map_err(|e| format!("Bot token validation failed: {}", e))
}

fn configure_transport(config: &TelegramConfig, webhook_mode: bool) -> Result<(), String> {
    if webhook_mode {
        start_webhook_mode(config)
    } else {
        start_polling_mode()
    }
}

fn poll_config_for_mode(webhook_mode: bool) -> Option<PollConfig> {
    if webhook_mode {
        None
    } else {
        Some(PollConfig {
            interval_ms: 30000, // 30 seconds minimum
            enabled: true,
        })
    }
}

fn send_typing_action(chat_id: i64) {
    let payload = serde_json::json!({
        "chat_id": chat_id,
        "action": "typing"
    });

    let payload_bytes = match serde_json::to_vec(&payload) {
        Ok(b) => b,
        Err(_) => return,
    };

    let headers = serde_json::json!({ "Content-Type": "application/json" });

    let result = channel_host::http_request(&channel_host::HttpRequestParams {
        method: "POST".to_string(),
        url: "https://api.telegram.org/bot{TELEGRAM_BOT_TOKEN}/sendChatAction".to_string(),
        headers_json: headers.to_string(),
        body: Some(payload_bytes),
        timeout_ms: None,
    });

    if let Err(e) = result {
        channel_host::log(
            channel_host::LogLevel::Debug,
            &format!("sendChatAction failed: {}", e),
        );
    }
}

fn send_status_notify(chat_id: i64, message_id: i64, prompt: &str) {
    if let Err(first_err) = send_message(chat_id, MessageText(prompt), Some(message_id), None) {
        channel_host::log(
            channel_host::LogLevel::Warn,
            &format!(
                "Failed to send status reply ({}), retrying without reply context",
                first_err
            ),
        );

        if let Err(retry_err) = send_message(chat_id, MessageText(prompt), None, None) {
            channel_host::log(
                channel_host::LogLevel::Debug,
                &format!(
                    "Failed to send status message without reply context: {}",
                    retry_err
                ),
            );
        }
    }
}

impl Guest for TelegramChannel {
    fn on_start(config_json: String) -> Result<ChannelConfig, String> {
        channel_host::log(
            channel_host::LogLevel::Debug,
            &format!("Telegram channel config: {}", config_json),
        );

        let config: TelegramConfig = serde_json::from_str(&config_json)
            .map_err(|e| format!("Failed to parse config: {}", e))?;

        channel_host::log(channel_host::LogLevel::Info, "Telegram channel starting");

        // Persist owner_id so subsequent callbacks (on_http_request, on_poll) can read it
        // Persist dm_policy and allow_from for DM pairing in handle_message
        // Persist bot_username and respond_to_all_group_messages for group handling
        log_bot_username(&config);
        persist_owner_config(config.owner_id);
        persist_runtime_config(&config);

        // Mode: use polling if explicitly enabled, otherwise use webhooks when tunnel available.
        let webhook_mode = webhook_mode_enabled(&config);
        // Register webhook with Telegram API — propagate errors so a bad token
        // causes activation to fail rather than silently succeeding.
        configure_transport(&config, webhook_mode)?;

        // Configure polling only if not in webhook mode
        let poll = poll_config_for_mode(webhook_mode);

        // Webhook secret validation is handled by the host
        let require_secret = config.webhook_secret.is_some();

        Ok(ChannelConfig {
            display_name: "Telegram".to_string(),
            http_endpoints: vec![HttpEndpointConfig {
                path: "/webhook/telegram".to_string(),
                methods: vec!["POST".to_string()],
                require_secret,
            }],
            poll,
        })
    }

    fn on_http_request(req: IncomingHttpRequest) -> OutgoingHttpResponse {
        // Check if webhook secret validation passed (if required)
        // The host validates X-Telegram-Bot-Api-Secret-Token header and sets secret_validated
        // If require_secret was true in config but validation failed, secret_validated will be false
        if !req.secret_validated {
            // This means require_secret was set but the secret didn't match
            // We still check the field even though the host should have already rejected invalid requests
            // This is defense in depth
            channel_host::log(
                channel_host::LogLevel::Warn,
                "Webhook request with invalid or missing secret token",
            );
            // Return 401 but Telegram will keep retrying, so this is just for logging
            // In practice, the host should reject these before they reach us
        }

        // Parse the request body as UTF-8
        let body_str = match std::str::from_utf8(&req.body) {
            Ok(s) => s,
            Err(_) => {
                return json_response(400, serde_json::json!({"error": "Invalid UTF-8 body"}));
            }
        };

        // Parse as Telegram Update
        let update: crate::types::TelegramUpdate = match serde_json::from_str(body_str) {
            Ok(u) => u,
            Err(e) => {
                channel_host::log(
                    channel_host::LogLevel::Error,
                    &format!("Failed to parse Telegram update: {}", e),
                );
                // Still return 200 to prevent Telegram from retrying
                return json_response(200, serde_json::json!({"ok": true}));
            }
        };

        // Handle the update
        handle_update(update);

        // Always respond 200 quickly (Telegram expects fast responses)
        json_response(200, serde_json::json!({"ok": true}))
    }

    fn on_poll() {
        let offset = match channel_host::workspace_read(POLLING_STATE_PATH) {
            Some(s) => s.parse::<i64>().unwrap_or(0),
            None => 0,
        };

        channel_host::log(
            channel_host::LogLevel::Debug,
            &format!("Polling getUpdates with offset {}", offset),
        );

        let headers_json = serde_json::json!({}).to_string();

        match fetch_updates(offset, &headers_json) {
            Ok(response) => process_updates_response(response, offset),
            Err(e) => channel_host::log(
                channel_host::LogLevel::Error,
                &format!("getUpdates request failed: {}", e),
            ),
        }
    }

    fn on_respond(response: AgentResponse) -> Result<(), String> {
        let metadata: TelegramMessageMetadata = serde_json::from_str(&response.metadata_json)
            .map_err(|e| format!("Failed to parse metadata: {}", e))?;

        send_response(metadata.chat_id, &response, Some(metadata.message_id))
    }

    fn on_broadcast(user_id: String, response: AgentResponse) -> Result<(), String> {
        let chat_id: i64 = user_id
            .parse()
            .map_err(|e| format!("Invalid chat_id '{}': {}", user_id, e))?;

        send_response(chat_id, &response, None)
    }

    fn on_status(update: StatusUpdate) {
        let action = match classify_status_update(&update) {
            Some(action) => action,
            None => return,
        };

        let metadata: TelegramMessageMetadata = match serde_json::from_str(&update.metadata_json) {
            Ok(m) => m,
            Err(_) => {
                channel_host::log(
                    channel_host::LogLevel::Debug,
                    "on_status: no valid Telegram metadata, skipping status update",
                );
                return;
            }
        };

        match action {
            TelegramStatusAction::Typing => send_typing_action(metadata.chat_id),
            TelegramStatusAction::Notify(prompt) => {
                send_status_notify(metadata.chat_id, metadata.message_id, &prompt)
            }
        }
    }

    fn on_shutdown() {
        channel_host::log(
            channel_host::LogLevel::Info,
            "Telegram channel shutting down",
        );
    }
}

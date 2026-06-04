use crate::attachments::extract_attachments;
use crate::downloads::{
    download_and_store_documents, download_and_store_images, download_and_store_voice,
};
use crate::near::agent::channel_host::{self, EmittedMessage};
use crate::send::send_pairing_reply;
use crate::state::{
    ALLOW_FROM_PATH, BOT_USERNAME_PATH, CHANNEL_NAME, DM_POLICY_PATH, OWNER_ID_PATH,
    RESPOND_TO_ALL_GROUP_PATH,
};
use crate::types::{TelegramMessage, TelegramUpdate};

/// Process a Telegram update and emit messages if applicable.
pub(crate) fn handle_update(update: TelegramUpdate) {
    // Handle regular messages
    if let Some(message) = update.message {
        handle_message(message);
    }

    // Optionally handle edited messages the same way
    if let Some(message) = update.edited_message {
        handle_message(message);
    }
}

fn handle_message(message: TelegramMessage) {
    // Extract attachments from media fields (pure data mapping, no host calls)
    let mut attachments = extract_attachments(&message);

    // Download and store voice attachments for host-side transcription
    download_and_store_voice(&attachments);

    // Download and store image attachments for host-side vision pipeline
    download_and_store_images(&attachments);

    // Download and store document attachments for host-side text extraction
    download_and_store_documents(&mut attachments);

    // Use text or caption (for media messages)
    let has_voice = message.voice.is_some();
    let content = message
        .text
        .filter(|t| !t.is_empty())
        .or_else(|| message.caption.filter(|c| !c.is_empty()))
        .unwrap_or_else(|| {
            if has_voice {
                "[Voice note]".to_string()
            } else {
                String::new()
            }
        });

    // Allow messages with attachments even if text content is empty
    if content.is_empty() && attachments.is_empty() {
        return;
    }

    // Skip messages without a sender (channel posts)
    let from = match message.from {
        Some(f) => f,
        None => return,
    };

    // Skip bot messages to avoid loops
    if from.is_bot {
        return;
    }

    let is_private = message.chat.chat_type == "private";

    // Owner validation: when owner_id is set, only that user can message
    let owner_id_str = channel_host::workspace_read(OWNER_ID_PATH).filter(|s| !s.is_empty());

    if let Some(ref id_str) = owner_id_str {
        if let Ok(owner_id) = id_str.parse::<i64>() {
            if from.id != owner_id {
                channel_host::log(
                    channel_host::LogLevel::Debug,
                    &format!(
                        "Dropping message from non-owner user {} (owner: {})",
                        from.id, owner_id
                    ),
                );
                return;
            }
        }
    } else {
        // No owner_id: apply authorization based on dm_policy and allow_from
        // This applies to both private and group chats when owner_id is null
        let dm_policy =
            channel_host::workspace_read(DM_POLICY_PATH).unwrap_or_else(|| "pairing".to_string());

        // For private chats with non-open policy, check allowlist
        // For group chats with non-open policy, also check allowlist
        if dm_policy != "open" {
            // Build effective allow list: config allow_from + pairing store
            let mut allowed: Vec<String> = channel_host::workspace_read(ALLOW_FROM_PATH)
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();

            if let Ok(store_allowed) = channel_host::pairing_read_allow_from(CHANNEL_NAME) {
                allowed.extend(store_allowed);
            }

            let id_str = from.id.to_string();
            let username_opt = from.username.as_deref();
            let is_allowed = allowed.contains(&"*".to_string())
                || allowed.contains(&id_str)
                || username_opt.is_some_and(|u| allowed.contains(&u.to_string()));

            if !is_allowed {
                if is_private && dm_policy == "pairing" {
                    // Upsert pairing request and send reply (only for private chats)
                    let meta = serde_json::json!({
                        "chat_id": message.chat.id,
                        "user_id": from.id,
                        "username": username_opt,
                    })
                    .to_string();

                    match channel_host::pairing_upsert_request(&channel_host::PairingUpsertParams {
                        identity: channel_host::PairingIdentity {
                            channel: CHANNEL_NAME.to_string(),
                            id: id_str.clone(),
                        },
                        meta_json: meta,
                    }) {
                        Ok(result) => {
                            channel_host::log(
                                channel_host::LogLevel::Info,
                                &format!(
                                    "Pairing request for user {} (chat {}): code {}",
                                    from.id, message.chat.id, result.code
                                ),
                            );
                            if result.created {
                                let _ = send_pairing_reply(message.chat.id, &result.code);
                            }
                        }
                        Err(e) => {
                            channel_host::log(
                                channel_host::LogLevel::Error,
                                &format!("Pairing upsert failed: {}", e),
                            );
                        }
                    }
                } else if !is_private {
                    // For group chats with non-open dm_policy, just log and drop
                    channel_host::log(
                        channel_host::LogLevel::Debug,
                        &format!(
                            "Dropping message from unauthorized user {} in group chat",
                            from.id
                        ),
                    );
                }
                return;
            }
        }
    }

    // For group chats, only respond if bot was mentioned or respond_to_all is enabled
    if !is_private {
        let respond_to_all = channel_host::workspace_read(RESPOND_TO_ALL_GROUP_PATH)
            .as_deref()
            .unwrap_or("false")
            == "true";

        if !respond_to_all {
            let has_command = content.starts_with('/');
            let bot_username = channel_host::workspace_read(BOT_USERNAME_PATH).unwrap_or_default();
            let has_bot_mention = if bot_username.is_empty() {
                content.contains('@')
            } else {
                let mention = format!("@{}", bot_username);
                content.to_lowercase().contains(&mention.to_lowercase())
            };

            if !has_command && !has_bot_mention {
                channel_host::log(
                    channel_host::LogLevel::Debug,
                    &format!("Ignoring group message without mention: {}", content),
                );
                return;
            }
        }
    }

    // Build user display name
    let user_name = if let Some(ref last) = from.last_name {
        format!("{} {}", from.first_name, last)
    } else {
        from.first_name.clone()
    };

    // Build metadata for response routing
    let metadata = crate::types::TelegramMessageMetadata {
        chat_id: message.chat.id,
        message_id: message.message_id,
        user_id: from.id,
        is_private,
    };

    let metadata_json = serde_json::to_string(&metadata).unwrap_or_else(|_| "{}".to_string());

    let bot_username = channel_host::workspace_read(BOT_USERNAME_PATH).unwrap_or_default();
    let content_to_emit = match content_to_emit_for_agent(
        &content,
        if bot_username.is_empty() {
            None
        } else {
            Some(bot_username.as_str())
        },
    ) {
        Some(value) => value,
        // Allow attachment-only messages even without text
        None if !attachments.is_empty() => String::new(),
        None => return,
    };

    // Emit the message to the agent
    channel_host::emit_message(&EmittedMessage {
        user_id: from.id.to_string(),
        user_name: Some(user_name),
        content: content_to_emit,
        thread_id: None, // Telegram doesn't have threads in the same way
        metadata_json,
        attachments,
    });

    channel_host::log(
        channel_host::LogLevel::Debug,
        &format!(
            "Emitted message from user {} in chat {}",
            from.id, message.chat.id
        ),
    );
}

/// Clean message text by removing bot commands and @mentions at the start.
/// When bot_username is set, only strips that specific mention; otherwise strips any leading @mention.
pub(crate) fn clean_message_text(text: &str, bot_username: Option<&str>) -> String {
    let mut result = text.trim().to_string();

    // Remove leading /command
    if result.starts_with('/') {
        if let Some(space_idx) = result.find(' ') {
            result = result[space_idx..].trim_start().to_string();
        } else {
            // Just a command with no text
            return String::new();
        }
    }

    // Remove leading @mention
    if result.starts_with('@') {
        if let Some(bot) = bot_username {
            let mention = format!("@{}", bot);
            let mention_lower = mention.to_lowercase();
            let result_lower = result.to_lowercase();
            if result_lower.starts_with(&mention_lower) {
                let rest = result[mention.len()..].trim_start();
                if rest.is_empty() {
                    return String::new();
                }
                result = rest.to_string();
            } else if let Some(space_idx) = result.find(' ') {
                // Different leading @mention - only strip if it's the bot
                let first_word = &result[..space_idx];
                if first_word.eq_ignore_ascii_case(&mention) {
                    result = result[space_idx..].trim_start().to_string();
                }
            }
        } else {
            // No bot_username: strip any leading @mention
            if let Some(space_idx) = result.find(' ') {
                result = result[space_idx..].trim_start().to_string();
            } else {
                return String::new();
            }
        }
    }

    result
}

/// Decide which user content should be emitted to the agent loop.
///
/// - `/start` emits a placeholder so the agent can greet the user
/// - bare slash commands are passed through for Submission parsing
/// - empty/mention-only messages are ignored
/// - otherwise cleaned text is emitted
pub(crate) fn content_to_emit_for_agent(content: &str, bot_username: Option<&str>) -> Option<String> {
    let cleaned_text = clean_message_text(content, bot_username);
    let trimmed_content = content.trim();

    if trimmed_content.eq_ignore_ascii_case("/start") {
        return Some("[User started the bot]".to_string());
    }

    if cleaned_text.is_empty() && trimmed_content.starts_with('/') {
        return Some(trimmed_content.to_string());
    }

    if cleaned_text.is_empty() {
        return None;
    }

    Some(cleaned_text)
}

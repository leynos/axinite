use crate::attachments::extract_attachments;
use crate::downloads::{
    download_and_store_documents, download_and_store_images, download_and_store_voice,
};
use crate::near::agent::channel_host::{self, EmittedMessage, InboundAttachment};
use crate::send::{PairingCode, send_pairing_reply};
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

fn message_content(message: &TelegramMessage) -> String {
    let has_voice = message.voice.is_some();

    message
        .text
        .as_ref()
        .filter(|text| !text.is_empty())
        .cloned()
        .or_else(|| {
            message
                .caption
                .as_ref()
                .filter(|caption| !caption.is_empty())
                .cloned()
        })
        .unwrap_or_else(|| {
            if has_voice {
                "[Voice note]".to_string()
            } else {
                String::new()
            }
        })
}

fn user_display_name(first_name: &str, last_name: Option<&str>) -> String {
    match last_name {
        Some(last) => format!("{} {}", first_name, last),
        None => first_name.to_string(),
    }
}

fn owner_allows_sender(user_id: i64) -> Option<bool> {
    let owner_id_str = channel_host::workspace_read(OWNER_ID_PATH).filter(|s| !s.is_empty());

    let Some(id_str) = owner_id_str else {
        return None;
    };

    let Ok(owner_id) = id_str.parse::<i64>() else {
        return Some(true);
    };

    if user_id != owner_id {
        channel_host::log(
            channel_host::LogLevel::Debug,
            &format!(
                "Dropping message from non-owner user {} (owner: {})",
                user_id, owner_id
            ),
        );
        return Some(false);
    }

    Some(true)
}

fn load_allowed_senders() -> Vec<String> {
    let mut allowed: Vec<String> = channel_host::workspace_read(ALLOW_FROM_PATH)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    if let Ok(store_allowed) = channel_host::pairing_read_allow_from(CHANNEL_NAME) {
        allowed.extend(store_allowed);
    }

    allowed
}

fn sender_in_allow_list(allowed: &[String], user_id: i64, username: Option<&str>) -> bool {
    let id = user_id.to_string();

    allowed.iter().any(|entry| entry == "*")
        || allowed.iter().any(|entry| entry == &id)
        || username.is_some_and(|u| allowed.iter().any(|entry| entry == u))
}

fn send_pairing_reply_if_new(chat_id: i64, result: &channel_host::PairingUpsertResult) {
    if result.created {
        let _ = send_pairing_reply(chat_id, PairingCode(&result.code));
    }
}

fn request_pairing_for_sender(chat_id: i64, user_id: i64, username: Option<&str>) {
    let id_str = user_id.to_string();
    let meta = serde_json::json!({
        "chat_id": chat_id,
        "user_id": user_id,
        "username": username,
    })
    .to_string();

    match channel_host::pairing_upsert_request(&channel_host::PairingUpsertParams {
        identity: channel_host::PairingIdentity {
            channel: CHANNEL_NAME.to_string(),
            id: id_str,
        },
        meta_json: meta,
    }) {
        Ok(result) => {
            channel_host::log(
                channel_host::LogLevel::Info,
                &format!(
                    "Pairing request for user {} (chat {}): code {}",
                    user_id, chat_id, result.code
                ),
            );
            send_pairing_reply_if_new(chat_id, &result);
        }
        Err(e) => {
            channel_host::log(
                channel_host::LogLevel::Error,
                &format!("Pairing upsert failed: {}", e),
            );
        }
    }
}

fn log_unauthorized_group_sender(user_id: i64) {
    channel_host::log(
        channel_host::LogLevel::Debug,
        &format!(
            "Dropping message from unauthorized user {} in group chat",
            user_id
        ),
    );
}

fn sender_is_authorized(
    chat_id: i64,
    user_id: i64,
    username: Option<&str>,
    is_private: bool,
) -> bool {
    if let Some(allowed_by_owner) = owner_allows_sender(user_id) {
        return allowed_by_owner;
    }

    let dm_policy =
        channel_host::workspace_read(DM_POLICY_PATH).unwrap_or_else(|| "pairing".to_string());

    if dm_policy == "open" {
        return true;
    }

    let allowed = load_allowed_senders();
    if sender_in_allow_list(&allowed, user_id, username) {
        return true;
    }

    if is_private && dm_policy == "pairing" {
        request_pairing_for_sender(chat_id, user_id, username);
    } else if !is_private {
        log_unauthorized_group_sender(user_id);
    }

    false
}

fn bot_username() -> Option<String> {
    let username = channel_host::workspace_read(BOT_USERNAME_PATH).unwrap_or_default();
    if username.is_empty() {
        None
    } else {
        Some(username)
    }
}

fn content_mentions_bot(content: &str, bot_username: Option<&str>) -> bool {
    match bot_username {
        Some(username) => {
            let mention = format!("@{}", username);
            content.to_lowercase().contains(&mention.to_lowercase())
        }
        None => content.contains('@'),
    }
}

fn group_message_should_emit(content: &str) -> bool {
    let respond_to_all = channel_host::workspace_read(RESPOND_TO_ALL_GROUP_PATH)
        .as_deref()
        .unwrap_or("false")
        == "true";

    if respond_to_all {
        return true;
    }

    content.starts_with('/') || content_mentions_bot(content, bot_username().as_deref())
}

fn log_ignored_group_message(content: &str) {
    channel_host::log(
        channel_host::LogLevel::Debug,
        &format!("Ignoring group message without mention: {}", content),
    );
}

fn metadata_json_for_message(message: &TelegramMessage, user_id: i64, is_private: bool) -> String {
    let metadata = crate::types::TelegramMessageMetadata {
        chat_id: message.chat.id,
        message_id: message.message_id,
        user_id,
        is_private,
    };

    serde_json::to_string(&metadata).unwrap_or_else(|_| "{}".to_string())
}

fn content_to_emit(content: &str, has_attachments: bool) -> Option<String> {
    match content_to_emit_for_agent(content, bot_username().as_deref()) {
        Some(value) => Some(value),
        None if has_attachments => Some(String::new()),
        None => None,
    }
}

struct AgentMessageEmission {
    chat_id: i64,
    user_id: i64,
    user_name: String,
    content: String,
    metadata_json: String,
    attachments: Vec<InboundAttachment>,
}

fn emit_agent_message(emission: AgentMessageEmission) {
    channel_host::emit_message(&EmittedMessage {
        user_id: emission.user_id.to_string(),
        user_name: Some(emission.user_name),
        content: emission.content,
        thread_id: None, // Telegram doesn't have threads in the same way
        metadata_json: emission.metadata_json,
        attachments: emission.attachments,
    });

    channel_host::log(
        channel_host::LogLevel::Debug,
        &format!(
            "Emitted message from user {} in chat {}",
            emission.user_id, emission.chat_id
        ),
    );
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

    let content = message_content(&message);

    // Allow messages with attachments even if text content is empty
    if content.is_empty() && attachments.is_empty() {
        return;
    }

    // Skip messages without a sender (channel posts)
    let Some(from) = message.from.as_ref() else {
        return;
    };

    // Skip bot messages to avoid loops
    if from.is_bot {
        return;
    }

    let is_private = message.chat.chat_type == "private";
    let username = from.username.as_deref();

    if !sender_is_authorized(message.chat.id, from.id, username, is_private) {
        return;
    }

    // For group chats, only respond if bot was mentioned or respond_to_all is enabled
    if !is_private && !group_message_should_emit(&content) {
        log_ignored_group_message(&content);
        return;
    }

    let user_name = user_display_name(&from.first_name, from.last_name.as_deref());
    let metadata_json = metadata_json_for_message(&message, from.id, is_private);

    let Some(content_to_emit) = content_to_emit(&content, !attachments.is_empty()) else {
        return;
    };

    emit_agent_message(AgentMessageEmission {
        chat_id: message.chat.id,
        user_id: from.id,
        user_name,
        content: content_to_emit,
        metadata_json,
        attachments,
    });
}

fn strip_leading_command(text: String) -> Option<String> {
    if !text.starts_with('/') {
        return Some(text);
    }

    text.find(' ')
        .map(|space_idx| text[space_idx..].trim_start().to_string())
}

fn non_empty_trimmed_suffix(text: &str, start: usize) -> Option<String> {
    let rest = text[start..].trim_start();

    if rest.is_empty() {
        None
    } else {
        Some(rest.to_string())
    }
}

fn strip_exact_bot_mention_prefix(text: &str, mention: &str) -> Option<Option<String>> {
    let mention_lower = mention.to_lowercase();
    let text_lower = text.to_lowercase();

    if text_lower.starts_with(&mention_lower) {
        return Some(non_empty_trimmed_suffix(text, mention.len()));
    }

    None
}

fn strip_first_word_bot_mention(text: &str, mention: &str) -> Option<String> {
    let space_idx = text.find(' ')?;
    let first_word = &text[..space_idx];

    if first_word.eq_ignore_ascii_case(mention) {
        return Some(text[space_idx..].trim_start().to_string());
    }

    None
}

fn strip_known_bot_mention(text: String, bot: &str) -> Option<String> {
    if !text.starts_with('@') {
        return Some(text);
    }

    let mention = format!("@{}", bot);

    if let Some(stripped) = strip_exact_bot_mention_prefix(&text, &mention) {
        return stripped;
    }

    if let Some(stripped) = strip_first_word_bot_mention(&text, &mention) {
        return Some(stripped);
    }

    Some(text)
}

fn strip_any_leading_mention(text: String) -> Option<String> {
    if !text.starts_with('@') {
        return Some(text);
    }

    text.find(' ')
        .map(|space_idx| text[space_idx..].trim_start().to_string())
}

fn strip_leading_mention(text: String, bot_username: Option<&str>) -> Option<String> {
    match bot_username {
        Some(bot) => strip_known_bot_mention(text, bot),
        None => strip_any_leading_mention(text),
    }
}

/// Clean message text by removing bot commands and @mentions at the start.
/// When bot_username is set, only strips that specific mention; otherwise strips any leading @mention.
pub(crate) fn clean_message_text(text: &str, bot_username: Option<&str>) -> String {
    let trimmed = text.trim().to_string();

    strip_leading_command(trimmed)
        .and_then(|text| strip_leading_mention(text, bot_username))
        .unwrap_or_default()
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

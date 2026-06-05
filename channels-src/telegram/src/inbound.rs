use crate::attachments::extract_attachments;
use crate::downloads::{
    download_and_store_documents, download_and_store_images, download_and_store_voice,
};
use crate::near::agent::channel_host::{self, EmittedMessage, InboundAttachment};
use crate::send::{send_pairing_reply, PairingCode};
use crate::state::{
    ALLOW_FROM_PATH, BOT_USERNAME_PATH, CHANNEL_NAME, DM_POLICY_PATH, OWNER_ID_PATH,
    RESPOND_TO_ALL_GROUP_PATH,
};
use crate::types::{TelegramMessage, TelegramUpdate};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TelegramUserId(i64);

impl TelegramUserId {
    fn as_i64(self) -> i64 {
        self.0
    }

    fn as_pairing_id(self) -> String {
        self.0.to_string()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TelegramChatId(i64);

impl TelegramChatId {
    fn as_i64(self) -> i64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChatVisibility {
    Private,
    Group,
}

impl ChatVisibility {
    fn from_chat_type(chat_type: &str) -> Self {
        if chat_type == "private" {
            Self::Private
        } else {
            Self::Group
        }
    }

    fn is_private(self) -> bool {
        matches!(self, Self::Private)
    }
}

#[derive(Clone, Copy, Debug)]
struct SenderContext<'a> {
    chat_id: TelegramChatId,
    user_id: TelegramUserId,
    username: Option<&'a str>,
    visibility: ChatVisibility,
}

impl SenderContext<'_> {
    fn is_private(self) -> bool {
        self.visibility.is_private()
    }
}

#[derive(Clone, Copy, Debug)]
struct MessageContent<'a>(&'a str);

impl<'a> MessageContent<'a> {
    fn as_str(self) -> &'a str {
        self.0
    }

    fn starts_with_command(self) -> bool {
        self.0.starts_with('/')
    }
}

#[derive(Clone, Copy, Debug)]
enum AttachmentPresence {
    Present,
    Absent,
}

impl AttachmentPresence {
    fn is_present(self) -> bool {
        matches!(self, Self::Present)
    }
}

#[derive(Clone, Copy, Debug)]
struct BotUsername<'a>(&'a str);

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

fn owner_allows_sender(user_id: TelegramUserId) -> Option<bool> {
    let owner_id_str = channel_host::workspace_read(OWNER_ID_PATH).filter(|s| !s.is_empty());

    let Some(id_str) = owner_id_str else {
        return None;
    };

    let Ok(owner_id) = id_str.parse::<i64>() else {
        return Some(true);
    };

    if user_id.as_i64() != owner_id {
        channel_host::log(
            channel_host::LogLevel::Debug,
            &format!(
                "Dropping message from non-owner user {} (owner: {})",
                user_id.as_i64(),
                owner_id
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

fn sender_in_allow_list(allowed: &[String], sender: SenderContext<'_>) -> bool {
    let id = sender.user_id.as_pairing_id();

    allowed.iter().any(|entry| entry == "*")
        || allowed.iter().any(|entry| entry == &id)
        || sender
            .username
            .is_some_and(|u| allowed.iter().any(|entry| entry == u))
}

fn send_pairing_reply_if_new(chat_id: TelegramChatId, result: &channel_host::PairingUpsertResult) {
    if result.created {
        let _ = send_pairing_reply(chat_id.as_i64(), PairingCode(&result.code));
    }
}

fn request_pairing_for_sender(sender: SenderContext<'_>) {
    let id_str = sender.user_id.as_pairing_id();
    let meta = serde_json::json!({
        "chat_id": sender.chat_id.as_i64(),
        "user_id": sender.user_id.as_i64(),
        "username": sender.username,
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
                    sender.user_id.as_i64(),
                    sender.chat_id.as_i64(),
                    result.code
                ),
            );
            send_pairing_reply_if_new(sender.chat_id, &result);
        }
        Err(e) => {
            channel_host::log(
                channel_host::LogLevel::Error,
                &format!("Pairing upsert failed: {}", e),
            );
        }
    }
}

fn log_unauthorized_group_sender(user_id: TelegramUserId) {
    channel_host::log(
        channel_host::LogLevel::Debug,
        &format!(
            "Dropping message from unauthorized user {} in group chat",
            user_id.as_i64()
        ),
    );
}

fn sender_is_authorized(sender: SenderContext<'_>) -> bool {
    if let Some(allowed_by_owner) = owner_allows_sender(sender.user_id) {
        return allowed_by_owner;
    }

    let dm_policy =
        channel_host::workspace_read(DM_POLICY_PATH).unwrap_or_else(|| "pairing".to_string());

    if dm_policy == "open" {
        return true;
    }

    let allowed = load_allowed_senders();
    if sender_in_allow_list(&allowed, sender) {
        return true;
    }

    if sender.is_private() && dm_policy == "pairing" {
        request_pairing_for_sender(sender);
    } else if !sender.is_private() {
        log_unauthorized_group_sender(sender.user_id);
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

fn content_mentions_bot(
    content: MessageContent<'_>,
    bot_username: Option<BotUsername<'_>>,
) -> bool {
    match bot_username {
        Some(username) => {
            let mention = format!("@{}", username.0);
            content
                .as_str()
                .to_lowercase()
                .contains(&mention.to_lowercase())
        }
        None => content.as_str().contains('@'),
    }
}

fn group_message_should_emit(content: MessageContent<'_>) -> bool {
    let respond_to_all = channel_host::workspace_read(RESPOND_TO_ALL_GROUP_PATH)
        .as_deref()
        .unwrap_or("false")
        == "true";

    if respond_to_all {
        return true;
    }

    content.starts_with_command()
        || content_mentions_bot(content, bot_username().as_deref().map(BotUsername))
}

fn log_ignored_group_message(content: MessageContent<'_>) {
    channel_host::log(
        channel_host::LogLevel::Debug,
        &format!(
            "Ignoring group message without mention: {}",
            content.as_str()
        ),
    );
}

fn metadata_json_for_message(message: &TelegramMessage, sender: SenderContext<'_>) -> String {
    let metadata = crate::types::TelegramMessageMetadata {
        chat_id: message.chat.id,
        message_id: message.message_id,
        user_id: sender.user_id.as_i64(),
        is_private: sender.is_private(),
    };

    serde_json::to_string(&metadata).unwrap_or_else(|_| "{}".to_string())
}

fn content_to_emit(
    content: MessageContent<'_>,
    attachment_presence: AttachmentPresence,
) -> Option<String> {
    match content_to_emit_for_agent(content.as_str(), bot_username().as_deref()) {
        Some(value) => Some(value),
        None if attachment_presence.is_present() => Some(String::new()),
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

    let visibility = ChatVisibility::from_chat_type(&message.chat.chat_type);
    let sender = SenderContext {
        chat_id: TelegramChatId(message.chat.id),
        user_id: TelegramUserId(from.id),
        username: from.username.as_deref(),
        visibility,
    };

    if !sender_is_authorized(sender) {
        return;
    }

    let message_content = MessageContent(&content);

    // For group chats, only respond if bot was mentioned or respond_to_all is enabled
    if !sender.is_private() && !group_message_should_emit(message_content) {
        log_ignored_group_message(message_content);
        return;
    }

    let user_name = user_display_name(&from.first_name, from.last_name.as_deref());
    let metadata_json = metadata_json_for_message(&message, sender);

    let attachment_presence = if attachments.is_empty() {
        AttachmentPresence::Absent
    } else {
        AttachmentPresence::Present
    };

    let Some(content_to_emit) = content_to_emit(message_content, attachment_presence) else {
        return;
    };

    emit_agent_message(AgentMessageEmission {
        chat_id: sender.chat_id.as_i64(),
        user_id: sender.user_id.as_i64(),
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
pub(crate) fn content_to_emit_for_agent(
    content: &str,
    bot_username: Option<&str>,
) -> Option<String> {
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

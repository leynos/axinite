use serde::{Deserialize, Serialize};

/// Telegram Update object (webhook payload).
/// https://core.telegram.org/bots/api#update
#[derive(Debug, Deserialize)]
pub(crate) struct TelegramUpdate {
    /// Unique update identifier.
    pub(crate) update_id: i64,

    /// New incoming message.
    pub(crate) message: Option<TelegramMessage>,

    /// Edited message.
    pub(crate) edited_message: Option<TelegramMessage>,

    /// Channel post (we ignore these for now).
    pub(crate) channel_post: Option<TelegramMessage>,
}

/// Telegram Message object.
/// https://core.telegram.org/bots/api#message
#[derive(Debug, Deserialize)]
pub(crate) struct TelegramMessage {
    /// Unique message identifier.
    pub(crate) message_id: i64,

    /// Sender (empty for channel posts).
    pub(crate) from: Option<TelegramUser>,

    /// Chat the message belongs to.
    pub(crate) chat: TelegramChat,

    /// Message text.
    pub(crate) text: Option<String>,

    /// Caption for media (photo, video, document, etc.).
    #[serde(default)]
    pub(crate) caption: Option<String>,

    /// Original message if this is a reply.
    pub(crate) reply_to_message: Option<Box<TelegramMessage>>,

    /// Bot command entities (for /commands).
    pub(crate) entities: Option<Vec<MessageEntity>>,

    /// Photo sizes (Telegram sends multiple sizes; last is largest).
    #[serde(default)]
    pub(crate) photo: Option<Vec<PhotoSize>>,

    /// Document attachment.
    pub(crate) document: Option<TelegramDocument>,

    /// Audio attachment.
    pub(crate) audio: Option<TelegramAudio>,

    /// Video attachment.
    pub(crate) video: Option<TelegramVideo>,

    /// Voice message.
    pub(crate) voice: Option<TelegramVoice>,

    /// Sticker.
    pub(crate) sticker: Option<TelegramSticker>,
}

/// Telegram PhotoSize object.
#[derive(Debug, Deserialize)]
pub(crate) struct PhotoSize {
    pub(crate) file_id: String,
    pub(crate) file_unique_id: String,
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) file_size: Option<i64>,
}

/// Telegram Document object.
#[derive(Debug, Deserialize)]
pub(crate) struct TelegramDocument {
    pub(crate) file_id: String,
    pub(crate) file_unique_id: String,
    pub(crate) file_name: Option<String>,
    pub(crate) mime_type: Option<String>,
    pub(crate) file_size: Option<i64>,
}

/// Telegram Audio object.
#[derive(Debug, Deserialize)]
pub(crate) struct TelegramAudio {
    pub(crate) file_id: String,
    pub(crate) file_unique_id: String,
    pub(crate) duration: Option<u32>,
    pub(crate) file_name: Option<String>,
    pub(crate) mime_type: Option<String>,
    pub(crate) file_size: Option<i64>,
}

/// Telegram Video object.
#[derive(Debug, Deserialize)]
pub(crate) struct TelegramVideo {
    pub(crate) file_id: String,
    pub(crate) file_unique_id: String,
    pub(crate) duration: Option<u32>,
    pub(crate) file_name: Option<String>,
    pub(crate) mime_type: Option<String>,
    pub(crate) file_size: Option<i64>,
}

/// Telegram Voice message object.
#[derive(Debug, Deserialize)]
pub(crate) struct TelegramVoice {
    pub(crate) file_id: String,
    pub(crate) file_unique_id: String,
    pub(crate) duration: u32,
    pub(crate) mime_type: Option<String>,
    pub(crate) file_size: Option<i64>,
}

/// Telegram Sticker object.
#[derive(Debug, Deserialize)]
pub(crate) struct TelegramSticker {
    pub(crate) file_id: String,
    pub(crate) file_unique_id: String,
    #[serde(rename = "type")]
    pub(crate) sticker_type: Option<String>,
    pub(crate) file_size: Option<i64>,
}

/// Telegram User object.
/// https://core.telegram.org/bots/api#user
#[derive(Debug, Deserialize)]
pub(crate) struct TelegramUser {
    /// Unique user identifier.
    pub(crate) id: i64,

    /// True if this is a bot.
    pub(crate) is_bot: bool,

    /// User's first name.
    pub(crate) first_name: String,

    /// User's last name.
    pub(crate) last_name: Option<String>,

    /// Username (without @).
    pub(crate) username: Option<String>,
}

/// Telegram Chat object.
/// https://core.telegram.org/bots/api#chat
#[derive(Debug, Deserialize)]
pub(crate) struct TelegramChat {
    /// Unique chat identifier.
    pub(crate) id: i64,

    /// Type of chat: private, group, supergroup, or channel.
    #[serde(rename = "type")]
    pub(crate) chat_type: String,

    /// Title for groups/channels.
    pub(crate) title: Option<String>,

    /// Username for private chats.
    pub(crate) username: Option<String>,
}

/// Message entity (for parsing @mentions, commands, etc.).
/// https://core.telegram.org/bots/api#messageentity
#[derive(Debug, Deserialize)]
pub(crate) struct MessageEntity {
    /// Type: mention, bot_command, etc.
    #[serde(rename = "type")]
    pub(crate) entity_type: String,

    /// Offset in UTF-16 code units.
    pub(crate) offset: i64,

    /// Length in UTF-16 code units.
    pub(crate) length: i64,

    /// For "mention" type, the mentioned user.
    pub(crate) user: Option<TelegramUser>,
}

/// Telegram File object returned by getFile.
/// https://core.telegram.org/bots/api#file
#[derive(Debug, Deserialize)]
pub(crate) struct TelegramFile {
    /// Identifier for this file.
    #[allow(dead_code)]
    pub(crate) file_id: String,

    /// File path for downloading. Use https://api.telegram.org/file/bot<token>/<file_path>.
    pub(crate) file_path: Option<String>,
}

/// Telegram API response wrapper.
#[derive(Debug, Deserialize)]
pub(crate) struct TelegramApiResponse<T> {
    /// True if the request was successful.
    pub(crate) ok: bool,

    /// Error description if not ok.
    pub(crate) description: Option<String>,

    /// Result on success.
    pub(crate) result: Option<T>,
}

/// Response from sendMessage.
#[derive(Debug, Deserialize)]
pub(crate) struct SentMessage {
    pub(crate) message_id: i64,
}

/// Metadata stored with emitted messages for response routing.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct TelegramMessageMetadata {
    /// Chat ID where the message was received.
    pub(crate) chat_id: i64,

    /// Original message ID (for reply_to_message_id).
    pub(crate) message_id: i64,

    /// User ID who sent the message.
    pub(crate) user_id: i64,

    /// Whether this is a private (DM) chat.
    pub(crate) is_private: bool,
}

/// Channel configuration injected by host.
///
/// The host injects runtime values like tunnel_url and webhook_secret.
/// The channel doesn't need to know about polling vs webhook mode - it just
/// checks if tunnel_url is set to determine behaviour.
#[derive(Debug, Deserialize)]
pub(crate) struct TelegramConfig {
    /// Bot username (without @) for mention detection in groups.
    #[serde(default)]
    pub(crate) bot_username: Option<String>,

    /// Telegram user ID of the bot owner. When set, only messages from this
    /// user are processed. All others are silently dropped.
    #[serde(default)]
    pub(crate) owner_id: Option<i64>,

    /// DM policy: "pairing" (default), "allowlist", or "open".
    #[serde(default)]
    pub(crate) dm_policy: Option<String>,

    /// Allowed sender IDs/usernames from config (merged with pairing-approved store).
    #[serde(default)]
    pub(crate) allow_from: Option<Vec<String>>,

    /// Whether to respond to all group messages (not just mentions).
    #[serde(default)]
    pub(crate) respond_to_all_group_messages: bool,

    /// Public tunnel URL for webhook mode (injected by host from global settings).
    /// When set, webhook mode is enabled and polling is disabled.
    #[serde(default)]
    pub(crate) tunnel_url: Option<String>,

    /// Secret token for webhook validation (injected by host from secrets store).
    /// Telegram will include this in the X-Telegram-Bot-Api-Secret-Token header.
    #[serde(default)]
    pub(crate) webhook_secret: Option<String>,

    /// When true, use polling mode even if tunnel_url is available.
    #[serde(default)]
    pub(crate) polling_enabled: bool,
}

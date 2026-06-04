use crate::near::agent::channel_host::InboundAttachment;
use crate::send::percent_encode;
use crate::types::{
    PhotoSize, TelegramAudio, TelegramDocument, TelegramMessage, TelegramSticker, TelegramVideo,
    TelegramVoice,
};

/// Build extras-json with optional duration.
pub(crate) fn extras_json(duration_secs: Option<u32>) -> String {
    match duration_secs {
        Some(d) => format!(r#"{{"duration_secs":{}}}"#, d),
        None => String::new(),
    }
}

/// Field values used to construct an inbound attachment.
struct InboundAttachmentParts {
    id: String,
    mime_type: String,
    filename: Option<String>,
    size_bytes: Option<u64>,
    source_url: Option<String>,
    extracted_text: Option<String>,
    duration_secs: Option<u32>,
}

/// Build an inbound attachment with the standard fields.
fn make_inbound_attachment(parts: InboundAttachmentParts) -> InboundAttachment {
    InboundAttachment {
        id: parts.id,
        mime_type: parts.mime_type,
        filename: parts.filename,
        size_bytes: parts.size_bytes,
        source_url: parts.source_url,
        storage_key: None,
        extracted_text: parts.extracted_text,
        extras_json: extras_json(parts.duration_secs),
    }
}

/// The variable fields extracted from a Telegram media object.
///
/// Shared by all per-media attachment helpers; the common fields
/// (`source_url`, `storage_key`, `extracted_text`) are computed once
/// inside [`attachment_from_spec`].
struct MediaSpec {
    file_id: String,
    mime_type: String,
    filename: Option<String>,
    /// Raw `file_size` field from the Telegram object (signed because the
    /// Telegram API returns `Integer`, which may be signed in practice).
    file_size: Option<i64>,
    duration_secs: Option<u32>,
}

/// Borrowed reference to one Telegram media payload that can become an
/// inbound attachment.
enum TelegramMediaRef<'a> {
    Photo(&'a PhotoSize),
    Document(&'a TelegramDocument),
    Audio(&'a TelegramAudio),
    Video(&'a TelegramVideo),
    Voice(&'a TelegramVoice),
    Sticker(&'a TelegramSticker),
}

impl TelegramMediaRef<'_> {
    fn file_id(&self) -> &str {
        match self {
            Self::Photo(media) => &media.file_id,
            Self::Document(media) => &media.file_id,
            Self::Audio(media) => &media.file_id,
            Self::Video(media) => &media.file_id,
            Self::Voice(media) => &media.file_id,
            Self::Sticker(media) => &media.file_id,
        }
    }

    fn mime_type(&self) -> String {
        match self {
            Self::Photo(_) => "image/jpeg".to_string(),
            Self::Document(media) => resolve_mime(&media.mime_type, "application/octet-stream"),
            Self::Audio(media) => resolve_mime(&media.mime_type, "audio/mpeg"),
            Self::Video(media) => resolve_mime(&media.mime_type, "video/mp4"),
            Self::Voice(media) => resolve_mime(&media.mime_type, "audio/ogg"),
            Self::Sticker(_) => "image/webp".to_string(),
        }
    }

    fn filename(&self) -> Option<String> {
        match self {
            Self::Photo(_) | Self::Sticker(_) => None,
            Self::Document(media) => media.file_name.clone(),
            Self::Audio(media) => media.file_name.clone(),
            Self::Video(media) => media.file_name.clone(),
            Self::Voice(media) => Some(format!("voice_{}.ogg", media.file_id)),
        }
    }

    fn file_size(&self) -> Option<i64> {
        match self {
            Self::Photo(media) => media.file_size,
            Self::Document(media) => media.file_size,
            Self::Audio(media) => media.file_size,
            Self::Video(media) => media.file_size,
            Self::Voice(media) => media.file_size,
            Self::Sticker(media) => media.file_size,
        }
    }

    fn duration_secs(&self) -> Option<u32> {
        match self {
            Self::Audio(media) => media.duration,
            Self::Video(media) => media.duration,
            Self::Voice(media) => Some(media.duration),
            Self::Photo(_) | Self::Document(_) | Self::Sticker(_) => None,
        }
    }
}

fn telegram_media_sources(message: &TelegramMessage) -> Vec<TelegramMediaRef<'_>> {
    let mut sources = Vec::new();

    sources.extend(
        message
            .photo
            .as_ref()
            .and_then(|photos| photos.last())
            .map(TelegramMediaRef::Photo),
    );
    sources.extend(message.document.as_ref().map(TelegramMediaRef::Document));
    sources.extend(message.audio.as_ref().map(TelegramMediaRef::Audio));
    sources.extend(message.video.as_ref().map(TelegramMediaRef::Video));
    sources.extend(message.voice.as_ref().map(TelegramMediaRef::Voice));
    sources.extend(message.sticker.as_ref().map(TelegramMediaRef::Sticker));

    sources
}

fn media_spec_from_ref(media: TelegramMediaRef<'_>) -> MediaSpec {
    MediaSpec {
        file_id: media.file_id().to_string(),
        mime_type: media.mime_type(),
        filename: media.filename(),
        file_size: media.file_size(),
        duration_secs: media.duration_secs(),
    }
}

/// Converts a [`MediaSpec`] into an [`InboundAttachment`], computing the
/// source URL via `get_file_url` and hard-coding the fields that are always
/// `None` for Telegram media (`storage_key`, `extracted_text`).
fn attachment_from_spec(
    spec: MediaSpec,
    get_file_url: &impl Fn(&str) -> String,
) -> InboundAttachment {
    make_inbound_attachment(InboundAttachmentParts {
        id: spec.file_id.clone(),
        mime_type: spec.mime_type,
        filename: spec.filename,
        size_bytes: spec.file_size.map(|s| s as u64),
        source_url: Some(get_file_url(&spec.file_id)),
        extracted_text: None,
        duration_secs: spec.duration_secs,
    })
}

/// Returns `mime.clone()` when present, falling back to `default`.
fn resolve_mime(mime: &Option<String>, default: &str) -> String {
    mime.clone().unwrap_or_else(|| default.to_string())
}

/// Extract attachments from a Telegram message.
pub(crate) fn extract_attachments(message: &TelegramMessage) -> Vec<InboundAttachment> {
    let get_file_url = |file_id: &str| {
        format!(
            "https://api.telegram.org/bot{{TELEGRAM_BOT_TOKEN}}/getFile?file_id={}",
            percent_encode(file_id)
        )
    };

    telegram_media_sources(message)
        .into_iter()
        .map(media_spec_from_ref)
        .map(|spec| attachment_from_spec(spec, &get_file_url))
        .collect()
}

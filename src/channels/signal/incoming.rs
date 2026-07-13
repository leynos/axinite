//! Inbound envelope processing for the Signal channel: sender resolution,
//! reply-target derivation, deterministic thread identifiers, and conversion
//! of SSE envelopes into `IncomingMessage`s.

use uuid::Uuid;

use crate::channels::IncomingMessage;

use super::{DataMessage, Envelope, GROUP_TARGET_PREFIX, SignalChannel};

impl SignalChannel {
    /// Effective sender: prefer `sourceNumber` (E.164), fall back to `source`
    /// (UUID for privacy-enabled users).
    pub(super) fn sender(envelope: &Envelope) -> Option<String> {
        envelope
            .source_number
            .as_deref()
            .or(envelope.source.as_deref())
            .map(String::from)
    }

    /// Generate a deterministic UUID from an identifier (phone number or group ID).
    ///
    /// This ensures that the same phone number or group always produces the same UUID,
    /// allowing conversation history to persist across gateway restarts.
    pub(super) fn thread_id_from_identifier(identifier: &str) -> String {
        // Use a stable, deterministic UUID v5 derived from the identifier.
        // This avoids relying on `DefaultHasher` implementation details and
        // provides a full 128 bits of entropy.
        Uuid::new_v5(&Uuid::NAMESPACE_URL, identifier.as_bytes()).to_string()
    }

    /// Determine the reply target: group id (prefixed) or the sender's identifier.
    pub(super) fn reply_target(data_msg: &DataMessage, sender: &str) -> String {
        if let Some(group_id) = data_msg
            .group_info
            .as_ref()
            .and_then(|g| g.group_id.as_deref())
        {
            format!("{GROUP_TARGET_PREFIX}{group_id}")
        } else {
            sender.to_string()
        }
    }

    /// Process a single SSE envelope, returning an `IncomingMessage` if valid.
    pub(super) fn process_envelope(
        &self,
        envelope: &Envelope,
    ) -> Option<(IncomingMessage, String)> {
        // Skip story messages when configured.
        if self.config.ignore_stories && envelope.story_message.is_some() {
            tracing::debug!("Signal: dropping story message");
            return None;
        }

        let data_msg = envelope.data_message.as_ref()?;

        // Skip attachment-only messages when configured.
        let has_attachments = data_msg.attachments.as_ref().is_some_and(|a| !a.is_empty());
        let has_message_text = data_msg.message.as_ref().is_some_and(|m| !m.is_empty());
        if self.should_drop_attachment_only(has_attachments, has_message_text) {
            tracing::debug!("Signal: dropping attachment-only message");
            return None;
        }

        // Use message text, or fall back to "[Attachment]" for attachment-only messages
        // when ignore_attachments is false. This ensures attachment-only messages are
        // still processed when the user wants them (rather than always being dropped).
        let text = data_msg
            .message
            .as_deref()
            .filter(|t| !t.is_empty())
            .map(String::from)
            .or_else(|| has_attachments.then(|| "[Attachment]".to_string()))?;
        let sender = Self::sender(envelope)?;

        // Log sender info including UUID if available
        tracing::debug!(
            sender = %sender,
            uuid = ?envelope.source_uuid,
            "Signal: received message"
        );

        // Check if this is a group message
        let is_group = data_msg
            .group_info
            .as_ref()
            .and_then(|g| g.group_id.as_deref())
            .is_some();

        // Apply group policy first (before DM policy for group messages)
        let allowed = if is_group {
            self.group_message_allowed(data_msg, &sender)
        } else {
            // DM message - apply DM policy
            self.dm_message_allowed(&sender, envelope)
        };
        if !allowed {
            return None;
        }

        let target = Self::reply_target(data_msg, &sender);

        let timestamp = data_msg
            .timestamp
            .or(envelope.timestamp)
            .unwrap_or_else(|| {
                u64::try_from(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis(),
                )
                .unwrap_or(u64::MAX)
            });

        // Build metadata with signal-specific routing info.
        let sender_uuid = envelope.source_uuid.as_deref();
        let metadata = serde_json::json!({
            "signal_sender": &sender,
            "signal_sender_uuid": sender_uuid,
            "signal_target": &target,
            "signal_timestamp": timestamp,
        });

        let mut msg = IncomingMessage::new("signal", &sender, text).with_metadata(metadata);

        // Use sourceName as display name if available.
        if let Some(ref name) = envelope.source_name
            && !name.is_empty()
        {
            msg = msg.with_user_name(name);
        }

        // Use a deterministic UUID as thread_id for all conversations.
        // This ensures DMs and groups continue the same thread AND work with
        // maybe_hydrate_thread, enabling conversation history persistence.
        // Priority: source_uuid > generated UUID from phone/group
        if data_msg.group_info.is_some() {
            // For groups, use the group ID to generate a deterministic UUID
            msg = msg.with_thread(Self::thread_id_from_identifier(&target));
        } else if let Some(ref uuid) = envelope.source_uuid {
            // Privacy mode users already have a UUID
            msg = msg.with_thread(uuid.clone());
        } else {
            // For regular DMs, generate a deterministic UUID from the phone number
            msg = msg.with_thread(Self::thread_id_from_identifier(&sender));
        }

        Some((msg, target))
    }
}

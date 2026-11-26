//! The chat message a WebSocket client sends, and its conversion for the agent.

use crate::channels::IncomingMessage;

/// The fields of a client chat message.
pub(super) struct ChatMessage {
    pub(super) content: String,
    pub(super) thread_id: Option<String>,
    pub(super) timezone: Option<String>,
    pub(super) images: Vec<crate::channels::web::types::ImageData>,
}

impl ChatMessage {
    /// Build the agent-bound message, with timezone, thread and image attachments.
    pub(super) fn into_incoming(self, user_id: &str) -> IncomingMessage {
        let mut incoming = IncomingMessage::new("gateway", user_id, &self.content);
        if let Some(ref tz) = self.timezone {
            incoming = incoming.with_timezone(tz);
        }
        if let Some(ref tid) = self.thread_id {
            incoming = incoming.with_thread(tid);
        }

        // Convert uploaded images to IncomingAttachments
        if !self.images.is_empty() {
            let attachments =
                crate::channels::web::handlers::chat::images_to_attachments(&self.images);
            incoming = incoming.with_attachments(attachments);
        }
        incoming
    }
}

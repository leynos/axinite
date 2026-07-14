//! Context types for approval flows: message environment, turn scope, and
//! approval parameters.

use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::session::Session;
use crate::channels::IncomingMessage;

/// Message environment context.
#[derive(Clone)]
pub(crate) struct MsgEnv {
    pub(super) channel: String,
    pub(super) user_id: String,
    pub(super) metadata: serde_json::Value,
    pub(super) timezone: Option<String>,
    pub(super) content: String,
}

impl From<&IncomingMessage> for MsgEnv {
    fn from(m: &IncomingMessage) -> Self {
        Self {
            channel: m.channel.clone(),
            user_id: m.user_id.clone(),
            metadata: m.metadata.clone(),
            timezone: m.timezone.clone(),
            content: m.content.clone(),
        }
    }
}

/// Turn scope context bundling session, thread, and message environment.
#[derive(Clone)]
pub(crate) struct TurnScope {
    pub(super) session: Arc<Mutex<Session>>,
    pub(super) thread_id: Uuid,
    pub(super) env: MsgEnv,
}

impl TurnScope {
    pub(crate) fn new(
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
        message: &IncomingMessage,
    ) -> Self {
        Self {
            session,
            thread_id,
            env: MsgEnv::from(message),
        }
    }

    /// Create a mock IncomingMessage from the environment for use with
    /// functions that require the full message type.
    pub(super) fn to_message(&self) -> IncomingMessage {
        IncomingMessage {
            id: uuid::Uuid::new_v4(),
            channel: self.env.channel.clone(),
            user_id: self.env.user_id.clone(),
            user_name: None,
            content: self.env.content.clone(),
            thread_id: None,
            received_at: chrono::Utc::now(),
            metadata: self.env.metadata.clone(),
            attachments: vec![],
            timezone: self.env.timezone.clone(),
        }
    }
}

/// Approval parameters.
#[derive(Clone, Copy)]
pub(crate) struct ApprovalParams {
    pub(crate) request_id: Option<Uuid>,
    pub(crate) approved: bool,
    pub(crate) always: bool,
}

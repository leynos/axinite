//! Channel trait and message types.

mod messages;

use core::future::Future;
use std::collections::HashMap;
use std::pin::Pin;

use crate::error::ChannelError;

pub use messages::{
    AttachmentKind, IncomingAttachment, IncomingMessage, MessageStream, OutgoingResponse,
    StatusUpdate,
};

/// Boxed future used at the dyn `Channel` boundary.
pub type ChannelFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Trait for message channels.
///
/// This is the dyn-safe object boundary. Concrete implementations should
/// implement [`NativeChannel`] instead; the blanket adapter provides this
/// trait automatically.
///
/// Channels receive messages from external sources and convert them to
/// a unified format. They also handle sending responses back.
pub trait Channel: Send + Sync {
    /// Get the channel name (e.g., "cli", "slack", "telegram", "http").
    fn name(&self) -> &str;

    /// Start listening for messages.
    ///
    /// Returns a stream of incoming messages. The channel should handle
    /// reconnection and error recovery internally.
    fn start<'a>(&'a self) -> ChannelFuture<'a, Result<MessageStream, ChannelError>>;

    /// Send a response back to the user.
    ///
    /// The response is sent in the context of the original message
    /// (same channel, same thread if applicable).
    fn respond<'a>(
        &'a self,
        msg: &'a IncomingMessage,
        response: OutgoingResponse,
    ) -> ChannelFuture<'a, Result<(), ChannelError>>;

    /// Send a status update (thinking, tool execution, etc.).
    ///
    /// The metadata contains channel-specific routing info (e.g., Telegram chat_id)
    /// needed to deliver the status to the correct destination.
    ///
    /// Default implementation does nothing (for channels that don't support status).
    fn send_status<'a>(
        &'a self,
        _status: StatusUpdate,
        _metadata: &'a serde_json::Value,
    ) -> ChannelFuture<'a, Result<(), ChannelError>> {
        Box::pin(async { Ok(()) })
    }

    /// Send a proactive message without a prior incoming message.
    ///
    /// Used for alerts, heartbeat notifications, and other agent-initiated communication.
    /// The user_id helps target a specific user within the channel.
    ///
    /// Default implementation does nothing (for channels that don't support broadcast).
    fn broadcast<'a>(
        &'a self,
        _user_id: &'a str,
        _response: OutgoingResponse,
    ) -> ChannelFuture<'a, Result<(), ChannelError>> {
        Box::pin(async { Ok(()) })
    }

    /// Check if the channel is healthy.
    fn health_check<'a>(&'a self) -> ChannelFuture<'a, Result<(), ChannelError>>;

    /// Get conversation context from message metadata for system prompt.
    ///
    /// Returns key-value pairs like "sender", "sender_uuid", "group" that
    /// help the LLM understand who it's talking to.
    ///
    /// Default implementation returns empty map.
    fn conversation_context(&self, _metadata: &serde_json::Value) -> HashMap<String, String> {
        HashMap::new()
    }

    /// Gracefully shut down the channel.
    fn shutdown<'a>(&'a self) -> ChannelFuture<'a, Result<(), ChannelError>>;
}

/// Native (non-dyn) sibling of [`Channel`] for concrete implementations.
///
/// Implement this trait instead of [`Channel`] directly. The blanket adapter
/// below automatically implements [`Channel`] for every `T: NativeChannel`.
pub trait NativeChannel: Send + Sync {
    /// Get the channel name (e.g., "cli", "slack", "telegram", "http").
    fn name(&self) -> &str;

    /// Start listening for messages.
    fn start(&self) -> impl Future<Output = Result<MessageStream, ChannelError>> + Send + '_;

    /// Send a response back to the user.
    fn respond<'a>(
        &'a self,
        msg: &'a IncomingMessage,
        response: OutgoingResponse,
    ) -> impl Future<Output = Result<(), ChannelError>> + Send + 'a;

    /// Send a status update (thinking, tool execution, etc.).
    ///
    /// Default implementation does nothing (for channels that don't support status).
    fn send_status<'a>(
        &'a self,
        _status: StatusUpdate,
        _metadata: &'a serde_json::Value,
    ) -> impl Future<Output = Result<(), ChannelError>> + Send + 'a {
        async { Ok(()) }
    }

    /// Send a proactive message without a prior incoming message.
    ///
    /// Default implementation does nothing (for channels that don't support broadcast).
    fn broadcast<'a>(
        &'a self,
        _user_id: &'a str,
        _response: OutgoingResponse,
    ) -> impl Future<Output = Result<(), ChannelError>> + Send + 'a {
        async { Ok(()) }
    }

    /// Check if the channel is healthy.
    fn health_check(&self) -> impl Future<Output = Result<(), ChannelError>> + Send + '_;

    /// Get conversation context from message metadata for system prompt.
    ///
    /// Default implementation returns empty map.
    fn conversation_context(&self, _metadata: &serde_json::Value) -> HashMap<String, String> {
        HashMap::new()
    }

    /// Gracefully shut down the channel.
    fn shutdown(&self) -> impl Future<Output = Result<(), ChannelError>> + Send + '_ {
        async { Ok(()) }
    }
}

impl<T: NativeChannel> Channel for T {
    fn name(&self) -> &str {
        NativeChannel::name(self)
    }

    fn start<'a>(&'a self) -> ChannelFuture<'a, Result<MessageStream, ChannelError>> {
        Box::pin(NativeChannel::start(self))
    }

    fn respond<'a>(
        &'a self,
        msg: &'a IncomingMessage,
        response: OutgoingResponse,
    ) -> ChannelFuture<'a, Result<(), ChannelError>> {
        Box::pin(NativeChannel::respond(self, msg, response))
    }

    fn send_status<'a>(
        &'a self,
        status: StatusUpdate,
        metadata: &'a serde_json::Value,
    ) -> ChannelFuture<'a, Result<(), ChannelError>> {
        Box::pin(NativeChannel::send_status(self, status, metadata))
    }

    fn broadcast<'a>(
        &'a self,
        user_id: &'a str,
        response: OutgoingResponse,
    ) -> ChannelFuture<'a, Result<(), ChannelError>> {
        Box::pin(NativeChannel::broadcast(self, user_id, response))
    }

    fn health_check<'a>(&'a self) -> ChannelFuture<'a, Result<(), ChannelError>> {
        Box::pin(NativeChannel::health_check(self))
    }

    fn conversation_context(&self, metadata: &serde_json::Value) -> HashMap<String, String> {
        NativeChannel::conversation_context(self, metadata)
    }

    fn shutdown<'a>(&'a self) -> ChannelFuture<'a, Result<(), ChannelError>> {
        Box::pin(NativeChannel::shutdown(self))
    }
}

/// Boxed future used at the dyn channel-secret-updater boundary.
pub type ChannelSecretUpdaterFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

/// Trait for channels that support hot-secret-swapping during SIGHUP reload.
///
/// This allows channels to update authentication credentials without restarting,
/// enabling zero-downtime configuration reloads. Channels that don't support
/// secret updates can simply not implement this trait.
pub trait ChannelSecretUpdater: Send + Sync {
    /// Update the secret for this channel.
    ///
    /// Called during SIGHUP configuration reload. Implementation should:
    /// - Apply the new secret atomically
    /// - Not fail the entire reload if secret update fails
    /// - Log appropriate errors/info messages
    ///
    /// The secret is optional (may be None if secret is no longer configured).
    fn update_secret<'a>(
        &'a self,
        new_secret: Option<secrecy::SecretString>,
    ) -> ChannelSecretUpdaterFuture<'a>;
}

/// Native async sibling trait for concrete channel-secret-updater implementations.
pub trait NativeChannelSecretUpdater: Send + Sync {
    /// See [`ChannelSecretUpdater::update_secret`].
    fn update_secret(
        &self,
        new_secret: Option<secrecy::SecretString>,
    ) -> impl Future<Output = ()> + Send + '_;
}

impl<T> ChannelSecretUpdater for T
where
    T: NativeChannelSecretUpdater + Send + Sync,
{
    fn update_secret<'a>(
        &'a self,
        new_secret: Option<secrecy::SecretString>,
    ) -> ChannelSecretUpdaterFuture<'a> {
        Box::pin(NativeChannelSecretUpdater::update_secret(self, new_secret))
    }
}

#[cfg(test)]
mod tests;

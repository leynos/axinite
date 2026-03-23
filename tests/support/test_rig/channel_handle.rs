//! Test rig channel adapter that exposes [`TestChannel`] through the runtime
//! [`NativeChannel`] adapter.
//!
//! This support module lets integration tests share one in-memory
//! [`TestChannel`] while still handing a trait object to the production
//! channel manager.

use std::collections::HashMap;
use std::sync::Arc;

use ironclaw::channels::{
    IncomingMessage, MessageStream, NativeChannel, OutgoingResponse, StatusUpdate,
};
use ironclaw::error::ChannelError;

use crate::support::test_channel::TestChannel;

/// A thin wrapper around `Arc<TestChannel>` that implements `Channel` for tests.
///
/// `TestChannelHandle` lets the test rig pass a channel trait object into the
/// runtime while preserving shared access to the underlying `TestChannel` for
/// assertions.
pub struct TestChannelHandle {
    inner: Arc<TestChannel>,
}

impl TestChannelHandle {
    /// Construct a `TestChannelHandle` from a shared `Arc<TestChannel>`.
    pub fn new(inner: Arc<TestChannel>) -> Self {
        Self { inner }
    }
}

impl NativeChannel for TestChannelHandle {
    fn name(&self) -> &str {
        self.inner.name()
    }

    async fn start(&self) -> Result<MessageStream, ChannelError> {
        self.inner.start().await
    }

    async fn respond(
        &self,
        msg: &IncomingMessage,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        self.inner.respond(msg, response).await
    }

    async fn send_status(
        &self,
        status: StatusUpdate,
        metadata: &serde_json::Value,
    ) -> Result<(), ChannelError> {
        self.inner.send_status(status, metadata).await
    }

    async fn broadcast(
        &self,
        user_id: &str,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        self.inner.broadcast(user_id, response).await
    }

    async fn health_check(&self) -> Result<(), ChannelError> {
        self.inner.health_check().await
    }

    fn conversation_context(&self, metadata: &serde_json::Value) -> HashMap<String, String> {
        self.inner.conversation_context(metadata)
    }

    async fn shutdown(&self) -> Result<(), ChannelError> {
        self.inner.shutdown().await
    }
}

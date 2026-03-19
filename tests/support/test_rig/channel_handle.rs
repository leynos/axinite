use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use ironclaw::channels::{Channel, IncomingMessage, MessageStream, OutgoingResponse, StatusUpdate};
use ironclaw::error::ChannelError;

use crate::support::test_channel::TestChannel;

/// A thin wrapper around `Arc<TestChannel>` that implements `Channel`.
///
/// This lets the test rig hand a `Box<dyn Channel>` to `ChannelManager::add()`
/// while still keeping a shared `Arc<TestChannel>` for assertions.
pub struct TestChannelHandle {
    inner: Arc<TestChannel>,
}

impl TestChannelHandle {
    pub fn new(inner: Arc<TestChannel>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl Channel for TestChannelHandle {
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

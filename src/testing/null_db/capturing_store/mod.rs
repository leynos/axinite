//! Capturing database wrapper for tests.
//!
//! Provides a [`CapturingStore`] that wraps [`NullDatabase`] and captures
//! specific method calls for test assertions.

use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::context::JobState;
use crate::db::SandboxEventType;

use super::NullDatabase;

mod delegation;

/// Captured status update call.
#[derive(Debug, Clone)]
pub struct StatusCall {
    /// The job status that was recorded.
    pub status: JobState,
    /// Optional failure reason associated with the status.
    pub reason: Option<String>,
}

/// Captured job event call.
#[derive(Debug, Clone)]
pub struct EventCall {
    /// The event type string (e.g., "result").
    pub event_type: String,
    /// The JSON data payload associated with the event.
    pub data: serde_json::Value,
}

/// Thread-safe storage for captured calls.
#[derive(Debug, Default)]
pub struct Calls {
    /// The last status update call captured, if any.
    pub last_status: Mutex<Option<StatusCall>>,
    /// The last event call captured, if any.
    pub last_event: Mutex<Option<EventCall>>,
}

impl Calls {
    /// Create a new empty Calls container.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a status update call.
    ///
    /// The job ID parameter is accepted for API compatibility but is intentionally
    /// discarded. Only the most recent call is retained in `last_status`.
    /// Per-job tracking is not implemented for this null test store to keep the
    /// implementation simple; future extensions can use the ID to scope calls.
    pub async fn record_status(&self, _id: Uuid, status: JobState, reason: Option<&str>) {
        *self.last_status.lock().await = Some(StatusCall {
            status,
            reason: reason.map(ToOwned::to_owned),
        });
    }

    /// Record an event call.
    ///
    /// The job ID parameter is accepted for API compatibility but is intentionally
    /// discarded. Only the most recent call is retained in `last_event`.
    /// Per-job tracking is not implemented for this null test store to keep the
    /// implementation simple; future extensions can use the ID to scope calls.
    pub async fn record_event(
        &self,
        _job_id: Uuid,
        event_type: SandboxEventType,
        data: &serde_json::Value,
    ) {
        *self.last_event.lock().await = Some(EventCall {
            event_type: event_type.as_str().to_string(),
            data: data.clone(),
        });
    }
}

/// A database wrapper that captures calls to specific methods for testing.
///
/// Delegates all other methods to the inner [`NullDatabase`].
#[derive(Debug)]
pub struct CapturingStore {
    pub(crate) inner: NullDatabase,
    calls: Arc<Calls>,
}

impl CapturingStore {
    /// Create a new capturing store with an inner NullDatabase.
    pub fn new() -> Self {
        Self {
            inner: NullDatabase::new(),
            calls: Arc::new(Calls::new()),
        }
    }

    /// Access the captured calls for assertions.
    pub fn calls(&self) -> &Arc<Calls> {
        &self.calls
    }
}

impl Default for CapturingStore {
    fn default() -> Self {
        Self::new()
    }
}

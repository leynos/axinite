//! Capturing database wrapper for tests.
//!
//! Provides a [`CapturingStore`] that wraps [`NullDatabase`] and captures
//! specific method calls for test assertions.
//!
//! Captured calls include job IDs via [`StatusCallWithId`] and [`EventCallWithId`]
//! in the `status_history` and `event_history` collections, while [`StatusCall`]
//! and [`EventCall`] provide the simpler view without IDs in `last_status` and
//! `last_event`.

use std::sync::Arc;
use std::sync::Mutex as SyncMutex;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::context::JobState;
use crate::db::SandboxEventType;
use crate::error::DatabaseError;

use super::NullDatabase;

mod delegation;
mod delegation_workspace;

/// Captured status update call.
#[derive(Debug, Clone)]
pub struct StatusCall {
    /// The job status that was recorded.
    pub status: JobState,
    /// Optional failure reason associated with the status.
    pub reason: Option<String>,
}

/// Captured status update call with job ID.
#[derive(Debug, Clone)]
pub struct StatusCallWithId {
    /// The job ID associated with this status update.
    pub job_id: Uuid,
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

/// Captured job event call with job ID.
#[derive(Debug, Clone)]
pub struct EventCallWithId {
    /// The job ID associated with this event.
    pub job_id: Uuid,
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
    /// Full history of all status calls with job IDs.
    pub status_history: Mutex<Vec<StatusCallWithId>>,
    /// Full history of all event calls with job IDs.
    pub event_history: Mutex<Vec<EventCallWithId>>,
    /// Tool names passed to `mark_tool_repaired`.
    pub repaired_tools: Mutex<Vec<String>>,
}

impl Calls {
    /// Create a new empty Calls container.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a status update call.
    ///
    /// The call is stored in both `last_status` (overwriting previous)
    /// and appended to `status_history` with the job ID for tests that need
    /// to verify call counts or per-job tracking.
    pub async fn record_status(&self, job_id: Uuid, status: JobState, reason: Option<&str>) {
        let last_call = StatusCall {
            status,
            reason: reason.map(ToOwned::to_owned),
        };
        let history_call = StatusCallWithId {
            job_id,
            status,
            reason: reason.map(ToOwned::to_owned),
        };
        *self.last_status.lock().await = Some(last_call);
        self.status_history.lock().await.push(history_call);
    }

    /// Record an event call.
    ///
    /// The call is stored in both `last_event` (overwriting previous)
    /// and appended to `event_history` with the job ID for tests that need
    /// to verify call counts or per-job tracking.
    pub async fn record_event(
        &self,
        job_id: Uuid,
        event_type: SandboxEventType,
        data: &serde_json::Value,
    ) {
        let last_call = EventCall {
            event_type: event_type.as_str().to_string(),
            data: data.clone(),
        };
        let history_call = EventCallWithId {
            job_id,
            event_type: event_type.as_str().to_string(),
            data: data.clone(),
        };
        *self.last_event.lock().await = Some(last_call);
        self.event_history.lock().await.push(history_call);
    }

    /// Record a repaired-tool marker call.
    pub async fn record_repaired_tool(&self, tool_name: &str) {
        self.repaired_tools.lock().await.push(tool_name.to_string());
    }

    /// Clear all captured call history.
    pub async fn clear(&self) {
        *self.last_status.lock().await = None;
        *self.last_event.lock().await = None;
        self.status_history.lock().await.clear();
        self.event_history.lock().await.clear();
        self.repaired_tools.lock().await.clear();
    }
}

/// A database wrapper that captures calls to specific methods for testing.
///
/// Delegates all other methods to the inner [`NullDatabase`].
///
/// The `last_status` and `last_event` fields store the most recent call
/// (without job ID), while `status_history` and `event_history` maintain
/// full call sequences with job IDs via [`StatusCallWithId`] and
/// [`EventCallWithId`]. This supports tests that need to verify call counts
/// (e.g., duplicate transition rejection) or per-job tracking.
#[derive(Debug)]
pub struct CapturingStore {
    pub(crate) inner: NullDatabase,
    calls: Arc<Calls>,
    mark_repaired_error: SyncMutex<Option<DatabaseError>>,
}

impl CapturingStore {
    /// Create a new capturing store with an inner NullDatabase.
    pub fn new() -> Self {
        Self {
            inner: NullDatabase::new(),
            calls: Arc::new(Calls::new()),
            mark_repaired_error: SyncMutex::new(None),
        }
    }

    /// Create a capturing store that fails `mark_tool_repaired`.
    pub fn failing_mark_tool_repaired(error: DatabaseError) -> Self {
        Self {
            inner: NullDatabase::new(),
            calls: Arc::new(Calls::new()),
            mark_repaired_error: SyncMutex::new(Some(error)),
        }
    }

    /// Access the captured calls for assertions.
    pub fn calls(&self) -> &Arc<Calls> {
        &self.calls
    }

    pub(crate) fn take_mark_repaired_error(&self) -> Option<DatabaseError> {
        self.mark_repaired_error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}

impl Default for CapturingStore {
    fn default() -> Self {
        Self::new()
    }
}

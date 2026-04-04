//! Sandbox job persistence traits.
//!
//! Defines the dyn-safe [`SandboxStore`] and its native-async sibling
//! [`NativeSandboxStore`] for sandbox job lifecycle and event storage.

use core::{fmt, future::Future};

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::db::params::DbFuture;
use crate::db::traits::settings::UserId;
use crate::error::DatabaseError;
use crate::history::{JobEventRecord, SandboxJobRecord, SandboxJobSummary};

/// Supported execution modes for sandbox jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SandboxMode {
    Worker,
    ClaudeCode,
}

impl SandboxMode {
    /// Return the stable storage/API string for this sandbox execution mode.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Worker => "worker",
            Self::ClaudeCode => "claude_code",
        }
    }
}

impl fmt::Display for SandboxMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TryFrom<&str> for SandboxMode {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "worker" => Ok(Self::Worker),
            "claude_code" => Ok(Self::ClaudeCode),
            other => Err(format!("unexpected sandbox mode '{other}'")),
        }
    }
}

/// Strongly typed sandbox job event discriminator.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SandboxEventType(String);

impl SandboxEventType {
    /// Return a borrowed view of the stored event-type string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for SandboxEventType {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl From<String> for SandboxEventType {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl fmt::Display for SandboxEventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl PartialEq<&str> for SandboxEventType {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<String> for SandboxEventType {
    fn eq(&self, other: &String) -> bool {
        self.0 == *other
    }
}

/// Parameters for `update_sandbox_job_status`.
pub struct SandboxJobStatusUpdate<'a> {
    /// Sandbox job UUID to update.
    pub id: Uuid,
    /// New persisted status string.
    pub status: &'a str,
    /// Optional success flag to persist alongside the status.
    pub success: Option<bool>,
    /// Optional failure or result message to persist.
    pub message: Option<&'a str>,
    /// Optional start timestamp for the sandbox job.
    pub started_at: Option<DateTime<Utc>>,
    /// Optional completion timestamp for the sandbox job.
    pub completed_at: Option<DateTime<Utc>>,
}

/// Object-safe persistence surface for sandbox job lifecycle and events.
///
/// This trait provides the dyn-safe boundary for sandbox job storage
/// operations, enabling trait-object usage (e.g., `Arc<dyn SandboxStore>`).
/// It uses boxed futures ([`DbFuture`]) to maintain object safety.
///
/// Companion trait: [`NativeSandboxStore`] provides the same API using native
/// async traits (RPITIT).  A blanket adapter automatically bridges
/// implementations of `NativeSandboxStore` to satisfy this trait.
///
/// Thread-safety: All implementations must be `Send + Sync` to support
/// concurrent access.
pub trait SandboxStore: Send + Sync {
    /// Persist a sandbox job snapshot.
    fn save_sandbox_job<'a>(
        &'a self,
        job: &'a SandboxJobRecord,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    /// Load a sandbox job by UUID.
    fn get_sandbox_job<'a>(
        &'a self,
        id: Uuid,
    ) -> DbFuture<'a, Result<Option<SandboxJobRecord>, DatabaseError>>;
    /// List sandbox jobs in backend-defined recency order.
    fn list_sandbox_jobs<'a>(
        &'a self,
    ) -> DbFuture<'a, Result<Vec<SandboxJobRecord>, DatabaseError>>;
    /// Update mutable execution fields for a sandbox job.
    fn update_sandbox_job_status<'a>(
        &'a self,
        params: SandboxJobStatusUpdate<'a>,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    /// Mark stale in-flight sandbox jobs as interrupted.
    fn cleanup_stale_sandbox_jobs<'a>(&'a self) -> DbFuture<'a, Result<u64, DatabaseError>>;
    /// Summarize sandbox job counts grouped by status.
    fn sandbox_job_summary<'a>(&'a self) -> DbFuture<'a, Result<SandboxJobSummary, DatabaseError>>;
    /// List sandbox jobs strictly owned by `user_id`.
    fn list_sandbox_jobs_for_user<'a>(
        &'a self,
        user_id: UserId,
    ) -> DbFuture<'a, Result<Vec<SandboxJobRecord>, DatabaseError>>;
    /// Summarize sandbox jobs visible to `user_id`.
    fn sandbox_job_summary_for_user<'a>(
        &'a self,
        user_id: UserId,
    ) -> DbFuture<'a, Result<SandboxJobSummary, DatabaseError>>;
    /// Check whether `job_id` is strictly owned by `user_id`.
    fn sandbox_job_belongs_to_user<'a>(
        &'a self,
        job_id: Uuid,
        user_id: UserId,
    ) -> DbFuture<'a, Result<bool, DatabaseError>>;
    /// Persist the sandbox execution mode for `id`.
    fn update_sandbox_job_mode<'a>(
        &'a self,
        id: Uuid,
        mode: SandboxMode,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    /// Load the persisted sandbox execution mode for `id`.
    fn get_sandbox_job_mode<'a>(
        &'a self,
        id: Uuid,
    ) -> DbFuture<'a, Result<Option<SandboxMode>, DatabaseError>>;
    /// Persist a structured sandbox job event with its JSON payload.
    fn save_job_event<'a>(
        &'a self,
        job_id: Uuid,
        event_type: SandboxEventType,
        data: &'a serde_json::Value,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    /// Load job events ordered by ascending id.
    ///
    /// When `before_id` is set, only events with ids strictly smaller than the
    /// cursor are returned. When `limit` is set, at most that many events are
    /// returned. Implementations may page newest-first internally so long as
    /// the returned vector is ordered oldest-to-newest.
    fn list_job_events<'a>(
        &'a self,
        job_id: Uuid,
        before_id: Option<i64>,
        limit: Option<i64>,
    ) -> DbFuture<'a, Result<Vec<JobEventRecord>, DatabaseError>>;
}

/// Native async sibling trait for concrete sandbox-store implementations.
pub trait NativeSandboxStore: Send + Sync {
    /// Native async form of [`SandboxStore::save_sandbox_job`].
    fn save_sandbox_job<'a>(
        &'a self,
        job: &'a SandboxJobRecord,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    /// Native async form of [`SandboxStore::get_sandbox_job`].
    fn get_sandbox_job<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<SandboxJobRecord>, DatabaseError>> + Send + 'a;
    /// Native async form of [`SandboxStore::list_sandbox_jobs`].
    fn list_sandbox_jobs<'a>(
        &'a self,
    ) -> impl Future<Output = Result<Vec<SandboxJobRecord>, DatabaseError>> + Send + 'a;
    /// Native async form of [`SandboxStore::update_sandbox_job_status`].
    fn update_sandbox_job_status<'a>(
        &'a self,
        params: SandboxJobStatusUpdate<'a>,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    /// Native async form of [`SandboxStore::cleanup_stale_sandbox_jobs`].
    fn cleanup_stale_sandbox_jobs<'a>(
        &'a self,
    ) -> impl Future<Output = Result<u64, DatabaseError>> + Send + 'a;
    /// Native async form of [`SandboxStore::sandbox_job_summary`].
    fn sandbox_job_summary<'a>(
        &'a self,
    ) -> impl Future<Output = Result<SandboxJobSummary, DatabaseError>> + Send + 'a;
    /// Native async form of [`SandboxStore::list_sandbox_jobs_for_user`].
    fn list_sandbox_jobs_for_user<'a>(
        &'a self,
        user_id: UserId,
    ) -> impl Future<Output = Result<Vec<SandboxJobRecord>, DatabaseError>> + Send + 'a;
    /// Native async form of [`SandboxStore::sandbox_job_summary_for_user`].
    fn sandbox_job_summary_for_user<'a>(
        &'a self,
        user_id: UserId,
    ) -> impl Future<Output = Result<SandboxJobSummary, DatabaseError>> + Send + 'a;
    /// Native async form of [`SandboxStore::sandbox_job_belongs_to_user`].
    fn sandbox_job_belongs_to_user<'a>(
        &'a self,
        job_id: Uuid,
        user_id: UserId,
    ) -> impl Future<Output = Result<bool, DatabaseError>> + Send + 'a;
    /// Native async form of [`SandboxStore::update_sandbox_job_mode`].
    fn update_sandbox_job_mode<'a>(
        &'a self,
        id: Uuid,
        mode: SandboxMode,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    /// Native async form of [`SandboxStore::get_sandbox_job_mode`].
    fn get_sandbox_job_mode<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<SandboxMode>, DatabaseError>> + Send + 'a;
    /// Native async form of [`SandboxStore::save_job_event`].
    fn save_job_event<'a>(
        &'a self,
        job_id: Uuid,
        event_type: SandboxEventType,
        data: &'a serde_json::Value,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    /// Native async form of [`SandboxStore::list_job_events`].
    fn list_job_events<'a>(
        &'a self,
        job_id: Uuid,
        before_id: Option<i64>,
        limit: Option<i64>,
    ) -> impl Future<Output = Result<Vec<JobEventRecord>, DatabaseError>> + Send + 'a;
}

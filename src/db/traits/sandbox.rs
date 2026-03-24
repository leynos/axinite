//! Sandbox job persistence traits.
//!
//! Defines the dyn-safe [`SandboxStore`] and its native-async sibling
//! [`NativeSandboxStore`] for sandbox job lifecycle and event storage.

use core::future::Future;

use uuid::Uuid;

use crate::db::params::{DbFuture, SandboxJobStatusUpdate};
use crate::error::DatabaseError;
use crate::history::{JobEventRecord, SandboxJobRecord, SandboxJobSummary};

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
    fn save_sandbox_job<'a>(
        &'a self,
        job: &'a SandboxJobRecord,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn get_sandbox_job<'a>(
        &'a self,
        id: Uuid,
    ) -> DbFuture<'a, Result<Option<SandboxJobRecord>, DatabaseError>>;
    fn list_sandbox_jobs<'a>(
        &'a self,
    ) -> DbFuture<'a, Result<Vec<SandboxJobRecord>, DatabaseError>>;
    fn update_sandbox_job_status<'a>(
        &'a self,
        params: SandboxJobStatusUpdate<'a>,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn cleanup_stale_sandbox_jobs<'a>(&'a self) -> DbFuture<'a, Result<u64, DatabaseError>>;
    fn sandbox_job_summary<'a>(&'a self) -> DbFuture<'a, Result<SandboxJobSummary, DatabaseError>>;
    fn list_sandbox_jobs_for_user<'a>(
        &'a self,
        user_id: &'a str,
    ) -> DbFuture<'a, Result<Vec<SandboxJobRecord>, DatabaseError>>;
    fn sandbox_job_summary_for_user<'a>(
        &'a self,
        user_id: &'a str,
    ) -> DbFuture<'a, Result<SandboxJobSummary, DatabaseError>>;
    fn sandbox_job_belongs_to_user<'a>(
        &'a self,
        job_id: Uuid,
        user_id: &'a str,
    ) -> DbFuture<'a, Result<bool, DatabaseError>>;
    fn update_sandbox_job_mode<'a>(
        &'a self,
        id: Uuid,
        mode: &'a str,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn get_sandbox_job_mode<'a>(
        &'a self,
        id: Uuid,
    ) -> DbFuture<'a, Result<Option<String>, DatabaseError>>;
    fn save_job_event<'a>(
        &'a self,
        job_id: Uuid,
        event_type: &'a str,
        data: &'a serde_json::Value,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    /// Load job events ordered by ascending id.
    ///
    /// When `before_id` is set, only events with ids strictly smaller than the
    /// cursor are returned.  When `limit` is set, at most that many events are
    /// returned.
    fn list_job_events<'a>(
        &'a self,
        job_id: Uuid,
        before_id: Option<i64>,
        limit: Option<i64>,
    ) -> DbFuture<'a, Result<Vec<JobEventRecord>, DatabaseError>>;
}

/// Native async sibling trait for concrete sandbox-store implementations.
pub trait NativeSandboxStore: Send + Sync {
    fn save_sandbox_job<'a>(
        &'a self,
        job: &'a SandboxJobRecord,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn get_sandbox_job<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<SandboxJobRecord>, DatabaseError>> + Send + 'a;
    fn list_sandbox_jobs<'a>(
        &'a self,
    ) -> impl Future<Output = Result<Vec<SandboxJobRecord>, DatabaseError>> + Send + 'a;
    fn update_sandbox_job_status<'a>(
        &'a self,
        params: SandboxJobStatusUpdate<'a>,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn cleanup_stale_sandbox_jobs<'a>(
        &'a self,
    ) -> impl Future<Output = Result<u64, DatabaseError>> + Send + 'a;
    fn sandbox_job_summary<'a>(
        &'a self,
    ) -> impl Future<Output = Result<SandboxJobSummary, DatabaseError>> + Send + 'a;
    fn list_sandbox_jobs_for_user<'a>(
        &'a self,
        user_id: &'a str,
    ) -> impl Future<Output = Result<Vec<SandboxJobRecord>, DatabaseError>> + Send + 'a;
    fn sandbox_job_summary_for_user<'a>(
        &'a self,
        user_id: &'a str,
    ) -> impl Future<Output = Result<SandboxJobSummary, DatabaseError>> + Send + 'a;
    fn sandbox_job_belongs_to_user<'a>(
        &'a self,
        job_id: Uuid,
        user_id: &'a str,
    ) -> impl Future<Output = Result<bool, DatabaseError>> + Send + 'a;
    fn update_sandbox_job_mode<'a>(
        &'a self,
        id: Uuid,
        mode: &'a str,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn get_sandbox_job_mode<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<String>, DatabaseError>> + Send + 'a;
    fn save_job_event<'a>(
        &'a self,
        job_id: Uuid,
        event_type: &'a str,
        data: &'a serde_json::Value,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn list_job_events<'a>(
        &'a self,
        job_id: Uuid,
        before_id: Option<i64>,
        limit: Option<i64>,
    ) -> impl Future<Output = Result<Vec<JobEventRecord>, DatabaseError>> + Send + 'a;
}

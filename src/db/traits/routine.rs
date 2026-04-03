//! Routine persistence traits.
//!
//! Defines the dyn-safe [`RoutineStore`] and its native-async sibling
//! [`NativeRoutineStore`] for scheduled routines and their execution history.

use core::future::Future;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::agent::routine::{Routine, RoutineRun, RunStatus};
use crate::db::params::DbFuture;
use crate::error::DatabaseError;

/// Parameters for `update_routine_runtime`.
pub struct RoutineRuntimeUpdate<'a> {
    pub id: Uuid,
    pub last_run_at: DateTime<Utc>,
    pub next_fire_at: Option<DateTime<Utc>>,
    pub run_count: u64,
    pub consecutive_failures: u32,
    pub state: &'a serde_json::Value,
}

/// Parameters for `complete_routine_run`.
pub struct RoutineRunCompletion<'a> {
    pub id: Uuid,
    pub status: RunStatus,
    pub result_summary: Option<&'a str>,
    pub tokens_used: Option<i32>,
}

/// Object-safe persistence surface for scheduled routines and their execution
/// history.
///
/// This trait provides the dyn-safe boundary for routine storage operations,
/// enabling trait-object usage (e.g., `Arc<dyn RoutineStore>`).  It uses boxed
/// futures ([`DbFuture`]) to maintain object safety.
///
/// Companion trait: [`NativeRoutineStore`] provides the same API using native
/// async traits (RPITIT).  A blanket adapter automatically bridges
/// implementations of `NativeRoutineStore` to satisfy this trait.
///
/// Thread-safety: All implementations must be `Send + Sync` to support
/// concurrent access.
pub trait RoutineStore: Send + Sync {
    /// Persist a new routine definition.
    fn create_routine<'a>(
        &'a self,
        routine: &'a Routine,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    /// Load one routine by ID, returning `Ok(None)` when absent.
    fn get_routine<'a>(&'a self, id: Uuid) -> DbFuture<'a, Result<Option<Routine>, DatabaseError>>;
    /// Load one routine by `(user_id, name)`.
    fn get_routine_by_name<'a>(
        &'a self,
        user_id: &'a str,
        name: &'a str,
    ) -> DbFuture<'a, Result<Option<Routine>, DatabaseError>>;
    /// List routines owned by one user.
    fn list_routines<'a>(
        &'a self,
        user_id: &'a str,
    ) -> DbFuture<'a, Result<Vec<Routine>, DatabaseError>>;
    /// List routines across all users.
    fn list_all_routines<'a>(&'a self) -> DbFuture<'a, Result<Vec<Routine>, DatabaseError>>;
    /// List enabled routines that can react to events.
    fn list_event_routines<'a>(&'a self) -> DbFuture<'a, Result<Vec<Routine>, DatabaseError>>;
    /// List enabled cron routines whose `next_fire_at` is due.
    fn list_due_cron_routines<'a>(&'a self) -> DbFuture<'a, Result<Vec<Routine>, DatabaseError>>;
    /// Replace the mutable fields of an existing routine.
    fn update_routine<'a>(
        &'a self,
        routine: &'a Routine,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    /// Persist runtime counters and state after a run.
    fn update_routine_runtime<'a>(
        &'a self,
        params: RoutineRuntimeUpdate<'a>,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    /// Delete a routine by ID.
    ///
    /// Returns `Ok(true)` when a row was removed.
    fn delete_routine<'a>(&'a self, id: Uuid) -> DbFuture<'a, Result<bool, DatabaseError>>;
    /// Persist the start of a routine run.
    fn create_routine_run<'a>(
        &'a self,
        run: &'a RoutineRun,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    /// Persist the terminal status for an existing routine run.
    fn complete_routine_run<'a>(
        &'a self,
        params: RoutineRunCompletion<'a>,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    /// List recent runs for one routine, ordered by backend policy.
    fn list_routine_runs<'a>(
        &'a self,
        routine_id: Uuid,
        limit: i64,
    ) -> DbFuture<'a, Result<Vec<RoutineRun>, DatabaseError>>;
    /// Count routine runs currently persisted as running.
    fn count_running_routine_runs<'a>(
        &'a self,
        routine_id: Uuid,
    ) -> DbFuture<'a, Result<i64, DatabaseError>>;
    /// Associate a persisted routine run with a dispatched job ID.
    fn link_routine_run_to_job<'a>(
        &'a self,
        run_id: Uuid,
        job_id: Uuid,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
}

/// Native async sibling trait for concrete routine-store implementations.
pub trait NativeRoutineStore: Send + Sync {
    /// Persist a new routine definition.
    fn create_routine<'a>(
        &'a self,
        routine: &'a Routine,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    /// Load one routine by ID, returning `Ok(None)` when absent.
    fn get_routine<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<Routine>, DatabaseError>> + Send + 'a;
    /// Load one routine by `(user_id, name)`.
    fn get_routine_by_name<'a>(
        &'a self,
        user_id: &'a str,
        name: &'a str,
    ) -> impl Future<Output = Result<Option<Routine>, DatabaseError>> + Send + 'a;
    /// List routines owned by one user.
    fn list_routines<'a>(
        &'a self,
        user_id: &'a str,
    ) -> impl Future<Output = Result<Vec<Routine>, DatabaseError>> + Send + 'a;
    /// List routines across all users.
    fn list_all_routines<'a>(
        &'a self,
    ) -> impl Future<Output = Result<Vec<Routine>, DatabaseError>> + Send + 'a;
    /// List enabled routines that can react to events.
    fn list_event_routines<'a>(
        &'a self,
    ) -> impl Future<Output = Result<Vec<Routine>, DatabaseError>> + Send + 'a;
    /// List enabled cron routines whose `next_fire_at` is due.
    fn list_due_cron_routines<'a>(
        &'a self,
    ) -> impl Future<Output = Result<Vec<Routine>, DatabaseError>> + Send + 'a;
    /// Replace the mutable fields of an existing routine.
    fn update_routine<'a>(
        &'a self,
        routine: &'a Routine,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    /// Persist runtime counters and state after a run.
    fn update_routine_runtime<'a>(
        &'a self,
        params: RoutineRuntimeUpdate<'a>,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    /// Delete a routine by ID and report whether a row was removed.
    fn delete_routine<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<bool, DatabaseError>> + Send + 'a;
    /// Persist the start of a routine run.
    fn create_routine_run<'a>(
        &'a self,
        run: &'a RoutineRun,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    /// Persist the terminal status for an existing routine run.
    fn complete_routine_run<'a>(
        &'a self,
        params: RoutineRunCompletion<'a>,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    /// List recent runs for one routine.
    fn list_routine_runs<'a>(
        &'a self,
        routine_id: Uuid,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<RoutineRun>, DatabaseError>> + Send + 'a;
    /// Count routine runs currently persisted as running.
    fn count_running_routine_runs<'a>(
        &'a self,
        routine_id: Uuid,
    ) -> impl Future<Output = Result<i64, DatabaseError>> + Send + 'a;
    /// Associate a persisted routine run with a dispatched job ID.
    fn link_routine_run_to_job<'a>(
        &'a self,
        run_id: Uuid,
        job_id: Uuid,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
}

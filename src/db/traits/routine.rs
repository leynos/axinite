//! Routine persistence traits.
//!
//! Defines the dyn-safe [`RoutineStore`] and its native-async sibling
//! [`NativeRoutineStore`] for scheduled routines and their execution history.

use core::future::Future;

use uuid::Uuid;

use crate::agent::routine::{Routine, RoutineRun};
use crate::db::params::{DbFuture, RoutineRunCompletion, RoutineRuntimeUpdate};
use crate::error::DatabaseError;

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
    fn create_routine<'a>(
        &'a self,
        routine: &'a Routine,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn get_routine<'a>(&'a self, id: Uuid) -> DbFuture<'a, Result<Option<Routine>, DatabaseError>>;
    fn get_routine_by_name<'a>(
        &'a self,
        user_id: &'a str,
        name: &'a str,
    ) -> DbFuture<'a, Result<Option<Routine>, DatabaseError>>;
    fn list_routines<'a>(
        &'a self,
        user_id: &'a str,
    ) -> DbFuture<'a, Result<Vec<Routine>, DatabaseError>>;
    fn list_all_routines<'a>(&'a self) -> DbFuture<'a, Result<Vec<Routine>, DatabaseError>>;
    fn list_event_routines<'a>(&'a self) -> DbFuture<'a, Result<Vec<Routine>, DatabaseError>>;
    fn list_due_cron_routines<'a>(&'a self) -> DbFuture<'a, Result<Vec<Routine>, DatabaseError>>;
    fn update_routine<'a>(
        &'a self,
        routine: &'a Routine,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn update_routine_runtime<'a>(
        &'a self,
        params: RoutineRuntimeUpdate<'a>,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn delete_routine<'a>(&'a self, id: Uuid) -> DbFuture<'a, Result<bool, DatabaseError>>;
    fn create_routine_run<'a>(
        &'a self,
        run: &'a RoutineRun,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn complete_routine_run<'a>(
        &'a self,
        params: RoutineRunCompletion<'a>,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn list_routine_runs<'a>(
        &'a self,
        routine_id: Uuid,
        limit: i64,
    ) -> DbFuture<'a, Result<Vec<RoutineRun>, DatabaseError>>;
    fn count_running_routine_runs<'a>(
        &'a self,
        routine_id: Uuid,
    ) -> DbFuture<'a, Result<i64, DatabaseError>>;
    fn link_routine_run_to_job<'a>(
        &'a self,
        run_id: Uuid,
        job_id: Uuid,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
}

/// Native async sibling trait for concrete routine-store implementations.
pub trait NativeRoutineStore: Send + Sync {
    fn create_routine<'a>(
        &'a self,
        routine: &'a Routine,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn get_routine<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<Routine>, DatabaseError>> + Send + 'a;
    fn get_routine_by_name<'a>(
        &'a self,
        user_id: &'a str,
        name: &'a str,
    ) -> impl Future<Output = Result<Option<Routine>, DatabaseError>> + Send + 'a;
    fn list_routines<'a>(
        &'a self,
        user_id: &'a str,
    ) -> impl Future<Output = Result<Vec<Routine>, DatabaseError>> + Send + 'a;
    fn list_all_routines<'a>(
        &'a self,
    ) -> impl Future<Output = Result<Vec<Routine>, DatabaseError>> + Send + 'a;
    fn list_event_routines<'a>(
        &'a self,
    ) -> impl Future<Output = Result<Vec<Routine>, DatabaseError>> + Send + 'a;
    fn list_due_cron_routines<'a>(
        &'a self,
    ) -> impl Future<Output = Result<Vec<Routine>, DatabaseError>> + Send + 'a;
    fn update_routine<'a>(
        &'a self,
        routine: &'a Routine,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn update_routine_runtime<'a>(
        &'a self,
        params: RoutineRuntimeUpdate<'a>,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn delete_routine<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<bool, DatabaseError>> + Send + 'a;
    fn create_routine_run<'a>(
        &'a self,
        run: &'a RoutineRun,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn complete_routine_run<'a>(
        &'a self,
        params: RoutineRunCompletion<'a>,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn list_routine_runs<'a>(
        &'a self,
        routine_id: Uuid,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<RoutineRun>, DatabaseError>> + Send + 'a;
    fn count_running_routine_runs<'a>(
        &'a self,
        routine_id: Uuid,
    ) -> impl Future<Output = Result<i64, DatabaseError>> + Send + 'a;
    fn link_routine_run_to_job<'a>(
        &'a self,
        run_id: Uuid,
        job_id: Uuid,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
}

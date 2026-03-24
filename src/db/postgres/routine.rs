//! RoutineStore implementation for PostgreSQL backend.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::agent::routine::{Routine, RoutineRun};
use crate::db::{NativeRoutineStore, RoutineRunCompletion, RoutineRuntimeUpdate};
use crate::error::DatabaseError;

use super::PgBackend;

impl NativeRoutineStore for PgBackend {
    async fn create_routine(&self, routine: &Routine) -> Result<(), DatabaseError> {
        self.store.create_routine(routine).await
    }

    async fn get_routine(&self, id: Uuid) -> Result<Option<Routine>, DatabaseError> {
        self.store.get_routine(id).await
    }

    async fn get_routine_by_name(
        &self,
        user_id: &str,
        name: &str,
    ) -> Result<Option<Routine>, DatabaseError> {
        self.store.get_routine_by_name(user_id, name).await
    }

    async fn list_routines(&self, user_id: &str) -> Result<Vec<Routine>, DatabaseError> {
        self.store.list_routines(user_id).await
    }

    async fn list_all_routines(&self) -> Result<Vec<Routine>, DatabaseError> {
        self.store.list_all_routines().await
    }

    async fn list_event_routines(&self) -> Result<Vec<Routine>, DatabaseError> {
        self.store.list_event_routines().await
    }

    async fn list_due_cron_routines(&self) -> Result<Vec<Routine>, DatabaseError> {
        self.store.list_due_cron_routines().await
    }

    async fn update_routine(&self, routine: &Routine) -> Result<(), DatabaseError> {
        self.store.update_routine(routine).await
    }

    async fn update_routine_runtime(
        &self,
        params: RoutineRuntimeUpdate<'_>,
    ) -> Result<(), DatabaseError> {
        let RoutineRuntimeUpdate {
            id,
            last_run_at,
            next_fire_at,
            run_count,
            consecutive_failures,
            state,
        } = params;
        self.store
            .update_routine_runtime(
                id,
                last_run_at,
                next_fire_at,
                run_count,
                consecutive_failures,
                state,
            )
            .await
    }

    async fn delete_routine(&self, id: Uuid) -> Result<(), DatabaseError> {
        self.store.delete_routine(id).await
    }

    async fn log_routine_run(&self, run: &RoutineRun) -> Result<(), DatabaseError> {
        self.store.log_routine_run(run).await
    }

    async fn complete_routine_run(
        &self,
        params: RoutineRunCompletion<'_>,
    ) -> Result<(), DatabaseError> {
        let RoutineRunCompletion {
            run_id,
            success,
            failure_reason,
            output,
        } = params;
        self.store
            .complete_routine_run(run_id, success, failure_reason, output)
            .await
    }

    async fn list_routine_runs(
        &self,
        routine_id: Uuid,
        limit: i64,
    ) -> Result<Vec<RoutineRun>, DatabaseError> {
        self.store.list_routine_runs(routine_id, limit).await
    }

    async fn cleanup_stale_routine_runs(
        &self,
        started_before: DateTime<Utc>,
    ) -> Result<u64, DatabaseError> {
        self.store.cleanup_stale_routine_runs(started_before).await
    }
}

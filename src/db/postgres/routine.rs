//! RoutineStore implementation for PostgreSQL backend.

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
        self.store.update_routine_runtime(params).await
    }

    async fn delete_routine(&self, id: Uuid) -> Result<bool, DatabaseError> {
        self.store.delete_routine(id).await
    }

    async fn create_routine_run(&self, run: &RoutineRun) -> Result<(), DatabaseError> {
        self.store.create_routine_run(run).await
    }

    async fn complete_routine_run(
        &self,
        params: RoutineRunCompletion<'_>,
    ) -> Result<(), DatabaseError> {
        self.store.complete_routine_run(params).await
    }

    async fn list_routine_runs(
        &self,
        routine_id: Uuid,
        limit: i64,
    ) -> Result<Vec<RoutineRun>, DatabaseError> {
        self.store.list_routine_runs(routine_id, limit).await
    }

    async fn count_running_routine_runs(&self, routine_id: Uuid) -> Result<i64, DatabaseError> {
        self.store.count_running_routine_runs(routine_id).await
    }

    async fn link_routine_run_to_job(
        &self,
        run_id: Uuid,
        job_id: Uuid,
    ) -> Result<(), DatabaseError> {
        self.store.link_routine_run_to_job(run_id, job_id).await
    }
}

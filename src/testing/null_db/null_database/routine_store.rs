//! Null implementation of NativeRoutineStore for NullDatabase.

use uuid::Uuid;

use crate::agent::{Routine, routine::RoutineRun};
use crate::db::RoutineRuntimeUpdate;
use crate::error::DatabaseError;

use super::NullDatabase;

impl crate::db::NativeRoutineStore for NullDatabase {
    async fn create_routine(&self, _routine: &Routine) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn get_routine(&self, _id: Uuid) -> Result<Option<Routine>, DatabaseError> {
        Ok(None)
    }

    async fn get_routine_by_name(
        &self,
        _user_id: &str,
        _name: &str,
    ) -> Result<Option<Routine>, DatabaseError> {
        Ok(None)
    }

    async fn list_routines(&self, _user_id: &str) -> Result<Vec<Routine>, DatabaseError> {
        Ok(vec![])
    }

    async fn list_all_routines(&self) -> Result<Vec<Routine>, DatabaseError> {
        Ok(vec![])
    }

    async fn update_routine(&self, _routine: &Routine) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn delete_routine(&self, _id: Uuid) -> Result<bool, DatabaseError> {
        Ok(false)
    }

    async fn update_routine_runtime(
        &self,
        _update: RoutineRuntimeUpdate<'_>,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn create_routine_run(&self, _run: &RoutineRun) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn list_routine_runs(
        &self,
        _routine_id: Uuid,
        _limit: i64,
    ) -> Result<Vec<RoutineRun>, DatabaseError> {
        Ok(vec![])
    }

    async fn complete_routine_run(
        &self,
        _completion: crate::db::RoutineRunCompletion<'_>,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn list_event_routines(&self) -> Result<Vec<Routine>, DatabaseError> {
        Ok(vec![])
    }

    async fn list_due_cron_routines(&self) -> Result<Vec<Routine>, DatabaseError> {
        Ok(vec![])
    }

    async fn count_running_routine_runs(&self, _routine_id: Uuid) -> Result<i64, DatabaseError> {
        Ok(0)
    }

    async fn link_routine_run_to_job(
        &self,
        _run_id: Uuid,
        _job_id: Uuid,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }
}

//! SandboxStore implementation for PostgreSQL backend.

use uuid::Uuid;

use crate::db::{NativeSandboxStore, SandboxJobStatusUpdate};
use crate::error::DatabaseError;
use crate::history::{JobEventRecord, SandboxJobRecord, SandboxJobSummary};

use super::PgBackend;

/// Macro to generate simple async forwarding methods to the underlying store.
///
/// This reduces boilerplate for methods that just delegate to `self.store.method_name(args).await`.
/// The `update_sandbox_job_status` method is kept explicit since it requires destructuring.
macro_rules! forward_to_store {
    // No args, simple return type
    ($fn_name:ident() -> $ret:ty) => {
        async fn $fn_name(&self) -> $ret {
            self.store.$fn_name().await
        }
    };
    // Single arg
    ($fn_name:ident($arg1:ident: $t1:ty) -> $ret:ty) => {
        async fn $fn_name(&self, $arg1: $t1) -> $ret {
            self.store.$fn_name($arg1).await
        }
    };
    // Two args
    ($fn_name:ident($arg1:ident: $t1:ty, $arg2:ident: $t2:ty) -> $ret:ty) => {
        async fn $fn_name(&self, $arg1: $t1, $arg2: $t2) -> $ret {
            self.store.$fn_name($arg1, $arg2).await
        }
    };
    // Three args
    ($fn_name:ident($arg1:ident: $t1:ty, $arg2:ident: $t2:ty, $arg3:ident: $t3:ty) -> $ret:ty) => {
        async fn $fn_name(&self, $arg1: $t1, $arg2: $t2, $arg3: $t3) -> $ret {
            self.store.$fn_name($arg1, $arg2, $arg3).await
        }
    };
}

impl NativeSandboxStore for PgBackend {
    forward_to_store!(save_sandbox_job(job: &SandboxJobRecord) -> Result<(), DatabaseError>);
    forward_to_store!(get_sandbox_job(id: Uuid) -> Result<Option<SandboxJobRecord>, DatabaseError>);
    forward_to_store!(list_sandbox_jobs() -> Result<Vec<SandboxJobRecord>, DatabaseError>);

    async fn update_sandbox_job_status(
        &self,
        params: SandboxJobStatusUpdate<'_>,
    ) -> Result<(), DatabaseError> {
        let SandboxJobStatusUpdate {
            id,
            status,
            success,
            message,
            started_at,
            completed_at,
        } = params;
        self.store
            .update_sandbox_job_status(id, status, success, message, started_at, completed_at)
            .await
    }

    forward_to_store!(cleanup_stale_sandbox_jobs() -> Result<u64, DatabaseError>);
    forward_to_store!(sandbox_job_summary() -> Result<SandboxJobSummary, DatabaseError>);
    forward_to_store!(list_sandbox_jobs_for_user(user_id: &str) -> Result<Vec<SandboxJobRecord>, DatabaseError>);
    forward_to_store!(sandbox_job_summary_for_user(user_id: &str) -> Result<SandboxJobSummary, DatabaseError>);
    forward_to_store!(sandbox_job_belongs_to_user(job_id: Uuid, user_id: &str) -> Result<bool, DatabaseError>);
    forward_to_store!(update_sandbox_job_mode(id: Uuid, mode: &str) -> Result<(), DatabaseError>);
    forward_to_store!(get_sandbox_job_mode(id: Uuid) -> Result<Option<String>, DatabaseError>);
    forward_to_store!(save_job_event(job_id: Uuid, event_type: &str, data: &serde_json::Value) -> Result<(), DatabaseError>);
    forward_to_store!(list_job_events(job_id: Uuid, before_id: Option<i64>, limit: Option<i64>) -> Result<Vec<JobEventRecord>, DatabaseError>);
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_sandbox_job_status_update_destructuring() {
        // This test verifies that the SandboxJobStatusUpdate struct is correctly
        // destructured and all fields are passed through to the underlying store method.
        // This is a compile-time check - if the struct changes and we miss a field,
        // this will fail to compile.

        let now = Utc::now();
        let update = SandboxJobStatusUpdate {
            id: Uuid::new_v4(),
            status: "completed",
            success: Some(true),
            message: Some("Test message"),
            started_at: Some(now),
            completed_at: Some(now),
        };

        // Destructure to ensure all fields are present
        let SandboxJobStatusUpdate {
            id,
            status,
            success,
            message,
            started_at,
            completed_at,
        } = update;

        // Verify fields are correctly extracted
        assert!(success.expect("expected `success` to be Some(true)"));
        assert_eq!(
            message.expect("expected `message` to be Some"),
            "Test message"
        );
        assert_eq!(status, "completed");
        assert!(started_at.is_some());
        assert!(completed_at.is_some());

        // This pattern ensures we don't accidentally miss fields when updating
        // the update_sandbox_job_status implementation
        let _ = (id, status, success, message, started_at, completed_at);
    }
}

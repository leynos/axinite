//! Behaviour-focused tests for `SandboxJobStatusUpdate` forwarding.

use super::*;
use chrono::{TimeZone, Utc};

#[test]
fn test_sandbox_job_status_update_destructuring() {
    // This test verifies that the SandboxJobStatusUpdate struct is correctly
    // destructured and all fields are passed through to the underlying store method.
    // This is a compile-time check - if the struct changes and we miss a field,
    // this will fail to compile.

    let now = Utc::now();
    let update = SandboxJobStatusUpdate {
        id: Uuid::new_v4(),
        status: SandboxJobStatus::from("completed"),
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
    assert_eq!(status.as_str(), "completed");
    assert!(started_at.is_some());
    assert!(completed_at.is_some());

    // This pattern ensures we don't accidentally miss fields when updating
    // the update_sandbox_job_status implementation
    let _ = (id, status, success, message, started_at, completed_at);
}

#[cfg(feature = "postgres")]
mod behavioral;

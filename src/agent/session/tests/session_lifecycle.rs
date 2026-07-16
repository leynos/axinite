//! Tests for session-level behaviour: thread creation, switching,
//! auto-approved tools, and session serialization.

use uuid::Uuid;

use crate::agent::session::{Session, ThreadState};

#[test]
fn test_session_creation() {
    let mut session = Session::new("user-123");
    assert!(session.active_thread.is_none());

    session.create_thread();
    assert!(session.active_thread.is_some());
}

#[test]
fn test_thread_switch() {
    let mut session = Session::new("user-1");

    let t1_id = session.create_thread().id;
    let t2_id = session.create_thread().id;

    // After creating two threads, active should be the last one
    assert_eq!(session.active_thread, Some(t2_id));

    // Switch back to the first
    assert!(session.switch_thread(t1_id));
    assert_eq!(session.active_thread, Some(t1_id));

    // Switching to a nonexistent thread should fail
    let fake_id = Uuid::new_v4();
    assert!(!session.switch_thread(fake_id));
    // Active thread should remain unchanged
    assert_eq!(session.active_thread, Some(t1_id));
}

#[test]
fn test_get_or_create_thread_idempotent() {
    let mut session = Session::new("user-1");

    let tid1 = session.get_or_create_thread().id;
    let tid2 = session.get_or_create_thread().id;

    // Should return the same thread (not create a new one each time)
    assert_eq!(tid1, tid2);
    assert_eq!(session.threads.len(), 1);
}

#[test]
fn test_session_serialization_round_trip() {
    let mut session = Session::new("user-ser");
    session.create_thread();
    session.auto_approve_tool("echo");

    let json = serde_json::to_string(&session).unwrap();
    let restored: Session = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.user_id, "user-ser");
    assert_eq!(restored.threads.len(), 1);
    assert!(restored.is_tool_auto_approved("echo"));
    assert!(!restored.is_tool_auto_approved("shell"));
}

#[test]
fn test_auto_approved_tools() {
    let mut session = Session::new("user-1");

    assert!(!session.is_tool_auto_approved("shell"));
    session.auto_approve_tool("shell");
    assert!(session.is_tool_auto_approved("shell"));

    // Idempotent
    session.auto_approve_tool("shell");
    assert_eq!(session.auto_approved_tools.len(), 1);
}

#[test]
fn test_active_thread_accessors() {
    let mut session = Session::new("user-1");

    assert!(session.active_thread().is_none());
    assert!(session.active_thread_mut().is_none());

    let tid = session.create_thread().id;

    assert!(session.active_thread().is_some());
    assert_eq!(session.active_thread().unwrap().id, tid);

    // Mutably modify through accessor
    session.active_thread_mut().unwrap().start_turn("test");
    assert_eq!(
        session.active_thread().unwrap().state,
        ThreadState::Processing
    );
}

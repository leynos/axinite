//! Unit tests for thread control command handlers.
//!
//! These tests cover interrupt, clear, undo, and redo behaviour against the
//! real session and undo-manager state transitions.

use std::sync::Arc;

use rstest::{fixture, rstest};
use tokio::sync::Mutex;
use uuid::Uuid;

use super::*;
use crate::agent::SessionManager;
use crate::agent::session::{Session, ThreadState};
use crate::agent::thread_ops::test_support::{incoming_message, make_agent, session_manager};
use crate::channels::IncomingMessage;
use crate::llm::{ChatMessage, LlmProvider};
use crate::testing::StubLlm;

fn serialise_messages(messages: &[ChatMessage]) -> serde_json::Value {
    serde_json::to_value(messages).expect("chat messages should serialise for test assertions")
}

fn test_message() -> IncomingMessage {
    incoming_message()
}

fn make_test_agent(session_manager: Arc<SessionManager>) -> Agent {
    let llm: Arc<dyn LlmProvider> = Arc::new(StubLlm::new("ok"));
    make_agent(None, llm, session_manager)
}

fn make_session_with_thread() -> (Arc<Mutex<Session>>, Uuid) {
    let message = test_message();
    let mut session = Session::new(message.user_id);
    let thread_id = session.create_thread().id;
    (Arc::new(Mutex::new(session)), thread_id)
}

#[fixture]
fn session_with_thread() -> (Arc<Mutex<Session>>, Uuid) {
    make_session_with_thread()
}

#[rstest]
#[case(ThreadState::Processing)]
#[case(ThreadState::AwaitingApproval)]
#[tokio::test]
async fn process_interrupt_transitions_processing_thread(
    session_manager: Arc<SessionManager>,
    session_with_thread: (Arc<Mutex<Session>>, Uuid),
    #[case] initial_state: ThreadState,
) {
    let agent = make_test_agent(session_manager);
    let (session, thread_id) = session_with_thread;

    {
        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .expect("thread should exist in session fixture");
        thread.state = initial_state;
    }

    let result = agent
        .process_interrupt(Arc::clone(&session), thread_id)
        .await
        .expect("interrupt should succeed");

    assert!(
        matches!(
            result,
            SubmissionResult::Ok {
                message: Some(ref message)
            } if message == "Interrupted."
        ),
        "expected interrupt acknowledgement"
    );

    let sess = session.lock().await;
    assert_eq!(
        sess.threads[&thread_id].state,
        ThreadState::Interrupted,
        "interrupt should transition the thread to Interrupted"
    );
}

#[rstest]
#[tokio::test]
async fn process_clear_clears_undo_history(
    session_manager: Arc<SessionManager>,
    session_with_thread: (Arc<Mutex<Session>>, Uuid),
) {
    let agent = make_test_agent(Arc::clone(&session_manager));
    let (session, thread_id) = session_with_thread;

    let undo_mgr = session_manager.get_undo_manager(thread_id).await;
    undo_mgr
        .lock()
        .await
        .checkpoint(0, vec![ChatMessage::user("before clear")], "Before clear");

    agent
        .process_clear(Arc::clone(&session), thread_id)
        .await
        .expect("clear should succeed");

    let mgr = undo_mgr.lock().await;
    assert!(
        !mgr.can_undo(),
        "clear should remove all undo history for the thread"
    );
}

#[rstest]
#[tokio::test]
async fn process_undo_restores_checkpoint(
    session_manager: Arc<SessionManager>,
    session_with_thread: (Arc<Mutex<Session>>, Uuid),
) {
    let agent = make_test_agent(Arc::clone(&session_manager));
    let (session, thread_id) = session_with_thread;

    let checkpoint_messages = {
        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .expect("thread should exist in session fixture");
        thread.start_turn("first turn");
        thread.complete_turn("first reply");
        thread.messages()
    };

    let undo_mgr = session_manager.get_undo_manager(thread_id).await;
    {
        let mut mgr = undo_mgr.lock().await;
        mgr.checkpoint(0, Vec::new(), "Before turn 1");
        mgr.checkpoint(1, checkpoint_messages.clone(), "Turn 1");
    }

    {
        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .expect("thread should exist before undo mutation");
        thread.start_turn("second turn");
        thread.complete_turn("second reply");
    }

    let result = agent
        .process_undo(Arc::clone(&session), thread_id)
        .await
        .expect("undo should succeed");

    assert!(
        matches!(
            result,
            SubmissionResult::Ok {
                message: Some(ref message)
            } if message.contains("Undone to turn 1.")
                && message.contains("1 undo(s) remaining.")
        ),
        "expected undo success message with remaining undo count"
    );

    let sess = session.lock().await;
    let restored_messages = sess.threads[&thread_id].messages();
    assert_eq!(
        serialise_messages(&restored_messages),
        serialise_messages(&checkpoint_messages),
        "undo should restore the checkpoint snapshot"
    );
}

#[rstest]
#[tokio::test]
async fn process_redo_restores_after_undo(
    session_manager: Arc<SessionManager>,
    session_with_thread: (Arc<Mutex<Session>>, Uuid),
) {
    let agent = make_test_agent(Arc::clone(&session_manager));
    let (session, thread_id) = session_with_thread;

    let checkpoint_messages = {
        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .expect("thread should exist in session fixture");
        thread.start_turn("first turn");
        thread.complete_turn("first reply");
        thread.messages()
    };

    let undo_mgr = session_manager.get_undo_manager(thread_id).await;
    {
        let mut mgr = undo_mgr.lock().await;
        mgr.checkpoint(0, Vec::new(), "Before turn 1");
        mgr.checkpoint(1, checkpoint_messages, "Turn 1");
    }

    let messages_before_undo = {
        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .expect("thread should exist before redo mutation");
        thread.start_turn("second turn");
        thread.complete_turn("second reply");
        thread.messages()
    };

    agent
        .process_undo(Arc::clone(&session), thread_id)
        .await
        .expect("undo should succeed before redo");

    let result = agent
        .process_redo(Arc::clone(&session), thread_id)
        .await
        .expect("redo should succeed");

    assert!(
        matches!(
            result,
            SubmissionResult::Ok {
                message: Some(ref message)
            } if message.contains("Redone to turn")
        ),
        "expected redo success message"
    );

    let sess = session.lock().await;
    let restored_messages = sess.threads[&thread_id].messages();
    assert_eq!(
        serialise_messages(&restored_messages),
        serialise_messages(&messages_before_undo),
        "redo should restore the pre-undo thread messages"
    );
}

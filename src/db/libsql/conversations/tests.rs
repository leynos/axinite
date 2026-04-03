use super::*;
use crate::db::Database;

struct BackendFixture {
    _dir: tempfile::TempDir,
    backend: LibSqlBackend,
}

async fn create_backend(db_name: &str) -> BackendFixture {
    let dir = tempfile::tempdir().expect("create tempdir for libsql conversation test");
    let db_path = dir.path().join(db_name);
    let backend = LibSqlBackend::new_local(&db_path)
        .await
        .expect("create libsql backend");
    backend
        .run_migrations()
        .await
        .expect("run libsql migrations");
    BackendFixture { _dir: dir, backend }
}

#[tokio::test]
async fn test_get_or_create_routine_conversation_is_idempotent() {
    let fixture = create_backend("test_routine_conv.db").await;
    let backend = &fixture.backend;
    let routine_id = Uuid::new_v4();
    let user_id = "test_user";

    let id1 = backend
        .get_or_create_routine_conversation(routine_id, "my-routine", user_id)
        .await
        .unwrap();
    let id2 = backend
        .get_or_create_routine_conversation(routine_id, "my-routine", user_id)
        .await
        .unwrap();
    let id3 = backend
        .get_or_create_routine_conversation(routine_id, "my-routine", user_id)
        .await
        .unwrap();
    let other_routine_id = Uuid::new_v4();
    let id4 = backend
        .get_or_create_routine_conversation(other_routine_id, "other-routine", user_id)
        .await
        .unwrap();

    assert_eq!(id1, id2, "Expected same conversation ID on repeated calls");
    assert_eq!(id1, id3);
    assert_ne!(
        id1, id4,
        "Different routines should get different conversations"
    );
}

#[tokio::test]
async fn test_routine_conversation_persists_across_messages() {
    let fixture = create_backend("test_routine_persist.db").await;
    let backend = &fixture.backend;
    let routine_id = Uuid::new_v4();
    let user_id = "test_user";

    let id1 = backend
        .get_or_create_routine_conversation(routine_id, "my-routine", user_id)
        .await
        .unwrap();
    backend
        .add_conversation_message(id1, "assistant", "[cron] Completed: all good")
        .await
        .unwrap();

    let id2 = backend
        .get_or_create_routine_conversation(routine_id, "my-routine", user_id)
        .await
        .unwrap();
    assert_eq!(id1, id2, "Second invocation should reuse same conversation");

    backend
        .add_conversation_message(id2, "assistant", "[cron] Completed: still good")
        .await
        .unwrap();

    let convs = backend
        .list_conversations_all_channels(user_id, 50)
        .await
        .unwrap();
    let routine_convs: Vec<_> = convs.iter().filter(|c| c.channel == "routine").collect();
    assert_eq!(
        routine_convs.len(),
        1,
        "Should have exactly 1 routine conversation, found {}",
        routine_convs.len()
    );
}

#[tokio::test]
async fn test_get_or_create_heartbeat_conversation_is_idempotent() {
    let fixture = create_backend("test_heartbeat_conv.db").await;
    let backend = &fixture.backend;
    let user_id = "test_user";

    let id1 = backend
        .get_or_create_heartbeat_conversation(user_id)
        .await
        .unwrap();
    let id2 = backend
        .get_or_create_heartbeat_conversation(user_id)
        .await
        .unwrap();

    assert_eq!(
        id1, id2,
        "Expected same heartbeat conversation on repeated calls"
    );
}

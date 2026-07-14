//! Unit tests for mapping routine rows to and from the database schema.

use super::*;

/// Create the routines table for testing.
async fn create_routines_table(
    conn: &libsql::Connection,
) -> Result<(), Box<dyn std::error::Error>> {
    conn.execute_batch(
        r#"
        CREATE TABLE routines (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT NOT NULL,
            user_id TEXT NOT NULL,
            enabled INTEGER NOT NULL,
            trigger_type TEXT NOT NULL,
            trigger_config TEXT NOT NULL,
            action_type TEXT NOT NULL,
            action_config TEXT NOT NULL,
            cooldown_secs INTEGER NOT NULL,
            max_concurrent INTEGER NOT NULL,
            dedup_window_secs INTEGER,
            notify_channel TEXT,
            notify_user TEXT NOT NULL,
            notify_on_success INTEGER NOT NULL,
            notify_on_failure INTEGER NOT NULL,
            notify_on_attention INTEGER NOT NULL,
            state TEXT NOT NULL,
            last_run_at TEXT,
            next_fire_at TEXT,
            run_count INTEGER NOT NULL,
            consecutive_failures INTEGER NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        "#,
    )
    .await?;
    Ok(())
}

/// Insert a test routine row with the specified dedup_window_secs.
/// When `raw_dedup` is provided, it is used verbatim (for injecting malformed values).
async fn insert_test_routine(
    conn: &libsql::Connection,
    dedup_window_secs: Option<i64>,
    raw_dedup: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let dedup_val = raw_dedup
        .map(|s| s.to_string())
        .or_else(|| dedup_window_secs.map(|v| v.to_string()))
        .unwrap_or_else(|| "NULL".to_string());
    let action_config = r#"{"prompt": "test", "context_paths": [], "max_tokens": 1000}"#;
    conn.execute(
        &format!(
            r#"
            INSERT INTO routines VALUES (
                '550e8400-e29b-41d4-a716-446655440000',
                'Test Routine',
                'Test Description',
                'test-user',
                1,
                'cron',
                '{{"schedule": "0 0 * * *"}}',
                'lightweight',
                '{action_config}',
                60,
                5,
                {dedup_val},
                'telegram',
                'test-user',
                1,
                1,
                0,
                '{{}}',
                NULL,
                NULL,
                0,
                0,
                '2024-01-01T00:00:00Z',
                '2024-01-01T00:00:00Z'
            )
            "#
        ),
        (),
    )
    .await?;
    Ok(())
}

/// Helper to create a mock libsql::Row for testing.
/// Uses a local in-memory database to create a real row.
/// When `raw_dedup` is provided, it is used verbatim (for injecting malformed values).
async fn mock_routine_row_with_dedup_window(
    dedup_window_secs: Option<i64>,
    raw_dedup: Option<&str>,
) -> Result<libsql::Row, Box<dyn std::error::Error>> {
    let db = libsql::Builder::new_local(":memory:").build().await?;
    let conn = db.connect()?;
    create_routines_table(&conn).await?;
    insert_test_routine(&conn, dedup_window_secs, raw_dedup).await?;
    let mut rows = conn.query("SELECT * FROM routines", ()).await?;
    let row = rows.next().await?.ok_or("no row returned")?;
    Ok(row)
}

/// Helper to create a mock libsql::Row for routine_runs testing.
/// When `raw_tokens` is provided, it is used verbatim (for injecting malformed values).
async fn mock_routine_run_row_with_tokens(
    tokens_used: Option<i64>,
    raw_tokens: Option<&str>,
) -> Result<libsql::Row, Box<dyn std::error::Error>> {
    let db = libsql::Builder::new_local(":memory:").build().await?;
    let conn = db.connect()?;

    conn.execute_batch(
        r#"
        CREATE TABLE routine_runs (
            id TEXT PRIMARY KEY,
            routine_id TEXT NOT NULL,
            trigger_type TEXT NOT NULL,
            trigger_detail TEXT,
            started_at TEXT NOT NULL,
            status TEXT NOT NULL,
            completed_at TEXT,
            result_summary TEXT,
            tokens_used INTEGER,
            job_id TEXT,
            created_at TEXT NOT NULL
        );
        "#,
    )
    .await?;

    let tokens_val = raw_tokens
        .map(|s| s.to_string())
        .or_else(|| tokens_used.map(|v| v.to_string()))
        .unwrap_or_else(|| "NULL".to_string());
    conn.execute(
        &format!(
            r#"
            INSERT INTO routine_runs VALUES (
                '550e8400-e29b-41d4-a716-446655440001',
                '550e8400-e29b-41d4-a716-446655440000',
                'cron',
                NULL,
                '2024-01-01T00:00:00Z',
                'ok',
                '2024-01-01T00:01:00Z',
                NULL,
                {tokens_val},
                NULL,
                '2024-01-01T00:00:00Z'
            )
            "#
        ),
        (),
    )
    .await?;

    let mut rows = conn.query("SELECT * FROM routine_runs", ()).await?;
    let row = rows.next().await?.ok_or("no row returned")?;
    Ok(row)
}

/// Assert that a `dedup_window_secs` value decodes to the expected `dedup_window`.
async fn assert_dedup_window_maps_to(
    dedup_window_secs: Option<i64>,
    expected: Option<std::time::Duration>,
) -> Result<(), Box<dyn std::error::Error>> {
    let row = mock_routine_row_with_dedup_window(dedup_window_secs, None)
        .await
        .map_err(|e| format!("failed to create mock row: {e}"))?;
    let routine =
        row_to_routine_libsql(&row).map_err(|e| format!("failed to map row to routine: {e}"))?;
    assert_eq!(
        routine.guardrails.dedup_window, expected,
        "dedup_window mismatch for input {dedup_window_secs:?}",
    );
    Ok(())
}

#[tokio::test]
async fn test_dedup_window_null_yields_none() {
    assert_dedup_window_maps_to(None, None)
        .await
        .expect("dedup window assertion failed");
}

#[tokio::test]
async fn test_dedup_window_valid_value() {
    assert_dedup_window_maps_to(Some(300), Some(std::time::Duration::from_secs(300)))
        .await
        .expect("dedup window assertion failed");
}

#[tokio::test]
async fn test_tokens_used_null_yields_none() {
    let row = mock_routine_run_row_with_tokens(None, None)
        .await
        .expect("failed to create mock row");
    let run = row_to_routine_run_libsql(&row).expect("failed to map row to run");
    assert!(run.tokens_used.is_none());
}

#[tokio::test]
async fn test_tokens_used_valid_value() {
    let row = mock_routine_run_row_with_tokens(Some(1500), None)
        .await
        .expect("failed to create mock row");
    let run = row_to_routine_run_libsql(&row).expect("failed to map row to run");
    assert_eq!(run.tokens_used, Some(1500));
}

/// Assert that a tokens_used value results in a Serialization error.
async fn assert_tokens_used_serialisation_error(
    tokens: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    let row = mock_routine_run_row_with_tokens(Some(tokens), None)
        .await
        .map_err(|e| format!("failed to create mock row: {e}"))?;
    let result = row_to_routine_run_libsql(&row);
    assert!(
        matches!(result, Err(DatabaseError::Serialization(_))),
        "Expected Serialization error for tokens_used = {tokens}, got {result:?}",
    );
    Ok(())
}

#[tokio::test]
async fn test_tokens_used_out_of_range_returns_serialization_error() {
    assert_tokens_used_serialisation_error(i64::from(i32::MAX) + 1)
        .await
        .expect("serialisation error assertion failed");
}

#[tokio::test]
async fn test_tokens_used_negative_returns_serialization_error() {
    assert_tokens_used_serialisation_error(-1)
        .await
        .expect("serialisation error assertion failed");
}

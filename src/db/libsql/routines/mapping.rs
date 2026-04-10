//! Row-decoding helpers for libSQL routine persistence.
//!
//! `ROUTINE_COLUMNS` and `ROUTINE_RUN_COLUMNS` define the strict positional
//! column order expected by the mapping functions below. Any schema or SELECT
//! list changes must update these constants and the corresponding parsing logic
//! together so row decoding remains correct.

use crate::agent::routine::{
    NotifyConfig, Routine, RoutineAction, RoutineGuardrails, RoutineRun, RunStatus, Trigger,
};
use crate::db::libsql::helpers::{get_i64, get_json, get_opt_text, get_opt_ts, get_text, get_ts};
use crate::error::DatabaseError;

#[derive(Clone, Copy)]
enum RoutineNumericField {
    CooldownSecs,
    MaxConcurrent,
    DedupWindowSecs,
    RunCount,
    ConsecutiveFailures,
    TokensUsed,
}

impl RoutineNumericField {
    fn as_str(self) -> &'static str {
        match self {
            Self::CooldownSecs => "routines.cooldown_secs",
            Self::MaxConcurrent => "routines.max_concurrent",
            Self::DedupWindowSecs => "routines.dedup_window_secs",
            Self::RunCount => "routines.run_count",
            Self::ConsecutiveFailures => "routines.consecutive_failures",
            Self::TokensUsed => "routine_runs.tokens_used",
        }
    }
}

fn parse_uuid_field(raw: &str, field: &str) -> Result<uuid::Uuid, DatabaseError> {
    raw.parse()
        .map_err(|error| DatabaseError::Serialization(format!("invalid {field} '{raw}': {error}")))
}

fn parse_optional_uuid_field(
    raw: Option<String>,
    field: &str,
) -> Result<Option<uuid::Uuid>, DatabaseError> {
    raw.map(|value| {
        value.parse().map_err(|error| {
            DatabaseError::Serialization(format!("invalid {field} '{value}': {error}"))
        })
    })
    .transpose()
}

fn parse_non_negative_field<T>(value: i64, field: RoutineNumericField) -> Result<T, DatabaseError>
where
    T: TryFrom<i64>,
    <T as TryFrom<i64>>::Error: std::fmt::Display,
{
    if value < 0 {
        return Err(DatabaseError::Serialization(format!(
            "{} must be non-negative: {value}",
            field.as_str()
        )));
    }

    T::try_from(value).map_err(|error| {
        DatabaseError::Serialization(format!(
            "{} exceeds range: {value} ({error})",
            field.as_str()
        ))
    })
}

fn parse_i32_field(value: i64, field: RoutineNumericField) -> Result<i32, DatabaseError> {
    parse_non_negative_field(value, field)
}

pub(crate) const ROUTINE_COLUMNS: &str = concat!(
    "id, name, description, user_id, enabled, ",
    "trigger_type, trigger_config, action_type, action_config, ",
    "cooldown_secs, max_concurrent, dedup_window_secs, ",
    "notify_channel, notify_user, notify_on_success, notify_on_failure, notify_on_attention, ",
    "state, last_run_at, next_fire_at, run_count, consecutive_failures, ",
    "created_at, updated_at"
);

pub(crate) const ROUTINE_RUN_COLUMNS: &str = concat!(
    "id, routine_id, trigger_type, trigger_detail, started_at, ",
    "status, completed_at, result_summary, tokens_used, job_id, created_at"
);

pub(crate) fn row_to_routine_libsql(row: &libsql::Row) -> Result<Routine, DatabaseError> {
    let trigger_type = get_text(row, 5);
    let trigger_config = get_json(row, 6);
    let action_type = get_text(row, 7);
    let action_config = get_json(row, 8);
    let cooldown_secs = get_i64(row, 9);
    let max_concurrent = get_i64(row, 10);
    let dedup_window_secs: Option<i64> = row.get::<Option<i64>>(11).map_err(|error| {
        DatabaseError::Serialization(format!(
            "invalid {}: {error}",
            RoutineNumericField::DedupWindowSecs.as_str()
        ))
    })?;

    let trigger = Trigger::from_db(&trigger_type, trigger_config)
        .map_err(|e| DatabaseError::Serialization(e.to_string()))?;
    let action = RoutineAction::from_db(&action_type, action_config)
        .map_err(|e| DatabaseError::Serialization(e.to_string()))?;
    let id_raw = get_text(row, 0);
    let cooldown = parse_non_negative_field(cooldown_secs, RoutineNumericField::CooldownSecs)?;
    let max_concurrent =
        parse_non_negative_field(max_concurrent, RoutineNumericField::MaxConcurrent)?;
    let dedup_window = dedup_window_secs
        .map(|seconds| parse_non_negative_field(seconds, RoutineNumericField::DedupWindowSecs))
        .transpose()?;
    let run_count = parse_non_negative_field(get_i64(row, 20), RoutineNumericField::RunCount)?;
    let consecutive_failures =
        parse_non_negative_field(get_i64(row, 21), RoutineNumericField::ConsecutiveFailures)?;

    Ok(Routine {
        id: parse_uuid_field(&id_raw, "routines.id")?,
        name: get_text(row, 1),
        description: get_text(row, 2),
        user_id: get_text(row, 3),
        enabled: get_i64(row, 4) != 0,
        trigger,
        action,
        guardrails: RoutineGuardrails {
            cooldown: std::time::Duration::from_secs(cooldown),
            max_concurrent,
            dedup_window: dedup_window.map(std::time::Duration::from_secs),
        },
        notify: NotifyConfig {
            channel: get_opt_text(row, 12),
            user: get_text(row, 13),
            on_success: get_i64(row, 14) != 0,
            on_failure: get_i64(row, 15) != 0,
            on_attention: get_i64(row, 16) != 0,
        },
        state: get_json(row, 17),
        last_run_at: get_opt_ts(row, 18),
        next_fire_at: get_opt_ts(row, 19),
        run_count,
        consecutive_failures,
        created_at: get_ts(row, 22),
        updated_at: get_ts(row, 23),
    })
}

pub(crate) fn row_to_routine_run_libsql(row: &libsql::Row) -> Result<RoutineRun, DatabaseError> {
    let status_str = get_text(row, 5);
    let status: RunStatus = status_str
        .parse()
        .map_err(|e: crate::error::RoutineError| DatabaseError::Serialization(e.to_string()))?;
    let id_raw = get_text(row, 0);
    let routine_id_raw = get_text(row, 1);

    Ok(RoutineRun {
        id: parse_uuid_field(&id_raw, "routine_runs.id")?,
        routine_id: parse_uuid_field(&routine_id_raw, "routine_runs.routine_id")?,
        trigger_type: get_text(row, 2),
        trigger_detail: get_opt_text(row, 3),
        started_at: get_ts(row, 4),
        completed_at: get_opt_ts(row, 6),
        status,
        result_summary: get_opt_text(row, 7),
        tokens_used: row
            .get::<Option<i64>>(8)
            .map_err(|error| {
                DatabaseError::Serialization(format!(
                    "invalid {}: {error}",
                    RoutineNumericField::TokensUsed.as_str()
                ))
            })?
            .map(|value| parse_i32_field(value, RoutineNumericField::TokensUsed))
            .transpose()?,
        job_id: parse_optional_uuid_field(get_opt_text(row, 9), "routine_runs.job_id")?,
        created_at: get_ts(row, 10),
    })
}

#[cfg(test)]
mod tests {
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
    ) {
        let row = mock_routine_row_with_dedup_window(dedup_window_secs, None)
            .await
            .expect("failed to create mock row");
        let routine = row_to_routine_libsql(&row).expect("failed to map row to routine");
        assert_eq!(
            routine.guardrails.dedup_window, expected,
            "dedup_window mismatch for input {dedup_window_secs:?}",
        );
    }

    #[tokio::test]
    async fn test_dedup_window_null_yields_none() {
        assert_dedup_window_maps_to(None, None).await;
    }

    #[tokio::test]
    async fn test_dedup_window_valid_value() {
        assert_dedup_window_maps_to(Some(300), Some(std::time::Duration::from_secs(300))).await;
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
    async fn assert_tokens_used_serialisation_error(tokens: i64) {
        let row = mock_routine_run_row_with_tokens(Some(tokens), None)
            .await
            .expect("failed to create mock row");
        let result = row_to_routine_run_libsql(&row);
        assert!(
            matches!(result, Err(DatabaseError::Serialization(_))),
            "Expected Serialization error for tokens_used = {tokens}, got {result:?}",
        );
    }

    #[tokio::test]
    async fn test_tokens_used_out_of_range_returns_serialization_error() {
        assert_tokens_used_serialisation_error(i64::from(i32::MAX) + 1).await;
    }

    #[tokio::test]
    async fn test_tokens_used_negative_returns_serialization_error() {
        assert_tokens_used_serialisation_error(-1).await;
    }
}

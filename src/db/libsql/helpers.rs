//! Shared parsing and value-conversion helpers for the libSQL backend.

use chrono::{DateTime, NaiveDateTime, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::context::JobState;

/// Parse an ISO-8601 timestamp string from SQLite into `DateTime<Utc>`.
///
/// Tries multiple formats in order:
/// 1. RFC 3339 with timezone (e.g. `2024-01-15T10:30:00.123Z`)
/// 2. Naive datetime with fractional seconds (e.g. `2024-01-15 10:30:00.123`)
/// 3. Naive datetime without fractional seconds (e.g. `2024-01-15 10:30:00`)
///
/// Returns an error if none of the formats match.
pub(crate) fn parse_timestamp(s: &str) -> Result<DateTime<Utc>, String> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f") {
        tracing::warn!(
            timestamp = %s,
            "parsed naive timestamp without timezone; assuming UTC for backward compatibility"
        );
        return Ok(ndt.and_utc());
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        tracing::warn!(
            timestamp = %s,
            "parsed naive timestamp without timezone; assuming UTC for backward compatibility"
        );
        return Ok(ndt.and_utc());
    }
    Err(format!("unparseable timestamp: {:?}", s))
}

/// Format a `DateTime<Utc>` for SQLite storage (RFC 3339 with millisecond precision).
pub(crate) fn fmt_ts(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Format an optional `DateTime<Utc>`.
pub(crate) fn fmt_opt_ts(dt: &Option<DateTime<Utc>>) -> libsql::Value {
    match dt {
        Some(dt) => libsql::Value::Text(fmt_ts(dt)),
        None => libsql::Value::Null,
    }
}

pub(crate) fn parse_job_state(s: &str) -> JobState {
    s.parse().unwrap_or(JobState::Pending)
}

pub(crate) fn parse_uuid_or_nil(raw: &str, idx: i32, field: &str) -> Uuid {
    match raw.parse() {
        Ok(uuid) => uuid,
        Err(error) => {
            tracing::warn!(
                column = idx,
                field,
                value = %raw,
                error = %error,
                "UUID parse failure; defaulting to nil UUID"
            );
            Uuid::nil()
        }
    }
}

pub(crate) fn parse_opt_uuid(raw: Option<String>, idx: i32, field: &str) -> Option<Uuid> {
    raw.and_then(|value| match value.parse() {
        Ok(uuid) => Some(uuid),
        Err(error) => {
            tracing::warn!(
                column = idx,
                field,
                value = %value,
                error = %error,
                "UUID parse failure; dropping invalid optional UUID"
            );
            None
        }
    })
}

/// Extract a text column from a libsql Row, returning empty string for NULL.
pub(crate) fn get_text(row: &libsql::Row, idx: i32) -> String {
    row.get::<String>(idx).unwrap_or_default()
}

/// Extract an optional text column.
/// Returns None for SQL NULL, preserves empty strings as `Some("")`.
pub(crate) fn get_opt_text(row: &libsql::Row, idx: i32) -> Option<String> {
    row.get::<String>(idx).ok()
}

/// Convert an `Option<&str>` to a `libsql::Value` (Text or Null).
/// Use this instead of `.unwrap_or("")` to preserve NULL semantics.
pub(crate) fn opt_text(s: Option<&str>) -> libsql::Value {
    match s {
        Some(s) => libsql::Value::Text(s.to_string()),
        None => libsql::Value::Null,
    }
}

/// Convert an `Option<String>` to a `libsql::Value` (Text or Null).
pub(crate) fn opt_text_owned(s: Option<String>) -> libsql::Value {
    match s {
        Some(s) => libsql::Value::Text(s),
        None => libsql::Value::Null,
    }
}

/// Extract an i64 column, defaulting to 0.
pub(crate) fn get_i64(row: &libsql::Row, idx: i32) -> i64 {
    row.get::<i64>(idx).unwrap_or(0)
}

/// Extract an optional bool from an integer column.
pub(crate) fn get_opt_bool(row: &libsql::Row, idx: i32) -> Option<bool> {
    row.get::<i64>(idx).ok().map(|v| v != 0)
}

/// Parse a Decimal from a text column.
pub(crate) fn get_decimal(row: &libsql::Row, idx: i32) -> Decimal {
    row.get::<String>(idx)
        .ok()
        .and_then(|s| s.parse::<Decimal>().ok())
        .unwrap_or_default()
}

/// Parse an optional Decimal from a text column.
pub(crate) fn get_opt_decimal(row: &libsql::Row, idx: i32) -> Option<Decimal> {
    row.get::<String>(idx)
        .ok()
        .and_then(|s| s.parse::<Decimal>().ok())
}

/// Parse a JSON value from a text column.
pub(crate) fn get_json(row: &libsql::Row, idx: i32) -> serde_json::Value {
    row.get::<String>(idx)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null)
}

/// Parse a timestamp from a text column.
///
/// If the column is NULL or the value cannot be parsed, logs a warning and
/// returns the Unix epoch (1970-01-01T00:00:00Z) so the error is detectable
/// rather than silently replaced by the current time.
pub(crate) fn get_ts(row: &libsql::Row, idx: i32) -> DateTime<Utc> {
    match row.get::<String>(idx) {
        Ok(s) => match parse_timestamp(&s) {
            Ok(dt) => dt,
            Err(e) => {
                tracing::warn!("Timestamp parse failure at column {}: {}", idx, e);
                DateTime::UNIX_EPOCH
            }
        },
        Err(_) => DateTime::UNIX_EPOCH,
    }
}

/// Parse an optional timestamp from a text column.
///
/// Returns `None` if the column is NULL. Logs a warning and returns `None` if
/// the value is present but cannot be parsed.
pub(crate) fn get_opt_ts(row: &libsql::Row, idx: i32) -> Option<DateTime<Utc>> {
    match row.get::<String>(idx) {
        Ok(s) if s.is_empty() => None,
        Ok(s) => match parse_timestamp(&s) {
            Ok(dt) => Some(dt),
            Err(e) => {
                tracing::warn!("Timestamp parse failure at column {}: {}", idx, e);
                None
            }
        },
        Err(_) => None,
    }
}

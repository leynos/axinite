//! Row-to-domain mapping helpers for the libSQL WASM tool store.

use chrono::{DateTime, Utc};

use super::super::{StoredWasmTool, WasmStorageError};

pub(super) fn libsql_wasm_opt_text(s: Option<&str>) -> libsql::Value {
    match s {
        Some(s) => libsql::Value::Text(s.to_string()),
        None => libsql::Value::Null,
    }
}

pub(super) fn libsql_wasm_parse_ts(s: &str) -> Result<DateTime<Utc>, WasmStorageError> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f") {
        return Ok(ndt.and_utc());
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Ok(ndt.and_utc());
    }
    Err(WasmStorageError::InvalidData(format!(
        "unparseable timestamp: {:?}",
        s
    )))
}

/// Parse a tool row with standard column order (no binary columns).
/// Columns: id(0), user_id(1), name(2), version(3), wit_version(4), description(5),
///          parameters_schema(6), source_url(7), trust_level(8), status(9),
///          created_at(10), updated_at(11)
pub(super) fn libsql_row_to_tool(row: &libsql::Row) -> Result<StoredWasmTool, WasmStorageError> {
    libsql_row_to_tool_at(row, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11)
}

/// Parse a tool row when binary columns are present (get_with_binary query).
/// Columns: id(0), user_id(1), name(2), version(3), wit_version(4), description(5),
///          wasm_binary(6), binary_hash(7),
///          parameters_schema(8), source_url(9), trust_level(10), status(11),
///          created_at(12), updated_at(13)
pub(super) fn libsql_row_to_tool_with_offset(
    row: &libsql::Row,
) -> Result<StoredWasmTool, WasmStorageError> {
    libsql_row_to_tool_at(row, 0, 1, 2, 3, 4, 5, 8, 9, 10, 11, 12, 13)
}

#[allow(clippy::too_many_arguments)]
fn libsql_row_to_tool_at(
    row: &libsql::Row,
    id_idx: i32,
    user_id_idx: i32,
    name_idx: i32,
    version_idx: i32,
    wit_version_idx: i32,
    description_idx: i32,
    schema_idx: i32,
    source_url_idx: i32,
    trust_level_idx: i32,
    status_idx: i32,
    created_at_idx: i32,
    updated_at_idx: i32,
) -> Result<StoredWasmTool, WasmStorageError> {
    let id_str: String = row
        .get(id_idx)
        .map_err(|e| WasmStorageError::Database(e.to_string()))?;
    let trust_level_str: String = row
        .get(trust_level_idx)
        .map_err(|e| WasmStorageError::Database(e.to_string()))?;
    let status_str: String = row
        .get(status_idx)
        .map_err(|e| WasmStorageError::Database(e.to_string()))?;
    let schema_str: String = row
        .get(schema_idx)
        .map_err(|e| WasmStorageError::Database(e.to_string()))?;
    let created_at_str: String = row
        .get(created_at_idx)
        .map_err(|e| WasmStorageError::Database(e.to_string()))?;
    let updated_at_str: String = row
        .get(updated_at_idx)
        .map_err(|e| WasmStorageError::Database(e.to_string()))?;

    Ok(StoredWasmTool {
        id: id_str
            .parse()
            .map_err(|e: uuid::Error| WasmStorageError::InvalidData(e.to_string()))?,
        user_id: row
            .get(user_id_idx)
            .map_err(|e| WasmStorageError::Database(e.to_string()))?,
        name: row
            .get(name_idx)
            .map_err(|e| WasmStorageError::Database(e.to_string()))?,
        version: row
            .get(version_idx)
            .map_err(|e| WasmStorageError::Database(e.to_string()))?,
        wit_version: row
            .get(wit_version_idx)
            .map_err(|e| WasmStorageError::Database(e.to_string()))?,
        description: row
            .get(description_idx)
            .map_err(|e| WasmStorageError::Database(e.to_string()))?,
        parameters_schema: serde_json::from_str(&schema_str).unwrap_or_default(),
        source_url: row
            .get::<String>(source_url_idx)
            .ok()
            .filter(|s| !s.is_empty()),
        trust_level: trust_level_str
            .parse()
            .map_err(WasmStorageError::InvalidData)?,
        status: status_str.parse().map_err(WasmStorageError::InvalidData)?,
        created_at: libsql_wasm_parse_ts(&created_at_str)?,
        updated_at: libsql_wasm_parse_ts(&updated_at_str)?,
    })
}

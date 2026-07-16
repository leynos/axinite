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
        "unparsable timestamp: {:?}",
        s
    )))
}

/// Column positions of tool fields within a query's result row.
///
/// Groups the per-field indices so row-mapping helpers take one layout value
/// instead of a dozen positional integers.
struct ToolRowIndices {
    id: i32,
    user_id: i32,
    name: i32,
    version: i32,
    wit_version: i32,
    description: i32,
    schema: i32,
    source_url: i32,
    trust_level: i32,
    status: i32,
    created_at: i32,
    updated_at: i32,
}

/// Standard column order (no binary columns).
const STANDARD_TOOL_ROW: ToolRowIndices = ToolRowIndices {
    id: 0,
    user_id: 1,
    name: 2,
    version: 3,
    wit_version: 4,
    description: 5,
    schema: 6,
    source_url: 7,
    trust_level: 8,
    status: 9,
    created_at: 10,
    updated_at: 11,
};

/// Column order when wasm_binary(6) and binary_hash(7) are present
/// (get_with_binary query); later fields shift by two.
const WITH_BINARY_TOOL_ROW: ToolRowIndices = ToolRowIndices {
    schema: 8,
    source_url: 9,
    trust_level: 10,
    status: 11,
    created_at: 12,
    updated_at: 13,
    ..STANDARD_TOOL_ROW
};

/// Parse a tool row with standard column order (no binary columns).
pub(super) fn libsql_row_to_tool(row: &libsql::Row) -> Result<StoredWasmTool, WasmStorageError> {
    libsql_row_to_tool_at(row, &STANDARD_TOOL_ROW)
}

/// Parse a tool row when binary columns are present (get_with_binary query).
pub(super) fn libsql_row_to_tool_with_offset(
    row: &libsql::Row,
) -> Result<StoredWasmTool, WasmStorageError> {
    libsql_row_to_tool_at(row, &WITH_BINARY_TOOL_ROW)
}

/// Map a result row to a [`StoredWasmTool`] using the given column layout.
fn libsql_row_to_tool_at(
    row: &libsql::Row,
    indices: &ToolRowIndices,
) -> Result<StoredWasmTool, WasmStorageError> {
    let id_str: String = row
        .get(indices.id)
        .map_err(|e| WasmStorageError::Database(e.to_string()))?;
    let trust_level_str: String = row
        .get(indices.trust_level)
        .map_err(|e| WasmStorageError::Database(e.to_string()))?;
    let status_str: String = row
        .get(indices.status)
        .map_err(|e| WasmStorageError::Database(e.to_string()))?;
    let schema_str: String = row
        .get(indices.schema)
        .map_err(|e| WasmStorageError::Database(e.to_string()))?;
    let created_at_str: String = row
        .get(indices.created_at)
        .map_err(|e| WasmStorageError::Database(e.to_string()))?;
    let updated_at_str: String = row
        .get(indices.updated_at)
        .map_err(|e| WasmStorageError::Database(e.to_string()))?;

    Ok(StoredWasmTool {
        id: id_str
            .parse()
            .map_err(|e: uuid::Error| WasmStorageError::InvalidData(e.to_string()))?,
        user_id: row
            .get(indices.user_id)
            .map_err(|e| WasmStorageError::Database(e.to_string()))?,
        name: row
            .get(indices.name)
            .map_err(|e| WasmStorageError::Database(e.to_string()))?,
        version: row
            .get(indices.version)
            .map_err(|e| WasmStorageError::Database(e.to_string()))?,
        wit_version: row
            .get(indices.wit_version)
            .map_err(|e| WasmStorageError::Database(e.to_string()))?,
        description: row
            .get(indices.description)
            .map_err(|e| WasmStorageError::Database(e.to_string()))?,
        parameters_schema: serde_json::from_str(&schema_str).unwrap_or_default(),
        source_url: row
            .get::<String>(indices.source_url)
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

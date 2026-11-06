//! Settings and health DTOs for the web gateway API.

use serde::{Deserialize, Serialize};

// --- Settings ---

/// One stored setting as returned by the settings API.
#[derive(Debug, Serialize)]
pub struct SettingResponse {
    /// Key under which the setting is stored.
    pub key: String,
    /// Stored value as JSON. Any JSON type is possible.
    pub value: serde_json::Value,
    /// Time of the last write, as an RFC 3339 timestamp.
    pub updated_at: String,
}

/// Response body listing the stored settings.
#[derive(Debug, Serialize)]
pub struct SettingsListResponse {
    /// One entry per stored setting.
    pub settings: Vec<SettingResponse>,
}

/// Request body that writes the value of a single setting.
#[derive(Debug, Deserialize)]
pub struct SettingWriteRequest {
    /// New value to store. Any JSON type is accepted.
    pub value: serde_json::Value,
}

/// Request body that bulk-imports settings.
#[derive(Debug, Deserialize)]
pub struct SettingsImportRequest {
    /// Map from setting key to the JSON value to store under that key.
    pub settings: std::collections::HashMap<String, serde_json::Value>,
}

/// Snapshot of all settings returned by the export endpoint.
#[derive(Debug, Serialize)]
pub struct SettingsExportResponse {
    /// Map from setting key to its stored JSON value.
    pub settings: std::collections::HashMap<String, serde_json::Value>,
}

// --- Health ---

/// Liveness response for the gateway health endpoint.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    /// Health state. The gateway handler always reports `healthy`.
    pub status: &'static str,
    /// Name of the serving channel. The gateway handler always reports `gateway`.
    pub channel: &'static str,
}

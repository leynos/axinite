//! Keyed get/set/reset/list access to [`Settings`] via dotted paths.

use super::Settings;

impl Settings {
    /// Get a setting value by dotted path (e.g., "agent.max_parallel_jobs").
    pub fn get(&self, path: &str) -> Option<String> {
        let json = serde_json::to_value(self).ok()?;
        let mut current = &json;

        for part in path.split('.') {
            current = current.get(part)?;
        }

        match current {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            serde_json::Value::Bool(b) => Some(b.to_string()),
            serde_json::Value::Null => Some("null".to_string()),
            serde_json::Value::Array(arr) => Some(serde_json::to_string(arr).unwrap_or_default()),
            serde_json::Value::Object(obj) => Some(serde_json::to_string(obj).unwrap_or_default()),
        }
    }

    /// Set a setting value by dotted path.
    ///
    /// Returns error if path is invalid or value cannot be parsed.
    // A dotted settings path and its textual value are free-form strings with no invariant a newtype could enforce.
    // @codescene(disable:"String Heavy Function Arguments")
    pub fn set(&mut self, path: &str, value: &str) -> Result<(), String> {
        let mut json = serde_json::to_value(&self)
            .map_err(|e| format!("Failed to serialize settings: {}", e))?;

        let parts: Vec<&str> = path.split('.').collect();
        if parts.is_empty() {
            return Err("Empty path".to_string());
        }

        let final_key = *parts.last().unwrap();
        let obj = parent_object_mut(&mut json, &parts, path)?;

        let assignment = RawAssignment { path, value };
        let new_value = coerce_value(obj.get(final_key), &assignment)?;
        obj.insert(final_key.to_string(), new_value);

        // Deserialize back to Settings
        *self =
            serde_json::from_value(json).map_err(|e| format!("Failed to apply setting: {}", e))?;

        Ok(())
    }

    /// Reset a setting to its default value.
    pub fn reset(&mut self, path: &str) -> Result<(), String> {
        let default = Self::default();
        let default_value = default
            .get(path)
            .ok_or_else(|| format!("Unknown setting: {}", path))?;

        self.set(path, &default_value)
    }

    /// List all settings as (path, value) pairs.
    pub fn list(&self) -> Vec<(String, String)> {
        let json = match serde_json::to_value(self) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };

        let mut results = Vec::new();
        collect_settings(&json, String::new(), &mut results);
        results.sort_by(|a, b| a.0.cmp(&b.0));
        results
    }
}

/// Navigate to the object holding the final key of a dotted path.
fn parent_object_mut<'a>(
    json: &'a mut serde_json::Value,
    parts: &[&str],
    path: &str,
) -> Result<&'a mut serde_json::Map<String, serde_json::Value>, String> {
    let mut current = json;
    for part in &parts[..parts.len() - 1] {
        current = current
            .get_mut(*part)
            .ok_or_else(|| format!("Path not found: {}", path))?;
    }
    current
        .as_object_mut()
        .ok_or_else(|| format!("Parent is not an object: {}", path))
}

/// A textual assignment to a dotted settings path, as received from the
/// caller. Groups the path (used in error messages) with the raw value to
/// be coerced.
struct RawAssignment<'a> {
    path: &'a str,
    value: &'a str,
}

/// Coerce a textual value to a JSON value, inferring the target type from
/// the existing value at the path (if any).
fn coerce_value(
    existing: Option<&serde_json::Value>,
    assignment: &RawAssignment<'_>,
) -> Result<serde_json::Value, String> {
    let RawAssignment { path, value } = *assignment;
    let Some(existing) = existing else {
        // Key doesn't exist, try to parse as JSON or use string
        return Ok(
            serde_json::from_str(value).unwrap_or(serde_json::Value::String(value.to_string()))
        );
    };

    match existing {
        serde_json::Value::Bool(_) => {
            let b = value
                .parse::<bool>()
                .map_err(|_| format!("Expected boolean for {}, got '{}'", path, value))?;
            Ok(serde_json::Value::Bool(b))
        }
        serde_json::Value::Number(n) => coerce_number(n, assignment),
        serde_json::Value::Null => {
            // Could be Option<T>, try to parse as JSON or use string
            Ok(serde_json::from_str(value).unwrap_or(serde_json::Value::String(value.to_string())))
        }
        serde_json::Value::Array(_) => serde_json::from_str(value)
            .map_err(|e| format!("Invalid JSON array for {}: {}", path, e)),
        serde_json::Value::Object(_) => serde_json::from_str(value)
            .map_err(|e| format!("Invalid JSON object for {}: {}", path, e)),
        serde_json::Value::String(_) => Ok(serde_json::Value::String(value.to_string())),
    }
}

/// Coerce a textual value to a JSON number matching the width and sign of
/// the existing number (unsigned, signed, or floating point).
fn coerce_number(
    existing: &serde_json::Number,
    assignment: &RawAssignment<'_>,
) -> Result<serde_json::Value, String> {
    let RawAssignment { path, value } = *assignment;
    if existing.is_u64() {
        let n = value
            .parse::<u64>()
            .map_err(|_| format!("Expected integer for {}, got '{}'", path, value))?;
        Ok(serde_json::Value::Number(n.into()))
    } else if existing.is_i64() {
        let n = value
            .parse::<i64>()
            .map_err(|_| format!("Expected integer for {}, got '{}'", path, value))?;
        Ok(serde_json::Value::Number(n.into()))
    } else {
        let n = value
            .parse::<f64>()
            .map_err(|_| format!("Expected number for {}, got '{}'", path, value))?;
        Ok(serde_json::Number::from_f64(n)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::String(value.to_string())))
    }
}

/// Recursively collect settings paths and values.
fn collect_settings(
    value: &serde_json::Value,
    prefix: String,
    results: &mut Vec<(String, String)>,
) {
    match value {
        serde_json::Value::Object(obj) => {
            for (key, val) in obj {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", prefix, key)
                };
                collect_settings(val, path, results);
            }
        }
        serde_json::Value::Array(arr) => {
            let display = serde_json::to_string(arr).unwrap_or_default();
            results.push((prefix, display));
        }
        serde_json::Value::String(s) => {
            results.push((prefix, s.clone()));
        }
        serde_json::Value::Number(n) => {
            results.push((prefix, n.to_string()));
        }
        serde_json::Value::Bool(b) => {
            results.push((prefix, b.to_string()));
        }
        serde_json::Value::Null => {
            results.push((prefix, "null".to_string()));
        }
    }
}

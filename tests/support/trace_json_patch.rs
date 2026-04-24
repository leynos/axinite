//! Shared JSON patch helper for trace-based test support modules.

/// Recursively patch string values in a JSON value, replacing `from` with `to`.
pub(crate) fn patch_json_value(value: &mut serde_json::Value, from: &str, to: &str) {
    match value {
        serde_json::Value::String(s) if s.contains(from) => {
            *s = s.replace(from, to);
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                patch_json_value(item, from, to);
            }
        }
        serde_json::Value::Object(obj) => {
            for value in obj.values_mut() {
                patch_json_value(value, from, to);
            }
        }
        _ => {}
    }
}

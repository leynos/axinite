// Verifies that ParamName is accepted at require_str / require_param call sites.
fn main() {
    use ironclaw::tools::{require_param, require_str, ParamName};
    let params = serde_json::json!({"key": "value"});
    let _ = require_str(&params, ParamName::from("key"));
    let _ = require_param(&params, ParamName::from("key"));
}

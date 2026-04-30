// Verifies that a plain &str is accepted by require_str and require_param.
fn main() {
    use ironclaw::tools::{require_param, require_str};
    let params = serde_json::json!({"key": "value"});
    let _ = require_str(&params, "key");
    let _ = require_param(&params, "key");
}

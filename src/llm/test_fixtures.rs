//! Shared test fixtures for LLM schema and adapter tests.
//!
//! These fixtures keep representative provider-facing schemas in one place so
//! normalization and adapter tests exercise the same inputs without drifting.

use rstest::fixture;
use serde_json::Value as JsonValue;

#[fixture]
pub(crate) fn github_style_schema() -> JsonValue {
    serde_json::json!({
        "type": "object",
        "required": ["action"],
        "oneOf": [
            {
                "properties": {
                    "action": { "const": "get_repo" },
                    "owner": { "type": "string" },
                    "repo": { "type": "string" }
                },
                "required": ["action", "owner", "repo"]
            },
            {
                "properties": {
                    "action": { "const": "create_issue" },
                    "owner": { "type": "string" },
                    "repo": { "type": "string" },
                    "title": { "type": "string" }
                },
                "required": ["action", "owner", "repo", "title"]
            }
        ]
    })
}

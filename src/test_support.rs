//! Shared test fixtures and builders for complex tool definitions.
//!
//! This module provides canonical complex tool definition builders that are
//! reused across both orchestrator and worker test suites to ensure consistency
//! and reduce duplication.

use crate::llm::ToolDefinition;

/// Returns the canonical complex parameters JSON schema used for fidelity testing.
///
/// This schema exercises nested objects, arrays, enums, constraints, and various
/// JSON Schema features to validate that tool definitions survive serialization,
/// transport, and reconstruction without data loss.
pub fn complex_tool_definition_parameters() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "title": "ComplexParams",
        "description": "Nested schema with multiple property types",
        "properties": {
            "query": {
                "type": "string",
                "description": "Search query with constraints",
                "minLength": 1,
                "maxLength": 500
            },
            "options": {
                "type": "object",
                "description": "Nested configuration object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 100,
                        "default": 10
                    },
                    "include_metadata": {
                        "type": "boolean",
                        "default": false
                    },
                    "filters": {
                        "type": "array",
                        "items": {
                            "type": "string",
                            "enum": ["active", "archived", "draft"]
                        }
                    }
                },
                "required": ["limit"]
            },
            "callback_url": {
                "type": "string",
                "format": "uri",
                "description": "Optional webhook URL"
            }
        },
        "required": ["query", "options"],
        "additionalProperties": false
    })
}

/// Builds a complex tool definition for fidelity testing.
///
/// Constructs a `ToolDefinition` with the given name and description, using
/// the canonical complex parameters schema. This ensures both orchestrator and
/// worker tests use identical parameter structures while allowing test-specific
/// names and descriptions.
///
/// # Arguments
///
/// * `name` - The tool name (e.g., "remote_tool_fidelity_fixture")
/// * `description` - The tool description (should include complex UTF-8, markdown, etc.)
///
/// # Returns
///
/// A `ToolDefinition` with the canonical complex parameters schema.
pub fn build_complex_tool_definition(name: &str, description: &str) -> ToolDefinition {
    ToolDefinition {
        name: name.to_string(),
        description: description.to_string(),
        parameters: complex_tool_definition_parameters(),
    }
}

//! Tests for MCP tool definitions, annotations, and approval hints.

use super::super::*;

#[test]
fn test_mcp_tool_deserialize_camel_case_input_schema() {
    // MCP protocol uses camelCase "inputSchema"
    let json = serde_json::json!({
        "name": "list_issues",
        "description": "List GitHub issues",
        "inputSchema": {
            "type": "object",
            "properties": {
                "owner": { "type": "string" },
                "repo": { "type": "string" }
            },
            "required": ["owner", "repo"]
        }
    });

    let tool: McpTool = serde_json::from_value(json).expect("deserialize McpTool");
    assert_eq!(tool.name, "list_issues");
    assert_eq!(tool.description, "List GitHub issues");

    // The schema must have the properties, not the empty default
    let props = tool.input_schema.get("properties").expect("has properties");
    assert!(props.get("owner").is_some());
    assert!(props.get("repo").is_some());
}

#[test]
fn test_mcp_tool_deserialize_snake_case_alias() {
    // Also accept snake_case "input_schema" for flexibility
    let json = serde_json::json!({
        "name": "search",
        "description": "Search",
        "input_schema": {
            "type": "object",
            "properties": {
                "query": { "type": "string" }
            }
        }
    });

    let tool: McpTool = serde_json::from_value(json).expect("deserialize McpTool");
    let props = tool.input_schema.get("properties").expect("has properties");
    assert!(props.get("query").is_some());
}

#[test]
fn test_mcp_tool_missing_schema_gets_default() {
    let json = serde_json::json!({
        "name": "ping",
        "description": "Ping"
    });

    let tool: McpTool = serde_json::from_value(json).expect("deserialize McpTool");
    assert_eq!(tool.input_schema["type"], "object");
    assert!(tool.input_schema["properties"].is_object());
}

#[test]
fn test_requires_approval_with_destructive_hint() {
    let tool = McpTool {
        name: "delete_all".to_string(),
        description: "Deletes everything".to_string(),
        input_schema: default_input_schema(),
        annotations: Some(McpToolAnnotations {
            destructive_hint: true,
            ..Default::default()
        }),
    };
    assert!(tool.requires_approval());
}

#[test]
fn test_requires_approval_without_destructive_hint() {
    let tool = McpTool {
        name: "read_file".to_string(),
        description: "Reads a file".to_string(),
        input_schema: default_input_schema(),
        annotations: Some(McpToolAnnotations {
            destructive_hint: false,
            read_only_hint: true,
            ..Default::default()
        }),
    };
    assert!(!tool.requires_approval());
}

#[test]
fn test_requires_approval_no_annotations() {
    let tool = McpTool {
        name: "ping".to_string(),
        description: "Ping".to_string(),
        input_schema: default_input_schema(),
        annotations: None,
    };
    assert!(!tool.requires_approval());
}

#[test]
fn test_mcp_tool_annotations_defaults() {
    let annotations = McpToolAnnotations::default();
    assert!(!annotations.destructive_hint);
    assert!(!annotations.side_effects_hint);
    assert!(!annotations.read_only_hint);
    assert!(annotations.execution_time_hint.is_none());
}

#[test]
fn test_execution_time_hint_serde() {
    // Fast
    let json = serde_json::json!("fast");
    let hint: ExecutionTimeHint = serde_json::from_value(json).expect("deserialize fast");
    assert_eq!(hint, ExecutionTimeHint::Fast);
    let serialized = serde_json::to_value(hint).expect("serialize fast");
    assert_eq!(serialized, "fast");

    // Medium
    let json = serde_json::json!("medium");
    let hint: ExecutionTimeHint = serde_json::from_value(json).expect("deserialize medium");
    assert_eq!(hint, ExecutionTimeHint::Medium);
    let serialized = serde_json::to_value(hint).expect("serialize medium");
    assert_eq!(serialized, "medium");

    // Slow
    let json = serde_json::json!("slow");
    let hint: ExecutionTimeHint = serde_json::from_value(json).expect("deserialize slow");
    assert_eq!(hint, ExecutionTimeHint::Slow);
    let serialized = serde_json::to_value(hint).expect("serialize slow");
    assert_eq!(serialized, "slow");
}

#[test]
fn test_mcp_tool_roundtrip_preserves_schema() {
    // Simulate what list_tools returns from a real MCP server
    let server_response = serde_json::json!({
        "tools": [{
            "name": "github-copilot_list_issues",
            "description": "List issues for a repository",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "owner": { "type": "string", "description": "Repository owner" },
                    "repo": { "type": "string", "description": "Repository name" },
                    "state": { "type": "string", "enum": ["open", "closed", "all"] }
                },
                "required": ["owner", "repo"]
            }
        }]
    });

    let result: ListToolsResult =
        serde_json::from_value(server_response).expect("deserialize ListToolsResult");
    assert_eq!(result.tools.len(), 1);

    let tool = &result.tools[0];
    assert_eq!(tool.name, "github-copilot_list_issues");

    let required = tool.input_schema.get("required").expect("has required");
    assert!(required.as_array().expect("is array").len() == 2);
}

//! Test coverage for MCP wrapper policies and integration helpers.

use std::sync::Arc;

use rstest::rstest;

use super::client::{McpClient, McpToolWrapper};
use super::protocol::{McpTool, McpToolAnnotations};
use crate::tools::tool::{HostedToolEligibility, Tool};

#[rstest]
#[case(
    McpTool {
        name: "delete_all".to_string(),
        description: "Deletes everything".to_string(),
        input_schema: serde_json::json!({"type": "object"}),
        annotations: Some(McpToolAnnotations {
            destructive_hint: true,
            side_effects_hint: false,
            read_only_hint: false,
            execution_time_hint: None,
        }),
    },
    HostedToolEligibility::ApprovalGated
)]
#[case(
    McpTool {
        name: "simple_tool".to_string(),
        description: "A simple tool".to_string(),
        input_schema: serde_json::json!({"type": "object"}),
        annotations: None,
    },
    HostedToolEligibility::Eligible
)]
fn test_mcp_tool_wrapper_hosted_tool_eligibility(
    #[case] tool: McpTool,
    #[case] expected: HostedToolEligibility,
) {
    let prefixed_name = format!("mcp__{}", tool.name);
    let wrapper = McpToolWrapper {
        prefixed_name,
        tool,
        client: Arc::new(McpClient::new("http://localhost:1234")),
    };

    assert_eq!(wrapper.hosted_tool_eligibility(), expected);
}

use std::sync::Arc;

use super::*;

#[test]
fn test_tool_search_schema() {
    let tool = ToolSearchTool::new(test_manager_stub());
    assert_eq!(tool.name(), "tool_search");
    let schema = tool.parameters_schema();
    assert!(schema.get("properties").is_some());
    assert!(schema["properties"].get("query").is_some());
}

#[test]
fn test_tool_install_schema() {
    use crate::tools::tool::ApprovalRequirement;
    let tool = ToolInstallTool::new(test_manager_stub());
    assert_eq!(tool.name(), "tool_install");
    assert_eq!(
        tool.requires_approval(&serde_json::json!({})),
        ApprovalRequirement::UnlessAutoApproved
    );
    let schema = tool.parameters_schema();
    assert!(schema["properties"].get("name").is_some());
    assert!(schema["properties"].get("url").is_some());
}

#[test]
fn test_tool_auth_schema() {
    use crate::tools::tool::ApprovalRequirement;
    let tool = ToolAuthTool::new(test_manager_stub());
    assert_eq!(tool.name(), "tool_auth");
    assert_eq!(
        tool.requires_approval(&serde_json::json!({})),
        ApprovalRequirement::UnlessAutoApproved
    );
    let schema = tool.parameters_schema();
    assert!(schema["properties"].get("name").is_some());
    assert!(
        schema["properties"].get("token").is_none(),
        "tool_auth must not have a token parameter"
    );
}

#[test]
fn test_tool_activate_schema() {
    use crate::tools::tool::ApprovalRequirement;
    let tool = ToolActivateTool::new(test_manager_stub());
    assert_eq!(tool.name(), "tool_activate");
    assert_eq!(
        tool.requires_approval(&serde_json::json!({})),
        ApprovalRequirement::UnlessAutoApproved
    );
}

#[test]
fn test_tool_list_schema() {
    use crate::tools::tool::ApprovalRequirement;
    let tool = ToolListTool::new(test_manager_stub());
    assert_eq!(tool.name(), "tool_list");
    assert_eq!(
        tool.requires_approval(&serde_json::json!({})),
        ApprovalRequirement::Never
    );
    let schema = tool.parameters_schema();
    assert!(schema["properties"].get("kind").is_some());
}

#[test]
fn test_tool_remove_schema() {
    use crate::tools::tool::ApprovalRequirement;
    let tool = ToolRemoveTool::new(test_manager_stub());
    assert_eq!(tool.name(), "tool_remove");
    assert_eq!(
        tool.requires_approval(&serde_json::json!({})),
        ApprovalRequirement::Always
    );
}

#[test]
fn tool_remove_always_requires_approval_regardless_of_params() {
    use crate::tools::tool::ApprovalRequirement;
    let tool = ToolRemoveTool::new(test_manager_stub());

    let test_cases = vec![
        ("no params", serde_json::json!({})),
        ("empty name", serde_json::json!({"name": ""})),
        ("slack", serde_json::json!({"name": "slack"})),
        ("github-cli", serde_json::json!({"name": "github-cli"})),
        (
            "with extra fields",
            serde_json::json!({"name": "tool", "extra": "field"}),
        ),
    ];

    for (case_name, params) in test_cases {
        assert_eq!(
            tool.requires_approval(&params),
            ApprovalRequirement::Always,
            "tool_remove must always require approval for case: {}",
            case_name
        );
    }
}

#[test]
fn test_tool_upgrade_schema() {
    use crate::tools::tool::ApprovalRequirement;
    let tool = ToolUpgradeTool::new(test_manager_stub());
    assert_eq!(tool.name(), "tool_upgrade");
    assert_eq!(
        tool.requires_approval(&serde_json::json!({})),
        ApprovalRequirement::UnlessAutoApproved
    );
    let schema = tool.parameters_schema();
    assert!(schema["properties"].get("name").is_some());
    assert!(
        schema.get("required").is_none(),
        "tool_upgrade should have no required params"
    );
}

#[test]
fn test_extension_info_schema() {
    let tool = ExtensionInfoTool::new(test_manager_stub());
    assert_eq!(tool.name(), "extension_info");
    let schema = tool.parameters_schema();
    assert!(schema["properties"].get("name").is_some());
    let required = schema["required"]
        .as_array()
        .expect("extension_info required array");
    assert!(required.iter().any(|v| v.as_str() == Some("name")));
}

/// Create a stub manager for schema tests (these don't call execute).
fn test_manager_stub() -> Arc<ExtensionManager> {
    use crate::secrets::{InMemorySecretsStore, SecretsCrypto};
    use crate::testing::credentials::TEST_CRYPTO_KEY;
    use crate::tools::ToolRegistry;
    use crate::tools::mcp::session::McpSessionManager;

    let master_key = secrecy::SecretString::from(TEST_CRYPTO_KEY.to_string());
    let crypto = Arc::new(SecretsCrypto::new(master_key).expect("create secrets crypto"));

    Arc::new(ExtensionManager::new(
        Arc::new(McpSessionManager::new()),
        Arc::new(crate::tools::mcp::McpProcessManager::new()),
        Arc::new(InMemorySecretsStore::new(crypto)),
        Arc::new(ToolRegistry::new()),
        None,
        None,
        std::env::temp_dir().join("ironclaw-test-tools"),
        std::env::temp_dir().join("ironclaw-test-channels"),
        None,
        "test".to_string(),
        None,
        Vec::new(),
    ))
}

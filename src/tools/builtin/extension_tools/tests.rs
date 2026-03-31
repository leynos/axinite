//! Tests for extension-management tools and their delegated metadata.

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

#[test]
fn hosted_worker_proxy_safety_is_explicit() {
    use crate::tools::tool::ApprovalRequirement;

    for safe_kind in [
        ExtensionToolKind::Search,
        ExtensionToolKind::Activate,
        ExtensionToolKind::List,
        ExtensionToolKind::Info,
    ] {
        assert!(
            safe_kind.is_hosted_worker_proxy_safe(),
            "{safe_kind:?} should stay in the hosted-worker allowlist"
        );
    }

    for restricted_kind in [
        ExtensionToolKind::Install,
        ExtensionToolKind::Auth,
        ExtensionToolKind::Remove,
        ExtensionToolKind::Upgrade,
    ] {
        assert!(
            !restricted_kind.is_hosted_worker_proxy_safe(),
            "{restricted_kind:?} should stay out of the hosted-worker allowlist"
        );
    }

    assert_eq!(
        ExtensionToolKind::Activate.approval_requirement(),
        ApprovalRequirement::UnlessAutoApproved
    );
    assert!(
        ExtensionToolKind::Activate.is_hosted_worker_proxy_safe(),
        "hosted visibility must not be inferred from blanket Never approval"
    );
}

/// Create a stub manager for schema tests (these don't call execute).
fn test_manager_stub() -> Arc<ExtensionManager> {
    use crate::secrets::{InMemorySecretsStore, SecretsCrypto};
    use crate::testing::credentials::TEST_CRYPTO_KEY;
    use crate::tools::ToolRegistry;

    let master_key = secrecy::SecretString::from(TEST_CRYPTO_KEY.to_string());
    let crypto = Arc::new(SecretsCrypto::new(master_key).expect("create secrets crypto"));
    let mcp_clients = Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new()));

    Arc::new(ExtensionManager::new(
        crate::extensions::ExtensionManagerConfig {
            discovery: Arc::new(crate::extensions::NoOpDiscovery),
            relay_config: None,
            gateway_token: None,
            mcp_activation: Arc::new(crate::extensions::NoOpMcpActivation),
            wasm_tool_activation: Arc::new(crate::extensions::NoOpWasmToolActivation),
            wasm_channel_activation: Arc::new(crate::extensions::NoOpWasmChannelActivation),
            mcp_clients,
            secrets: Arc::new(InMemorySecretsStore::new(crypto)),
            tool_registry: Arc::new(ToolRegistry::new()),
            hooks: None,
            wasm_tools_dir: std::env::temp_dir().join("ironclaw-test-tools"),
            wasm_channels_dir: std::env::temp_dir().join("ironclaw-test-channels"),
            tunnel_url: None,
            user_id: "test".to_string(),
            store: None,
            catalog_entries: Vec::new(),
        },
    ))
}

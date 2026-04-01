//! Validates that all built-in tool schemas conform to OpenAI strict-mode rules.
//!
//! This catches the class of bugs where `required` keys aren't in `properties`,
//! properties are missing `type` (intentional freeform is allowed), or nested
//! objects/arrays are malformed.
//!
//! See: <https://github.com/nearai/ironclaw/issues/352> (QA plan, item 1.1)

use ironclaw::tools::builtin::extension_tools::ExtensionToolKind;
use ironclaw::tools::schema_validator::validate_strict_schema;
use ironclaw::tools::validate_tool_schema;
use ironclaw::tools::wasm::WasmToolLoader;
use ironclaw::tools::{Tool, ToolRegistry};
use rstest::{fixture, rstest};
use std::sync::Arc;

struct ExtensionManagerFixture {
    _dir: tempfile::TempDir,
    manager: Arc<ironclaw::extensions::ExtensionManager>,
}

#[fixture]
fn extension_manager_fixture() -> ExtensionManagerFixture {
    use ironclaw::secrets::{InMemorySecretsStore, SecretsCrypto};

    let dir = tempfile::tempdir().expect("temp dir");
    let tools_dir = dir.path().join("tools");
    let channels_dir = dir.path().join("channels");
    std::fs::create_dir_all(&tools_dir).expect("create tools dir");
    std::fs::create_dir_all(&channels_dir).expect("create channels dir");

    let master_key = secrecy::SecretString::from("0123456789abcdef0123456789abcdef".to_string());
    let crypto = std::sync::Arc::new(SecretsCrypto::new(master_key).expect("crypto"));

    let mcp_clients = ironclaw::extensions::McpClientsMap::default();

    ExtensionManagerFixture {
        _dir: dir,
        manager: std::sync::Arc::new(ironclaw::extensions::ExtensionManager::new(
            ironclaw::extensions::ExtensionManagerConfig {
                discovery: std::sync::Arc::new(ironclaw::extensions::NoOpDiscovery),
                relay_config: None,
                gateway_token: None,
                mcp_activation: std::sync::Arc::new(ironclaw::extensions::NoOpMcpActivation),
                wasm_tool_activation: std::sync::Arc::new(
                    ironclaw::extensions::NoOpWasmToolActivation,
                ),
                wasm_channel_activation: std::sync::Arc::new(
                    ironclaw::extensions::NoOpWasmChannelActivation,
                ),
                mcp_clients,
                secrets: std::sync::Arc::new(InMemorySecretsStore::new(crypto)),
                tool_registry: std::sync::Arc::new(ToolRegistry::new()),
                hooks: None,
                wasm_tools_dir: tools_dir,
                wasm_channels_dir: channels_dir,
                tunnel_url: None,
                user_id: "test".to_string(),
                store: None,
                catalog_entries: Vec::new(),
            },
        )),
    }
}

/// Validate schemas of all tools registered via `register_builtin_tools()` and
/// `register_dev_tools()` (echo, time, json, http, shell, file tools).
///
/// These tools can be constructed without external dependencies (no DB, no
/// workspace, no extension manager). Tools requiring dependencies (memory, job,
/// skill, extension, routine) are validated individually below where test
/// construction helpers exist.
#[tokio::test]
async fn all_core_builtin_tool_schemas_are_valid() {
    let registry = ToolRegistry::new();
    registry.register_builtin_tools();
    registry.register_dev_tools();

    let tools = registry.all().await;
    assert!(
        !tools.is_empty(),
        "registry should have tools after registration"
    );

    let mut all_errors = Vec::new();
    for tool in &tools {
        let schema = tool.parameters_schema();
        if let Err(errors) = validate_strict_schema(&schema, tool.name()) {
            all_errors.push(format!(
                "Tool '{}' has schema errors:\n  {}",
                tool.name(),
                errors.join("\n  ")
            ));
        }
    }

    assert!(
        all_errors.is_empty(),
        "Tool schema validation failures:\n{}",
        all_errors.join("\n\n")
    );
}

/// Verify the exact set of tools registered by the core registration methods.
/// This guards against a new tool being added without schema validation coverage.
#[tokio::test]
async fn core_registration_covers_expected_tools() {
    let registry = ToolRegistry::new();
    registry.register_builtin_tools();
    registry.register_dev_tools();

    let mut names = registry.list().await;
    names.sort();

    let expected = &[
        "apply_patch",
        "echo",
        "http",
        "json",
        "list_dir",
        "read_file",
        "shell",
        "time",
        "write_file",
    ];

    assert_eq!(
        names, expected,
        "Core tool set changed. Update this test and ensure new tools have valid schemas."
    );
}

#[rstest]
#[tokio::test]
async fn extension_registration_covers_expected_tools(
    extension_manager_fixture: ExtensionManagerFixture,
) {
    let registry = ToolRegistry::new();
    registry.register_extension_tools(Arc::clone(&extension_manager_fixture.manager));

    let mut names = registry.list().await;
    names.sort();

    let mut expected: Vec<&str> = ExtensionToolKind::ALL
        .into_iter()
        .map(ExtensionToolKind::name)
        .collect();
    expected.sort_unstable();

    assert_eq!(
        names, expected,
        "Extension tool set changed. Update this test and ensure new tools have schema coverage."
    );
}

#[rstest]
#[tokio::test]
async fn extension_tool_schemas_are_valid(extension_manager_fixture: ExtensionManagerFixture) {
    let registry = ToolRegistry::new();
    registry.register_extension_tools(Arc::clone(&extension_manager_fixture.manager));

    let tools = registry.all().await;
    let mut all_errors = Vec::new();
    for tool in &tools {
        let schema = tool.parameters_schema();
        if let Err(errors) = validate_strict_schema(&schema, tool.name()) {
            all_errors.push(format!(
                "Tool '{}' has schema errors:\n  {}",
                tool.name(),
                errors.join("\n  ")
            ));
        }
    }

    assert!(
        all_errors.is_empty(),
        "Extension tool schema validation failures:\n{}",
        all_errors.join("\n\n")
    );
}

/// Validate individual tool schemas that are known to use non-trivial patterns.
/// These are regression tests for specific bugs.
#[test]
fn json_tool_freeform_data_field_is_valid() {
    // Regression: json tool's "data" field intentionally has no "type" for
    // OpenAI compatibility (union types with arrays require "items").
    let tool = ironclaw::tools::builtin::JsonTool;
    let schema = tool.parameters_schema();
    let errors = validate_tool_schema(&schema, "json");
    assert!(errors.is_empty(), "json tool schema errors: {errors:?}");

    // Verify the freeform pattern is still in place
    let data = schema
        .get("properties")
        .and_then(|p| p.get("data"))
        .expect("json tool should have 'data' property");
    assert!(
        data.get("type").is_none(),
        "json.data should be freeform (no type) for OpenAI compatibility"
    );
}

#[test]
fn http_tool_headers_array_is_valid() {
    // Regression: http tool's "headers" is an array of {name, value} objects.
    let tool = ironclaw::tools::builtin::HttpTool::new();
    let schema = tool.parameters_schema();
    let errors = validate_tool_schema(&schema, "http");
    assert!(errors.is_empty(), "http tool schema errors: {errors:?}");

    // Verify array structure
    let headers = schema
        .get("properties")
        .and_then(|p| p.get("headers"))
        .expect("http tool should have 'headers' property");
    assert_eq!(
        headers.get("type").and_then(|t| t.as_str()),
        Some("array"),
        "headers should be an array"
    );
    assert!(
        headers.get("items").is_some(),
        "headers array should have items defined"
    );
}

#[test]
fn time_tool_schema_is_valid() {
    let tool = ironclaw::tools::builtin::TimeTool;
    let schema = tool.parameters_schema();
    let errors = validate_tool_schema(&schema, "time");
    assert!(errors.is_empty(), "time tool schema errors: {errors:?}");
}

#[test]
fn shell_tool_schema_is_valid() {
    let tool = ironclaw::tools::builtin::ShellTool::new();
    let schema = tool.parameters_schema();
    let errors = validate_tool_schema(&schema, "shell");
    assert!(errors.is_empty(), "shell tool schema errors: {errors:?}");
}

/// Validates that all core tools work correctly under a multi-threaded tokio runtime.
/// This catches sync-async boundary bugs like tokio::sync::RwLock::blocking_read()
/// panicking when called from within a multi-threaded runtime context.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn all_core_tools_work_in_multi_thread_runtime() {
    let registry = ToolRegistry::new();
    registry.register_builtin_tools();
    registry.register_dev_tools();

    let tools = registry.all().await;
    assert!(
        !tools.is_empty(),
        "registry should have tools after registration"
    );

    for tool in &tools {
        // These sync trait methods must not panic in multi-thread runtime
        let _ = tool.name();
        let _ = tool.description();
        let _ = tool.parameters_schema();
        let _ = tool.requires_approval(&serde_json::json!({}));
        let _ = tool.requires_sanitization();
        let _ = tool.domain();
    }
}

#[tokio::test]
async fn file_loaded_github_wasm_tool_definitions_publish_real_schema() {
    let source_dir = crate::support::github_tool_source_dir();
    let wasm_path =
        crate::support::github_wasm_artifact().expect("build or find github WASM artifact");
    let caps_path = source_dir.join("github-tool.capabilities.json");
    assert!(
        caps_path.exists(),
        "github capabilities sidecar must exist for schema tests: {}",
        caps_path.display()
    );

    let registry = Arc::new(ToolRegistry::new());
    let runtime = crate::support::metadata_test_runtime().expect("create metadata test runtime");
    let loader = WasmToolLoader::new(runtime, Arc::clone(&registry));

    loader
        .load_from_files("github", &wasm_path, Some(&caps_path))
        .await
        .expect("load github wasm tool");

    let defs = registry.tool_definitions().await;
    let github = defs
        .iter()
        .find(|def| def.name == "github")
        .expect("github tool definition");

    if let Err(errors) = validate_strict_schema(&github.parameters, "github") {
        panic!("github tool definition must satisfy strict validation: {errors:#?}");
    }
    assert_eq!(github.parameters["type"], serde_json::json!("object"));
    assert!(
        github.parameters.get("oneOf").is_none(),
        "top-level oneOf is rejected by OpenAI tool schemas: {}",
        github.parameters
    );
    assert!(
        github.parameters["required"]
            .as_array()
            .expect("required array")
            .iter()
            .any(|value| value == "action"),
        "expected required action in tool definition: {}",
        github.parameters
    );
    assert!(
        github.parameters["properties"]["owner"].is_object(),
        "expected owner property in tool definition: {}",
        github.parameters
    );
    assert!(
        github.parameters["properties"]["action"]["enum"]
            .as_array()
            .expect("action enum")
            .iter()
            .any(|value| value == "get_repo"),
        "expected get_repo action enum in tool definition: {}",
        github.parameters
    );
    assert!(
        github.description.contains("GitHub integration"),
        "expected real github description, got: {}",
        github.description
    );
}

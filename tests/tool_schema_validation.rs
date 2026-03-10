//! Validates that all built-in tool schemas conform to OpenAI strict-mode rules.
//!
//! This catches the class of bugs where `required` keys aren't in `properties`,
//! properties are missing `type` (intentional freeform is allowed), or nested
//! objects/arrays are malformed.
//!
//! See: <https://github.com/nearai/ironclaw/issues/352> (QA plan, item 1.1)

use ironclaw::tools::builtin::extension_tools::ExtensionToolKind;
use ironclaw::tools::validate_tool_schema;
use ironclaw::tools::wasm::{WasmRuntimeConfig, WasmToolLoader, WasmToolRuntime};
use ironclaw::tools::{Tool, ToolRegistry};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

struct ExtensionManagerFixture {
    _dir: tempfile::TempDir,
    manager: Arc<ironclaw::extensions::ExtensionManager>,
}
fn github_artifact_paths() -> Option<(PathBuf, PathBuf)> {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source_dir = repo_root.join("tools-src/github");
    let wasm_path =
        ironclaw::registry::artifacts::find_wasm_artifact(&source_dir, "github-tool", "release")?;
    let caps_path = source_dir.join("github-tool.capabilities.json");
    caps_path.exists().then_some((wasm_path, caps_path))
}

fn wasm_metadata_test_runtime() -> Arc<WasmToolRuntime> {
    let config = WasmRuntimeConfig {
        default_limits: ironclaw::tools::wasm::ResourceLimits::default()
            .with_memory(8 * 1024 * 1024)
            .with_fuel(100_000)
            .with_timeout(Duration::from_secs(5)),
        ..WasmRuntimeConfig::for_testing()
    };
    Arc::new(WasmToolRuntime::new(config).expect("create wasm runtime"))
}

fn test_extension_manager() -> ExtensionManagerFixture {
    use ironclaw::secrets::{InMemorySecretsStore, SecretsCrypto};
    use ironclaw::tools::mcp::session::McpSessionManager;

    let dir = tempfile::tempdir().expect("temp dir");
    let tools_dir = dir.path().join("tools");
    let channels_dir = dir.path().join("channels");
    std::fs::create_dir_all(&tools_dir).expect("create tools dir");
    std::fs::create_dir_all(&channels_dir).expect("create channels dir");

    let master_key = secrecy::SecretString::from("0123456789abcdef0123456789abcdef".to_string());
    let crypto = std::sync::Arc::new(SecretsCrypto::new(master_key).expect("crypto"));

    ExtensionManagerFixture {
        _dir: dir,
        manager: std::sync::Arc::new(ironclaw::extensions::ExtensionManager::new(
            std::sync::Arc::new(McpSessionManager::new()),
            std::sync::Arc::new(ironclaw::tools::mcp::process::McpProcessManager::new()),
            std::sync::Arc::new(InMemorySecretsStore::new(crypto)),
            std::sync::Arc::new(ToolRegistry::new()),
            None,
            None,
            tools_dir,
            channels_dir,
            None,
            "test".to_string(),
            None,
            Vec::new(),
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
        let errors = validate_tool_schema(&schema, tool.name());
        if !errors.is_empty() {
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

#[tokio::test]
async fn extension_registration_covers_expected_tools() {
    let fixture = test_extension_manager();
    let registry = ToolRegistry::new();
    registry.register_extension_tools(Arc::clone(&fixture.manager));

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

#[tokio::test]
async fn extension_tool_schemas_are_valid() {
    let fixture = test_extension_manager();
    let registry = ToolRegistry::new();
    registry.register_extension_tools(Arc::clone(&fixture.manager));

    let tools = registry.all().await;
    let mut all_errors = Vec::new();
    for tool in &tools {
        let schema = tool.parameters_schema();
        let errors = validate_tool_schema(&schema, tool.name());
        if !errors.is_empty() {
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

async fn file_loaded_github_wasm_tool_definitions_publish_real_schema() {
    let Some((wasm_path, caps_path)) = github_artifact_paths() else {
        eprintln!("Skipping GitHub schema regression: github WASM artifact not built");
        return;
    };

    let registry = Arc::new(ToolRegistry::new());
    let runtime = wasm_metadata_test_runtime();
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

    assert_eq!(github.parameters["type"], serde_json::json!("object"));
    assert!(
        github.parameters["required"]
            .as_array()
            .expect("required array")
            .iter()
            .any(|value| value == "action"),
        "expected required action in tool definition: {}",
        github.parameters
    );
    let first_variant = github.parameters["oneOf"]
        .as_array()
        .and_then(|variants| variants.first())
        .expect("oneOf variants");
    assert!(
        first_variant["properties"]["owner"].is_object(),
        "expected owner property in tool definition: {}",
        github.parameters
    );
    assert_eq!(
        first_variant["properties"]["action"]["const"],
        serde_json::json!("get_repo")
    );
    assert!(
        github.description.contains("GitHub integration"),
        "expected real github description, got: {}",
        github.description
    );
}

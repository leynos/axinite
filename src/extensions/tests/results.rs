//! Unit tests for extension operation results: search, install, activate,
//! installed-extension serde, and error display.

use crate::extensions::{
    ActivateResult, AuthHint, ExtensionError, ExtensionKind, ExtensionSource, InstallResult,
    InstalledExtension, RegistryEntry, ResultSource, SearchResult,
};

// ── SearchResult ─────────────────────────────────────────────────

#[test]
fn search_result_serde_registry_source() {
    // SearchResult uses #[serde(flatten)] on entry, which means
    // RegistryEntry.source (ExtensionSource) and SearchResult.source
    // (ResultSource) collide on the "source" key. The last writer wins
    // during serialization, so we test serialize-only (no roundtrip).
    let entry = RegistryEntry {
        name: "notion".to_string(),
        display_name: "Notion".to_string(),
        kind: ExtensionKind::McpServer,
        description: "Notion integration".to_string(),
        keywords: vec!["notes".to_string(), "wiki".to_string()],
        source: ExtensionSource::McpUrl {
            url: "https://mcp.notion.so".to_string(),
        },
        fallback_source: None,
        auth_hint: AuthHint::Dcr,
        version: None,
    };
    let sr = SearchResult {
        entry,
        source: ResultSource::Registry,
        validated: false,
    };
    let json = serde_json::to_value(&sr).unwrap();
    assert_eq!(json["name"], "notion");
    assert_eq!(json["kind"], "mcp_server");
    assert_eq!(json["description"], "Notion integration");
    assert_eq!(json["validated"], false);
    // The flattened entry fields are present at the top level
    assert!(json.get("auth_hint").is_some());
    assert_eq!(json["keywords"].as_array().unwrap().len(), 2);
}

#[test]
fn search_result_serde_discovered_source() {
    let entry = RegistryEntry {
        name: "custom-api".to_string(),
        display_name: "Custom API".to_string(),
        kind: ExtensionKind::McpServer,
        description: "Discovered MCP server".to_string(),
        keywords: vec![],
        source: ExtensionSource::Discovered {
            url: "https://custom.example.com/.well-known/mcp".to_string(),
        },
        fallback_source: None,
        auth_hint: AuthHint::None,
        version: None,
    };
    let sr = SearchResult {
        entry,
        source: ResultSource::Discovered,
        validated: true,
    };
    let json = serde_json::to_value(&sr).unwrap();
    assert_eq!(json["name"], "custom-api");
    assert_eq!(json["display_name"], "Custom API");
    assert_eq!(json["validated"], true);
    assert!(json.get("keywords").is_some());
}

// ── InstallResult ────────────────────────────────────────────────

#[test]
fn install_result_serde_roundtrip() {
    let ir = InstallResult {
        name: "weather".to_string(),
        kind: ExtensionKind::WasmTool,
        message: "Installed successfully".to_string(),
    };
    let json = serde_json::to_value(&ir).unwrap();
    assert_eq!(json["name"], "weather");
    assert_eq!(json["kind"], "wasm_tool");
    assert_eq!(json["message"], "Installed successfully");
    let back: InstallResult = serde_json::from_value(json).unwrap();
    assert_eq!(back.name, "weather");
    assert_eq!(back.kind, ExtensionKind::WasmTool);
}

// ── ActivateResult ───────────────────────────────────────────────

#[test]
fn activate_result_serde_roundtrip() {
    let ar = ActivateResult {
        name: "slack".to_string(),
        kind: ExtensionKind::WasmChannel,
        tools_loaded: vec!["send_message".to_string(), "read_channel".to_string()],
        message: "Activated with 2 tools".to_string(),
    };
    let json = serde_json::to_value(&ar).unwrap();
    assert_eq!(json["name"], "slack");
    assert_eq!(json["kind"], "wasm_channel");
    assert_eq!(json["tools_loaded"].as_array().unwrap().len(), 2);
    let back: ActivateResult = serde_json::from_value(json).unwrap();
    assert_eq!(back.tools_loaded, vec!["send_message", "read_channel"]);
}

// ── InstalledExtension ───────────────────────────────────────────

#[test]
fn installed_extension_serde_defaults() {
    // Minimal JSON: optional fields absent, defaults kick in
    let json = serde_json::json!({
        "name": "echo",
        "kind": "wasm_tool",
        "authenticated": false,
        "active": false,
    });
    let ext: InstalledExtension = serde_json::from_value(json).unwrap();
    assert_eq!(ext.name, "echo");
    assert!(ext.installed, "installed should default to true");
    assert!(!ext.needs_setup, "needs_setup should default to false");
    assert!(!ext.has_auth);
    assert!(ext.tools.is_empty());
    assert!(ext.display_name.is_none());
    assert!(ext.description.is_none());
    assert!(ext.url.is_none());
    assert!(ext.activation_error.is_none());
}

#[test]
fn installed_extension_serde_all_fields() {
    let ext = InstalledExtension {
        name: "gmail".to_string(),
        kind: ExtensionKind::WasmTool,
        display_name: Some("Gmail Tool".to_string()),
        description: Some("Read and send emails".to_string()),
        url: Some("https://gmail.example.com".to_string()),
        authenticated: true,
        active: true,
        tools: vec!["send_email".to_string(), "read_inbox".to_string()],
        needs_setup: true,
        has_auth: true,
        installed: false,
        activation_error: Some("token expired".to_string()),
        version: None,
    };
    let json = serde_json::to_value(&ext).unwrap();
    assert_eq!(json["display_name"], "Gmail Tool");
    assert_eq!(json["description"], "Read and send emails");
    assert_eq!(json["url"], "https://gmail.example.com");
    assert_eq!(json["needs_setup"], true);
    assert_eq!(json["installed"], false);
    assert_eq!(json["activation_error"], "token expired");

    let back: InstalledExtension = serde_json::from_value(json).unwrap();
    assert_eq!(back.name, "gmail");
    assert_eq!(back.tools.len(), 2);
    assert!(back.needs_setup);
    assert!(!back.installed);
    assert_eq!(back.activation_error.as_deref(), Some("token expired"));
}

// ── ExtensionError Display ───────────────────────────────────────

#[test]
fn extension_error_display_all_variants() {
    let cases: Vec<(ExtensionError, &str)> = vec![
        (
            ExtensionError::NotFound("foo".into()),
            "Extension not found: foo",
        ),
        (
            ExtensionError::AlreadyInstalled("bar".into()),
            "Extension already installed: bar",
        ),
        (
            ExtensionError::NotInstalled("baz".into()),
            "Extension not installed: baz",
        ),
        (
            ExtensionError::AuthFailed("bad token".into()),
            "Authentication failed: bad token",
        ),
        (
            ExtensionError::ActivationFailed("crash".into()),
            "Activation failed: crash",
        ),
        (
            ExtensionError::InstallFailed("disk full".into()),
            "Installation failed: disk full",
        ),
        (
            ExtensionError::DiscoveryFailed("timeout".into()),
            "Discovery failed: timeout",
        ),
        (
            ExtensionError::InvalidUrl("not a url".into()),
            "Invalid URL: not a url",
        ),
        (
            ExtensionError::DownloadFailed("404".into()),
            "Download failed: 404",
        ),
        (
            ExtensionError::Config("missing key".into()),
            "Config error: missing key",
        ),
        (ExtensionError::AuthRequired, "Authentication required"),
        (
            ExtensionError::Other("something broke".into()),
            "something broke",
        ),
        (
            ExtensionError::FallbackFailed {
                primary: Box::new(ExtensionError::DownloadFailed("404".into())),
                fallback: Box::new(ExtensionError::InstallFailed("no cargo".into())),
            },
            "Primary install failed: Download failed: 404; fallback install also failed: Installation failed: no cargo",
        ),
    ];
    for (err, expected) in cases {
        assert_eq!(err.to_string(), expected);
    }
}

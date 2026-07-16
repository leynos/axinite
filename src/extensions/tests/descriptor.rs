//! Unit tests for extension descriptor metadata: kind display, source and
//! auth-hint serde, and result-source serde.

use crate::extensions::{AuthHint, ExtensionKind, ExtensionSource, ResultSource};

// ── ExtensionKind ────────────────────────────────────────────────

#[test]
fn extension_kind_display() {
    assert_eq!(ExtensionKind::McpServer.to_string(), "mcp_server");
    assert_eq!(ExtensionKind::WasmTool.to_string(), "wasm_tool");
    assert_eq!(ExtensionKind::WasmChannel.to_string(), "wasm_channel");
}

#[test]
fn extension_kind_serde_roundtrip() {
    for kind in [
        ExtensionKind::McpServer,
        ExtensionKind::WasmTool,
        ExtensionKind::WasmChannel,
    ] {
        let json = serde_json::to_value(kind).unwrap();
        let back: ExtensionKind = serde_json::from_value(json).unwrap();
        assert_eq!(back, kind);
    }
    // Verify the serialized strings match rename_all = "snake_case"
    assert_eq!(
        serde_json::to_value(ExtensionKind::McpServer).unwrap(),
        "mcp_server"
    );
    assert_eq!(
        serde_json::to_value(ExtensionKind::WasmTool).unwrap(),
        "wasm_tool"
    );
    assert_eq!(
        serde_json::to_value(ExtensionKind::WasmChannel).unwrap(),
        "wasm_channel"
    );
}

// ── ExtensionSource ──────────────────────────────────────────────

#[test]
fn extension_source_serde_mcp_url() {
    let src = ExtensionSource::McpUrl {
        url: "https://mcp.example.com".to_string(),
    };
    let json = serde_json::to_value(&src).unwrap();
    assert_eq!(json["type"], "mcp_url");
    assert_eq!(json["url"], "https://mcp.example.com");
    let back: ExtensionSource = serde_json::from_value(json).unwrap();
    assert!(matches!(back, ExtensionSource::McpUrl { url } if url == "https://mcp.example.com"));
}

#[test]
fn extension_source_serde_wasm_download() {
    let src = ExtensionSource::WasmDownload {
        wasm_url: "https://cdn.example.com/tool.wasm".to_string(),
        capabilities_url: Some("https://cdn.example.com/caps.json".to_string()),
    };
    let json = serde_json::to_value(&src).unwrap();
    assert_eq!(json["type"], "wasm_download");
    assert_eq!(json["wasm_url"], "https://cdn.example.com/tool.wasm");
    assert_eq!(
        json["capabilities_url"],
        "https://cdn.example.com/caps.json"
    );
    let back: ExtensionSource = serde_json::from_value(json).unwrap();
    assert!(
        matches!(back, ExtensionSource::WasmDownload { capabilities_url: Some(c), .. } if c.contains("caps.json"))
    );
}

#[test]
fn extension_source_serde_wasm_buildable() {
    let src = ExtensionSource::WasmBuildable {
        source_dir: "/home/user/tools/my-tool".to_string(),
        build_dir: Some("target/wasm32-wasip2/release".to_string()),
        crate_name: Some("my_tool".to_string()),
    };
    let json = serde_json::to_value(&src).unwrap();
    assert_eq!(json["type"], "wasm_buildable");
    assert_eq!(json["source_dir"], "/home/user/tools/my-tool");
    let back: ExtensionSource = serde_json::from_value(json).unwrap();
    assert!(
        matches!(back, ExtensionSource::WasmBuildable { source_dir, .. } if source_dir.contains("my-tool"))
    );
}

#[test]
fn extension_source_serde_discovered() {
    let src = ExtensionSource::Discovered {
        url: "https://discovered.example.com".to_string(),
    };
    let json = serde_json::to_value(&src).unwrap();
    assert_eq!(json["type"], "discovered");
    let back: ExtensionSource = serde_json::from_value(json).unwrap();
    assert!(matches!(back, ExtensionSource::Discovered { url } if url.contains("discovered")));
}

// ── AuthHint ─────────────────────────────────────────────────────

#[test]
fn auth_hint_serde_all_variants() {
    // Dcr
    let json = serde_json::to_value(&AuthHint::Dcr).unwrap();
    assert_eq!(json["type"], "dcr");
    let back: AuthHint = serde_json::from_value(json).unwrap();
    assert!(matches!(back, AuthHint::Dcr));

    // OAuthPreConfigured
    let hint = AuthHint::OAuthPreConfigured {
        setup_url: "https://dev.example.com/apps".to_string(),
    };
    let json = serde_json::to_value(&hint).unwrap();
    assert_eq!(json["type"], "o_auth_pre_configured");
    assert_eq!(json["setup_url"], "https://dev.example.com/apps");
    let back: AuthHint = serde_json::from_value(json).unwrap();
    assert!(
        matches!(back, AuthHint::OAuthPreConfigured { setup_url } if setup_url.contains("dev.example"))
    );

    // CapabilitiesAuth
    let json = serde_json::to_value(&AuthHint::CapabilitiesAuth).unwrap();
    assert_eq!(json["type"], "capabilities_auth");
    let back: AuthHint = serde_json::from_value(json).unwrap();
    assert!(matches!(back, AuthHint::CapabilitiesAuth));

    // None
    let json = serde_json::to_value(&AuthHint::None).unwrap();
    assert_eq!(json["type"], "none");
    let back: AuthHint = serde_json::from_value(json).unwrap();
    assert!(matches!(back, AuthHint::None));
}

// ── ResultSource ─────────────────────────────────────────────────

#[test]
fn result_source_serde() {
    let json = serde_json::to_value(ResultSource::Registry).unwrap();
    assert_eq!(json, "registry");
    let back: ResultSource = serde_json::from_value(json).unwrap();
    assert_eq!(back, ResultSource::Registry);

    let json = serde_json::to_value(ResultSource::Discovered).unwrap();
    assert_eq!(json, "discovered");
    let back: ResultSource = serde_json::from_value(json).unwrap();
    assert_eq!(back, ResultSource::Discovered);
}

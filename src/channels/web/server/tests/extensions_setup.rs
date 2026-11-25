//! Tests for the extension setup HTTP contract: typed fields, tagged
//! operations, value-free rejections, and store states.

#[cfg(all(feature = "libsql", feature = "test-helpers"))]
use std::sync::Arc;

#[cfg(all(feature = "libsql", feature = "test-helpers"))]
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    routing::get,
};
#[cfg(all(feature = "libsql", feature = "test-helpers"))]
use tower::ServiceExt;

#[cfg(all(feature = "libsql", feature = "test-helpers"))]
use super::fixtures::TestGatewayStateFactory;
#[cfg(all(feature = "libsql", feature = "test-helpers"))]
use crate::channels::web::handlers::extensions::setup::{
    extensions_setup_handler, extensions_setup_submit_handler,
};
#[cfg(all(feature = "libsql", feature = "test-helpers"))]
use crate::channels::web::server::GatewayState;
#[cfg(all(feature = "libsql", feature = "test-helpers"))]
use crate::extensions::setup::binding_test_support::{MemoryRepository, crypto};
#[cfg(all(feature = "libsql", feature = "test-helpers"))]
use crate::extensions::setup::{EndpointBindingRepository, EndpointBindingService};
#[cfg(all(feature = "libsql", feature = "test-helpers"))]
use crate::extensions::{
    ExtensionManager, ExtensionManagerConfig, NoOpDiscovery, NoOpMcpActivation,
    NoOpWasmChannelActivation, NoOpWasmToolActivation,
};
#[cfg(all(feature = "libsql", feature = "test-helpers"))]
use crate::secrets::{InMemorySecretsStore, SecretsStore};
#[cfg(all(feature = "libsql", feature = "test-helpers"))]
use crate::testing::credentials::TEST_CRYPTO_KEY;
#[cfg(all(feature = "libsql", feature = "test-helpers"))]
use crate::tools::ToolRegistry;

/// Synthetic tool manifest: one credential and two endpoints.
#[cfg(all(feature = "libsql", feature = "test-helpers"))]
const TOOL_MANIFEST: &str = r#"{
  "setup": {
    "fields": [
      {"type": "secret", "name": "token", "prompt": "API token"},
      {"type": "endpoint", "name": "calendar", "kind": "caldav", "prompt": "Calendar URL"},
      {"type": "endpoint", "name": "contacts", "kind": "carddav", "prompt": "Contacts URL", "optional": true}
    ]
  }
}"#;

/// Installed synthetic tool plus its binding store, for one test.
#[cfg(all(feature = "libsql", feature = "test-helpers"))]
struct SetupFixture {
    state: Arc<GatewayState>,
    secrets: Arc<dyn SecretsStore + Send + Sync>,
    _dir: tempfile::TempDir,
}

#[cfg(all(feature = "libsql", feature = "test-helpers"))]
fn setup_fixture() -> SetupFixture {
    let dir = tempfile::tempdir().expect("temp dir");
    let tools_dir = dir.path().join("tools");
    let channels_dir = dir.path().join("channels");
    ambient_fs::create_dir_all(&tools_dir).expect("tools dir");
    ambient_fs::create_dir_all(&channels_dir).expect("channels dir");
    ambient_fs::write(tools_dir.join("jmap.wasm"), b"").expect("wasm marker");
    ambient_fs::write(
        tools_dir.join("jmap.capabilities.json"),
        TOOL_MANIFEST.as_bytes(),
    )
    .expect("manifest");

    let crypto = crypto(TEST_CRYPTO_KEY);
    let secrets: Arc<dyn SecretsStore + Send + Sync> =
        Arc::new(InMemorySecretsStore::new(Arc::clone(&crypto)));
    let repository: Arc<dyn EndpointBindingRepository> = Arc::new(MemoryRepository::default());
    let manager = ExtensionManager::new(ExtensionManagerConfig {
        shared_state: crate::extensions::LiveWasmChannelSharedState::default(),
        discovery: Arc::new(NoOpDiscovery),
        relay_config: None,
        gateway_token: None,
        mcp_activation: Arc::new(NoOpMcpActivation),
        wasm_tool_activation: Arc::new(NoOpWasmToolActivation),
        wasm_channel_activation: Arc::new(NoOpWasmChannelActivation),
        mcp_clients: crate::extensions::McpClientsMap::default(),
        secrets: Arc::clone(&secrets),
        tool_registry: Arc::new(ToolRegistry::new()),
        hooks: None,
        wasm_tools_dir: tools_dir,
        wasm_channels_dir: channels_dir,
        tunnel_url: None,
        user_id: "test".to_string(),
        store: None,
        catalog_entries: Vec::new(),
        endpoint_bindings: Some(Arc::new(EndpointBindingService::new(repository, crypto))),
    });
    let state = TestGatewayStateFactory.build(Some(Arc::new(manager)), None);
    SetupFixture {
        state,
        secrets,
        _dir: dir,
    }
}

#[cfg(all(feature = "libsql", feature = "test-helpers"))]
fn setup_router(state: Arc<GatewayState>) -> Router {
    Router::new()
        .route(
            "/api/extensions/{name}/setup",
            get(extensions_setup_handler).post(extensions_setup_submit_handler),
        )
        .with_state(state)
}

#[cfg(all(feature = "libsql", feature = "test-helpers"))]
async fn send(router: Router, method: &str, body: Option<&str>) -> (StatusCode, String) {
    let request = Request::builder()
        .method(method)
        .uri("/api/extensions/jmap/setup")
        .header("content-type", "application/json")
        .body(Body::from(body.unwrap_or("").to_string()))
        .expect("request");
    let response = router.oneshot(request).await.expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(all(feature = "libsql", feature = "test-helpers"))]
#[tokio::test]
async fn the_schema_reports_typed_fields_and_readiness() {
    let fixture = setup_fixture();
    let (status, body) = send(setup_router(fixture.state), "GET", None).await;

    assert_eq!(status, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(json["ready"], serde_json::json!(false));
    assert_eq!(json["secrets"][0]["name"], "token");
    assert_eq!(json["secrets"][0]["provided"], false);
    assert_eq!(json["endpoints"][0]["name"], "calendar");
    assert_eq!(json["endpoints"][0]["state"], "missing");
    assert_eq!(json["endpoints"][1]["optional"], true);
    assert_eq!(json["blockers"][0]["field"], "token");
    assert_eq!(json["blockers"][0]["kind"], "secret");
}

#[cfg(all(feature = "libsql", feature = "test-helpers"))]
#[tokio::test]
async fn a_mixed_submission_reports_per_field_outcomes() {
    let fixture = setup_fixture();
    let _ = send(setup_router(Arc::clone(&fixture.state)), "GET", None).await;
    let body = serde_json::json!({
        "secrets": {"token": "s3cret"},
        "endpoints": {"set": {"calendar": "https://mail.example.com/caldav"}}
    })
    .to_string();
    let (status, response) = send(setup_router(fixture.state), "POST", Some(&body)).await;

    assert_eq!(status, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&response).expect("json");
    assert_eq!(json["success"], true);
    assert_eq!(json["ready"], true);
    assert_eq!(json["outcomes"][0]["outcome"], "stored");
    assert_eq!(json["outcomes"][1]["outcome"], "stored");
    assert_eq!(json["endpoints"][0]["state"], "configured");
    assert!(
        !response.contains("mail.example.com"),
        "the response must never echo the submitted URL"
    );
}

#[cfg(all(feature = "libsql", feature = "test-helpers"))]
#[tokio::test]
async fn a_malformed_endpoint_is_rejected_without_writing() {
    let fixture = setup_fixture();
    let body = serde_json::json!({
        "secrets": {"token": "s3cret"},
        "endpoints": {"set": {"calendar": "http://plain.example.com/"}}
    })
    .to_string();
    let (status, response) = send(setup_router(fixture.state), "POST", Some(&body)).await;

    assert_eq!(status, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&response).expect("json");
    assert_eq!(json["success"], false);
    assert_eq!(json["category"], "endpoint_insecure_scheme");
    assert_eq!(json["ready"], false);
    assert!(
        !response.contains("plain.example.com"),
        "rejections must not echo the value"
    );
    assert!(
        !fixture
            .secrets
            .exists("test", "token")
            .await
            .expect("exists")
    );
}

#[cfg(all(feature = "libsql", feature = "test-helpers"))]
#[tokio::test]
async fn an_undeclared_endpoint_is_rejected() {
    let fixture = setup_fixture();
    let body = serde_json::json!({
        "endpoints": {"set": {"unknown": "https://mail.example.com/"}}
    })
    .to_string();
    let (_, response) = send(setup_router(fixture.state), "POST", Some(&body)).await;

    let json: serde_json::Value = serde_json::from_str(&response).expect("json");
    assert_eq!(json["category"], "undeclared_field");
}

#[cfg(all(feature = "libsql", feature = "test-helpers"))]
#[tokio::test]
async fn clearing_a_missing_binding_is_idempotent() {
    let fixture = setup_fixture();
    let body = serde_json::json!({"endpoints": {"clear": ["calendar"]}}).to_string();
    let (_, response) = send(setup_router(fixture.state), "POST", Some(&body)).await;

    let json: serde_json::Value = serde_json::from_str(&response).expect("json");
    assert_eq!(json["outcomes"][0]["outcome"], "cleared");
    assert_eq!(json["endpoints"][0]["state"], "missing");
}

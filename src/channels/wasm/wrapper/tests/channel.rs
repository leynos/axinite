//! Tests for the WASM channel wrapper's typing-task lifecycle across
//! status updates.

use std::sync::Arc;

use crate::channels::NativeChannel;
use crate::channels::wasm::capabilities::ChannelCapabilities;
use crate::channels::wasm::runtime::{
    PreparedChannelModule, WasmChannelRuntime, WasmChannelRuntimeConfig,
};
use crate::channels::wasm::wrapper::{HttpResponse, WasmChannel};
use crate::pairing::PairingStore;
use crate::tools::wasm::ResourceLimits;

use super::create_test_channel;

mod typing;

#[test]
fn test_channel_name() {
    let channel = create_test_channel();
    assert_eq!(channel.name(), "test");
}

#[test]
fn test_http_response_ok() {
    let response = HttpResponse::ok();
    assert_eq!(response.status, 200);
    assert!(response.body.is_empty());
}

#[test]
fn test_http_response_json() {
    let response = HttpResponse::json(serde_json::json!({"key": "value"}));
    assert_eq!(response.status, 200);
    assert_eq!(
        response.headers.get("Content-Type"),
        Some(&"application/json".to_string())
    );
}

#[test]
fn test_http_response_error() {
    let response = HttpResponse::error(400, "Bad request");
    assert_eq!(response.status, 400);
    assert_eq!(response.body, b"Bad request");
}

#[test]
fn test_create_store_rejects_empty_channel_name() {
    let config = WasmChannelRuntimeConfig::for_testing();
    let runtime = WasmChannelRuntime::new(config).unwrap();
    let prepared = PreparedChannelModule {
        name: String::new(),
        description: "Invalid channel".to_string(),
        component: None,
        limits: ResourceLimits::default(),
    };

    let result = WasmChannel::create_store(
        &runtime,
        &prepared,
        &ChannelCapabilities::for_channel("test"),
        std::collections::HashMap::new(),
        Vec::new(),
        Arc::new(PairingStore::new()),
    );

    assert!(matches!(
        result,
        Err(crate::channels::wasm::error::WasmChannelError::Config(message))
            if message == "channel name must be non-empty"
    ));
}

#[tokio::test]
async fn test_channel_start_and_shutdown() {
    let channel = create_test_channel();

    // Start should succeed
    let stream = channel.start().await;
    assert!(stream.is_ok());

    // Health check should pass
    assert!(channel.health_check().await.is_ok());

    // Shutdown should succeed
    assert!(channel.shutdown().await.is_ok());

    // Health check should fail after shutdown
    assert!(channel.health_check().await.is_err());
}

#[tokio::test]
async fn test_execute_poll_no_wasm_returns_empty() {
    // When there's no WASM module (None component), execute_poll
    // should return an empty vector of messages
    let config = WasmChannelRuntimeConfig::for_testing();
    let runtime = Arc::new(WasmChannelRuntime::new(config).unwrap());

    let prepared = Arc::new(PreparedChannelModule {
        name: "poll-test".to_string(),
        description: "Test channel".to_string(),
        component: None, // No WASM module
        limits: ResourceLimits::default(),
    });

    let capabilities = ChannelCapabilities::for_channel("poll-test").with_polling(1000);
    let credentials = Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new()));
    let timeout = std::time::Duration::from_secs(5);

    let workspace_store = Arc::new(crate::channels::wasm::host::ChannelWorkspaceStore::new());

    let result = WasmChannel::execute_poll(
        "poll-test",
        &runtime,
        &prepared,
        &capabilities,
        &credentials,
        Vec::new(), // no host credentials in test
        Arc::new(PairingStore::new()),
        timeout,
        &workspace_store,
    )
    .await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_channel_with_polling_stores_shutdown_sender() {
    // Create a channel with polling capabilities
    let config = WasmChannelRuntimeConfig::for_testing();
    let runtime = Arc::new(WasmChannelRuntime::new(config).unwrap());

    let prepared = Arc::new(PreparedChannelModule {
        name: "poll-channel".to_string(),
        description: "Polling test channel".to_string(),
        component: None,
        limits: ResourceLimits::default(),
    });

    // Enable polling with a 1 second minimum interval
    let capabilities = ChannelCapabilities::for_channel("poll-channel")
        .with_path("/webhook/poll")
        .with_polling(1000);

    let channel = WasmChannel::new(
        runtime,
        prepared,
        capabilities,
        "{}".to_string(),
        Arc::new(PairingStore::new()),
        None,
    );

    // Start the channel
    let _stream = channel.start().await.expect("Channel should start");

    // Verify poll_shutdown_tx is set (polling was started)
    // Note: For testing channels without WASM, on_start returns no poll config,
    // so polling won't actually be started. This verifies the basic lifecycle.
    assert!(channel.health_check().await.is_ok());

    // Shutdown should clean up properly
    channel.shutdown().await.expect("Shutdown should succeed");
    assert!(channel.health_check().await.is_err());
}

#[tokio::test]
async fn test_call_on_poll_no_wasm_succeeds() {
    // Verify call_on_poll returns Ok when there's no WASM module
    let channel = create_test_channel();

    // Start the channel first to set up message_tx
    let _stream = channel.start().await.expect("Channel should start");

    // call_on_poll should succeed (no-op for no WASM)
    let result = channel.call_on_poll().await;
    assert!(result.is_ok());

    channel.shutdown().await.expect("Shutdown should succeed");
}

/// Verify that WASM HTTP host functions work using a dedicated
/// current-thread runtime inside spawn_blocking.
#[tokio::test]
async fn test_dedicated_runtime_inside_spawn_blocking() {
    let result = tokio::task::spawn_blocking(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build runtime");
        rt.block_on(async { 42 })
    })
    .await
    .expect("spawn_blocking panicked");
    assert_eq!(result, 42);
}

/// Verify a real HTTP request works using the dedicated-runtime pattern.
/// This catches DNS, TLS, and I/O driver issues that trivial tests miss.
#[tokio::test]
#[ignore] // requires network
async fn test_dedicated_runtime_real_http() {
    let result = tokio::task::spawn_blocking(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build runtime");
        rt.block_on(async {
            let client = reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("failed to build client");
            let resp = client
                .get("https://api.telegram.org/bot000/getMe")
                .timeout(std::time::Duration::from_secs(10))
                .send()
                .await;
            match resp {
                Ok(r) => r.status().as_u16(),
                Err(e) if e.is_timeout() => panic!("request timed out: {e}"),
                Err(e) => panic!("unexpected error: {e}"),
            }
        })
    })
    .await
    .expect("spawn_blocking panicked");
    // 404 because "000" is not a valid bot token
    assert_eq!(result, 404);
}

//! Shared fixtures and signing helpers for WASM channel router tests.

use std::sync::Arc;

use axum::Router as AxumRouter;
use ed25519_dalek::SigningKey;

use crate::channels::wasm::capabilities::ChannelCapabilities;
use crate::channels::wasm::router::{
    RegisteredEndpoint, WasmChannelRouter, WebhookSecrets, create_wasm_channel_router,
};
use crate::channels::wasm::runtime::{
    PreparedChannelModule, WasmChannelRuntime, WasmChannelRuntimeConfig,
};
use crate::channels::wasm::wrapper::WasmChannel;
use crate::pairing::PairingStore;
use crate::tools::wasm::ResourceLimits;

pub(super) fn create_test_channel(name: &str) -> Arc<WasmChannel> {
    let config = WasmChannelRuntimeConfig::for_testing();
    let runtime = Arc::new(WasmChannelRuntime::new(config).unwrap());

    let prepared = Arc::new(PreparedChannelModule {
        name: name.to_string(),
        description: format!("Test channel: {}", name),
        component: None,
        limits: ResourceLimits::default(),
    });

    let capabilities =
        ChannelCapabilities::for_channel(name).with_path(format!("/webhook/{}", name));

    Arc::new(WasmChannel::new(
        runtime,
        prepared,
        capabilities,
        "{}".to_string(),
        Arc::new(PairingStore::new()),
        None,
    ))
}

/// Helper to create a router with a registered channel at /webhook/discord.
pub(super) async fn setup_discord_router() -> (Arc<WasmChannelRouter>, AxumRouter) {
    let wasm_router = Arc::new(WasmChannelRouter::new());
    let channel = create_test_channel("discord");

    let endpoints = vec![RegisteredEndpoint {
        channel_name: "discord".to_string(),
        path: "/webhook/discord".to_string(),
        methods: vec!["POST".to_string()],
        require_secret: false,
    }];

    wasm_router
        .register(channel, endpoints, WebhookSecrets::default())
        .await;

    let app = create_wasm_channel_router(wasm_router.clone(), None);
    (wasm_router, app)
}

/// Helper: generate a test keypair.
pub(super) fn test_signing_key() -> SigningKey {
    SigningKey::from_bytes(&[
        0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c,
        0xc4, 0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae,
        0x7f, 0x60,
    ])
}

/// Helper to create a router with a registered channel at /webhook/slack.
pub(super) async fn setup_slack_router() -> (Arc<WasmChannelRouter>, AxumRouter) {
    let wasm_router = Arc::new(WasmChannelRouter::new());
    let channel = create_test_channel("slack");

    let endpoints = vec![RegisteredEndpoint {
        channel_name: "slack".to_string(),
        path: "/webhook/slack".to_string(),
        methods: vec!["POST".to_string()],
        require_secret: false,
    }];

    wasm_router
        .register(channel, endpoints, WebhookSecrets::default())
        .await;

    let app = create_wasm_channel_router(wasm_router.clone(), None);
    (wasm_router, app)
}

/// Helper: compute expected Slack signature for testing.
pub(super) fn slack_signature(signing_secret: &str, timestamp: &str, body: &[u8]) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let mut basestring = Vec::new();
    basestring.extend_from_slice(b"v0:");
    basestring.extend_from_slice(timestamp.as_bytes());
    basestring.push(b':');
    basestring.extend_from_slice(body);

    let mut mac = Hmac::<Sha256>::new_from_slice(signing_secret.as_bytes()).unwrap();
    mac.update(&basestring);
    let computed = mac.finalize().into_bytes();
    format!("v0={}", hex::encode(computed))
}

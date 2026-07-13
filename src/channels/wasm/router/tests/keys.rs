//! Tests for HMAC secret and Ed25519 signature key management,
//! including key validation on registration.

use crate::channels::wasm::router::{RegisteredEndpoint, WasmChannelRouter};

use super::helpers::create_test_channel;

// ── Category 3: Router HMAC Secret Management ───────────────────────

#[tokio::test]
async fn test_register_and_get_hmac_secret() {
    let router = WasmChannelRouter::new();
    let channel = create_test_channel("slack");

    router.register(channel, vec![], None, None).await;

    let hmac_secret = "my-slack-signing-secret";
    router.register_hmac_secret("slack", hmac_secret).await;

    let retrieved = router.get_hmac_secret("slack").await;
    assert_eq!(retrieved, Some(hmac_secret.to_string()));
}

#[tokio::test]
async fn test_no_hmac_secret_returns_none() {
    let router = WasmChannelRouter::new();
    let channel = create_test_channel("slack");
    router.register(channel, vec![], None, None).await;

    // Slack has no HMAC secret registered
    let secret = router.get_hmac_secret("slack").await;
    assert!(secret.is_none());
}

#[tokio::test]
async fn test_unregister_removes_hmac_secret() {
    let router = WasmChannelRouter::new();
    let channel = create_test_channel("slack");

    let endpoints = vec![RegisteredEndpoint {
        channel_name: "slack".to_string(),
        path: "/webhook/slack".to_string(),
        methods: vec!["POST".to_string()],
        require_secret: false,
    }];

    router.register(channel, endpoints, None, None).await;
    router.register_hmac_secret("slack", "signing-secret").await;

    // Secret should exist
    assert!(router.get_hmac_secret("slack").await.is_some());

    // Unregister
    router.unregister("slack").await;

    // Secret should be gone
    assert!(router.get_hmac_secret("slack").await.is_none());
}

// ── Category 4: Router Signature Key Management ─────────────────────

#[tokio::test]
async fn test_register_and_get_signature_key() {
    let router = WasmChannelRouter::new();
    let channel = create_test_channel("discord");

    router.register(channel, vec![], None, None).await;

    let fake_pub_key = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2";
    router
        .register_signature_key("discord", fake_pub_key)
        .await
        .unwrap();

    let key = router.get_signature_key("discord").await;
    assert_eq!(key, Some(fake_pub_key.to_string()));
}

#[tokio::test]
async fn test_no_signature_key_returns_none() {
    let router = WasmChannelRouter::new();
    let channel = create_test_channel("slack");
    router.register(channel, vec![], None, None).await;

    // Slack has no signature key registered
    let key = router.get_signature_key("slack").await;
    assert!(key.is_none());
}

#[tokio::test]
async fn test_unregister_removes_signature_key() {
    let router = WasmChannelRouter::new();
    let channel = create_test_channel("discord");

    let endpoints = vec![RegisteredEndpoint {
        channel_name: "discord".to_string(),
        path: "/webhook/discord".to_string(),
        methods: vec!["POST".to_string()],
        require_secret: false,
    }];

    router.register(channel, endpoints, None, None).await;
    // Use a valid 32-byte Ed25519 key for this test
    let valid_key = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa3f4a18446b7e8c7ac6602";
    router
        .register_signature_key("discord", valid_key)
        .await
        .unwrap();

    // Key should exist
    assert!(router.get_signature_key("discord").await.is_some());

    // Unregister
    router.unregister("discord").await;

    // Key should be gone
    assert!(router.get_signature_key("discord").await.is_none());
}

// ── Key Validation Tests ──────────────────────────────────────────

#[tokio::test]
async fn test_register_valid_signature_key_succeeds() {
    let router = WasmChannelRouter::new();
    let channel = create_test_channel("discord");
    router.register(channel, vec![], None, None).await;

    // Valid 32-byte Ed25519 public key (from test keypair)
    let valid_key = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa3f4a18446b7e8c7ac6602";
    let result = router.register_signature_key("discord", valid_key).await;
    assert!(result.is_ok(), "Valid Ed25519 key should be accepted");
}

#[tokio::test]
async fn test_register_invalid_hex_key_fails() {
    let router = WasmChannelRouter::new();
    let channel = create_test_channel("discord");
    router.register(channel, vec![], None, None).await;

    let result = router
        .register_signature_key("discord", "not-valid-hex-zzz")
        .await;
    assert!(result.is_err(), "Invalid hex should be rejected");
}

#[tokio::test]
async fn test_register_wrong_length_key_fails() {
    let router = WasmChannelRouter::new();
    let channel = create_test_channel("discord");
    router.register(channel, vec![], None, None).await;

    // 16 bytes instead of 32
    let short_key = hex::encode([0u8; 16]);
    let result = router.register_signature_key("discord", &short_key).await;
    assert!(result.is_err(), "Wrong-length key should be rejected");
}

#[tokio::test]
async fn test_register_empty_key_fails() {
    let router = WasmChannelRouter::new();
    let channel = create_test_channel("discord");
    router.register(channel, vec![], None, None).await;

    let result = router.register_signature_key("discord", "").await;
    assert!(result.is_err(), "Empty key should be rejected");
}

#[tokio::test]
async fn test_valid_key_is_retrievable() {
    let router = WasmChannelRouter::new();
    let channel = create_test_channel("discord");
    router.register(channel, vec![], None, None).await;

    let valid_key = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa3f4a18446b7e8c7ac6602";
    router
        .register_signature_key("discord", valid_key)
        .await
        .unwrap();

    let stored = router.get_signature_key("discord").await;
    assert_eq!(stored, Some(valid_key.to_string()));
}

#[tokio::test]
async fn test_invalid_key_does_not_store() {
    let router = WasmChannelRouter::new();
    let channel = create_test_channel("discord");
    router.register(channel, vec![], None, None).await;

    // Attempt to register invalid key
    let _ = router
        .register_signature_key("discord", "not-valid-hex")
        .await;

    // Should not have stored anything
    let stored = router.get_signature_key("discord").await;
    assert!(stored.is_none(), "Invalid key should not be stored");
}

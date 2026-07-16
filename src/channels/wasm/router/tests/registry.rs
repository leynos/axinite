//! Tests for channel registration, path lookup, secret validation,
//! unregistration, and secret header configuration.

use crate::channels::wasm::router::{RegisteredEndpoint, WasmChannelRouter, WebhookSecrets};

use super::helpers::create_test_channel;

#[tokio::test]
async fn test_router_register_and_lookup() {
    let router = WasmChannelRouter::new();
    let channel = create_test_channel("slack");

    let endpoints = vec![RegisteredEndpoint {
        channel_name: "slack".to_string(),
        path: "/webhook/slack".to_string(),
        methods: vec!["POST".to_string()],
        require_secret: true,
    }];

    router
        .register(
            channel,
            endpoints,
            WebhookSecrets {
                secret: Some("secret123".to_string()),
                header: None,
            },
        )
        .await;

    // Should find channel by path
    let found = router.get_channel_for_path("/webhook/slack").await;
    assert!(found.is_some());
    assert_eq!(found.unwrap().channel_name(), "slack");

    // Should not find non-existent path
    let not_found = router.get_channel_for_path("/webhook/telegram").await;
    assert!(not_found.is_none());
}

#[tokio::test]
async fn test_router_secret_validation() {
    let router = WasmChannelRouter::new();
    let channel = create_test_channel("slack");

    router
        .register(
            channel,
            vec![],
            WebhookSecrets {
                secret: Some("secret123".to_string()),
                header: None,
            },
        )
        .await;

    // Correct secret
    assert!(router.validate_secret("slack", "secret123").await);

    // Wrong secret
    assert!(!router.validate_secret("slack", "wrong").await);

    // Channel without secret always validates
    let channel2 = create_test_channel("telegram");
    router
        .register(channel2, vec![], WebhookSecrets::default())
        .await;
    assert!(router.validate_secret("telegram", "anything").await);
}

#[tokio::test]
async fn test_router_unregister() {
    let router = WasmChannelRouter::new();
    let channel = create_test_channel("slack");

    let endpoints = vec![RegisteredEndpoint {
        channel_name: "slack".to_string(),
        path: "/webhook/slack".to_string(),
        methods: vec!["POST".to_string()],
        require_secret: false,
    }];

    router
        .register(channel, endpoints, WebhookSecrets::default())
        .await;

    // Should exist
    assert!(
        router
            .get_channel_for_path("/webhook/slack")
            .await
            .is_some()
    );

    // Unregister
    router.unregister("slack").await;

    // Should no longer exist
    assert!(
        router
            .get_channel_for_path("/webhook/slack")
            .await
            .is_none()
    );
}

#[tokio::test]
async fn test_router_list_channels() {
    let router = WasmChannelRouter::new();

    let channel1 = create_test_channel("slack");
    let channel2 = create_test_channel("telegram");

    router
        .register(channel1, vec![], WebhookSecrets::default())
        .await;
    router
        .register(channel2, vec![], WebhookSecrets::default())
        .await;

    let channels = router.list_channels().await;
    assert_eq!(channels.len(), 2);
    assert!(channels.contains(&"slack".to_string()));
    assert!(channels.contains(&"telegram".to_string()));
}

#[tokio::test]
async fn test_router_secret_header() {
    let router = WasmChannelRouter::new();
    let channel = create_test_channel("telegram");

    // Register with custom secret header
    router
        .register(
            channel,
            vec![],
            WebhookSecrets {
                secret: Some("secret123".to_string()),
                header: Some("X-Telegram-Bot-Api-Secret-Token".to_string()),
            },
        )
        .await;

    // Should return the custom header
    assert_eq!(
        router.get_secret_header("telegram").await,
        "X-Telegram-Bot-Api-Secret-Token"
    );

    // Channel without custom header should use default
    let channel2 = create_test_channel("slack");
    router
        .register(
            channel2,
            vec![],
            WebhookSecrets {
                secret: Some("secret456".to_string()),
                header: None,
            },
        )
        .await;
    assert_eq!(router.get_secret_header("slack").await, "X-Webhook-Secret");
}

//! Tests for registering channels and routing via the WASM router.

use super::*;

#[tokio::test]
async fn test_register_and_route_channel() {
    let router = WasmChannelRouter::new();
    let runtime = create_test_runtime();

    let channel = Arc::new(create_test_channel(
        runtime,
        "test-channel",
        vec!["/webhook/test"],
    ));

    let endpoints = vec![RegisteredEndpoint {
        channel_name: "test-channel".to_string(),
        path: "/webhook/test".to_string(),
        methods: vec!["POST".to_string()],
        require_secret: false,
    }];

    router
        .register(channel.clone(), endpoints, None, None)
        .await;

    // Verify channel is found by path
    let found = router.get_channel_for_path("/webhook/test").await;
    assert!(found.is_some());
    assert_eq!(found.unwrap().channel_name(), "test-channel");

    // Verify non-existent path returns None
    let not_found = router.get_channel_for_path("/webhook/nonexistent").await;
    assert!(not_found.is_none());
}

#[tokio::test]
async fn test_secret_validation() {
    let router = WasmChannelRouter::new();
    let runtime = create_test_runtime();

    let channel = Arc::new(create_test_channel(
        runtime,
        "secure-channel",
        vec!["/webhook/secure"],
    ));

    router
        .register(channel, vec![], Some("my-secret-123".to_string()), None)
        .await;

    // Correct secret validates
    assert!(
        router
            .validate_secret("secure-channel", "my-secret-123")
            .await
    );

    // Wrong secret fails
    assert!(
        !router
            .validate_secret("secure-channel", "wrong-secret")
            .await
    );

    // Non-existent channel without secret always validates
    assert!(router.validate_secret("nonexistent", "anything").await);
}

#[tokio::test]
async fn test_unregister_channel() {
    let router = WasmChannelRouter::new();
    let runtime = create_test_runtime();

    let channel = Arc::new(create_test_channel(
        runtime,
        "temp-channel",
        vec!["/webhook/temp"],
    ));

    let endpoints = vec![RegisteredEndpoint {
        channel_name: "temp-channel".to_string(),
        path: "/webhook/temp".to_string(),
        methods: vec!["POST".to_string()],
        require_secret: false,
    }];

    router.register(channel, endpoints, None, None).await;

    // Channel exists
    assert!(router.get_channel_for_path("/webhook/temp").await.is_some());

    // Unregister
    router.unregister("temp-channel").await;

    // Channel no longer exists
    assert!(router.get_channel_for_path("/webhook/temp").await.is_none());
}

#[tokio::test]
async fn test_multiple_channels() {
    let router = WasmChannelRouter::new();
    let runtime = create_test_runtime();

    // Register multiple channels
    for name in &["slack", "telegram", "discord"] {
        let channel = Arc::new(create_test_channel(
            Arc::clone(&runtime),
            name,
            vec![&format!("/webhook/{}", name)],
        ));

        let endpoints = vec![RegisteredEndpoint {
            channel_name: name.to_string(),
            path: format!("/webhook/{}", name),
            methods: vec!["POST".to_string()],
            require_secret: false,
        }];

        router.register(channel, endpoints, None, None).await;
    }

    // Verify all channels are registered
    let channels = router.list_channels().await;
    assert_eq!(channels.len(), 3);
    assert!(channels.contains(&"slack".to_string()));
    assert!(channels.contains(&"telegram".to_string()));
    assert!(channels.contains(&"discord".to_string()));

    // Verify all paths work
    for name in &["slack", "telegram", "discord"] {
        let found = router
            .get_channel_for_path(&format!("/webhook/{}", name))
            .await;
        assert!(found.is_some());
        assert_eq!(found.unwrap().channel_name(), *name);
    }
}

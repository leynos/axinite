//! End-to-end tests for context-based channel credential injection.
//!
//! Each test drives [`inject_channel_credentials_from_context`] with an
//! explicit environment snapshot and an optional secrets store, covering store
//! precedence, the environment fallback, name normalisation, a channel that
//! declares no HTTP capability, and the summaries that report unresolved
//! declared credentials. The fixtures they share live in the parent test
//! module.

use super::*;

/// Verify that a whitespace-only channel name injects no credentials.
#[tokio::test]
async fn blank_channel_name_injects_nothing() -> anyhow::Result<()> {
    let channel = test_channel("telegram", "telegram_bot_token");
    let env = EnvContext::default().with_env("TELEGRAM_BOT_TOKEN", "from-env");

    let count = inject_channel_credentials_from_context(&channel, None, "   ", &env).await?;

    assert_eq!(count, 0);
    assert!(channel.get_credentials().await.is_empty());
    Ok(())
}

/// Verify that an environment variable matching a channel-prefixed
/// placeholder is injected when no persistent secret exists.
#[tokio::test]
async fn environment_fallback_injects_prefixed_credential() -> anyhow::Result<()> {
    let channel = test_channel("telegram", "telegram_bot_token");
    let env = EnvContext::default().with_env("TELEGRAM_BOT_TOKEN", "from-env");

    let count = inject_channel_credentials_from_context(&channel, None, "telegram", &env).await?;

    assert_eq!(count, 1);
    assert_eq!(
        channel.get_credentials().await.get("TELEGRAM_BOT_TOKEN"),
        Some(&"from-env".to_string())
    );
    Ok(())
}

/// Verify that a missing environment value leaves the credential uninjected.
#[tokio::test]
async fn environment_fallback_skips_missing_credential() -> anyhow::Result<()> {
    let channel = test_channel("telegram", "telegram_bot_token");
    let env = EnvContext::default();

    let count = inject_channel_credentials_from_context(&channel, None, "telegram", &env).await?;

    assert_eq!(count, 0);
    assert!(channel.get_credentials().await.is_empty());
    Ok(())
}

/// Verify that an environment variable present with an empty value is treated
/// as unset, so a blank entry does not inject an empty credential.
#[tokio::test]
async fn environment_fallback_skips_empty_credential() -> anyhow::Result<()> {
    let channel = test_channel("telegram", "telegram_bot_token");
    let env = EnvContext::default().with_env("TELEGRAM_BOT_TOKEN", "");
    assert!(
        env.contains_env_var("TELEGRAM_BOT_TOKEN"),
        "the snapshot must hold the empty value for this test to mean anything"
    );

    let count = inject_channel_credentials_from_context(&channel, None, "telegram", &env).await?;

    assert_eq!(count, 0);
    assert!(channel.get_credentials().await.is_empty());
    Ok(())
}

/// Verify that mixed-case channel and secret names normalise to the uppercase
/// placeholder the channel declares, so the stored secret is injected once.
#[tokio::test]
async fn mixed_case_names_normalise_to_one_placeholder() -> anyhow::Result<()> {
    let channel = test_channel("Telegram", "Telegram_Bot_Token");
    let store = in_memory_store();
    NativeSecretsStore::create(
        store.as_ref(),
        "default",
        CreateSecretParams::new("Telegram_Bot_Token", "from-store"),
    )
    .await?;

    let count = inject_channel_credentials_from_context(
        &channel,
        Some(store.as_ref() as &dyn SecretsStore),
        "Telegram",
        &EnvContext::default(),
    )
    .await?;

    assert_eq!(count, 1);
    assert_eq!(
        channel.get_credentials().await.get("TELEGRAM_BOT_TOKEN"),
        Some(&"from-store".to_string())
    );
    Ok(())
}

/// Verify that an environment variable whose name does not carry the channel
/// prefix is not injected.
#[tokio::test]
async fn environment_fallback_ignores_non_prefixed_placeholder() -> anyhow::Result<()> {
    let channel = test_channel("telegram", "slack_bot_token");
    let env = EnvContext::default().with_env("SLACK_BOT_TOKEN", "from-env");

    let count = inject_channel_credentials_from_context(&channel, None, "telegram", &env).await?;

    assert_eq!(count, 0);
    assert!(channel.get_credentials().await.is_empty());
    Ok(())
}

/// Verify that a stored secret wins over a matching environment value and is
/// injected only once.
#[tokio::test]
async fn persistent_secret_takes_precedence_over_environment() -> anyhow::Result<()> {
    let channel = test_channel("telegram", "telegram_bot_token");
    let store = in_memory_store();
    NativeSecretsStore::create(
        store.as_ref(),
        "default",
        CreateSecretParams::new("telegram_bot_token", "from-store"),
    )
    .await?;
    let env = EnvContext::default().with_env("TELEGRAM_BOT_TOKEN", "from-env");

    let count = inject_channel_credentials_from_context(
        &channel,
        Some(store.as_ref() as &dyn SecretsStore),
        "telegram",
        &env,
    )
    .await?;

    assert_eq!(count, 1, "the environment must not double-inject");
    assert_eq!(
        channel.get_credentials().await.get("TELEGRAM_BOT_TOKEN"),
        Some(&"from-store".to_string())
    );
    Ok(())
}

/// Verify that a failure to list stored secrets is reported rather than
/// silently treated as an empty store.
#[tokio::test]
async fn secret_list_error_is_reported() {
    let channel = test_channel("telegram", "telegram_bot_token");
    let store = StubSecretsStore {
        list_result: Err(SecretError::Database("list failed".to_string())),
        decrypt_result: Err(SecretError::DecryptionFailed("unused".to_string())),
    };

    let result = inject_channel_credentials_from_context(
        &channel,
        Some(&store as &dyn SecretsStore),
        "telegram",
        &EnvContext::default(),
    )
    .await;

    let error = result.expect_err("a list failure must surface as an error");
    assert!(error.to_string().contains("list failed"), "{error}");
}

/// Verify that a secret which fails to decrypt is skipped and the environment
/// value is injected instead.
#[tokio::test]
async fn decryption_failure_skips_secret_and_falls_back_to_environment() -> anyhow::Result<()> {
    let channel = test_channel("telegram", "telegram_bot_token");
    let store = StubSecretsStore {
        list_result: Ok(vec![SecretRef::new("telegram_bot_token")]),
        decrypt_result: Err(SecretError::DecryptionFailed("bad key".to_string())),
    };
    let env = EnvContext::default().with_env("TELEGRAM_BOT_TOKEN", "from-env");

    let count = inject_channel_credentials_from_context(
        &channel,
        Some(&store as &dyn SecretsStore),
        "telegram",
        &env,
    )
    .await?;

    assert_eq!(count, 1);
    assert_eq!(
        channel.get_credentials().await.get("TELEGRAM_BOT_TOKEN"),
        Some(&"from-env".to_string())
    );
    Ok(())
}

/// Verify that a channel without an HTTP capability injects no credentials.
#[tokio::test]
async fn channel_without_http_capability_injects_nothing() -> anyhow::Result<()> {
    let runtime = Arc::new(
        WasmChannelRuntime::new(WasmChannelRuntimeConfig::for_testing()).expect("test runtime"),
    );
    let prepared = Arc::new(PreparedChannelModule::for_testing(
        "telegram",
        "test channel",
    ));
    let channel = Arc::new(WasmChannel::new(
        runtime,
        prepared,
        ChannelCapabilities::for_channel("telegram"),
        "{}".to_string(),
        Arc::new(PairingStore::new()),
        None,
    ));
    let env = EnvContext::default().with_env("TELEGRAM_BOT_TOKEN", "from-env");

    let count = inject_channel_credentials_from_context(&channel, None, "telegram", &env).await?;

    assert_eq!(count, 0);
    assert!(channel.get_credentials().await.is_empty());
    Ok(())
}

/// Verify that an unrelated stored secret does not mask a declared credential
/// that no source resolved.
#[tokio::test]
#[tracing_test::traced_test]
async fn unrelated_store_secret_does_not_mask_missing_declared_credential() -> anyhow::Result<()> {
    let channel = test_channel("telegram", "telegram_bot_token");
    let store = in_memory_store();
    NativeSecretsStore::create(
        store.as_ref(),
        "default",
        CreateSecretParams::new("telegram_webhook_secret", "unrelated"),
    )
    .await?;

    let count = inject_channel_credentials_from_context(
        &channel,
        Some(store.as_ref() as &dyn SecretsStore),
        "telegram",
        &EnvContext::default(),
    )
    .await?;

    assert_eq!(count, 1, "the unrelated secret is still injected");
    assert!(
        logs_contain("Channel declares credentials that did not all resolve"),
        "a missing declared credential must still be reported"
    );
    assert!(
        logs_contain("declared=1 resolved=0"),
        "the warning must report that none of the declared credentials resolved"
    );
    Ok(())
}

/// Verify that a channel declaring several credentials is reported as
/// `partial` when only some of them resolve, and that the warning names the
/// channel and both counts without exposing a secret.
#[tokio::test]
#[tracing_test::traced_test]
async fn partially_resolved_declared_credentials_and_warning_are_reported() -> anyhow::Result<()> {
    let channel = test_channel_with_mappings(
        "telegram",
        &[
            "telegram_bot_token".to_string(),
            "telegram_api_key".to_string(),
        ],
    );
    let env = EnvContext::default().with_env("TELEGRAM_BOT_TOKEN", "from-env");

    let count = inject_channel_credentials_from_context(&channel, None, "telegram", &env).await?;

    assert_eq!(count, 1, "the supplied credential is still injected");
    assert!(
        logs_contain("Channel declares credentials that did not all resolve"),
        "a partially resolved channel must be reported"
    );
    assert!(
        logs_contain("channel=telegram declared=2 resolved=1"),
        "the warning must name the channel and both counts"
    );
    assert!(
        !logs_contain("from-env"),
        "the warning must not include the resolved secret value"
    );
    Ok(())
}

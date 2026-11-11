//! Counter assertions for the WASM credential-injection production path.

use super::*;
use crate::testing::metrics;

#[tokio::test]
async fn environment_injection_records_injected_outcome() -> anyhow::Result<()> {
    let channel = test_channel("telegram", "telegram_bot_token");
    let env = EnvContext::default().with_env("TELEGRAM_BOT_TOKEN", "from-env");

    let _guard = metrics::record();
    inject_channel_credentials_from_context(&channel, None, "telegram", &env).await?;

    assert_eq!(
        metrics::counter("axinite.wasm.channel_credentials", "injected"),
        1
    );
    Ok(())
}

#[tokio::test]
async fn partially_resolved_credentials_record_partial_outcome() -> anyhow::Result<()> {
    let channel = test_channel_with_mappings(
        "telegram",
        &[
            "telegram_bot_token".to_string(),
            "telegram_api_key".to_string(),
        ],
    );
    let env = EnvContext::default().with_env("TELEGRAM_BOT_TOKEN", "from-env");

    let _guard = metrics::record();
    inject_channel_credentials_from_context(&channel, None, "telegram", &env).await?;

    assert_eq!(
        metrics::counter("axinite.wasm.channel_credentials", "partial"),
        1
    );
    Ok(())
}

#[tokio::test]
async fn unresolved_declared_credential_records_missing_outcome() -> anyhow::Result<()> {
    let channel = test_channel("telegram", "telegram_bot_token");

    let _guard = metrics::record();
    inject_channel_credentials_from_context(&channel, None, "telegram", &EnvContext::default())
        .await?;

    assert_eq!(
        metrics::counter("axinite.wasm.channel_credentials", "missing"),
        1
    );
    Ok(())
}

#[tokio::test]
async fn store_list_failure_records_list_error_outcome() {
    let channel = test_channel("telegram", "telegram_bot_token");
    let store = StubSecretsStore {
        list_result: Err(SecretError::Database("list failed".to_string())),
        decrypt_result: Err(SecretError::DecryptionFailed("unused".to_string())),
    };

    let _guard = metrics::record();
    let result = inject_channel_credentials_from_context(
        &channel,
        Some(&store as &dyn SecretsStore),
        "telegram",
        &EnvContext::default(),
    )
    .await;

    assert!(result.is_err(), "a list failure must surface as an error");
    assert_eq!(
        metrics::counter("axinite.wasm.channel_credentials", "list_error"),
        1
    );
}

#[tokio::test]
async fn decryption_failure_records_decryption_error_outcome() -> anyhow::Result<()> {
    let channel = test_channel("telegram", "telegram_bot_token");
    let store = StubSecretsStore {
        list_result: Ok(vec![SecretRef::new("telegram_bot_token")]),
        decrypt_result: Err(SecretError::DecryptionFailed("bad key".to_string())),
    };

    let _guard = metrics::record();
    inject_channel_credentials_from_context(
        &channel,
        Some(&store as &dyn SecretsStore),
        "telegram",
        &EnvContext::default(),
    )
    .await?;

    assert_eq!(
        metrics::counter("axinite.wasm.channel_credentials", "decryption_error"),
        1
    );
    Ok(())
}

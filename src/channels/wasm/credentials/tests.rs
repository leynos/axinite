//! Unit tests for context-based channel credential injection.

use std::sync::Arc;

use secrecy::SecretString;
use uuid::Uuid;

use super::*;
use crate::channels::wasm::capabilities::ChannelCapabilities;
use crate::channels::wasm::runtime::{
    PreparedChannelModule, WasmChannelRuntime, WasmChannelRuntimeConfig,
};
use crate::channels::wasm::wrapper::WasmChannel;
use crate::pairing::PairingStore;
use crate::secrets::{
    CreateSecretParams, CredentialMapping, DecryptedSecret, InMemorySecretsStore,
    NativeSecretsStore, Secret, SecretError, SecretRef, SecretsCrypto,
};
use crate::tools::wasm::{Capabilities, HttpCapability};

mod properties;

/// Verify that a channel is reported as missing only when it declares
/// credentials that none of its sources supplied.
#[test]
fn credential_outcome_reports_missing_only_for_declared_credentials() {
    assert_eq!(credential_outcome(0, 0), None);
    assert_eq!(credential_outcome(2, 0), Some("missing"));
    assert_eq!(credential_outcome(0, 1), Some("injected"));
    assert_eq!(credential_outcome(2, 1), Some("injected"));
}

/// Build a channel whose HTTP capability declares one credential mapping.
fn test_channel(name: &str, secret_name: &str) -> Arc<WasmChannel> {
    test_channel_with_mappings(name, &[secret_name.to_string()])
}

/// Build a channel whose HTTP capability declares the supplied credential
/// mappings.
fn test_channel_with_mappings(name: &str, secret_names: &[String]) -> Arc<WasmChannel> {
    let runtime = Arc::new(
        WasmChannelRuntime::new(WasmChannelRuntimeConfig::for_testing()).expect("test runtime"),
    );
    let prepared = Arc::new(PreparedChannelModule::for_testing(name, "test channel"));
    let http = secret_names
        .iter()
        .fold(HttpCapability::default(), |http, secret_name| {
            http.with_credential(
                secret_name,
                CredentialMapping::bearer(secret_name, "example.com"),
            )
        });
    let capabilities = ChannelCapabilities::for_channel(name)
        .with_tool_capabilities(Capabilities::default().with_http(http));

    Arc::new(WasmChannel::new(
        runtime,
        prepared,
        capabilities,
        "{}".to_string(),
        Arc::new(PairingStore::new()),
        None,
    ))
}

fn in_memory_store() -> Arc<InMemorySecretsStore> {
    let crypto = Arc::new(
        SecretsCrypto::new(SecretString::from(
            "0123456789abcdef0123456789abcdef".to_string(),
        ))
        .expect("test crypto"),
    );
    Arc::new(InMemorySecretsStore::new(crypto))
}

/// A secrets store whose list and decrypt results are fixed by the test.
struct StubSecretsStore {
    list_result: Result<Vec<SecretRef>, SecretError>,
    decrypt_result: Result<DecryptedSecret, SecretError>,
}

impl NativeSecretsStore for StubSecretsStore {
    async fn create<'a>(
        &'a self,
        _user_id: &'a str,
        _params: CreateSecretParams,
    ) -> Result<Secret, SecretError> {
        Err(SecretError::AccessDenied)
    }

    async fn get<'a>(&'a self, _user_id: &'a str, _name: &'a str) -> Result<Secret, SecretError> {
        Err(SecretError::AccessDenied)
    }

    /// Return the decryption result fixed by the test, re-wrapping any
    /// failure as `DecryptionFailed`.
    async fn get_decrypted<'a>(
        &'a self,
        _user_id: &'a str,
        _name: &'a str,
    ) -> Result<DecryptedSecret, SecretError> {
        match &self.decrypt_result {
            Ok(secret) => Ok(DecryptedSecret::from_bytes(
                secret.expose().as_bytes().to_vec(),
            )?),
            Err(error) => Err(match error {
                SecretError::DecryptionFailed(message) => {
                    SecretError::DecryptionFailed(message.clone())
                }
                other => SecretError::DecryptionFailed(other.to_string()),
            }),
        }
    }

    async fn exists<'a>(&'a self, _user_id: &'a str, _name: &'a str) -> Result<bool, SecretError> {
        Ok(false)
    }

    /// Return the secret metadata fixed by the test.
    async fn list<'a>(&'a self, _user_id: &'a str) -> Result<Vec<SecretRef>, SecretError> {
        match &self.list_result {
            Ok(refs) => Ok(refs.clone()),
            Err(error) => Err(match error {
                SecretError::Database(message) => SecretError::Database(message.clone()),
                other => SecretError::Database(other.to_string()),
            }),
        }
    }

    async fn delete<'a>(&'a self, _user_id: &'a str, _name: &'a str) -> Result<bool, SecretError> {
        Ok(false)
    }

    async fn record_usage(&self, _secret_id: Uuid) -> Result<(), SecretError> {
        Ok(())
    }

    async fn is_accessible<'a>(
        &'a self,
        _user_id: &'a str,
        _secret_name: &'a str,
        _allowed_secrets: &'a [String],
    ) -> Result<bool, SecretError> {
        Ok(false)
    }
}

#[tokio::test]
/// Verify that a whitespace-only channel name injects no credentials.
async fn blank_channel_name_injects_nothing() -> anyhow::Result<()> {
    let channel = test_channel("telegram", "telegram_bot_token");
    let env = EnvContext::default().with_env("TELEGRAM_BOT_TOKEN", "from-env");

    let count = inject_channel_credentials_from_context(&channel, None, "   ", &env).await?;

    assert_eq!(count, 0);
    assert!(channel.get_credentials().await.is_empty());
    Ok(())
}

#[tokio::test]
/// Verify that an environment variable matching a channel-prefixed
/// placeholder is injected when no persistent secret exists.
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

#[tokio::test]
/// Verify that a missing environment value leaves the credential uninjected.
async fn environment_fallback_skips_missing_credential() -> anyhow::Result<()> {
    let channel = test_channel("telegram", "telegram_bot_token");
    let env = EnvContext::default();

    let count = inject_channel_credentials_from_context(&channel, None, "telegram", &env).await?;

    assert_eq!(count, 0);
    assert!(channel.get_credentials().await.is_empty());
    Ok(())
}

#[tokio::test]
/// Verify that an environment variable whose name does not carry the channel
/// prefix is not injected.
async fn environment_fallback_ignores_non_prefixed_placeholder() -> anyhow::Result<()> {
    let channel = test_channel("telegram", "slack_bot_token");
    let env = EnvContext::default().with_env("SLACK_BOT_TOKEN", "from-env");

    let count = inject_channel_credentials_from_context(&channel, None, "telegram", &env).await?;

    assert_eq!(count, 0);
    assert!(channel.get_credentials().await.is_empty());
    Ok(())
}

#[tokio::test]
/// Verify that a stored secret wins over a matching environment value and is
/// injected only once.
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

#[tokio::test]
/// Verify that a secret which fails to decrypt is skipped and the environment
/// value is injected instead.
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

#[tokio::test]
/// Verify that a channel without an HTTP capability injects no credentials.
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

#[tokio::test]
#[tracing_test::traced_test]
/// Verify that an unrelated stored secret does not mask a declared credential
/// that no source resolved.
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
        logs_contain("Channel declares credentials but none resolved"),
        "a missing declared credential must still be reported"
    );
    Ok(())
}

#[cfg(feature = "metrics")]
mod telemetry;

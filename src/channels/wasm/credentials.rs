//! Inject credentials declared by a WASM channel.

use std::collections::HashSet;
use std::sync::Arc;

use crate::channels::wasm::WasmChannel;
use crate::config::EnvContext;
use crate::secrets::SecretsStore;

/// Inject credentials for a channel based on naming convention.
///
/// Looks for secrets matching the pattern `{channel_name}_*` and injects them
/// as credential placeholders (e.g., `telegram_bot_token` -> `{TELEGRAM_BOT_TOKEN}`).
///
/// Falls back to environment variables starting with the uppercase channel name
/// prefix (e.g., `TELEGRAM_`) for credentials missing from the secrets store.
///
/// Returns the number of credentials injected.
pub async fn inject_channel_credentials(
    channel: &Arc<WasmChannel>,
    secrets: Option<&dyn SecretsStore>,
    channel_name: &str,
) -> anyhow::Result<usize> {
    inject_channel_credentials_from_context(
        channel,
        secrets,
        channel_name,
        &EnvContext::capture_ambient(),
    )
    .await
}

/// Inject channel credentials using an explicit environment snapshot.
pub async fn inject_channel_credentials_from_context(
    channel: &Arc<WasmChannel>,
    secrets: Option<&dyn SecretsStore>,
    channel_name: &str,
    env_context: &EnvContext,
) -> anyhow::Result<usize> {
    if channel_name.trim().is_empty() {
        return Ok(0);
    }

    let mut count = 0;
    let mut injected_placeholders = HashSet::new();

    // 1. Try injecting from persistent secrets store if available
    if let Some(secrets) = secrets {
        let all_secrets = secrets
            .list("default")
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list secrets: {}", e))?;

        let prefix = format!("{}_", channel_name.to_ascii_lowercase());

        for secret_meta in all_secrets {
            if !secret_meta.name.to_ascii_lowercase().starts_with(&prefix) {
                continue;
            }

            let decrypted = match secrets.get_decrypted("default", &secret_meta.name).await {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!(
                        secret = %secret_meta.name,
                        error = %e,
                        "Failed to decrypt secret for channel credential injection"
                    );
                    continue;
                }
            };

            let placeholder = secret_meta.name.to_uppercase();

            tracing::debug!(
                channel = %channel_name,
                secret = %secret_meta.name,
                placeholder = %placeholder,
                "Injecting credential"
            );

            channel
                .set_credential(&placeholder, decrypted.expose().to_string())
                .await;
            injected_placeholders.insert(placeholder);
            count += 1;
        }
    }

    // 2. Fall back to the supplied environment for credentials missing from the
    // secrets store.
    count +=
        inject_environment_credentials(channel, channel_name, &injected_placeholders, env_context)
            .await;

    Ok(count)
}

/// Inject matching environment-backed credentials, skipping secrets already
/// supplied by the persistent store.
async fn inject_environment_credentials(
    channel: &Arc<WasmChannel>,
    channel_name: &str,
    injected_placeholders: &HashSet<String>,
    env_context: &EnvContext,
) -> usize {
    let prefix = format!("{}_", channel_name.to_ascii_uppercase());
    let caps = channel.capabilities();
    let mut count = 0;
    if let Some(ref http_cap) = caps.tool_capabilities.http {
        for cred_mapping in http_cap.credentials.values() {
            let placeholder = cred_mapping.secret_name.to_uppercase();
            if injected_placeholders.contains(&placeholder) {
                continue;
            }
            if !placeholder.starts_with(&prefix) {
                tracing::warn!(
                    channel = %channel_name,
                    placeholder = %placeholder,
                    "Ignoring non-prefixed credential placeholder in environment fallback"
                );
                continue;
            }
            if let Some(env_value) = env_context.get_owned(&placeholder) {
                tracing::debug!(
                    channel = %channel_name,
                    placeholder = %placeholder,
                    "Injecting credential from environment variable"
                );
                channel.set_credential(&placeholder, env_value).await;
                count += 1;
            }
        }
    }

    count
}

#[cfg(test)]
mod tests {
    //! Unit tests for context-based channel credential injection.

    use std::collections::HashMap;
    use std::sync::Arc;

    use proptest::prelude::*;
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

        async fn get<'a>(
            &'a self,
            _user_id: &'a str,
            _name: &'a str,
        ) -> Result<Secret, SecretError> {
            Err(SecretError::AccessDenied)
        }

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

        async fn exists<'a>(
            &'a self,
            _user_id: &'a str,
            _name: &'a str,
        ) -> Result<bool, SecretError> {
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

        async fn delete<'a>(
            &'a self,
            _user_id: &'a str,
            _name: &'a str,
        ) -> Result<bool, SecretError> {
            Ok(false)
        }

        async fn record_usage<'a>(&'a self, _secret_id: Uuid) -> Result<(), SecretError> {
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
    async fn blank_channel_name_injects_nothing() -> anyhow::Result<()> {
        let channel = test_channel("telegram", "telegram_bot_token");
        let env = EnvContext::default().with_env("TELEGRAM_BOT_TOKEN", "from-env");

        let count = inject_channel_credentials_from_context(&channel, None, "   ", &env).await?;

        assert_eq!(count, 0);
        assert!(channel.get_credentials().await.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn environment_fallback_injects_prefixed_credential() -> anyhow::Result<()> {
        let channel = test_channel("telegram", "telegram_bot_token");
        let env = EnvContext::default().with_env("TELEGRAM_BOT_TOKEN", "from-env");

        let count =
            inject_channel_credentials_from_context(&channel, None, "telegram", &env).await?;

        assert_eq!(count, 1);
        assert_eq!(
            channel.get_credentials().await.get("TELEGRAM_BOT_TOKEN"),
            Some(&"from-env".to_string())
        );
        Ok(())
    }

    #[tokio::test]
    async fn environment_fallback_skips_missing_credential() -> anyhow::Result<()> {
        let channel = test_channel("telegram", "telegram_bot_token");
        let env = EnvContext::default();

        let count =
            inject_channel_credentials_from_context(&channel, None, "telegram", &env).await?;

        assert_eq!(count, 0);
        assert!(channel.get_credentials().await.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn environment_fallback_ignores_non_prefixed_placeholder() -> anyhow::Result<()> {
        let channel = test_channel("telegram", "slack_bot_token");
        let env = EnvContext::default().with_env("SLACK_BOT_TOKEN", "from-env");

        let count =
            inject_channel_credentials_from_context(&channel, None, "telegram", &env).await?;

        assert_eq!(count, 0);
        assert!(channel.get_credentials().await.is_empty());
        Ok(())
    }

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

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        /// Only channel-prefixed placeholders are injected, persistent secrets
        /// take precedence over environment values, and the reported count
        /// matches the injected placeholder set.
        #[test]
        fn prop_credential_injection_respects_prefix_and_precedence(
            channel_suffix in 0u8..3,
            secret_specs in prop::collection::vec((0u8..4, any::<bool>(), any::<bool>()), 0..6),
            mapping_specs in prop::collection::vec((0u8..4, any::<bool>()), 0..6),
        ) {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("test runtime");
            runtime.block_on(async move {
                let channel_name = format!("chan{channel_suffix}");
                let prefix = format!("{}_", channel_name.to_ascii_uppercase());

                let secret_name = |id: u8, prefixed: bool| {
                    if prefixed {
                        format!("{channel_name}_secret{id}")
                    } else {
                        format!("other_secret{id}")
                    }
                };

                let mut mapping_names: Vec<String> = mapping_specs
                    .iter()
                    .map(|(id, prefixed)| secret_name(*id, *prefixed))
                    .collect();
                mapping_names.sort();
                mapping_names.dedup();

                let channel = test_channel_with_mappings(&channel_name, &mapping_names);
                let store = in_memory_store();
                let mut expected_store = HashMap::new();
                for (index, (id, prefixed, persistent)) in secret_specs.iter().enumerate() {
                    if !*persistent {
                        continue;
                    }
                    let name = secret_name(*id, *prefixed);
                    let value = format!("store-{index}");
                    NativeSecretsStore::create(
                        store.as_ref(),
                        "default",
                        CreateSecretParams::new(&name, &value),
                    )
                    .await
                    .expect("test secret should be created");
                    if *prefixed {
                        expected_store.insert(name.to_uppercase(), value);
                    }
                }

                let mut env = EnvContext::default();
                let mut expected_env = HashMap::new();
                for (index, (id, prefixed)) in mapping_specs.iter().enumerate() {
                    let placeholder = secret_name(*id, *prefixed).to_uppercase();
                    if !placeholder.starts_with(&prefix)
                        || expected_store.contains_key(&placeholder)
                    {
                        continue;
                    }
                    let value = format!("env-{index}");
                    env = env.with_env(placeholder.as_str(), value.as_str());
                    expected_env.insert(placeholder, value);
                }

                let count = inject_channel_credentials_from_context(
                    &channel,
                    Some(store.as_ref() as &dyn SecretsStore),
                    &channel_name,
                    &env,
                )
                .await
                .expect("injection should succeed");

                let credentials = channel.get_credentials().await;
                assert_eq!(count, credentials.len());
                for (placeholder, value) in &credentials {
                    assert!(
                        placeholder.starts_with(&prefix),
                        "unprefixed placeholder {placeholder} was injected"
                    );
                    if let Some(expected) = expected_store.get(placeholder) {
                        assert_eq!(value, expected, "persistent secret must win");
                    } else if let Some(expected) = expected_env.get(placeholder) {
                        assert_eq!(value, expected);
                    } else {
                        panic!("unexpected placeholder {placeholder}");
                    }
                }
                for placeholder in expected_store.keys().chain(expected_env.keys()) {
                    assert!(
                        credentials.contains_key(placeholder),
                        "missing placeholder {placeholder}"
                    );
                }
            });
        }
    }

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

        let count =
            inject_channel_credentials_from_context(&channel, None, "telegram", &env).await?;

        assert_eq!(count, 0);
        assert!(channel.get_credentials().await.is_empty());
        Ok(())
    }
}

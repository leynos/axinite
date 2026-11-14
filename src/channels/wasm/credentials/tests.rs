//! Unit tests for context-based channel credential injection.
//!
//! This module holds the fixtures shared by the test submodules. The
//! behavioural tests live in [`credential_injection`], the generated cases in
//! `properties`, and the telemetry assertions behind the `metrics` feature in
//! `telemetry`.

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

mod credential_injection;
mod properties;

/// Verify that a channel is reported as missing only when it declares
/// credentials that none of its sources supplied, and as partial when only
/// some of them resolved.
#[test]
fn credential_outcome_reports_missing_only_for_declared_credentials() {
    assert_eq!(credential_outcome(0, 0), None);
    assert_eq!(credential_outcome(2, 0), Some("missing"));
    assert_eq!(credential_outcome(0, 1), Some("injected"));
    assert_eq!(credential_outcome(2, 1), Some("partial"));
    assert_eq!(credential_outcome(2, 2), Some("injected"));
}

/// Build a channel whose HTTP capability declares one credential mapping.
fn test_channel(name: &str, secret_name: &str) -> Arc<WasmChannel> {
    test_channel_with_mappings(name, &[secret_name.to_string()])
}

/// Build a channel whose HTTP capability declares the supplied credential
/// mappings.
fn test_channel_with_mappings(name: &str, secret_names: &[String]) -> Arc<WasmChannel> {
    let mappings: Vec<(String, String)> = secret_names
        .iter()
        .map(|secret_name| (secret_name.clone(), secret_name.clone()))
        .collect();
    test_channel_with_credential_mappings(name, &mappings)
}

/// Build a channel whose credential mappings use explicit `(key, secret_name)`
/// pairs.
///
/// Distinct keys may name the same secret, so a channel can declare a repeated
/// placeholder while keeping a valid capability map.
fn test_channel_with_credential_mappings(
    name: &str,
    mappings: &[(String, String)],
) -> Arc<WasmChannel> {
    let runtime = Arc::new(
        WasmChannelRuntime::new(WasmChannelRuntimeConfig::for_testing()).expect("test runtime"),
    );
    let prepared = Arc::new(PreparedChannelModule::for_testing(name, "test channel"));
    let http = mappings
        .iter()
        .fold(HttpCapability::default(), |http, (key, secret_name)| {
            http.with_credential(
                key.clone(),
                CredentialMapping::bearer(secret_name.clone(), "example.com"),
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

#[cfg(feature = "metrics")]
mod telemetry;

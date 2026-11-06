//! Unit tests for secret injection from the secrets store into the
//! explicit environment snapshots.

use std::sync::Arc;

use mockall::automock;
use secrecy::SecretString;

use super::*;
use crate::secrets::{
    CreateSecretParams, DecryptedSecret, InMemorySecretsStore, NativeSecretsStore, Secret,
    SecretError, SecretRef, SecretsCrypto, SecretsStore,
};
use crate::testing::credentials::TEST_CRYPTO_KEY;

/// Synchronous mirror of `NativeSecretsStore` for mockall compatibility.
///
/// This trait exists as a workaround for mockall's inability to mock async
/// traits with lifetime parameters directly. `SyncSecretsStore` mirrors
/// `NativeSecretsStore` but uses owned `String` types instead of `&str`
/// references, allowing mockall to generate `MockSyncSecretsStore`.
///
/// The `MockSecretsStore` wrapper implements `NativeSecretsStore` by
/// delegating to the generated mock, converting `&str` to `String` at
/// each call boundary. This keeps tests synchronous and avoids the
/// complexity of boxed futures or the `async-trait` crate.
#[automock]
trait SyncSecretsStore {
    fn create(&self, user_id: String, params: CreateSecretParams) -> Result<Secret, SecretError>;
    fn get(&self, user_id: String, name: String) -> Result<Secret, SecretError>;
    fn get_decrypted(&self, user_id: String, name: String) -> Result<DecryptedSecret, SecretError>;
    fn exists(&self, user_id: String, name: String) -> Result<bool, SecretError>;
    fn list(&self, user_id: String) -> Result<Vec<SecretRef>, SecretError>;
    fn delete(&self, user_id: String, name: String) -> Result<bool, SecretError>;
    fn record_usage(&self, secret_id: uuid::Uuid) -> Result<(), SecretError>;
    fn is_accessible(
        &self,
        user_id: String,
        secret_name: String,
        allowed_secrets: Vec<String>,
    ) -> Result<bool, SecretError>;
}

struct MockSecretsStore {
    inner: MockSyncSecretsStore,
}

impl MockSecretsStore {
    fn new(inner: MockSyncSecretsStore) -> Self {
        Self { inner }
    }
}

impl NativeSecretsStore for MockSecretsStore {
    async fn create(
        &self,
        user_id: &str,
        params: CreateSecretParams,
    ) -> Result<Secret, SecretError> {
        self.inner.create(user_id.to_string(), params)
    }

    async fn get(&self, user_id: &str, name: &str) -> Result<Secret, SecretError> {
        self.inner.get(user_id.to_string(), name.to_string())
    }

    async fn get_decrypted(
        &self,
        user_id: &str,
        name: &str,
    ) -> Result<DecryptedSecret, SecretError> {
        self.inner
            .get_decrypted(user_id.to_string(), name.to_string())
    }

    async fn exists(&self, user_id: &str, name: &str) -> Result<bool, SecretError> {
        self.inner.exists(user_id.to_string(), name.to_string())
    }

    async fn list(&self, user_id: &str) -> Result<Vec<SecretRef>, SecretError> {
        self.inner.list(user_id.to_string())
    }

    async fn delete(&self, user_id: &str, name: &str) -> Result<bool, SecretError> {
        self.inner.delete(user_id.to_string(), name.to_string())
    }

    async fn record_usage(&self, secret_id: uuid::Uuid) -> Result<(), SecretError> {
        self.inner.record_usage(secret_id)
    }

    async fn is_accessible(
        &self,
        user_id: &str,
        secret_name: &str,
        allowed_secrets: &[String],
    ) -> Result<bool, SecretError> {
        self.inner.is_accessible(
            user_id.to_string(),
            secret_name.to_string(),
            allowed_secrets.to_vec(),
        )
    }
}

fn make_error_store() -> Arc<dyn SecretsStore + Send + Sync> {
    let mut mock = MockSyncSecretsStore::new();
    mock.expect_get_decrypted()
        .returning(|_, _| Err(SecretError::Database("simulated store failure".to_string())));
    Arc::new(MockSecretsStore::new(mock))
}

/// Test the HTTP_WEBHOOK_SECRET_KEY constant value.
#[test]
fn http_webhook_secret_key_has_correct_value() {
    assert_eq!(HTTP_WEBHOOK_SECRET_KEY, "HTTP_WEBHOOK_SECRET");
}

#[tokio::test]
async fn db_secret_injector_injects_and_clears_webhook_secret() {
    let crypto = Arc::new(
        SecretsCrypto::new(SecretString::from(TEST_CRYPTO_KEY.to_string()))
            .expect("test crypto should initialize"),
    );
    let store: Arc<dyn SecretsStore + Send + Sync> = Arc::new(InMemorySecretsStore::new(crypto));
    store
        .create(
            "test_user",
            CreateSecretParams::new(SECRETS_STORE_KEY, "super-secret-value"),
        )
        .await
        .expect("secret should be created");

    let injector = DbSecretInjector::new(Arc::clone(&store), UserId::from("test_user"));
    let mut ctx = EnvContext::default();

    NativeSecretInjector::inject(&injector, &mut ctx).await;
    assert_eq!(
        ctx.get(HTTP_WEBHOOK_SECRET_KEY),
        Some("super-secret-value"),
        "inject() should populate the supplied snapshot from the secrets store"
    );

    store
        .delete("test_user", SECRETS_STORE_KEY)
        .await
        .expect("secret should be deleted");
    NativeSecretInjector::inject(&injector, &mut ctx).await;

    assert_eq!(
        ctx.get(HTTP_WEBHOOK_SECRET_KEY),
        None,
        "inject() should clear the snapshot when the secret is removed"
    );
}

#[tokio::test]
async fn db_secret_injector_reapplies_last_successful_secret_after_store_error() {
    let mut mock = MockSyncSecretsStore::new();
    let mut sequence = mockall::Sequence::new();
    mock.expect_get_decrypted()
        .times(1)
        .in_sequence(&mut sequence)
        .returning(|_, _| {
            Ok(DecryptedSecret::from_bytes(b"rotated-secret".to_vec())
                .expect("test secret should decode"))
        });
    mock.expect_get_decrypted()
        .times(1)
        .in_sequence(&mut sequence)
        .returning(|_, _| Err(SecretError::Database("transient failure".to_string())));
    mock.expect_get_decrypted()
        .times(1)
        .in_sequence(&mut sequence)
        .returning(|_, _| Err(SecretError::NotFound(SECRETS_STORE_KEY.to_string())));
    mock.expect_get_decrypted()
        .times(1)
        .in_sequence(&mut sequence)
        .returning(|_, _| Err(SecretError::Database("transient failure".to_string())));

    let store: Arc<dyn SecretsStore + Send + Sync> = Arc::new(MockSecretsStore::new(mock));
    let injector = DbSecretInjector::new(store, UserId::from("test_user"));

    let mut first_cycle = EnvContext::default();
    NativeSecretInjector::inject(&injector, &mut first_cycle).await;
    assert_eq!(
        first_cycle.get(HTTP_WEBHOOK_SECRET_KEY),
        Some("rotated-secret")
    );

    // Each reload cycle starts from a fresh snapshot that does not carry
    // the previous cycle's injected value.
    let mut second_cycle = EnvContext::default();
    NativeSecretInjector::inject(&injector, &mut second_cycle).await;
    assert_eq!(
        second_cycle.get(HTTP_WEBHOOK_SECRET_KEY),
        Some("rotated-secret"),
        "a transient store error must re-apply the last successfully injected secret"
    );

    let mut third_cycle =
        EnvContext::default().with_secret(HTTP_WEBHOOK_SECRET_KEY, "stale-secret");
    NativeSecretInjector::inject(&injector, &mut third_cycle).await;
    assert_eq!(
        third_cycle.get(HTTP_WEBHOOK_SECRET_KEY),
        None,
        "a missing secret must clear both the snapshot entry and the retained value"
    );

    let mut fourth_cycle = EnvContext::default();
    NativeSecretInjector::inject(&injector, &mut fourth_cycle).await;
    assert_eq!(
        fourth_cycle.get(HTTP_WEBHOOK_SECRET_KEY),
        None,
        "a cleared secret must not be resurrected by a later store error"
    );
}

#[tokio::test]
async fn db_secret_injector_does_not_resurrect_startup_secret_after_deletion() {
    let mut mock = MockSyncSecretsStore::new();
    let mut sequence = mockall::Sequence::new();
    mock.expect_get_decrypted()
        .times(1)
        .in_sequence(&mut sequence)
        .returning(|_, _| Err(SecretError::NotFound(SECRETS_STORE_KEY.to_string())));
    mock.expect_get_decrypted()
        .times(1)
        .in_sequence(&mut sequence)
        .returning(|_, _| Err(SecretError::Database("transient failure".to_string())));

    let store: Arc<dyn SecretsStore + Send + Sync> = Arc::new(MockSecretsStore::new(mock));
    let injector = DbSecretInjector::new(store, UserId::from("test_user"));

    // Every reload cycle starts from a fresh clone of the startup snapshot,
    // which still contains the credential captured at startup.
    let mut deletion_cycle =
        EnvContext::default().with_secret(HTTP_WEBHOOK_SECRET_KEY, "startup-secret");
    NativeSecretInjector::inject(&injector, &mut deletion_cycle).await;
    assert_eq!(
        deletion_cycle.get(HTTP_WEBHOOK_SECRET_KEY),
        None,
        "a confirmed deletion must clear the startup snapshot's secret"
    );

    let mut error_cycle =
        EnvContext::default().with_secret(HTTP_WEBHOOK_SECRET_KEY, "startup-secret");
    NativeSecretInjector::inject(&injector, &mut error_cycle).await;
    assert_eq!(
        error_cycle.get(HTTP_WEBHOOK_SECRET_KEY),
        None,
        "a transient error after a confirmed deletion must not resurrect the startup secret"
    );
}

#[tokio::test]
async fn db_secret_injector_preserves_snapshot_on_store_error() {
    let store: Arc<dyn SecretsStore + Send + Sync> = make_error_store();
    let injector = DbSecretInjector::new(store, UserId::from("test_user"));
    let mut ctx = EnvContext::default().with_secret(HTTP_WEBHOOK_SECRET_KEY, "existing-secret");

    NativeSecretInjector::inject(&injector, &mut ctx).await;

    assert_eq!(
        ctx.get(HTTP_WEBHOOK_SECRET_KEY),
        Some("existing-secret"),
        "inject() should preserve the existing snapshot when the store returns a non-NotFound error"
    );
}

mod properties;

#[cfg(feature = "metrics")]
mod telemetry;

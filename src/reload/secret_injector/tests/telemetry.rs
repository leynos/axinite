//! Counter assertions for the secret-injection production path.

use super::*;
use crate::testing::metrics;

#[tokio::test]
async fn injected_secret_records_injected_outcome() {
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
    let injector = DbSecretInjector::new(store, UserId::from("test_user"));

    let _guard = metrics::record();
    NativeSecretInjector::inject(&injector, &mut EnvContext::default()).await;

    assert_eq!(
        metrics::counter("axinite.reload.secret_injection", "injected"),
        1
    );
}

#[tokio::test]
async fn missing_secret_records_missing_outcome() {
    let mut mock = MockSyncSecretsStore::new();
    mock.expect_get_decrypted()
        .returning(|_, _| Err(SecretError::NotFound(SECRETS_STORE_KEY.to_string())));
    let store: Arc<dyn SecretsStore + Send + Sync> = Arc::new(MockSecretsStore::new(mock));
    let injector = DbSecretInjector::new(store, UserId::from("test_user"));

    let _guard = metrics::record();
    NativeSecretInjector::inject(&injector, &mut EnvContext::default()).await;

    assert_eq!(
        metrics::counter("axinite.reload.secret_injection", "missing"),
        1
    );
}

#[tokio::test]
async fn store_error_without_prior_success_records_error_outcome() {
    let store: Arc<dyn SecretsStore + Send + Sync> = make_error_store();
    let injector = DbSecretInjector::new(store, UserId::from("test_user"));

    let _guard = metrics::record();
    NativeSecretInjector::inject(&injector, &mut EnvContext::default()).await;

    assert_eq!(
        metrics::counter("axinite.reload.secret_injection", "error"),
        1
    );
}

#[tokio::test]
async fn store_error_after_success_records_retained_outcome() {
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
    let store: Arc<dyn SecretsStore + Send + Sync> = Arc::new(MockSecretsStore::new(mock));
    let injector = DbSecretInjector::new(store, UserId::from("test_user"));

    let _guard = metrics::record();
    NativeSecretInjector::inject(&injector, &mut EnvContext::default()).await;
    NativeSecretInjector::inject(&injector, &mut EnvContext::default()).await;

    assert_eq!(
        metrics::counter("axinite.reload.secret_injection", "retained_after_error"),
        1
    );
}

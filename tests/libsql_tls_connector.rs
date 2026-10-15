//! Ensure the registry SDK uses the patched built-in TLS connector.

#![cfg(feature = "libsql")]

use std::time::Duration;

#[tokio::test]
async fn replica_failures_propagate_without_installing_a_global_crypto_provider()
-> anyhow::Result<()> {
    #[cfg(feature = "postgres")]
    assert!(rustls::crypto::CryptoProvider::get_default().is_none());
    let directory = tempfile::tempdir()?;
    let database = libsql::Builder::new_remote_replica(
        directory.path().join("replica.db"),
        "http://127.0.0.1:0".to_owned(),
        "test-token".to_owned(),
    )
    .build()
    .await?;
    // Replicas connect lazily; sync exercises the SDK's built-in connector.
    let result = tokio::time::timeout(Duration::from_secs(5), database.sync()).await?;
    assert!(
        result.is_err(),
        "unreachable replicas must return connection errors"
    );
    #[cfg(feature = "postgres")]
    assert!(rustls::crypto::CryptoProvider::get_default().is_none());
    Ok(())
}

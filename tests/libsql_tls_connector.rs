//! Verify lazy SDK construction and unreachable HTTP replica error propagation.
//!
//! This boundary uses plaintext HTTP: it checks connector selection without a
//! global provider, not successful HTTPS replication or the replication protocol.

#![cfg(feature = "libsql")]

use std::time::Duration;

/// Surface an unreachable replica as a sync error without installing a global
/// provider.
#[tokio::test]
async fn unreachable_http_replica_fails_without_a_global_crypto_provider() -> anyhow::Result<()> {
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
        "unreachable HTTP replicas must return connection errors: {result:?}"
    );
    #[cfg(feature = "postgres")]
    assert!(rustls::crypto::CryptoProvider::get_default().is_none());
    Ok(())
}

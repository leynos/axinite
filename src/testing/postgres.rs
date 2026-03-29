//! Postgres-specific test helpers.

use crate::config::{DatabaseBackend, DatabaseConfig, SslMode};
use crate::db::postgres::PgBackend;
use crate::error::DatabaseError;
use secrecy::SecretString;

// These substrings are limited to concrete local transport and name-resolution
// failures observed when a test Postgres instance is absent. We intentionally
// exclude generic timeout wording so TLS, authentication, and other
// misconfiguration-related delays still fail loudly instead of being skipped.
use std::error::Error as _;
use deadpool_postgres::PoolError;
const UNAVAILABLE_PATTERNS: &[&str] = &[
    "connection refused",
    "failed to lookup address information",
    "name or service not known",
    "temporary failure in name resolution",
    "network is unreachable",
    "no such file or directory",
    "could not connect to server",
];

/// Create a PostgreSQL-backed test database.
///
/// Reads the test database URL from the `TEST_DATABASE_URL` environment
/// variable, or falls back to a default local Postgres instance.
/// Returns the `PgBackend` instance for testing, propagating any
/// connection or pool errors to the caller.
///
/// # Examples
///
/// ```no_run
/// use crate::testing::postgres::test_pg_db;
///
/// async fn example() -> Result<(), Box<dyn std::error::Error>> {
///     let db = test_pg_db().await?;
///     let _ = db;
///     Ok(())
/// }
/// ```
pub async fn test_pg_db() -> Result<PgBackend, DatabaseError> {
    let url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://localhost/ironclaw_test".to_string());

    let config = DatabaseConfig {
        backend: DatabaseBackend::Postgres,
        url: SecretString::from(url),
        pool_size: 5,
        ssl_mode: SslMode::Prefer,
        libsql_path: None,
        libsql_url: None,
        libsql_auth_token: None,
    };

    PgBackend::new(&config).await
}

/// Attempt to create a test `PgBackend`, returning `None` only when the
/// database is unavailable.
///
/// Use this in test fixtures that should be skipped when no local Postgres
/// instance is available, while still surfacing configuration and
/// authentication mistakes.
pub async fn try_test_pg_db() -> Result<Option<PgBackend>, DatabaseError> {
    match test_pg_db().await {
        Ok(db) => Ok(Some(db)),
        Err(error) if is_database_unavailable(&error) => {
            eprintln!("Skipping Postgres test (database unavailable): {error}");
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn is_database_unavailable(error: &DatabaseError) -> bool {
    match error {
        DatabaseError::PoolRuntime(pool_error) => is_pool_unavailable(pool_error),
        DatabaseError::Postgres(postgres_error) => is_postgres_unavailable(postgres_error),
        _ => false,
    }
}

fn is_pool_unavailable(error: &PoolError) -> bool {
    match error {
        PoolError::Timeout(_) | PoolError::Closed => true,
        PoolError::Backend(postgres_error) => is_postgres_unavailable(postgres_error),
        PoolError::PostCreateHook(hook_error) => hook_error
            .source()
            .and_then(|source| source.downcast_ref::<tokio_postgres::Error>())
            .is_some_and(is_postgres_unavailable),
        PoolError::NoRuntimeSpecified => false,
    }
}

fn has_unavailable_connection_cause(error: &tokio_postgres::Error) -> bool {
    let mut current = error.source();
    while let Some(source) = current {
        let lowered = source.to_string().to_lowercase();
        if UNAVAILABLE_PATTERNS
            .iter()
            .any(|pattern| lowered.contains(pattern))
        {
            return true;
        }
        current = source.source();
    }

    false
}

fn is_postgres_unavailable(error: &tokio_postgres::Error) -> bool {
    error.is_closed() || has_unavailable_connection_cause(error)
}

//! Postgres-specific test helpers.

use crate::config::{DatabaseBackend, DatabaseConfig, SslMode};
use crate::db::postgres::PgBackend;
use crate::error::DatabaseError;
use secrecy::SecretString;

const UNAVAILABLE_PATTERNS: &[&str] = &[
    "connection refused",
    "timed out",
    "timeout",
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
    let lowered = format!("{error:?} {error}").to_lowercase();

    UNAVAILABLE_PATTERNS.iter().any(|p| lowered.contains(p))
}

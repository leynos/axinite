//! Postgres-specific test helpers.

use crate::config::{DatabaseBackend, DatabaseConfig, SslMode};
use crate::db::postgres::PgBackend;
use crate::error::DatabaseError;
use secrecy::SecretString;
use std::ffi::OsStr;

// These substrings are limited to concrete local transport and name-resolution
// failures observed when a test Postgres instance is absent. We intentionally
// exclude generic timeout wording so TLS, authentication, and other
// misconfiguration-related delays still fail loudly instead of being skipped.
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
/// use axinite::testing::postgres::test_pg_db;
///
/// async fn example() -> Result<(), Box<dyn std::error::Error>> {
///     let db = test_pg_db().await?;
///     let _ = db;
///     Ok(())
/// }
/// ```
pub async fn test_pg_db() -> Result<PgBackend, DatabaseError> {
    PgBackend::new(&test_pg_config(test_database_url())).await
}

/// Read the URL a test database is reached at.
///
/// A lane that provides Postgres names it in `TEST_DATABASE_URL`; a checkout
/// that does not falls back to a local instance that may well be absent.
fn test_database_url() -> String {
    std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://localhost/axinite_test".to_string())
}

/// Build the test backend configuration for `url`.
fn test_pg_config(url: String) -> DatabaseConfig {
    DatabaseConfig {
        backend: DatabaseBackend::Postgres,
        url: SecretString::from(url),
        pool_size: 5,
        ssl_mode: SslMode::Prefer,
        libsql_path: None,
        libsql_url: None,
        libsql_auth_token: None,
    }
}

/// Set by a CI lane that provides Postgres on purpose.
///
/// A developer without a local Postgres should still be able to run the rest
/// of the suite, so an unreachable database is a skip by default. A lane that
/// starts a Postgres service and points the tests at it has no such excuse: a
/// skip there reports success for tests that never ran, which is how
/// `coverage.yml` stayed green while publishing coverage measured without
/// them. The lane says so by setting this, and the skip stops being available.
const REQUIRE_POSTGRES_ENV: &str = "AXINITE_REQUIRE_POSTGRES";

/// Whether a run may carry on without a database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PostgresRequirement {
    /// Nobody promised a database, so its absence skips the Postgres tests.
    Optional,
    /// A database was provided on purpose, so its absence is a failure.
    Required,
}

impl PostgresRequirement {
    /// Read the requirement from an environment value.
    ///
    /// Split from `from_env` so the reading is a pure function: a test that
    /// set the variable would have to be serialized against every other test
    /// in the process, and the environment is not a structural reason to
    /// serialize.
    fn from_value(value: Option<&str>) -> Self {
        match value {
            Some(raw) if !raw.trim().is_empty() => Self::Required,
            _ => Self::Optional,
        }
    }

    /// Read the requirement from the raw value the platform holds.
    ///
    /// A value that is not valid Unicode is still a value: the lane set the
    /// variable, so the reading that keeps the promise is `Required`. Going
    /// through `std::env::var(..).ok()` instead would map that read failure
    /// onto the same `None` as an unset variable, restoring the skip on
    /// exactly the lane that asked for it to be gone, and silently.
    fn from_os_value(value: Option<&OsStr>) -> Self {
        match value {
            None => Self::Optional,
            Some(raw) => raw
                .to_str()
                .map_or(Self::Required, |text| Self::from_value(Some(text))),
        }
    }

    /// Read the requirement this process was started with.
    fn from_env() -> Self {
        Self::from_os_value(std::env::var_os(REQUIRE_POSTGRES_ENV).as_deref())
    }
}

/// Report whether an unavailable database may be skipped over.
///
/// Both halves have to hold. The error must be one of the transport and
/// name-resolution failures that mean "nothing is listening", which is what
/// keeps an authentication or configuration mistake loud; and the run must not
/// have declared that it provides a database.
fn skip_is_allowed(error: &DatabaseError, requirement: PostgresRequirement) -> bool {
    requirement == PostgresRequirement::Optional && is_database_unavailable(error)
}

/// Attempt to create a test `PgBackend`, returning `None` only when the
/// database is unavailable and this run did not promise one.
///
/// Use this in test fixtures that should be skipped when no local Postgres
/// instance is available, while still surfacing configuration and
/// authentication mistakes. A lane that sets `AXINITE_REQUIRE_POSTGRES` gets no
/// skip at all: an unreachable database fails there rather than reporting
/// success for tests that never ran.
pub async fn try_test_pg_db() -> Result<Option<PgBackend>, DatabaseError> {
    try_pg_db_at(test_database_url(), PostgresRequirement::from_env()).await
}

/// Connect at `url`, skipping only where `requirement` leaves a skip available.
///
/// Split from `try_test_pg_db` so the decision can be exercised against a real
/// unreachable endpoint without setting an environment variable, which would
/// have to be serialized against every other test in the process. What is left
/// in the caller is the composition of two readings that are each tested on
/// their own: `test_database_url` and `PostgresRequirement::from_env`.
async fn try_pg_db_at(
    url: String,
    requirement: PostgresRequirement,
) -> Result<Option<PgBackend>, DatabaseError> {
    match PgBackend::new(&test_pg_config(url)).await {
        Ok(db) => Ok(Some(db)),
        Err(error) if skip_is_allowed(&error, requirement) => {
            eprintln!("Skipping Postgres test (database unavailable): {error}");
            Ok(None)
        }
        Err(error) => {
            if is_database_unavailable(&error) {
                eprintln!(
                    concat!(
                        "Postgres is unreachable and {env} is set, so this ",
                        "is a failure rather than a skip: {error}"
                    ),
                    env = REQUIRE_POSTGRES_ENV,
                    error = error
                );
            }
            Err(error)
        }
    }
}

fn is_database_unavailable(error: &DatabaseError) -> bool {
    let lowered = format!("{error:?} {error}").to_lowercase();

    matches!(
        error,
        DatabaseError::Postgres(_)
            | DatabaseError::Pool(_)
            | DatabaseError::PoolBuild(_)
            | DatabaseError::PoolRuntime(_)
    ) && UNAVAILABLE_PATTERNS.iter().any(|p| lowered.contains(p))
}

#[cfg(test)]
mod tests;

//! Postgres-specific test helpers.
//!
//! Every PostgreSQL-backed test reaches its database through
//! [`try_test_pg_db`]. A lane that names a database in `TEST_DATABASE_URL`
//! gets that database. Otherwise, on Linux with `test-helpers`, the test gets a
//! fresh database cloned from a migrated template on an embedded cluster the
//! test process owns (see `embedded`). No test reaches a PostgreSQL the harness
//! did not provision or the lane did not name: the old fallback to a local
//! instance is gone, because on a host whose PostgreSQL rejects the local user
//! it turned every PostgreSQL-backed test into a failure.

#[cfg(all(feature = "test-helpers", target_os = "linux"))]
pub mod embedded;

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

/// A test's database, together with whatever owns it.
///
/// Derefs to [`PgBackend`], so a test uses it exactly as it used the backend
/// before and most call sites need no change.
///
/// On the embedded path the guard holds the cloned database and drops it when
/// the test ends; on a named database there is nothing to drop, because the
/// database belongs to whoever started it. Field order matters: `backend` is
/// declared first so its pool closes before the guard drops the database, which
/// fails while any connection is still attached.
pub struct TestDatabase {
    backend: PgBackend,
    #[cfg(all(feature = "test-helpers", target_os = "linux"))]
    guard: Option<pg_embedded_setup_unpriv::TemporaryDatabase>,
}

impl TestDatabase {
    /// Wrap a backend whose cloned database this guard owns and will drop.
    #[cfg(all(feature = "test-helpers", target_os = "linux"))]
    fn owning(backend: PgBackend, database: pg_embedded_setup_unpriv::TemporaryDatabase) -> Self {
        Self {
            backend,
            guard: Some(database),
        }
    }

    /// Wrap a backend for a database this guard does not own.
    fn borrowed(backend: PgBackend) -> Self {
        Self {
            backend,
            #[cfg(all(feature = "test-helpers", target_os = "linux"))]
            guard: None,
        }
    }
}

impl std::ops::Deref for TestDatabase {
    type Target = PgBackend;

    fn deref(&self) -> &Self::Target {
        &self.backend
    }
}

#[cfg(all(feature = "test-helpers", target_os = "linux"))]
impl Drop for TestDatabase {
    /// Drop the cloned database from a thread that is allowed to block.
    ///
    /// The guard's own drop issues `DROP DATABASE` synchronously and builds a
    /// Tokio runtime to do it. Doing that inside a `#[tokio::test]` panics
    /// with "Cannot drop a runtime in a context where blocking is not
    /// allowed", so the guard moves onto a plain thread, which is joined so
    /// the database is gone before the process exits.
    fn drop(&mut self) {
        let Some(guard) = self.guard.take() else {
            return;
        };
        // A panic here would mask the test's own result, so a failure to drop
        // is reported and swallowed; the cluster is reaped at process exit.
        if let Err(error) = std::thread::spawn(move || drop(guard)).join() {
            eprintln!("failed to drop the test database: {error:?}");
        }
    }
}

/// Create a PostgreSQL-backed test database, failing if none can be had.
///
/// Uses the database `TEST_DATABASE_URL` names, or else an embedded cluster
/// where one is available (Linux, with `test-helpers`).
///
/// # Errors
///
/// Returns the connection, pool or bootstrap error, or an unavailability error
/// when no URL is named and no embedded cluster exists on this platform.
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
pub async fn test_pg_db() -> Result<TestDatabase, DatabaseError> {
    try_test_pg_db_with(PostgresRequirement::Required)
        .await?
        .ok_or_else(no_database_source)
}

/// Read the URL a lane names for its test database, if it names one.
fn configured_database_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

/// The error for a run with no named database and no embedded cluster.
///
/// Worded with "could not connect to server", one of the transport failures
/// [`UNAVAILABLE_PATTERNS`] lists, so the skip decision reads it as an absent
/// database and not as a misconfiguration.
fn no_database_source() -> DatabaseError {
    DatabaseError::Pool(
        "could not connect to server: TEST_DATABASE_URL is unset and the \
         embedded cluster needs Linux and the test-helpers feature"
            .to_string(),
    )
}

/// Connections each test's pool may open.
///
/// The production default is five, which suits a server handling concurrent
/// requests. A test owns its own database and drives it from one task, so it
/// needs one connection and a little slack for the pool's own bookkeeping.
///
/// The budget that matters is `TEST_POOL_SIZE` times the `pg-embed` nextest
/// group's `max-threads`, which must stay under the cluster's
/// `PG_MAX_CONNECTIONS` with room for the template connection and the
/// administrative connection that creates and drops each clone. At two per test
/// and sixteen threads that is thirty-two of sixty-four.
///
/// The external-database path (`TEST_DATABASE_URL`) takes the same size, so a
/// developer's or CI's server sees the same per-test connection budget.
pub(crate) const TEST_POOL_SIZE: usize = 2;

/// Build the test backend configuration for `url`.
fn test_pg_config(url: String) -> DatabaseConfig {
    DatabaseConfig {
        backend: DatabaseBackend::Postgres,
        url: SecretString::from(url),
        pool_size: TEST_POOL_SIZE,
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
pub async fn try_test_pg_db() -> Result<Option<TestDatabase>, DatabaseError> {
    try_test_pg_db_with(PostgresRequirement::from_env()).await
}

/// Choose the database source and apply `requirement` to its absence.
///
/// A named database is tried as before, skipping only where the requirement
/// allows. Without one, the embedded cluster is provisioned; a failure to
/// bootstrap it is an error, never a skip, because it means the harness is
/// broken rather than that nobody provided a database.
async fn try_test_pg_db_with(
    requirement: PostgresRequirement,
) -> Result<Option<TestDatabase>, DatabaseError> {
    if let Some(url) = configured_database_url() {
        let backend = try_pg_db_at(url, requirement).await?;
        return Ok(backend.map(TestDatabase::borrowed));
    }
    provision_without_url(requirement).await
}

/// Provision the embedded cluster's database for a run that names none.
#[cfg(all(feature = "test-helpers", target_os = "linux"))]
async fn provision_without_url(
    _requirement: PostgresRequirement,
) -> Result<Option<TestDatabase>, DatabaseError> {
    embedded::provision().await.map(Some)
}

/// Without an embedded cluster, a run that names no database has none.
#[cfg(not(all(feature = "test-helpers", target_os = "linux")))]
async fn provision_without_url(
    requirement: PostgresRequirement,
) -> Result<Option<TestDatabase>, DatabaseError> {
    let error = no_database_source();
    if skip_is_allowed(&error, requirement) {
        eprintln!("Skipping Postgres test (database unavailable): {error}");
        return Ok(None);
    }
    Err(error)
}

/// Connect at `url`, skipping only where `requirement` leaves a skip available.
///
/// Split from `try_test_pg_db` so the decision can be exercised against a real
/// unreachable endpoint without setting an environment variable, which would
/// have to be serialized against every other test in the process. What is left
/// in the caller is the composition of two readings that are each tested on
/// their own: `configured_database_url` and `PostgresRequirement::from_env`.
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

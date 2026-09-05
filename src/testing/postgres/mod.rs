//! Postgres-specific test helpers.

#[cfg(feature = "test-helpers")]
pub mod embedded;

use crate::config::{DatabaseBackend, DatabaseConfig, SslMode};
use crate::db::postgres::PgBackend;
use crate::error::DatabaseError;
use secrecy::SecretString;

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
/// Derefs to [`PgBackend`], so a test uses it exactly as it used the shared
/// backend and the existing call sites need no change.
///
/// The guard is what makes the isolation real. On the embedded path it holds
/// the cloned database and drops it when the test ends; on the external path
/// there is nothing to drop, because the database belongs to whoever started
/// it. Field order matters: `backend` is declared first so its pool closes its
/// connections before the guard tries to drop the database, which fails while
/// any connection is still attached.
pub struct TestDatabase {
    backend: PgBackend,
    #[cfg(feature = "test-helpers")]
    _guard: Option<pg_embedded_setup_unpriv::TemporaryDatabase>,
}

impl TestDatabase {
    /// Wraps a backend whose database this guard owns and will drop.
    #[cfg(feature = "test-helpers")]
    pub(crate) fn owning(
        backend: PgBackend,
        database: pg_embedded_setup_unpriv::TemporaryDatabase,
    ) -> Self {
        Self {
            backend,
            _guard: Some(database),
        }
    }

    /// Wraps a backend for a database this guard does not own.
    pub(crate) fn borrowed(backend: PgBackend) -> Self {
        Self {
            backend,
            #[cfg(feature = "test-helpers")]
            _guard: None,
        }
    }
}

impl std::ops::Deref for TestDatabase {
    type Target = PgBackend;

    fn deref(&self) -> &Self::Target {
        &self.backend
    }
}

#[cfg(feature = "test-helpers")]
impl Drop for TestDatabase {
    /// Drops the cloned database from a thread that is allowed to block.
    ///
    /// The guard's own drop issues `DROP DATABASE` synchronously, which spins
    /// up and tears down a Tokio runtime. Doing that inside a `#[tokio::test]`
    /// panics with "Cannot drop a runtime in a context where blocking is not
    /// allowed", so the guard is moved onto a plain thread and joined there.
    ///
    /// Joining rather than detaching is deliberate: the database must be gone
    /// before the test process exits, or clones accumulate in the cluster for
    /// the next run to trip over.
    fn drop(&mut self) {
        let Some(guard) = self._guard.take() else {
            return;
        };
        // A panic here would mask the test's own result, so a failure to drop
        // is reported and swallowed. The cluster is torn down at process exit
        // in any case.
        if let Err(error) = std::thread::spawn(move || drop(guard)).join() {
            eprintln!("failed to drop the test database: {error:?}");
        }
    }
}

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
pub async fn test_pg_db() -> Result<TestDatabase, DatabaseError> {
    #[cfg(feature = "test-helpers")]
    if std::env::var("TEST_DATABASE_URL").is_err() {
        return embedded::provision().await;
    }

    let url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://localhost/axinite_test".to_string());

    let config = DatabaseConfig {
        backend: DatabaseBackend::Postgres,
        url: SecretString::from(url),
        pool_size: 5,
        ssl_mode: SslMode::Prefer,
        libsql_path: None,
        libsql_url: None,
        libsql_auth_token: None,
    };

    Ok(TestDatabase::borrowed(PgBackend::new(&config).await?))
}

pub async fn try_test_pg_db() -> Result<Option<TestDatabase>, DatabaseError> {
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

    matches!(
        error,
        DatabaseError::Postgres(_)
            | DatabaseError::Pool(_)
            | DatabaseError::PoolBuild(_)
            | DatabaseError::PoolRuntime(_)
    ) && UNAVAILABLE_PATTERNS.iter().any(|p| lowered.contains(p))
}

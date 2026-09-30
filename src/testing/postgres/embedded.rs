//! Embedded PostgreSQL cluster for the Postgres-backed tests.
//!
//! The tests used to share one database supplied by a service container, and
//! isolated themselves by convention: fresh UUIDs and targeted `DELETE`
//! statements. That leaves every test able to see every other test's rows, and
//! one clean-up deletes by user id rather than by row id, which is safe only
//! while no two tests choose the same user.
//!
//! This module replaces that with a cluster owned by the test process: one
//! migrated template database, and a fresh clone per test that is dropped when
//! the test ends. Tests become independent, and the clean-up code that existed
//! only to keep them from colliding becomes unnecessary.
//!
//! `pg-embed-setup-unpriv` reads its configuration from the environment, and a
//! test process must not set its own once threads exist, so every value lives
//! in `.cargo/config.toml`'s `[env]`, which Cargo applies to every process it
//! runs: the PostgreSQL release, the extension hook's manifest and its digest,
//! the connection limit, and a per-checkout install root. Setting
//! `TEST_DATABASE_URL` bypasses everything here and uses the database it names.

use std::sync::OnceLock;

use pg_embedded_setup_unpriv::ClusterHandle;

use super::TestDatabase;

use crate::error::DatabaseError;

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
pub const TEST_POOL_SIZE: usize = 2;

/// Extension the schema requires.
///
/// `migrations/V1__initial.sql` runs `CREATE EXTENSION IF NOT EXISTS vector`
/// and declares a `VECTOR` column, so this is needed to apply the first
/// migration at all, not only by the tests that exercise the type. The
/// library's extension hook installs it from the digest-pinned
/// `df12-pg-extensions` manifest named in `.cargo/config.toml`.
pub const REQUIRED_EXTENSION: &str = "vector";

/// Prefix for the migrated template database.
pub(super) const TEMPLATE_PREFIX: &str = "axinite_template";

/// Attempts allowed when cloning the template.
///
/// Cloning fails if another connection is still attached to the template, which
/// happens when two tests start close together. The clone is cheap, so a short
/// retry is better than serializing every test behind one lock.
pub(super) const CLONE_ATTEMPTS: usize = 5;

/// Delay between clone attempts.
pub(super) const CLONE_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(200);

/// Caches the template name so migrations run once per process.
pub(super) static TEMPLATE: OnceLock<Result<String, String>> = OnceLock::new();

/// Runs a synchronous cluster operation where blocking is permitted.
///
/// Every `ClusterHandle` method that touches the server is synchronous and
/// builds a short-lived Tokio runtime internally. Dropping that runtime inside
/// a `#[tokio::test]` panics with "Cannot drop a runtime in a context where
/// blocking is not allowed", so each such call is moved onto a blocking worker
/// where it is allowed.
///
/// # Errors
/// Returns the operation's own error, or [`DatabaseError::Pool`] if the
/// blocking task itself panicked or was cancelled.
pub(super) async fn blocking<T, F>(operation: F) -> Result<T, DatabaseError>
where
    F: FnOnce() -> Result<T, DatabaseError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| DatabaseError::Pool(format!("cluster task: {error}")))?
}

/// Returns the shared cluster, bootstrapping it on first use.
///
/// The handle is process-wide and the library serializes the bootstrap
/// internally, so the first test through pays for it and the rest join. The
/// library registers an exit reaper for it, so each nextest process stops its
/// own cluster when it exits.
///
/// # Errors
/// Returns [`DatabaseError::Pool`] when the cluster cannot be started, with the
/// library's own message, or when it started without the extension the schema
/// needs, which means the hook's configuration did not reach this process.
pub async fn cluster() -> Result<&'static ClusterHandle, DatabaseError> {
    let cluster = blocking(|| {
        pg_embedded_setup_unpriv::test_support::shared_cluster_handle()
            .map_err(|error| DatabaseError::Pool(format!("embedded cluster: {error:?}")))
    })
    .await?;
    require_extension(cluster)?;
    Ok(cluster)
}

/// Fails unless the extension hook installed the extension the schema needs.
///
/// Without it the first migration fails with a message about a missing control
/// file, which says nothing about the cause: the hook reads `PG_EXTENSIONS`
/// and the manifest from the environment, and a process not started by Cargo
/// does not have `.cargo/config.toml`'s values.
fn require_extension(cluster: &ClusterHandle) -> Result<(), DatabaseError> {
    let installed = cluster
        .installed_extensions()
        .iter()
        .any(|extension| extension.name.as_str() == REQUIRED_EXTENSION);
    if installed {
        return Ok(());
    }
    Err(DatabaseError::Pool(format!(
        "the embedded cluster started without the `{REQUIRED_EXTENSION}` extension; \
         PG_EXTENSIONS and PG_EXTENSIONS_MANIFEST come from .cargo/config.toml, \
         so run the tests through Cargo or set them, or set TEST_DATABASE_URL"
    )))
}

/// Provisions a fresh database cloned from the migrated template.
///
/// Every writing test gets its own, which is what removes the need for the
/// targeted `DELETE` clean-ups and makes the clean-up that deletes by user id
/// safe: no two tests share a database, so no test can see another's rows.
///
/// Cloning retries because `CREATE DATABASE ... TEMPLATE` fails while any other
/// connection is attached to the template, which happens when two tests start
/// together. The clone itself is fast, so a short retry costs less than
/// serializing every test behind a lock.
///
/// # Errors
/// Returns [`DatabaseError::Pool`] when the cluster cannot be reached, the
/// template cannot be built, or every clone attempt fails.
pub async fn provision() -> Result<TestDatabase, DatabaseError> {
    let cluster = cluster().await?;
    let template = ensure_template(cluster).await?;

    let mut last: Option<String> = None;
    for _ in 0..CLONE_ATTEMPTS {
        let name = format!("axinite_test_{}", uuid::Uuid::new_v4().simple());
        let template_name = template.clone();
        let cloned = blocking(move || {
            cluster
                .temporary_database_from_template(name, template_name)
                .map_err(|error| DatabaseError::Pool(error.to_string()))
        })
        .await;
        match cloned {
            Ok(database) => {
                let config = test_database_config(database.url(), TEST_POOL_SIZE);
                let backend = crate::db::postgres::PgBackend::new(&config).await?;
                return Ok(TestDatabase::owning(backend, database));
            }
            Err(error) => last = Some(error.to_string()),
        }
        tokio::time::sleep(CLONE_RETRY_DELAY).await;
    }
    Err(DatabaseError::Pool(format!(
        "could not clone template {template} after {CLONE_ATTEMPTS} attempts: {}",
        last.unwrap_or_else(|| "no error recorded".to_string())
    )))
}

/// Builds the configuration a test's backend uses.
///
/// `pool_size` is the fixture's, not the production default: see
/// [`TEST_POOL_SIZE`] for why the number is what the connection budget allows.
pub(super) fn test_database_config(url: &str, pool_size: usize) -> crate::config::DatabaseConfig {
    crate::config::DatabaseConfig {
        backend: crate::config::DatabaseBackend::Postgres,
        url: secrecy::SecretString::from(url.to_string()),
        pool_size,
        ssl_mode: crate::config::SslMode::Prefer,
        libsql_path: None,
        libsql_url: None,
        libsql_auth_token: None,
    }
}

mod template;

use template::ensure_template;

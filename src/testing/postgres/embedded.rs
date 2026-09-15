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
//! The container is still supported. Setting `TEST_DATABASE_URL` bypasses
//! everything here and uses the external database, which is the escape hatch
//! for a developer who already has one running and the fallback if the embedded
//! path ever fails in CI.
//!
//! # The shared install root, and when this fails
//!
//! `pg-embed-setup-unpriv` 0.5.2 hard-codes where it installs PostgreSQL:
//!
//! ```text
//! let base = Utf8PathBuf::from(format!("/var/tmp/pg-embed-{}", uid.as_raw()));
//! ```
//!
//! `privileges.rs:235`, with no environment override and no setting. Every
//! project on a machine therefore shares one root per user. The shared cluster
//! also mints a fresh superuser password on each bootstrap while reusing the
//! existing data directory, so the credentials it hands back do not match what
//! the directory was initialized with.
//!
//! Together those mean the bootstrap is reliable on a clean machine and not on
//! one that already has state. CI starts clean, so this path is sound there.
//! A developer running a second project that uses the library, or returning to
//! a machine where an earlier run left a directory behind, may see either
//! `postgresql_embedded::setup() failed` or
//! `password authentication failed for user "postgres"`.
//!
//! Two ways through, in order of preference:
//!
//! 1. Set `TEST_DATABASE_URL` to a PostgreSQL with pgvector installed. That
//!    bypasses everything here and is the supported developer path today.
//! 2. Remove `/var/tmp/pg-embed-<uid>` and let the next run rebuild it. Check
//!    first that no other project is mid-run against it, because the root is
//!    shared.
//!
//! The real fix is upstream: an install-root override so each test binary can
//! have its own, and a password that survives a bootstrap. Both are in the
//! packet for pg-embed-setup-unpriv v0.6.0. When that ships, a follow-up sets
//! the override per test binary and this caveat goes.

use std::sync::OnceLock;

use pg_embedded_setup_unpriv::ClusterHandle;

use super::TestDatabase;

use crate::error::DatabaseError;

/// The PostgreSQL range the cluster runs, as set in `.cargo/config.toml`.
///
/// Recorded here so a contract can hold the two in step. The cap is 16 because
/// the prebuilt pgvector archive publishes PostgreSQL 16 assets only; the minor
/// is open because PostgreSQL's module magic block encodes the major version
/// and not the minor, so a module built for one 16.x loads into another.
pub const POSTGRES_VERSION_REQ: &str = "^16";

/// Connections the embedded cluster accepts at once.
///
/// This is the cluster's own default, recorded here because the fixture has to
/// fit inside it. pg-embed-setup-unpriv 0.5.2 exposes no way to set
/// `max_connections` at bootstrap: there is no setting on the handle and no
/// environment variable, and `ALTER SYSTEM` would need a restart the shared
/// handle does not offer. So the budget is met from the other side, by bounding
/// what the tests ask for.
///
/// The container's default is 100, which is why the shared-database design
/// never hit this. Measured on the embedded cluster before the fixture existed:
/// two failures in eight runs, a different test each time, all
/// `sorry, too many clients already`.
pub const CLUSTER_MAX_CONNECTIONS: u32 = 20;

/// Connections each test's pool may open.
///
/// The production default is five, which suits a server handling concurrent
/// requests. A test owns its own database and drives it from one task, so it
/// needs one connection and a little slack for the pool's own bookkeeping.
///
/// The budget that matters is `TEST_POOL_SIZE * <concurrent tests>` staying
/// under [`CLUSTER_MAX_CONNECTIONS`], with room for the template and
/// administrative connections. At two per test that allows eight concurrent
/// tests against a limit of twenty, which is why the `pg-embed` nextest group
/// caps the Postgres tests at eight threads.
/// `tests/workflow_contracts/nextest_postgres_test.py` asserts the two numbers
/// against each other, so raising one without the other fails rather than
/// producing intermittent connection errors.
pub const TEST_POOL_SIZE: usize = 2;

/// Concurrent Postgres tests the nextest group admits.
///
/// Derived: `CLUSTER_MAX_CONNECTIONS / TEST_POOL_SIZE` leaves ten, and half of
/// that is taken as headroom for the template connection, the administrative
/// connection that creates and drops each clone, and any pool that has not yet
/// released a connection when the next test starts.
pub const MAX_CONCURRENT_TESTS: u32 = 8;

/// Extension the schema requires.
///
/// `migrations/V1__initial.sql` runs `CREATE EXTENSION IF NOT EXISTS vector`
/// and declares a `VECTOR` column, so this is needed to apply the first
/// migration at all, not only by the tests that exercise the type.
pub const REQUIRED_EXTENSION: &str = "vector";

/// Guidance shown when the extension cannot be fetched.
///
/// The extension archive is served through the GitHub API, which rate-limits
/// unauthenticated callers to sixty requests an hour per address. A shared
/// runner or office address reaches that sooner than it sounds, and the failure
/// surfaces as a bare `403 Forbidden` naming an API URL, which says nothing
/// about what to do. This does.
pub const TOKEN_GUIDANCE: &str = concat!(
    "Could not fetch the pgvector extension archive. This is usually the ",
    "GitHub API rate limit: unauthenticated callers get sixty requests an ",
    "hour per address, and the archive is served through it. Set GITHUB_TOKEN ",
    "to any token with public read access and retry. Alternatively set ",
    "TEST_DATABASE_URL to an external PostgreSQL with pgvector installed, ",
    "which bypasses the embedded cluster entirely.",
);

/// Name of the nextest group that serializes cluster bootstraps.
pub const NEXTEST_GROUP: &str = "pg-embed";

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
pub(super) async fn blocking<T, F>(operation: F) -> Result<T, DatabaseError>
where
    F: FnOnce() -> Result<T, DatabaseError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| DatabaseError::Pool(format!("cluster task: {error}")))?
}

/// Reports whether the embedded cluster can be used at all.
///
/// The library drives PostgreSQL through a helper binary, so without it there
/// is no cluster to bootstrap. Deciding that here, rather than letting the
/// bootstrap fail, keeps a workflow that installs no worker behaving exactly as
/// it did before this fixture existed: `test.yml` has neither a worker nor a
/// database, and its Postgres tests skip on a refused connection.
///
/// The alternative was to treat a failed bootstrap as "no database" and skip.
/// That was rejected. `is_database_unavailable` deliberately excludes
/// configuration and authentication errors so a misconfigured job fails rather
/// than quietly reporting coverage for tests that never ran, which is the exact
/// failure issue #350 was about. A missing worker is a checkable condition, not
/// an error being swallowed.
///
/// Phase 2 installs the worker in the coverage lane and adds a contract that it
/// is there, so the lane that is meant to run these tests cannot silently take
/// this branch.
fn worker_available() -> bool {
    if let Ok(path) = std::env::var("PG_EMBEDDED_WORKER") {
        return std::path::Path::new(&path).is_file();
    }
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join("pg_worker").is_file()))
        .unwrap_or(false)
}

/// Returns the shared cluster, bootstrapping it on first use.
///
/// The handle is process-wide and the library serializes the bootstrap
/// internally, so the first test through pays for it and the rest join.
///
/// # Errors
/// Returns [`DatabaseError::Pool`] when the cluster cannot be started, with the
/// library's own message; the usual causes are a missing worker binary and a
/// binary cache that cannot be written.
pub async fn cluster() -> Result<&'static ClusterHandle, DatabaseError> {
    blocking(|| {
        pg_embedded_setup_unpriv::test_support::shared_cluster_handle()
            .map_err(|error| DatabaseError::Pool(format!("embedded cluster: {error}")))
    })
    .await
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
                return Ok(super::TestDatabase::owning(backend, database));
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

/// Reports whether this fixture should be used for the current run.
///
/// See [`worker_available`] for why a missing worker means "not usable" rather
/// than an error.
#[must_use]
pub fn is_usable() -> bool {
    worker_available()
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

mod extension;
mod template;

use template::ensure_template;

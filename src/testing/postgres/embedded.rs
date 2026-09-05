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

/// Name of the nextest group that serialises cluster bootstraps.
pub const NEXTEST_GROUP: &str = "pg-embed";

/// Prefix for the migrated template database.
const TEMPLATE_PREFIX: &str = "axinite_template";

/// Attempts allowed when cloning the template.
///
/// Cloning fails if another connection is still attached to the template, which
/// happens when two tests start close together. The clone is cheap, so a short
/// retry is better than serialising every test behind one lock.
const CLONE_ATTEMPTS: usize = 5;

/// Delay between clone attempts.
const CLONE_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(200);

/// Caches the template name so migrations run once per process.
static TEMPLATE: OnceLock<Result<String, String>> = OnceLock::new();

/// Runs a synchronous cluster operation where blocking is permitted.
///
/// Every `ClusterHandle` method that touches the server is synchronous and
/// builds a short-lived Tokio runtime internally. Dropping that runtime inside
/// a `#[tokio::test]` panics with "Cannot drop a runtime in a context where
/// blocking is not allowed", so each such call is moved onto a blocking worker
/// where it is allowed.
async fn blocking<T, F>(operation: F) -> Result<T, DatabaseError>
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
/// The handle is process-wide and the library serialises the bootstrap
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

/// Names the template after the migrations it contains.
///
/// Hashing `migrations/` means a changed migration produces a different
/// template rather than reusing a stale one, which is the failure this naming
/// exists to prevent: a developer edits a migration, the old template survives,
/// and the tests pass against a schema that no longer exists.
fn template_name() -> Result<String, DatabaseError> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let hash = pg_embedded_setup_unpriv::test_support::hash_directory(&dir)
        .map_err(|error| DatabaseError::Pool(format!("hash migrations: {error}")))?;
    let short = hash.get(..12).unwrap_or(hash.as_str());
    Ok(format!("{TEMPLATE_PREFIX}_{short}"))
}

/// Points `postgresql_extensions` at the cluster's own installation.
///
/// The extension crate derives the library and share directories by running
/// `pg_config` from the binary directory, so this only has to say where the
/// binaries are. The connection fields are unused for an install but the trait
/// requires them.
struct ExtensionTarget {
    binary_dir: std::path::PathBuf,
}

impl postgresql_commands::Settings for ExtensionTarget {
    fn get_binary_dir(&self) -> std::path::PathBuf {
        self.binary_dir.clone()
    }
    fn get_host(&self) -> std::ffi::OsString {
        "localhost".into()
    }
    fn get_port(&self) -> u16 {
        0
    }
    fn get_username(&self) -> std::ffi::OsString {
        "postgres".into()
    }
    fn get_password(&self) -> std::ffi::OsString {
        String::new().into()
    }
}

/// Installs pgvector into the cluster's tree if it is not already there.
///
/// Theseus builds PostgreSQL with its in-tree contrib modules, and pgvector is
/// out of tree, so the server arrives without it. The archive is prebuilt, so
/// nothing is compiled here and the estate's no-source-builds rule holds.
///
/// Idempotent: the extension crate records what it installed and skips a second
/// install, so every test can call this and only the first does work.
///
/// # Errors
/// Returns [`DatabaseError::Pool`] carrying [`TOKEN_GUIDANCE`] when the archive
/// cannot be fetched, because the overwhelmingly likely cause is the GitHub API
/// rate limit and the underlying error says only `403 Forbidden`.
async fn install_extension(cluster: &ClusterHandle) -> Result<(), DatabaseError> {
    let target = ExtensionTarget {
        binary_dir: cluster.settings().binary_dir(),
    };
    let installed = postgresql_extensions::get_installed_extensions(&target)
        .await
        .unwrap_or_default();
    if installed
        .iter()
        .any(|extension| extension.name() == "pgvector_compiled")
    {
        return Ok(());
    }
    postgresql_extensions::install(
        &target,
        "portal-corp",
        "pgvector_compiled",
        &semver::VersionReq::STAR,
    )
    .await
    .map_err(|error| DatabaseError::Pool(format!("{TOKEN_GUIDANCE}\n\nUnderlying error: {error}")))
}

/// Creates the migrated template if it is absent, and returns its name.
///
/// The template is built once per hash and reused by every test in every
/// process, so the seventeen migrations run once rather than once per test.
/// `ensure_template_exists` on the handle takes a synchronous closure, which
/// cannot drive refinery's async runner from inside a test's runtime, so the
/// steps are done here instead.
///
/// Two processes can reach this together. The loser of the create race sees the
/// database already exists, waits for the winner to finish migrating, and then
/// proceeds; that is what the readiness poll below is for. Without it the loser
/// would clone a template whose migrations were half applied.
async fn ensure_template(cluster: &'static ClusterHandle) -> Result<String, DatabaseError> {
    let name = template_name()?;
    if let Some(cached) = TEMPLATE.get() {
        return cached
            .clone()
            .map_err(|error| DatabaseError::Pool(format!("template: {error}")));
    }

    let outcome = build_template(cluster, &name).await;
    let stored = outcome
        .as_ref()
        .map(|()| name.clone())
        .map_err(ToString::to_string);
    let _ = TEMPLATE.set(stored);
    outcome.map(|()| name)
}

/// Creates and migrates the template database.
async fn build_template(cluster: &'static ClusterHandle, name: &str) -> Result<(), DatabaseError> {
    install_extension(cluster).await?;

    let owned = name.to_string();
    let existed = blocking(move || {
        cluster
            .database_exists(owned.as_str())
            .map_err(|error| DatabaseError::Pool(format!("template lookup: {error}")))
    })
    .await?;
    if !existed {
        let owned = name.to_string();
        let created = blocking(move || Ok(cluster.create_database(owned.as_str()).is_ok())).await?;
        if !created {
            // Another process won the race. Its migrations may still be
            // running, so fall through to the readiness poll rather than
            // cloning a half-built template.
            return wait_for_template(cluster, name).await;
        }
        let url = cluster.connection().database_url(name);
        migrate(&url).await?;
        return Ok(());
    }
    wait_for_template(cluster, name).await
}

/// Waits until the template carries the table the last migration creates.
///
/// Presence of the database says only that some process has started; presence
/// of a migrated table says the schema is there to clone.
async fn wait_for_template(
    cluster: &'static ClusterHandle,
    name: &str,
) -> Result<(), DatabaseError> {
    let url = cluster.connection().database_url(name);
    for _ in 0..CLONE_ATTEMPTS {
        if template_is_migrated(&url).await? {
            return Ok(());
        }
        tokio::time::sleep(CLONE_RETRY_DELAY).await;
    }
    Err(DatabaseError::Pool(format!(
        "template {name} did not finish migrating; another process may have \
         failed part-way through. Drop it and retry."
    )))
}

/// Reports whether the refinery history table names every migration.
async fn template_is_migrated(url: &str) -> Result<bool, DatabaseError> {
    let config = test_database_config(url, TEST_POOL_SIZE);
    let Ok(backend) = crate::db::postgres::PgBackend::new(&config).await else {
        return Ok(false);
    };
    // `PgBackend::store` is private outside its own module tree, so the store
    // is rebuilt from the pool the backend exposes.
    let store = crate::history::Store::from_pool(backend.pool());
    let conn = store.conn().await?;
    let row = conn
        .query_one(
            "SELECT count(*) FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_name = 'refinery_schema_history'",
            &[],
        )
        .await
        .map_err(|error| DatabaseError::Pool(error.to_string()))?;
    let present: i64 = row.get(0);
    Ok(present > 0)
}

/// Applies the embedded migrations to `url`.
async fn migrate(url: &str) -> Result<(), DatabaseError> {
    let config = test_database_config(url, TEST_POOL_SIZE);
    let backend = crate::db::postgres::PgBackend::new(&config).await?;
    crate::history::Store::from_pool(backend.pool())
        .run_migrations()
        .await
}

/// Builds the configuration a test's backend uses.
///
/// `pool_size` is the fixture's, not the production default: see
/// [`TEST_POOL_SIZE`] for why the number is what the connection budget allows.
pub(crate) fn test_database_config(url: &str, pool_size: usize) -> crate::config::DatabaseConfig {
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

/// Provisions a fresh database cloned from the migrated template.
///
/// Every writing test gets its own, which is what removes the need for the
/// targeted `DELETE` clean-ups and makes the clean-up that deletes by user id
/// safe: no two tests share a database, so no test can see another's rows.
///
/// Cloning retries because `CREATE DATABASE ... TEMPLATE` fails while any other
/// connection is attached to the template, which happens when two tests start
/// together. The clone itself is fast, so a short retry costs less than
/// serialising every test behind a lock.
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

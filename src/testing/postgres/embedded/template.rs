//! The migrated template database every test's clone comes from.
//!
//! Built once per hash of `migrations/`, so the seventeen migrations run once
//! rather than once per test. Naming the template after the migrations means a
//! changed migration produces a different template rather than reusing a stale
//! one, which is the failure this naming exists to prevent.

use pg_embedded_setup_unpriv::ClusterHandle;

use super::extension::install_extension;
use super::{
    CLONE_ATTEMPTS, CLONE_RETRY_DELAY, TEMPLATE, TEMPLATE_PREFIX, TEST_POOL_SIZE, blocking,
    test_database_config,
};
use crate::error::DatabaseError;

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
pub(super) async fn ensure_template(
    cluster: &'static ClusterHandle,
) -> Result<String, DatabaseError> {
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

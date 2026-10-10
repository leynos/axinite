//! The migrated template database every test's clone comes from.
//!
//! Built once per hash of `migrations/`, so the seventeen migrations run once
//! rather than once per test. Naming the template after the migrations means a
//! changed migration produces a different template rather than reusing a stale
//! one, which is the failure this naming exists to prevent.

use pg_embedded_setup_unpriv::ClusterHandle;

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
    let owned = name.to_string();
    let existed = blocking(move || {
        cluster
            .database_exists(owned.as_str())
            .map_err(|error| DatabaseError::Pool(format!("template lookup: {error:?}")))
    })
    .await?;
    if !existed {
        let owned = name.to_string();
        let created = blocking(move || create_template(cluster, owned.as_str())).await?;
        if !created {
            // Another process won the race. Its migrations may still be
            // running, so fall through to the readiness poll rather than
            // cloning a half-built template.
            return wait_for_template(cluster, name).await;
        }
        let url = cluster.connection().database_url(name);
        migrate(&url).await?;
        mark_ready(&url).await?;
        return Ok(());
    }
    wait_for_template(cluster, name).await
}

/// Creates the template database, reporting whether this call created it.
///
/// `create_database` has no dedicated "already exists" error, so a failure is
/// told apart by asking the cluster: if the database is there now, another
/// process created it first and this call lost the race, which is not an
/// error. Any other failure (an unreachable cluster, a refused command) is
/// returned, so it is not misreported as a race.
fn create_template(cluster: &ClusterHandle, name: &str) -> Result<bool, DatabaseError> {
    let Err(create_error) = cluster.create_database(name) else {
        return Ok(true);
    };
    let exists = cluster
        .database_exists(name)
        .map_err(|error| DatabaseError::Pool(format!("template lookup: {error:?}")))?;
    if exists {
        return Ok(false);
    }
    Err(DatabaseError::Pool(format!(
        "template create: {create_error:?}"
    )))
}

/// Waits until the template carries the marker the builder writes once every
/// migration has applied.
///
/// Presence of the database says only that some process has started, and the
/// refinery history table appears before the first migration runs, so neither
/// says the schema is there to clone. The marker is written after the last
/// migration succeeds, which is what a clone needs to wait for.
async fn wait_for_template(
    cluster: &'static ClusterHandle,
    name: &str,
) -> Result<(), DatabaseError> {
    let url = cluster.connection().database_url(name);
    for _ in 0..CLONE_ATTEMPTS {
        if template_is_ready(&url).await? {
            return Ok(());
        }
        tokio::time::sleep(CLONE_RETRY_DELAY).await;
    }
    Err(DatabaseError::Pool(format!(
        "template {name} did not finish migrating; another process may have \
         failed part-way through. Drop it and retry."
    )))
}

/// The comment the builder puts on the template database once it is migrated.
const READY_MARKER: &str = "axinite-template-ready";

/// Reports whether the template carries the marker its builder writes last.
async fn template_is_ready(url: &str) -> Result<bool, DatabaseError> {
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
            "SELECT shobj_description(oid, 'pg_database') FROM pg_database \
             WHERE datname = current_database()",
            &[],
        )
        .await
        .map_err(|error| DatabaseError::Pool(error.to_string()))?;
    let comment: Option<String> = row.get(0);
    Ok(comment.as_deref() == Some(READY_MARKER))
}

/// Writes the readiness marker on the database `url` names.
///
/// Called after the last migration succeeds, so a template that carries it has
/// every migration applied, and one that fails part-way never does.
async fn mark_ready(url: &str) -> Result<(), DatabaseError> {
    let config = test_database_config(url, TEST_POOL_SIZE);
    let backend = crate::db::postgres::PgBackend::new(&config).await?;
    let store = crate::history::Store::from_pool(backend.pool());
    let conn = store.conn().await?;
    conn.batch_execute(&format!(
        "DO $$ BEGIN EXECUTE format('COMMENT ON DATABASE %I IS %L', \
         current_database(), '{READY_MARKER}'); END $$"
    ))
    .await
    .map_err(|error| DatabaseError::Pool(error.to_string()))
}

/// Applies the embedded migrations to `url`.
async fn migrate(url: &str) -> Result<(), DatabaseError> {
    let config = test_database_config(url, TEST_POOL_SIZE);
    let backend = crate::db::postgres::PgBackend::new(&config).await?;
    crate::history::Store::from_pool(backend.pool())
        .run_migrations()
        .await
}

#[cfg(test)]
mod tests {
    //! The template's readiness marker and its race handling, against a real
    //! embedded cluster, because both are statements about what PostgreSQL
    //! reports rather than about this module's own state.

    use super::{ClusterHandle, create_template, mark_ready, template_is_ready};
    use crate::testing::postgres::embedded::cluster;

    /// Create a uniquely named scratch database and return its name and URL.
    async fn scratch(cluster: &'static ClusterHandle, tag: &str) -> (String, String) {
        let name = format!("axinite_template_probe_{tag}_{}", std::process::id());
        cluster
            .create_database(name.as_str())
            .expect("create the scratch database");
        let url = cluster.connection().database_url(&name);
        (name, url)
    }

    /// A database whose refinery history table exists is not ready.
    ///
    /// That table appears before the first migration runs, so a template
    /// whose migrations failed part-way carries it. Treating it as ready
    /// would clone an incomplete schema into every test.
    #[tokio::test]
    async fn a_history_table_without_the_marker_is_not_ready() {
        let cluster = cluster().await.expect("embedded cluster");
        let (name, url) = scratch(cluster, "history").await;
        let config = crate::testing::postgres::embedded::test_database_config(&url, 1);
        let backend = crate::db::postgres::PgBackend::new(&config)
            .await
            .expect("connect to the scratch database");
        crate::history::Store::from_pool(backend.pool())
            .conn()
            .await
            .expect("connection")
            .batch_execute("CREATE TABLE refinery_schema_history (version int)")
            .await
            .expect("create the history table");

        assert!(!template_is_ready(&url).await.expect("readiness query"));
        drop(backend);
        cluster.drop_database(name.as_str()).expect("drop");
    }

    /// Writing the marker is what makes a database ready.
    #[tokio::test]
    async fn marking_a_database_makes_it_ready() {
        let cluster = cluster().await.expect("embedded cluster");
        let (name, url) = scratch(cluster, "marked").await;

        mark_ready(&url).await.expect("write the marker");

        assert!(template_is_ready(&url).await.expect("readiness query"));
        cluster.drop_database(name.as_str()).expect("drop");
    }

    /// Losing the create race is `Ok(false)`; any other failure is an error.
    ///
    /// An empty name cannot exist and cannot be created, so it is a failure
    /// that is not a race, and it must not be reported as one.
    #[tokio::test]
    async fn only_a_lost_race_is_reported_as_one() {
        let cluster = cluster().await.expect("embedded cluster");
        let (name, _url) = scratch(cluster, "race").await;

        assert!(!create_template(cluster, &name).expect("a lost race is not an error"));
        assert!(create_template(cluster, "").is_err());
        cluster.drop_database(name.as_str()).expect("drop");
    }
}

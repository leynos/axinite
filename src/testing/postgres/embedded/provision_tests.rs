//! What `provision` hands a test: its own database, gone when the test ends.
//!
//! Both are statements about the cluster, not about this module's state, so
//! they run against a real embedded cluster.

use super::{blocking, cluster, provision};
use crate::{error::DatabaseError, history::Store};

/// Read the name of the database a test's pool is attached to.
async fn database_name(store: &Store) -> Result<String, DatabaseError> {
    let conn = store.conn().await?;
    let row = conn
        .query_one("SELECT current_database()", &[])
        .await
        .map_err(|error| DatabaseError::Pool(error.to_string()))?;
    Ok(row.get(0))
}

/// Report whether the cluster still has a database called `name`.
async fn exists(name: String) -> Result<bool, DatabaseError> {
    let cluster = cluster().await?;
    blocking(move || {
        cluster
            .database_exists(name.as_str())
            .map_err(|error| DatabaseError::Pool(format!("lookup: {error:?}")))
    })
    .await
}

/// Two provisions get distinct databases that cannot see each other's writes.
#[tokio::test]
async fn each_provision_gets_a_database_of_its_own() {
    let first = provision().await.expect("first database");
    let second = provision().await.expect("second database");
    let first_store = Store::from_pool(first.pool());
    let second_store = Store::from_pool(second.pool());

    let first_name = database_name(&first_store).await.expect("first name");
    let second_name = database_name(&second_store).await.expect("second name");
    assert_ne!(first_name, second_name, "clones must not share a database");

    first_store
        .conn()
        .await
        .expect("connection")
        .batch_execute(
            "CREATE TABLE isolation_probe (id int); INSERT INTO isolation_probe VALUES (1)",
        )
        .await
        .expect("write to the first database");
    let seen = second_store
        .conn()
        .await
        .expect("connection")
        .query(
            "SELECT 1 FROM pg_tables WHERE tablename = 'isolation_probe'",
            &[],
        )
        .await
        .expect("look for the table in the second database");
    assert!(
        seen.is_empty(),
        "the second database must not see the first's table"
    );
}

/// Dropping a `TestDatabase` that has pooled connections removes its clone.
///
/// The guard's `DROP DATABASE` fails while a connection is attached and only
/// logs the failure, so a clone left behind would pass every other test. The
/// connection is opened and returned to the pool first, which is the state a
/// finished test leaves.
#[tokio::test]
async fn dropping_a_database_with_pooled_connections_removes_it() {
    let database = provision().await.expect("database");
    let store = Store::from_pool(database.pool());
    let name = database_name(&store).await.expect("name");
    assert!(exists(name.clone()).await.expect("lookup"), "clone exists");

    drop(store);
    drop(database);

    assert!(
        !exists(name).await.expect("lookup"),
        "the clone must be gone once its TestDatabase is dropped"
    );
}

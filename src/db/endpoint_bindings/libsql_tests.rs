//! libSQL contract, migration, and persistence tests for endpoint bindings.

use std::sync::Arc;

use super::LibSqlEndpointBindingRepository;
use crate::db::NativeDatabase;
use crate::db::libsql::LibSqlBackend;
use crate::extensions::setup::{
    BindingRecord, BindingScope, EndpointBindingRepository, EndpointKind, EndpointName,
    binding_contract::run_binding_contract,
};

async fn migrated_backend() -> LibSqlBackend {
    let backend = LibSqlBackend::new_memory()
        .await
        .expect("file-backed libSQL database");
    NativeDatabase::run_migrations(&backend)
        .await
        .expect("migrations");
    backend
}

async fn table_exists(conn: &libsql::Connection) -> bool {
    let mut rows = conn
        .query(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'endpoint_bindings'",
            (),
        )
        .await
        .expect("schema query");
    rows.next().await.expect("schema row").is_some()
}

async fn migration_count(conn: &libsql::Connection) -> i64 {
    let mut rows = conn
        .query("SELECT COUNT(*) FROM _migrations WHERE version = 19", ())
        .await
        .expect("migration count");
    rows.next()
        .await
        .expect("migration row")
        .expect("one count row")
        .get::<i64>(0)
        .expect("count value")
}

fn record(scope: &BindingScope) -> BindingRecord {
    BindingRecord {
        scope: scope.clone(),
        endpoint_name: EndpointName::parse("calendar").expect("valid name"),
        endpoint_kind: EndpointKind::parse("jmap").expect("valid kind"),
        envelope_version: 1,
        validation_policy_version: 1,
        encrypted_value: vec![0xAB; 64],
        key_salt: vec![0xCD; 32],
    }
}

#[tokio::test]
async fn libsql_repository_satisfies_binding_contract() {
    let backend = migrated_backend().await;
    let shared = backend.shared_db();
    run_binding_contract(move || {
        Arc::new(LibSqlEndpointBindingRepository::new(Arc::clone(&shared)))
            as Arc<dyn EndpointBindingRepository>
    })
    .await;
}

#[tokio::test]
async fn fresh_database_has_the_endpoint_binding_table() {
    let backend = migrated_backend().await;
    let conn = backend.connect().await.expect("connection");

    assert!(table_exists(&conn).await);
}

#[tokio::test]
async fn incremental_migration_upgrades_an_existing_database_once() {
    let backend = migrated_backend().await;
    let conn = backend.connect().await.expect("connection");

    // Simulate a database created before V19 existed.
    conn.execute("DROP TABLE endpoint_bindings", ())
        .await
        .expect("drop table");
    conn.execute("DELETE FROM _migrations WHERE version = 19", ())
        .await
        .expect("forget migration");

    crate::db::libsql_migrations::run_incremental(&conn)
        .await
        .expect("incremental upgrade");

    assert!(table_exists(&conn).await);
    assert_eq!(migration_count(&conn).await, 1);

    // A second run must not reapply or duplicate the migration record.
    crate::db::libsql_migrations::run_incremental(&conn)
        .await
        .expect("repeat incremental");
    assert_eq!(migration_count(&conn).await, 1);
}

#[tokio::test]
async fn bindings_survive_reopening_a_file_database() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("bindings.db");
    let scope = BindingScope::new("user", "ext");
    let stored = record(&scope);

    {
        let backend = LibSqlBackend::new_local(&path)
            .await
            .expect("create database");
        NativeDatabase::run_migrations(&backend)
            .await
            .expect("migrations");
        let repository = LibSqlEndpointBindingRepository::new(backend.shared_db());
        repository.upsert(&stored).await.expect("upsert");
    }

    let reopened = LibSqlBackend::new_local(&path)
        .await
        .expect("reopen database");
    NativeDatabase::run_migrations(&reopened)
        .await
        .expect("repeat migrations");
    let repository = LibSqlEndpointBindingRepository::new(reopened.shared_db());
    let rows = repository.list(&scope, 16).await.expect("list");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].encrypted_value, stored.encrypted_value);
    assert_eq!(rows[0].key_salt, stored.key_salt);
    assert_eq!(rows[0].validation_policy_version, 1);
}

//! Integration tests covering `LibSqlBackend` and `Database` handling of
//! historical WASM WIT defaults under the `libsql` feature.

#![cfg(feature = "libsql")]

use ironclaw::db::{Database, libsql::LibSqlBackend};

const LEGACY_WASM_WIT_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS _migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

INSERT OR IGNORE INTO _migrations (version, name)
VALUES (9, 'flexible_embedding_dimension');

CREATE TABLE IF NOT EXISTS wasm_tools (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL DEFAULT '1.0.0',
    wit_version TEXT NOT NULL DEFAULT '0.1.0',
    description TEXT NOT NULL,
    wasm_binary BLOB NOT NULL,
    binary_hash BLOB NOT NULL,
    parameters_schema TEXT NOT NULL,
    source_url TEXT,
    trust_level TEXT NOT NULL DEFAULT 'user',
    status TEXT NOT NULL DEFAULT 'active',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (user_id, name, version)
);

CREATE TABLE IF NOT EXISTS wasm_channels (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL DEFAULT '0.1.0',
    wit_version TEXT NOT NULL DEFAULT '0.1.0',
    description TEXT NOT NULL DEFAULT '',
    wasm_binary BLOB NOT NULL,
    binary_hash BLOB NOT NULL,
    capabilities_json TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'active',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (user_id, name)
);
"#;

async fn insert_test_wasm_tool(conn: &libsql::Connection) {
    conn.execute(
        r#"
        INSERT INTO wasm_tools (
            id, user_id, name, description, wasm_binary, binary_hash, parameters_schema
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        "#,
        libsql::params![
            "tool-1",
            "user-1",
            "tool-one",
            "Tool one",
            libsql::Value::Blob(vec![0, 1, 2]),
            libsql::Value::Blob(vec![3, 4, 5]),
            "{}",
        ],
    )
    .await
    .expect("insert wasm tool without wit_version");
}

async fn insert_test_wasm_channel(conn: &libsql::Connection) {
    conn.execute(
        r#"
        INSERT INTO wasm_channels (
            id, user_id, name, description, wasm_binary, binary_hash
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        "#,
        libsql::params![
            "channel-1",
            "user-1",
            "channel-one",
            "Channel one",
            libsql::Value::Blob(vec![6, 7, 8]),
            libsql::Value::Blob(vec![9, 10, 11]),
        ],
    )
    .await
    .expect("insert wasm channel without wit_version");
}

async fn assert_wit_version(conn: &libsql::Connection, table: &str, id: &str, expected: &str) {
    let query = format!("SELECT wit_version FROM {table} WHERE id = ?1");
    let mut rows = conn
        .query(&query, libsql::params![id])
        .await
        .unwrap_or_else(|error| panic!("query {table} wit_version: {error}"));
    let row = rows
        .next()
        .await
        .unwrap_or_else(|error| panic!("read {table} wit_version row: {error}"))
        .unwrap_or_else(|| panic!("missing {table} row for id {id}"));
    let actual: String = row
        .get(0)
        .unwrap_or_else(|error| panic!("read {table} wit_version value: {error}"));
    assert_eq!(actual, expected);
}

async fn assert_migration_absent(conn: &libsql::Connection, version: i64) {
    let mut old_version_rows = conn
        .query(
            "SELECT name FROM _migrations WHERE version = ?1",
            libsql::params![version],
        )
        .await
        .unwrap_or_else(|error| panic!("query migration marker for version {version}: {error}"));
    assert!(
        old_version_rows
            .next()
            .await
            .unwrap_or_else(|error| panic!("read migration row for version {version}: {error}"))
            .is_none(),
        "unexpected _migrations row for version {version}; migration numbering regressed"
    );
}

async fn assert_migration_entries(conn: &libsql::Connection, entries: &[(i64, &str)]) {
    for (version, expected_name) in entries {
        let mut migration_rows = conn
            .query(
                "SELECT name FROM _migrations WHERE version = ?1",
                libsql::params![*version],
            )
            .await
            .unwrap_or_else(|error| {
                panic!("query migration marker for version {version}: {error}")
            });
        let migration_row = migration_rows
            .next()
            .await
            .unwrap_or_else(|error| panic!("read migration row for version {version}: {error}"))
            .unwrap_or_else(|| {
                panic!(
                    "expected _migrations row for version {version}; migration sequences diverged"
                )
            });
        let migration_name: String = migration_row
            .get(0)
            .unwrap_or_else(|error| panic!("read migration name for version {version}: {error}"));
        assert_eq!(migration_name, *expected_name);
    }
}

#[tokio::test]
async fn libsql_run_migrations_upgrades_legacy_wasm_wit_defaults() {
    let dir = tempfile::tempdir().expect("temp dir");
    let db_path = dir.path().join("legacy-wit-defaults.db");
    let backend = LibSqlBackend::new_local(&db_path).await.expect("backend");

    let conn = backend.connect().await.expect("connect");
    conn.execute_batch(LEGACY_WASM_WIT_SCHEMA)
        .await
        .expect("seed legacy schema");

    backend.run_migrations().await.expect("run migrations");

    let conn = backend.connect().await.expect("connect after migration");
    insert_test_wasm_tool(&conn).await;
    insert_test_wasm_channel(&conn).await;
    assert_wit_version(&conn, "wasm_tools", "tool-1", "0.3.0").await;
    assert_wit_version(&conn, "wasm_channels", "channel-1", "0.3.0").await;
    assert_migration_absent(&conn, 10).await;
    assert_migration_entries(
        &conn,
        &[
            (12_i64, "wasm_wit_default_0_3_0"),
            (13_i64, "job_token_budget"),
            (14_i64, "drop_redundant_wasm_tools_name_index"),
        ],
    )
    .await;
}

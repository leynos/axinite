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

/// Asserts that a row in `table` with the given `id` has `wit_version = "0.3.0"`.
async fn assert_wit_version_upgraded(conn: &libsql::Connection, table: &str, id: &str) {
    let mut rows = conn
        .query(
            &format!("SELECT wit_version FROM {table} WHERE id = ?1"),
            libsql::params![id],
        )
        .await
        .unwrap_or_else(|e| panic!("query {table}: {e}"));
    let row = rows
        .next()
        .await
        .unwrap_or_else(|e| panic!("{table} rows: {e}"))
        .unwrap_or_else(|| panic!("{table} row missing for id {id}"));
    let wit_version: String = row.get(0).expect("wit_version");
    assert_eq!(wit_version, "0.3.0", "{table} id={id} wit_version mismatch");
}

/// Asserts that no `_migrations` row exists for `version`.
async fn assert_migration_absent(conn: &libsql::Connection, version: i64) {
    let mut rows = conn
        .query(
            "SELECT name FROM _migrations WHERE version = ?1",
            libsql::params![version],
        )
        .await
        .unwrap_or_else(|e| panic!("query migration marker for version {version}: {e}"));
    assert!(
        rows.next()
            .await
            .unwrap_or_else(|e| panic!("read migration row for version {version}: {e}"))
            .is_none(),
        "unexpected _migrations row for version {version}; migration numbering regressed"
    );
}

/// Asserts that a `_migrations` row for `version` exists and has the given
/// `expected_name`.
async fn assert_migration_present(
    conn: &libsql::Connection,
    version: i64,
    expected_name: &str,
) {
    let mut rows = conn
        .query(
            "SELECT name FROM _migrations WHERE version = ?1",
            libsql::params![version],
        )
        .await
        .unwrap_or_else(|e| panic!("query migration marker for version {version}: {e}"));
    let row = rows
        .next()
        .await
        .unwrap_or_else(|e| panic!("read migration row for version {version}: {e}"))
        .unwrap_or_else(|| {
            panic!(
                "expected _migrations row for version {version}; migration sequences diverged"
            )
        });
    let name: String = row.get(0).expect("migration name");
    assert_eq!(
        name, expected_name,
        "migration name mismatch for version {version}"
    );
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

    assert_wit_version_upgraded(&conn, "wasm_tools", "tool-1").await;
    assert_wit_version_upgraded(&conn, "wasm_channels", "channel-1").await;

    assert_migration_absent(&conn, 10).await;

    for (version, expected_name) in [
        (12_i64, "wasm_wit_default_0_3_0"),
        (13_i64, "job_token_budget"),
        (14_i64, "drop_redundant_wasm_tools_name_index"),
    ] {
        assert_migration_present(&conn, version, expected_name).await;
    }
}
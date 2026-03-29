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

    let mut tool_rows = conn
        .query(
            "SELECT wit_version FROM wasm_tools WHERE id = ?1",
            libsql::params!["tool-1"],
        )
        .await
        .expect("query tool");
    let tool_row = tool_rows
        .next()
        .await
        .expect("tool rows")
        .expect("tool row");
    let tool_wit_version: String = tool_row.get(0).expect("tool wit_version");
    assert_eq!(tool_wit_version, "0.3.0");

    let mut channel_rows = conn
        .query(
            "SELECT wit_version FROM wasm_channels WHERE id = ?1",
            libsql::params!["channel-1"],
        )
        .await
        .expect("query channel");
    let channel_row = channel_rows
        .next()
        .await
        .expect("channel rows")
        .expect("channel row");
    let channel_wit_version: String = channel_row.get(0).expect("channel wit_version");
    assert_eq!(channel_wit_version, "0.3.0");

    let mut migration_rows = conn
        .query(
            "SELECT name FROM _migrations WHERE version = ?1",
            libsql::params![12],
        )
        .await
        .expect("query migration marker");
    let migration_row = migration_rows
        .next()
        .await
        .expect("migration rows")
        .expect("migration row");
    let migration_name: String = migration_row.get(0).expect("migration name");
    assert_eq!(migration_name, "wasm_wit_default_0_3_0");
}

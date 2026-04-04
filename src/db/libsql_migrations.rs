//! SQLite-dialect migrations for the libSQL/Turso backend.
//!
//! Consolidates all PostgreSQL migrations (V1-V8) into a single SQLite-compatible
//! schema. Run once on database creation; idempotent via `IF NOT EXISTS`.
//!
//! Incremental migrations (V9+) are tracked in the `_migrations` table and run
//! exactly once per database, in version order.

#[path = "libsql_migrations/v12_wasm.rs"]
mod v12_wasm;
#[path = "libsql_migrations/v16_context.rs"]
mod v16_context;

/// Consolidated schema for libSQL.
///
/// Translates PostgreSQL types and features:
/// - `UUID` -> `TEXT` (store as hex string)
/// - `TIMESTAMPTZ` -> `TEXT` (ISO-8601)
/// - `JSONB` -> `TEXT` (JSON encoded)
/// - `BYTEA` -> `BLOB`
/// - `NUMERIC` -> `TEXT` (preserve precision for rust_decimal)
/// - `TEXT[]` -> `TEXT` (JSON array)
/// - `VECTOR` -> `BLOB` (raw little-endian F32 bytes, any dimension)
/// - `TSVECTOR` -> FTS5 virtual table
/// - `BIGSERIAL` -> `INTEGER PRIMARY KEY AUTOINCREMENT`
/// - PL/pgSQL functions -> SQLite triggers
pub const SCHEMA: &str = include_str!("../../migrations/libsql_schema.sql");

pub(super) struct MigrationContext<'a> {
    version: i64,
    name: &'a str,
}

/// Incremental migrations applied after the base schema.
///
/// Each entry is `(version, name, sql)`. Migrations are idempotent: the
/// `_migrations` table tracks which versions have been applied.
///
/// `INCREMENTAL_MIGRATIONS` intentionally jumps from versions 9 -> 12 for the
/// libSQL backend. The PostgreSQL `wasm_versioning` (V10) and
/// `conversation_unique_indexes` (V11) migrations are already baked into
/// `libsql_schema.sql` rather than applied incrementally, so the gap is
/// backend-specific and should not be "filled in" when editing
/// `INCREMENTAL_MIGRATIONS` or adding later versions.
pub const INCREMENTAL_MIGRATIONS: &[(i64, &str, &str)] = &[
    (
        9,
        "flexible_embedding_dimension",
        // Rebuild memory_chunks to remove the fixed F32_BLOB(1536) type
        // constraint so any embedding dimension works. Existing embeddings
        // are preserved; users only need to re-embed if they change models.
        //
        // The vector index (libsql_vector_idx) requires a fixed-dimension
        // F32_BLOB(N), so we drop it entirely. Vector search falls back to
        // brute-force cosine distance which is fast enough for personal
        // assistant workspaces. This matches PostgreSQL after its V9 migration.
        //
        // SQLite cannot ALTER COLUMN types, so we recreate the table.
        r#"
-- Drop vector index (requires fixed F32_BLOB(N), incompatible with flexible dimensions)
DROP INDEX IF EXISTS idx_memory_chunks_embedding;

-- Drop FTS triggers that reference the old table
DROP TRIGGER IF EXISTS memory_chunks_fts_insert;
DROP TRIGGER IF EXISTS memory_chunks_fts_delete;
DROP TRIGGER IF EXISTS memory_chunks_fts_update;

-- Recreate table with flexible BLOB column (any embedding dimension)
CREATE TABLE IF NOT EXISTS memory_chunks_new (
    _rowid INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    document_id TEXT NOT NULL REFERENCES memory_documents(id) ON DELETE CASCADE,
    chunk_index INTEGER NOT NULL,
    content TEXT NOT NULL,
    embedding BLOB,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (document_id, chunk_index)
);

-- Copy all existing data (embeddings preserved as-is)
INSERT OR IGNORE INTO memory_chunks_new (_rowid, id, document_id, chunk_index, content, embedding, created_at)
    SELECT _rowid, id, document_id, chunk_index, content, embedding, created_at FROM memory_chunks;

-- Swap tables
DROP TABLE memory_chunks;
ALTER TABLE memory_chunks_new RENAME TO memory_chunks;

-- Recreate indexes (no vector index — see comment above)
CREATE INDEX IF NOT EXISTS idx_memory_chunks_document ON memory_chunks(document_id);

-- Recreate FTS triggers
CREATE TRIGGER IF NOT EXISTS memory_chunks_fts_insert AFTER INSERT ON memory_chunks BEGIN
    INSERT INTO memory_chunks_fts(rowid, content) VALUES (new._rowid, new.content);
END;

CREATE TRIGGER IF NOT EXISTS memory_chunks_fts_delete AFTER DELETE ON memory_chunks BEGIN
    INSERT INTO memory_chunks_fts(memory_chunks_fts, rowid, content)
        VALUES ('delete', old._rowid, old.content);
END;

CREATE TRIGGER IF NOT EXISTS memory_chunks_fts_update AFTER UPDATE ON memory_chunks BEGIN
    INSERT INTO memory_chunks_fts(memory_chunks_fts, rowid, content)
        VALUES ('delete', old._rowid, old.content);
    INSERT INTO memory_chunks_fts(rowid, content) VALUES (new._rowid, new.content);
END;
"#,
    ),
    (
        12,
        "wasm_wit_default_0_3_0",
        // Update existing databases that still default newly inserted wasm tool
        // and channel rows to the historical 0.1.0 WIT version. This rebuilds
        // the affected tables because SQLite cannot ALTER COLUMN defaults.
        //
        // `legacy_alter_table=ON` is required so child foreign keys keep pointing
        // at `wasm_tools` while we rename the old table out of the way.
        "-- generated by v12_wasm_wit_default_migration_sql()",
    ),
    (
        13,
        "job_token_budget",
        // Add token budget tracking columns to agent_jobs.
        // SQLite supports ALTER TABLE ADD COLUMN, so no table rebuild needed.
        r#"
ALTER TABLE agent_jobs ADD COLUMN max_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE agent_jobs ADD COLUMN total_tokens_used INTEGER NOT NULL DEFAULT 0;
"#,
    ),
    (
        14,
        "drop_redundant_wasm_tools_name_index",
        include_str!("../../migrations/V14__drop_redundant_wasm_tools_name_index.sql"),
    ),
    (
        15,
        "assistant_conversation_unique_index",
        include_str!("../../migrations/V15__assistant_conversation_unique_index.sql"),
    ),
    (16, v16_context::V16_NAME, v16_context::V16_SQL_MARKER),
];

pub(crate) use v12_wasm::v12_wasm_wit_default_migration_sql;
use v16_context::apply_agent_jobs_context_fields_migration;

/// Run incremental migrations that haven't been applied yet.
///
/// Each migration is wrapped in a transaction. On success the version is
/// recorded in `_migrations` so it won't run again.
pub async fn run_incremental(conn: &libsql::Connection) -> Result<(), crate::error::DatabaseError> {
    use crate::error::DatabaseError;

    for &(version, name, sql) in INCREMENTAL_MIGRATIONS {
        // Check if already applied
        let mut rows = conn
            .query(
                "SELECT 1 FROM _migrations WHERE version = ?1",
                libsql::params![version],
            )
            .await
            .map_err(|e| {
                DatabaseError::Migration(format!("Failed to check migration {version}: {e}"))
            })?;

        if (rows.next().await.map_err(|e| {
            DatabaseError::Migration(format!(
                "Failed to read migration {version} application state: {e}"
            ))
        })?)
        .is_some()
        {
            continue; // Already applied
        }

        tracing::info!(version, name, "libSQL: applying incremental migration");

        // V12 contains its own `BEGIN IMMEDIATE`/`COMMIT` block and sets
        // PRAGMAs that must execute outside a transaction, so bypass the
        // outer transaction wrapper.
        if version == 12 {
            let sql = v12_wasm_wit_default_migration_sql();
            apply_non_transactional_migration(conn, MigrationContext { version, name }, &sql)
                .await?;
            tracing::info!(version, name, "libSQL: migration applied successfully");
            continue;
        }

        if version == 16 {
            apply_agent_jobs_context_fields_migration(conn, MigrationContext { version, name })
                .await?;
            tracing::info!(version, name, "libSQL: migration applied successfully");
            continue;
        }

        // Wrap migration + recording in a transaction for atomicity.
        // If the process crashes mid-migration, the transaction rolls back
        // and the migration will be retried on next startup.
        let tx = conn.transaction().await.map_err(|e| {
            DatabaseError::Migration(format!(
                "libSQL migration V{version}: failed to start transaction: {e}"
            ))
        })?;

        tx.execute_batch(sql).await.map_err(|e| {
            DatabaseError::Migration(format!("libSQL migration V{version} ({name}) failed: {e}"))
        })?;

        // Record as applied (inside the same transaction)
        tx.execute(
            "INSERT INTO _migrations (version, name) VALUES (?1, ?2)",
            libsql::params![version, name],
        )
        .await
        .map_err(|e| {
            DatabaseError::Migration(format!(
                "Failed to record migration V{version} ({name}): {e}"
            ))
        })?;

        tx.commit().await.map_err(|e| {
            DatabaseError::Migration(format!(
                "libSQL migration V{version} ({name}): commit failed: {e}"
            ))
        })?;

        tracing::info!(version, name, "libSQL: migration applied successfully");
    }

    Ok(())
}

async fn apply_non_transactional_migration(
    conn: &libsql::Connection,
    ctx: MigrationContext<'_>,
    sql: &str,
) -> Result<(), crate::error::DatabaseError> {
    let MigrationContext { version, name } = ctx;
    use crate::error::DatabaseError;

    if let Err(e) = conn.execute_batch(sql).await {
        if let Err(cleanup_error) = conn
            .execute_batch("ROLLBACK; PRAGMA foreign_keys=ON; PRAGMA legacy_alter_table=OFF;")
            .await
        {
            tracing::warn!(
                version,
                name,
                error = %cleanup_error,
                "libSQL non-transactional migration cleanup failed"
            );
        }
        return Err(DatabaseError::Migration(format!(
            "libSQL migration V{version} ({name}) failed: {e}"
        )));
    }

    conn.execute(
        "INSERT INTO _migrations (version, name) VALUES (?1, ?2)",
        libsql::params![version, name],
    )
    .await
    .map_err(|e| {
        DatabaseError::Migration(format!(
            "Failed to record non-transactional migration V{version} ({name}): {e}"
        ))
    })?;

    Ok(())
}

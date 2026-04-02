//! PostgreSQL refinery migration repair and execution helpers.

#[cfg(feature = "postgres")]
use tokio_postgres::{Client, GenericClient};

#[cfg(feature = "postgres")]
use crate::error::DatabaseError;

#[cfg(feature = "postgres")]
const ASSERT_REFINERY_SCHEMA_HISTORY_TABLE_SQL: &str = concat!(
    // Intentionally rely on the current `search_path` so a session can shadow
    // the table with a temporary relation during tests and maintenance.
    "CREATE TABLE IF NOT EXISTS refinery_schema_history(",
    "version INT4 PRIMARY KEY, ",
    "name VARCHAR(255), ",
    "applied_on VARCHAR(255), ",
    "checksum VARCHAR(255));"
);
#[cfg(feature = "postgres")]
const LEGACY_V12_JOB_TOKEN_BUDGET_SQL: &str = concat!(
    "-- Add token budget tracking columns to agent_jobs.\n",
    "--\n",
    "-- Tracks max_tokens (configured limit per job) and total_tokens_used (running total)\n",
    "-- to enforce job-level token budgets and prevent budget bypass via user-supplied metadata.\n\n",
    "ALTER TABLE agent_jobs ADD COLUMN max_tokens BIGINT NOT NULL DEFAULT 0;\n",
    "ALTER TABLE agent_jobs ADD COLUMN total_tokens_used BIGINT NOT NULL DEFAULT 0;\n"
);
#[cfg(feature = "postgres")]
const LEGACY_V12_WASM_WIT_DEFAULT_SQL: &str = concat!(
    "ALTER TABLE wasm_tools\n",
    "    ALTER COLUMN wit_version SET DEFAULT '0.3.0';\n\n",
    "ALTER TABLE wasm_channels\n",
    "    ALTER COLUMN wit_version SET DEFAULT '0.3.0';\n"
);
#[cfg(feature = "postgres")]
const LEGACY_V13_DROP_REDUNDANT_WASM_TOOLS_NAME_INDEX_SQL: &str =
    "DROP INDEX IF EXISTS idx_wasm_tools_name;\n";
#[cfg(feature = "postgres")]
const CURRENT_V12_WASM_WIT_DEFAULT_SQL: &str =
    include_str!("../../migrations/V12__wasm_wit_default_0_3_0.sql");
#[cfg(feature = "postgres")]
const CURRENT_V13_JOB_TOKEN_BUDGET_SQL: &str =
    include_str!("../../migrations/V13__job_token_budget.sql");
#[cfg(feature = "postgres")]
const CURRENT_V14_DROP_REDUNDANT_WASM_TOOLS_NAME_INDEX_SQL: &str =
    include_str!("../../migrations/V14__drop_redundant_wasm_tools_name_index.sql");

/// Immutable identifier for a PostgreSQL migration-history row.
/// It stores the released migration version, name, and refinery checksum used
/// when matching exact legacy tuples for repair.
#[cfg(feature = "postgres")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MigrationIdentity {
    pub(crate) version: i32,
    pub(crate) name: &'static str,
    pub(crate) checksum: u64,
}

/// Three-phase rewrite plan for an applied PostgreSQL migration.
/// `from` is the legacy tuple to match, `temporary_version` is the staging slot
/// used during the transaction, and `to` is the canonical tuple to restore.
#[cfg(feature = "postgres")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MigrationHistoryRewrite {
    pub(crate) from: MigrationIdentity,
    pub(crate) temporary_version: i32,
    pub(crate) to: MigrationIdentity,
}

/// Primary `#[cfg(feature = "postgres")]` entry point for PostgreSQL
/// migrations, returning `Result<(), DatabaseError>`.
/// It first calls `repair_postgres_refinery_history(client)` to repair known
/// legacy history rows, then runs the embedded refinery migrations with
/// `migrations::runner().run_async(client)`.
#[cfg(feature = "postgres")]
pub(crate) async fn run_postgres_migrations(client: &mut Client) -> Result<(), DatabaseError> {
    use refinery::embed_migrations;
    embed_migrations!("migrations");

    repair_postgres_refinery_history(client).await?;
    migrations::runner()
        .run_async(client)
        .await
        .map_err(|e| DatabaseError::Migration(e.to_string()))?;
    Ok(())
}

/// Repairs PostgreSQL `refinery_schema_history` rows for the
/// `#[cfg(feature = "postgres")]` build and returns `Result<(), DatabaseError>`.
/// It ensures the history table exists, loads applied `(version, name,
/// checksum)` rows, computes applicable rewrites with
/// `plan_migration_history_rewrites`, and when needed applies them in a
/// two-phase transaction via `stage_migration_history_rewrites` and
/// `finalize_migration_history_rewrites` before committing.
/// Only exact legacy tuples are rewritten, which resolves divergent-history
/// errors without changing the canonical embedded migrations.
#[cfg(feature = "postgres")]
pub(crate) async fn repair_postgres_refinery_history(
    client: &mut Client,
) -> Result<(), DatabaseError> {
    client
        .batch_execute(ASSERT_REFINERY_SCHEMA_HISTORY_TABLE_SQL)
        .await?;

    let applied_rows = client
        .query(
            "SELECT version, name, checksum FROM refinery_schema_history ORDER BY version ASC",
            &[],
        )
        .await?;
    let applied_migrations = applied_rows
        .into_iter()
        .map(
            |row| -> Result<(i32, Option<String>, Option<String>), tokio_postgres::Error> {
                let version: i32 = row.get(0);
                let name: Option<String> = row.try_get(1)?;
                let checksum: Option<String> = row.try_get(2)?;
                Ok((version, name, checksum))
            },
        )
        .collect::<Result<Vec<_>, _>>()?;
    let applicable_rewrites = plan_migration_history_rewrites(&applied_migrations)?;
    if applicable_rewrites.is_empty() {
        return Ok(());
    }

    // `main` briefly renumbered already-released PostgreSQL migrations, which
    // changed refinery's versioned checksums for V12-V14. Existing databases
    // then fail startup with a divergent-history error before any new
    // migrations can run. Rewrite only the exact legacy tuples here so the
    // canonical embedded migration set remains stable again.
    let transaction = client.transaction().await?;
    stage_migration_history_rewrites(&transaction, &applicable_rewrites).await?;
    finalize_migration_history_rewrites(&transaction, &applicable_rewrites).await?;
    transaction.commit().await?;
    Ok(())
}

#[cfg(feature = "postgres")]
fn plan_migration_history_rewrites(
    applied_migrations: &[(i32, Option<String>, Option<String>)],
) -> Result<Vec<MigrationHistoryRewrite>, DatabaseError> {
    Ok(migration_history_rewrites()?
        .into_iter()
        .filter(|rewrite| {
            applied_migrations.iter().any(|(version, name, checksum)| {
                *version == rewrite.from.version
                    && name.as_deref() == Some(rewrite.from.name)
                    && checksum.as_deref() == Some(&rewrite.from.checksum.to_string())
            })
        })
        .collect())
}

#[cfg(feature = "postgres")]
fn migration_history_rewrites() -> Result<Vec<MigrationHistoryRewrite>, DatabaseError> {
    [
        (
            MigrationSpec {
                version: 12,
                name: "wasm_wit_default_0_3_0",
                sql: LEGACY_V12_WASM_WIT_DEFAULT_SQL,
            },
            MigrationSpec {
                version: 12,
                name: "wasm_wit_default_0_3_0",
                sql: CURRENT_V12_WASM_WIT_DEFAULT_SQL,
            },
        ),
        (
            MigrationSpec {
                version: 12,
                name: "job_token_budget",
                sql: LEGACY_V12_JOB_TOKEN_BUDGET_SQL,
            },
            MigrationSpec {
                version: 13,
                name: "job_token_budget",
                sql: CURRENT_V13_JOB_TOKEN_BUDGET_SQL,
            },
        ),
        (
            MigrationSpec {
                version: 13,
                name: "drop_redundant_wasm_tools_name_index",
                sql: LEGACY_V13_DROP_REDUNDANT_WASM_TOOLS_NAME_INDEX_SQL,
            },
            MigrationSpec {
                version: 14,
                name: "drop_redundant_wasm_tools_name_index",
                sql: CURRENT_V14_DROP_REDUNDANT_WASM_TOOLS_NAME_INDEX_SQL,
            },
        ),
        (
            MigrationSpec {
                version: 14,
                name: "wasm_wit_default_0_3_0",
                sql: CURRENT_V12_WASM_WIT_DEFAULT_SQL,
            },
            MigrationSpec {
                version: 12,
                name: "wasm_wit_default_0_3_0",
                sql: CURRENT_V12_WASM_WIT_DEFAULT_SQL,
            },
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (from, to))| migration_history_rewrite(from, to, staging_version(index)))
    .collect()
}

#[cfg(feature = "postgres")]
struct MigrationSpec {
    version: i32,
    name: &'static str,
    sql: &'static str,
}

#[cfg(feature = "postgres")]
fn migration_history_rewrite(
    from: MigrationSpec,
    to: MigrationSpec,
    temporary_version: i32,
) -> Result<MigrationHistoryRewrite, DatabaseError> {
    Ok(MigrationHistoryRewrite {
        from: migration_identity(from)?,
        temporary_version,
        to: migration_identity(to)?,
    })
}

#[cfg(feature = "postgres")]
fn staging_version(index: usize) -> i32 {
    // Reserve an internal staging range in the negative i32 space so rewrites
    // cannot collide with released positive migration versions.
    i32::MIN + i32::try_from(index).expect("rewrite index fits in i32")
}

#[cfg(feature = "postgres")]
fn migration_identity(spec: MigrationSpec) -> Result<MigrationIdentity, DatabaseError> {
    let filename = format!("V{}__{}.sql", spec.version, spec.name);
    let migration = refinery::Migration::unapplied(&filename, spec.sql)
        .map_err(|e| DatabaseError::Migration(format!("Failed to parse {filename}: {e}")))?;
    Ok(MigrationIdentity {
        version: spec.version,
        name: spec.name,
        checksum: migration.checksum(),
    })
}

#[cfg(feature = "postgres")]
async fn rewrite_history_row<C>(
    client: &C,
    where_version: i32,
    where_name: &str,
    where_checksum: &str,
    set_version: i32,
    set_name: &str,
    set_checksum: &str,
) -> Result<(), DatabaseError>
where
    C: GenericClient,
{
    client
        .execute(
            "UPDATE refinery_schema_history \
             SET version = $1, name = $2, checksum = $3 \
             WHERE version = $4 AND name = $5 AND checksum = $6",
            &[
                &set_version,
                &set_name,
                &set_checksum,
                &where_version,
                &where_name,
                &where_checksum,
            ],
        )
        .await?;
    Ok(())
}

#[cfg(feature = "postgres")]
async fn stage_migration_history_rewrites<C>(
    client: &C,
    rewrites: &[MigrationHistoryRewrite],
) -> Result<(), DatabaseError>
where
    C: GenericClient,
{
    for rewrite in rewrites {
        let from_checksum = rewrite.from.checksum.to_string();
        rewrite_history_row(
            client,
            rewrite.from.version,
            rewrite.from.name,
            &from_checksum,
            rewrite.temporary_version,
            rewrite.from.name,
            &from_checksum,
        )
        .await?;
    }

    Ok(())
}

#[cfg(feature = "postgres")]
async fn finalize_migration_history_rewrites<C>(
    client: &C,
    rewrites: &[MigrationHistoryRewrite],
) -> Result<(), DatabaseError>
where
    C: GenericClient,
{
    for rewrite in rewrites {
        let from_checksum = rewrite.from.checksum.to_string();
        let to_checksum = rewrite.to.checksum.to_string();
        rewrite_history_row(
            client,
            rewrite.temporary_version,
            rewrite.from.name,
            &from_checksum,
            rewrite.to.version,
            rewrite.to.name,
            &to_checksum,
        )
        .await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests;

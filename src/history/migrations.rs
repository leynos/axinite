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

#[cfg(feature = "postgres")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MigrationIdentity {
    pub(crate) version: i32,
    pub(crate) name: &'static str,
    pub(crate) checksum: u64,
}

#[cfg(feature = "postgres")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MigrationHistoryRewrite {
    pub(crate) from: MigrationIdentity,
    pub(crate) temporary_version: i32,
    pub(crate) to: MigrationIdentity,
}

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
        .map(|row| {
            let version: i32 = row.get(0);
            let name: String = row.get(1);
            let checksum: String = row.get(2);
            (version, name, checksum)
        })
        .collect::<Vec<_>>();
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
    applied_migrations: &[(i32, String, String)],
) -> Result<Vec<MigrationHistoryRewrite>, DatabaseError> {
    Ok(migration_history_rewrites()?
        .into_iter()
        .filter(|rewrite| {
            applied_migrations.iter().any(|(version, name, checksum)| {
                *version == rewrite.from.version
                    && name == rewrite.from.name
                    && checksum == &rewrite.from.checksum.to_string()
            })
        })
        .collect())
}

#[cfg(feature = "postgres")]
fn migration_history_rewrites() -> Result<Vec<MigrationHistoryRewrite>, DatabaseError> {
    Ok(vec![
        migration_history_rewrite(
            12,
            "wasm_wit_default_0_3_0",
            LEGACY_V12_WASM_WIT_DEFAULT_SQL,
            12,
            "wasm_wit_default_0_3_0",
            CURRENT_V12_WASM_WIT_DEFAULT_SQL,
        )?,
        migration_history_rewrite(
            12,
            "job_token_budget",
            LEGACY_V12_JOB_TOKEN_BUDGET_SQL,
            13,
            "job_token_budget",
            CURRENT_V13_JOB_TOKEN_BUDGET_SQL,
        )?,
        migration_history_rewrite(
            13,
            "drop_redundant_wasm_tools_name_index",
            LEGACY_V13_DROP_REDUNDANT_WASM_TOOLS_NAME_INDEX_SQL,
            14,
            "drop_redundant_wasm_tools_name_index",
            CURRENT_V14_DROP_REDUNDANT_WASM_TOOLS_NAME_INDEX_SQL,
        )?,
        migration_history_rewrite(
            14,
            "wasm_wit_default_0_3_0",
            CURRENT_V12_WASM_WIT_DEFAULT_SQL,
            12,
            "wasm_wit_default_0_3_0",
            CURRENT_V12_WASM_WIT_DEFAULT_SQL,
        )?,
    ])
}

#[cfg(feature = "postgres")]
fn migration_history_rewrite(
    from_version: i32,
    from_name: &'static str,
    from_sql: &'static str,
    to_version: i32,
    to_name: &'static str,
    to_sql: &'static str,
) -> Result<MigrationHistoryRewrite, DatabaseError> {
    Ok(MigrationHistoryRewrite {
        from: migration_identity(from_version, from_name, from_sql)?,
        temporary_version: 1_000 + from_version,
        to: migration_identity(to_version, to_name, to_sql)?,
    })
}

#[cfg(feature = "postgres")]
fn migration_identity(
    version: i32,
    name: &'static str,
    sql: &'static str,
) -> Result<MigrationIdentity, DatabaseError> {
    let filename = format!("V{version}__{name}.sql");
    let migration = refinery::Migration::unapplied(&filename, sql)
        .map_err(|e| DatabaseError::Migration(format!("Failed to parse {filename}: {e}")))?;
    Ok(MigrationIdentity {
        version,
        name,
        checksum: migration.checksum(),
    })
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
        client
            .execute(
                "UPDATE refinery_schema_history \
                 SET version = $1 \
                 WHERE version = $2 AND name = $3 AND checksum = $4",
                &[
                    &rewrite.temporary_version,
                    &rewrite.from.version,
                    &rewrite.from.name,
                    &from_checksum,
                ],
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
        client
            .execute(
                "UPDATE refinery_schema_history \
                 SET version = $1, name = $2, checksum = $3 \
                 WHERE version = $4 AND name = $5 AND checksum = $6",
                &[
                    &rewrite.to.version,
                    &rewrite.to.name,
                    &to_checksum,
                    &rewrite.temporary_version,
                    &rewrite.from.name,
                    &from_checksum,
                ],
            )
            .await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests;

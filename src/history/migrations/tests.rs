//! Fixed released PostgreSQL migration-history fixtures and repair tests.
//!
//! These tests pin the released version/name/checksum tuples that
//! `migration_history_rewrites`, `plan_migration_history_rewrites`,
//! `stage_migration_history_rewrites`, `finalize_migration_history_rewrites`,
//! and `repair_postgres_refinery_history` must continue to recognise and
//! repair. They cover exact rewrite-set planning plus the staged, finalized,
//! and end-to-end PostgreSQL `refinery_schema_history` repair flow.

use chrono::Utc;
use std::collections::BTreeSet;
use tokio_postgres::{Client, GenericClient, Row};

use super::{
    finalize_migration_history_rewrites, migration_history_rewrites,
    plan_migration_history_rewrites, repair_postgres_refinery_history,
    stage_migration_history_rewrites,
};

/// Released tuples that must still be recognised for rewrite planning even
/// though they cannot all coexist in `refinery_schema_history` because version
/// `12` appears twice.
///
/// Provenance:
/// - `(12, "wasm_wit_default_0_3_0", 17026967434177311328)` hashes the
///   `LEGACY_V12_WASM_WIT_DEFAULT_SQL` fragment in
///   `src/history/migrations.rs`: `ALTER TABLE wasm_tools ...` plus
///   `ALTER TABLE wasm_channels ...` without the later `UPDATE` statements.
/// - `(12, "job_token_budget", 13685500183340941819)` hashes the body of the
///   historical file `migrations/V12__job_token_budget.sql` from commit
///   `a677b20`.
/// - `(13, "drop_redundant_wasm_tools_name_index", 16100593955252925602)`
///   hashes the body of the historical file
///   `migrations/V13__drop_redundant_wasm_tools_name_index.sql` from commit
///   `8a29e56`.
/// - `(14, "wasm_wit_default_0_3_0", 9366402964940367356)` hashes the body of
///   the historical file `migrations/V14__wasm_wit_default_0_3_0.sql` from
///   commit `10050f2`; that SQL body is byte-for-byte identical to the current
///   `migrations/V12__wasm_wit_default_0_3_0.sql`, but refinery includes the
///   migration version in the checksum so `V14` and `V12` differ.
///
/// Checksums are produced exactly as production code does in
/// `migration_identity`: `refinery::Migration::unapplied(filename, sql)?
/// .checksum()`, which in refinery `0.8.16` hashes `(name, version, sql)` with
/// `SipHasher13`. Regenerate by constructing the matching filename and SQL body
/// in a small Rust snippet or test and printing `.checksum()`. The SQL blobs
/// above also have stable SHA-256 fingerprints:
/// `6e8cd37ab078c1efc6ca38f73d2adbdf9f9d533fffeebb2c98edecdfe02f9246` for the
/// WIT-default body, `67949c6df74e52722dd818cb45e3d829ef705bf5befda0a97807520e079660b9`
/// for `job_token_budget`, and
/// `7030813d915b74a62aaa19a9ace12d90e65002d3218525277d7041430def2146` for
/// `drop_redundant_wasm_tools_name_index`.
const LEGACY_RELEASED_ROWS: &[(i32, &str, u64)] = &[
    (12, "wasm_wit_default_0_3_0", 17026967434177311328),
    (12, "job_token_budget", 13685500183340941819),
    (
        13,
        "drop_redundant_wasm_tools_name_index",
        16100593955252925602,
    ),
    (14, "wasm_wit_default_0_3_0", 9366402964940367356),
];

/// Actual released tuples from the brief renumbered window that can coexist in
/// `refinery_schema_history` and therefore seed the end-to-end repair tests.
///
/// Provenance:
/// - `(12, "job_token_budget", 13685500183340941819)` hashes the historical
///   `migrations/V12__job_token_budget.sql` body from commit `a677b20`.
/// - `(13, "drop_redundant_wasm_tools_name_index", 16100593955252925602)`
///   hashes the historical `migrations/V13__drop_redundant_wasm_tools_name_index.sql`
///   body from commit `8a29e56`.
/// - `(14, "wasm_wit_default_0_3_0", 9366402964940367356)` hashes the
///   historical `migrations/V14__wasm_wit_default_0_3_0.sql` body from commit
///   `10050f2`.
///
/// Regenerate with `refinery::Migration::unapplied(...)?.checksum()` using the
/// exact historical filename/version pairing and SQL text. The underlying SQL
/// bodies are the same ones documented above for `LEGACY_RELEASED_ROWS`.
const RENUMBERED_RELEASE_WINDOW_ROWS: &[(i32, &str, u64)] = &[
    (12, "job_token_budget", 13685500183340941819),
    (
        13,
        "drop_redundant_wasm_tools_name_index",
        16100593955252925602,
    ),
    (14, "wasm_wit_default_0_3_0", 9366402964940367356),
];

/// Canonical tuples from the current on-disk migration files:
/// `migrations/V12__wasm_wit_default_0_3_0.sql`,
/// `migrations/V13__job_token_budget.sql`, and
/// `migrations/V14__drop_redundant_wasm_tools_name_index.sql`.
///
/// Provenance:
/// - `(12, "wasm_wit_default_0_3_0", 6506104151552529421)` hashes the exact
///   checked-in `V12` file body, including the `UPDATE wasm_tools` and
///   `UPDATE wasm_channels` statements.
/// - `(13, "job_token_budget", 8579391521996531151)` hashes the exact checked-in
///   `V13` file body that adds `max_tokens` and `total_tokens_used`.
/// - `(14, "drop_redundant_wasm_tools_name_index", 16545681577522743559)`
///   hashes the exact checked-in `V14` file body `DROP INDEX IF EXISTS
///   idx_wasm_tools_name;`.
///
/// Regenerate with `refinery::Migration::unapplied(...)?.checksum()` using the
/// live migration file contents. For a quick byte-level verification, the SQL
/// bodies currently hash to SHA-256
/// `6e8cd37ab078c1efc6ca38f73d2adbdf9f9d533fffeebb2c98edecdfe02f9246` (`V12`),
/// `67949c6df74e52722dd818cb45e3d829ef705bf5befda0a97807520e079660b9` (`V13`),
/// and `7030813d915b74a62aaa19a9ace12d90e65002d3218525277d7041430def2146`
/// (`V14`).
const CANONICAL_RELEASED_ROWS: &[(i32, &str, u64)] = &[
    (12, "wasm_wit_default_0_3_0", 6506104151552529421),
    (13, "job_token_budget", 8579391521996531151),
    (
        14,
        "drop_redundant_wasm_tools_name_index",
        16545681577522743559,
    ),
];

#[cfg(feature = "postgres")]
fn rewrite_tuple_set(
    rewrites: &[super::MigrationHistoryRewrite],
) -> BTreeSet<(i32, &'static str, u64, i32, &'static str, u64)> {
    rewrites
        .iter()
        .map(|rewrite| {
            (
                rewrite.from.version,
                rewrite.from.name,
                rewrite.from.checksum,
                rewrite.to.version,
                rewrite.to.name,
                rewrite.to.checksum,
            )
        })
        .collect()
}

#[cfg(feature = "postgres")]
fn rows_to_tuples(rows: Vec<Row>) -> Vec<(i32, String, String)> {
    rows.into_iter()
        .map(|row| {
            let version: i32 = row.get(0);
            let name: String = row.get(1);
            let checksum: String = row.get(2);
            (version, name, checksum)
        })
        .collect()
}

#[cfg(feature = "postgres")]
async fn create_temp_refinery_history_table(client: &Client) {
    client
        .batch_execute(
            "CREATE TEMP TABLE refinery_schema_history (\
             version INT4 PRIMARY KEY, \
             name VARCHAR(255), \
             applied_on VARCHAR(255), \
             checksum VARCHAR(255)) ON COMMIT DROP;",
        )
        .await
        .expect("Failed to create temp history table");
}

#[cfg(feature = "postgres")]
async fn seed_history_rows<C: GenericClient>(client: &C, rows: &[(i32, &str, u64)]) {
    for (version, name, checksum) in rows {
        client
            .execute(
                "INSERT INTO refinery_schema_history (version, name, applied_on, checksum) \
                 VALUES ($1, $2, $3, $4)",
                &[
                    version,
                    name,
                    &Utc::now().to_rfc3339(),
                    &checksum.to_string(),
                ],
            )
            .await
            .expect("Failed to seed history row");
    }
}

#[cfg(feature = "postgres")]
async fn seed_legacy_released_rows<C: GenericClient>(client: &C) {
    // `LEGACY_RELEASED_ROWS` documents the full rewrite catalogue, but the
    // real refinery table is keyed by `version`, so only the checksum-only
    // legacy V12 row can coexist with the canonical V13/V14 rows in one seed.
    seed_history_rows(
        client,
        &[
            LEGACY_RELEASED_ROWS[0],
            CANONICAL_RELEASED_ROWS[1],
            CANONICAL_RELEASED_ROWS[2],
        ],
    )
    .await;
}

#[cfg(feature = "postgres")]
fn renumbered_release_window_applied_rows() -> Vec<(i32, String, String)> {
    RENUMBERED_RELEASE_WINDOW_ROWS
        .iter()
        .map(|(version, name, checksum)| (*version, (*name).to_string(), checksum.to_string()))
        .collect()
}

#[cfg(feature = "postgres")]
fn canonical_release_window_rows() -> Vec<(i32, String, String)> {
    CANONICAL_RELEASED_ROWS
        .iter()
        .map(|(version, name, checksum)| (*version, (*name).to_string(), checksum.to_string()))
        .collect()
}

#[cfg(feature = "postgres")]
fn canonical_full_release_rows() -> Vec<(i32, String, String)> {
    canonical_release_window_rows()
}

#[cfg(feature = "postgres")]
fn staged_release_window_rows() -> Vec<(i32, String, String)> {
    vec![
        (
            1012,
            "job_token_budget".to_string(),
            "13685500183340941819".to_string(),
        ),
        (
            1013,
            "drop_redundant_wasm_tools_name_index".to_string(),
            "16100593955252925602".to_string(),
        ),
        (
            1014,
            "wasm_wit_default_0_3_0".to_string(),
            "9366402964940367356".to_string(),
        ),
    ]
}

#[cfg(feature = "postgres")]
fn expected_rewrite_set() -> BTreeSet<(i32, &'static str, u64, i32, &'static str, u64)> {
    BTreeSet::from([
        (
            12,
            "wasm_wit_default_0_3_0",
            17026967434177311328,
            12,
            "wasm_wit_default_0_3_0",
            6506104151552529421,
        ),
        (
            12,
            "job_token_budget",
            13685500183340941819,
            13,
            "job_token_budget",
            8579391521996531151,
        ),
        (
            13,
            "drop_redundant_wasm_tools_name_index",
            16100593955252925602,
            14,
            "drop_redundant_wasm_tools_name_index",
            16545681577522743559,
        ),
        (
            14,
            "wasm_wit_default_0_3_0",
            9366402964940367356,
            12,
            "wasm_wit_default_0_3_0",
            6506104151552529421,
        ),
    ])
}

#[cfg(feature = "postgres")]
#[test]
fn migration_history_rewrites_cover_the_known_released_mappings() {
    let rewrites = migration_history_rewrites().expect("released migration identities parse");
    assert_eq!(rewrite_tuple_set(&rewrites), expected_rewrite_set());
}

#[cfg(feature = "postgres")]
#[test]
fn plan_migration_history_rewrites_matches_fixed_released_rows() {
    let applied = LEGACY_RELEASED_ROWS
        .iter()
        .map(|(version, name, checksum)| (*version, (*name).to_string(), checksum.to_string()))
        .collect::<Vec<_>>();

    let rewrites =
        plan_migration_history_rewrites(&applied).expect("released migration identities parse");
    assert_eq!(rewrite_tuple_set(&rewrites), expected_rewrite_set());
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore]
async fn stage_and_finalize_migration_history_rewrites_two_phases() {
    use crate::config::Config;
    use crate::history::Store;

    let _ = dotenvy::dotenv();
    let config = Config::from_env()
        .await
        .unwrap_or_else(|e| panic!("Config::from_env failed: {e:?}"));
    let store = Store::new(&config.database)
        .await
        .expect("Failed to connect to database");
    let mut client = store.conn().await.expect("Failed to get connection");
    create_temp_refinery_history_table(&client).await;
    seed_history_rows(&**client, RENUMBERED_RELEASE_WINDOW_ROWS).await;

    let rewrites = plan_migration_history_rewrites(&renumbered_release_window_applied_rows())
        .expect("released migration identities parse");

    let transaction = (**client).transaction().await.expect("Failed to start tx");
    stage_migration_history_rewrites(&transaction, &rewrites)
        .await
        .expect("Failed to stage rewrites");

    let staged = transaction
        .query(
            "SELECT version, name, checksum FROM refinery_schema_history ORDER BY version ASC",
            &[],
        )
        .await
        .expect("Failed to read staged rows");
    assert_eq!(rows_to_tuples(staged), staged_release_window_rows());

    finalize_migration_history_rewrites(&transaction, &rewrites)
        .await
        .expect("Failed to finalize rewrites");
    let final_rows = transaction
        .query(
            "SELECT version, name, checksum FROM refinery_schema_history ORDER BY version ASC",
            &[],
        )
        .await
        .expect("Failed to read final rows");
    assert_eq!(rows_to_tuples(final_rows), canonical_release_window_rows());
    transaction
        .rollback()
        .await
        .expect("Failed to rollback staged tx");
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore]
async fn repair_postgres_refinery_history_end_to_end() {
    use crate::config::Config;
    use crate::history::Store;

    let _ = dotenvy::dotenv();
    let config = Config::from_env()
        .await
        .unwrap_or_else(|e| panic!("Config::from_env failed: {e:?}"));
    let store = Store::new(&config.database)
        .await
        .expect("Failed to connect to database");
    let mut client = store.conn().await.expect("Failed to get connection");
    create_temp_refinery_history_table(&client).await;
    seed_history_rows(&**client, RENUMBERED_RELEASE_WINDOW_ROWS).await;

    repair_postgres_refinery_history(&mut client)
        .await
        .expect("Failed to repair refinery history");

    let repaired_rows = client
        .query(
            "SELECT version, name, checksum FROM refinery_schema_history ORDER BY version ASC",
            &[],
        )
        .await
        .expect("Failed to read repaired rows");
    assert_eq!(
        rows_to_tuples(repaired_rows),
        canonical_release_window_rows()
    );
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore]
async fn repair_postgres_refinery_history_repairs_checksum_only_legacy_v12_e2e() {
    use crate::config::Config;
    use crate::history::Store;

    let _ = dotenvy::dotenv();
    let config = Config::from_env()
        .await
        .unwrap_or_else(|e| panic!("Config::from_env failed: {e:?}"));
    let store = Store::new(&config.database)
        .await
        .expect("Failed to connect to database");
    let mut client = store.conn().await.expect("Failed to get connection");
    create_temp_refinery_history_table(&client).await;
    seed_legacy_released_rows(&**client).await;

    repair_postgres_refinery_history(&mut client)
        .await
        .expect("Failed to repair refinery history");

    let repaired_rows = client
        .query(
            "SELECT version, name, checksum FROM refinery_schema_history ORDER BY version ASC",
            &[],
        )
        .await
        .expect("Failed to read repaired rows");
    assert_eq!(rows_to_tuples(repaired_rows), canonical_full_release_rows());
}

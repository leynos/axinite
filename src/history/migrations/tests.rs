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

const RENUMBERED_RELEASE_WINDOW_ROWS: &[(i32, &str, u64)] = &[
    (12, "job_token_budget", 13685500183340941819),
    (
        13,
        "drop_redundant_wasm_tools_name_index",
        16100593955252925602,
    ),
    (14, "wasm_wit_default_0_3_0", 9366402964940367356),
];

const CANONICAL_RELEASED_ROWS: &[(i32, &str, u64)] = &[
    (12, "wasm_wit_default_0_3_0", 6506104151552529421),
    (13, "job_token_budget", 8579391521996531151),
    (
        14,
        "drop_redundant_wasm_tools_name_index",
        16545681577522743559,
    ),
];

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

fn map_history_rows(rows: Vec<Row>) -> Vec<(i32, String, String)> {
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

fn renumbered_release_window_applied_rows() -> Vec<(i32, String, String)> {
    RENUMBERED_RELEASE_WINDOW_ROWS
        .iter()
        .map(|(version, name, checksum)| (*version, (*name).to_string(), checksum.to_string()))
        .collect()
}

fn canonical_rows_as_vec() -> Vec<(i32, String, String)> {
    CANONICAL_RELEASED_ROWS
        .iter()
        .map(|(version, name, checksum)| (*version, (*name).to_string(), checksum.to_string()))
        .collect()
}

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
#[test]
fn migration_history_rewrites_cover_the_known_released_mappings() {
    let rewrites = migration_history_rewrites().expect("released migration identities parse");
    let expected = BTreeSet::from([
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
    ]);

    assert_eq!(rewrite_tuple_set(&rewrites), expected);
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
    let expected = BTreeSet::from([
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
    ]);

    assert_eq!(rewrite_tuple_set(&rewrites), expected);
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
    assert_eq!(map_history_rows(staged), staged_release_window_rows());

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
    assert_eq!(map_history_rows(final_rows), canonical_rows_as_vec());
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
    assert_eq!(map_history_rows(repaired_rows), canonical_rows_as_vec());
}

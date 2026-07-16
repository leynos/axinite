//! PostgreSQL-only test scaffolding for migration-history repair tests.

#[cfg(feature = "postgres")]
use chrono::Utc;
#[cfg(feature = "postgres")]
use std::collections::BTreeSet;
#[cfg(feature = "postgres")]
use tokio_postgres::{Client, GenericClient, Row};

#[cfg(feature = "postgres")]
use super::fixtures::{CANONICAL_RELEASED_ROWS, LEGACY_RELEASED_ROWS};

/// Compile-time string equality for use in `const` contexts.
#[cfg(feature = "postgres")]
const fn str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Locate a fixture row by version and name.
///
/// Evaluated in `const` context by callers, so a missing fixture row fails
/// the build instead of panicking at run time.
#[cfg(feature = "postgres")]
const fn fixture_row(
    rows: &'static [(i32, &'static str, u64)],
    version: i32,
    name: &str,
) -> (i32, &'static str, u64) {
    let mut i = 0;
    while i < rows.len() {
        let (row_version, row_name, checksum) = rows[i];
        if row_version == version && str_eq(row_name, name) {
            return (row_version, row_name, checksum);
        }
        i += 1;
    }
    panic!("missing fixture row for requested version and name");
}

#[cfg(feature = "postgres")]
pub(super) fn rewrite_tuple_set(
    rewrites: &[super::super::MigrationHistoryRewrite],
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
pub(super) fn rows_to_tuples(rows: Vec<Row>) -> Vec<(i32, String, String)> {
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
pub(super) async fn create_temp_refinery_history_table(client: &Client) {
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
pub(super) async fn seed_history_rows<C: GenericClient>(client: &C, rows: &[(i32, &str, u64)]) {
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
pub(super) async fn seed_legacy_released_rows<C: GenericClient>(client: &C) {
    // `LEGACY_RELEASED_ROWS` documents the full rewrite catalogue, but the
    // real refinery table is keyed by `version`, so only the checksum-only
    // legacy V12 row can coexist with the canonical V13/V14 rows in one seed.
    // Resolved in `const` context so a missing fixture row fails the build.
    const SEED_ROWS: [(i32, &str, u64); 3] = [
        fixture_row(LEGACY_RELEASED_ROWS, 12, "wasm_wit_default_0_3_0"),
        fixture_row(CANONICAL_RELEASED_ROWS, 13, "job_token_budget"),
        fixture_row(
            CANONICAL_RELEASED_ROWS,
            14,
            "drop_redundant_wasm_tools_name_index",
        ),
    ];
    seed_history_rows(client, &SEED_ROWS).await;
}

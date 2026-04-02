//! Released PostgreSQL migration-history fixtures used by repair tests.

#[cfg(feature = "postgres")]
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
pub(super) const LEGACY_RELEASED_ROWS: &[(i32, &str, u64)] = &[
    (12, "wasm_wit_default_0_3_0", 17026967434177311328),
    (12, "job_token_budget", 13685500183340941819),
    (
        13,
        "drop_redundant_wasm_tools_name_index",
        16100593955252925602,
    ),
    (14, "wasm_wit_default_0_3_0", 9366402964940367356),
];

#[cfg(feature = "postgres")]
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
pub(super) const RENUMBERED_RELEASE_WINDOW_ROWS: &[(i32, &str, u64)] = &[
    (12, "job_token_budget", 13685500183340941819),
    (
        13,
        "drop_redundant_wasm_tools_name_index",
        16100593955252925602,
    ),
    (14, "wasm_wit_default_0_3_0", 9366402964940367356),
];

#[cfg(feature = "postgres")]
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
pub(super) const CANONICAL_RELEASED_ROWS: &[(i32, &str, u64)] = &[
    (12, "wasm_wit_default_0_3_0", 6506104151552529421),
    (13, "job_token_budget", 8579391521996531151),
    (
        14,
        "drop_redundant_wasm_tools_name_index",
        16545681577522743559,
    ),
];

#[cfg(feature = "postgres")]
pub(super) const EXPECTED_REWRITE_TUPLES: &[(i32, &str, u64, i32, &str, u64)] = &[
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
];

#[cfg(feature = "postgres")]
pub(super) fn renumbered_release_window_applied_rows() -> Vec<(i32, Option<String>, Option<String>)>
{
    RENUMBERED_RELEASE_WINDOW_ROWS
        .iter()
        .map(|(version, name, checksum)| {
            (
                *version,
                Some((*name).to_string()),
                Some(checksum.to_string()),
            )
        })
        .collect()
}

#[cfg(feature = "postgres")]
pub(super) fn legacy_released_applied_rows() -> Vec<(i32, Option<String>, Option<String>)> {
    LEGACY_RELEASED_ROWS
        .iter()
        .map(|(version, name, checksum)| {
            (
                *version,
                Some((*name).to_string()),
                Some(checksum.to_string()),
            )
        })
        .collect()
}

#[cfg(feature = "postgres")]
pub(super) fn canonical_release_window_rows() -> Vec<(i32, String, String)> {
    CANONICAL_RELEASED_ROWS
        .iter()
        .map(|(version, name, checksum)| (*version, (*name).to_string(), checksum.to_string()))
        .collect()
}

#[cfg(feature = "postgres")]
pub(super) fn canonical_full_release_rows() -> Vec<(i32, String, String)> {
    canonical_release_window_rows()
}

#[cfg(feature = "postgres")]
pub(super) fn staged_release_window_rows() -> Vec<(i32, String, String)> {
    vec![
        (
            i32::MIN + 1,
            "job_token_budget".to_string(),
            "13685500183340941819".to_string(),
        ),
        (
            i32::MIN + 2,
            "drop_redundant_wasm_tools_name_index".to_string(),
            "16100593955252925602".to_string(),
        ),
        (
            i32::MIN + 3,
            "wasm_wit_default_0_3_0".to_string(),
            "9366402964940367356".to_string(),
        ),
    ]
}

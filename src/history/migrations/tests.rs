//! Fixed released PostgreSQL migration-history fixtures and repair tests.
//!
//! These tests pin the released version/name/checksum tuples that
//! `migration_history_rewrites`, `plan_migration_history_rewrites`,
//! `stage_migration_history_rewrites`, `finalize_migration_history_rewrites`,
//! and `repair_postgres_refinery_history` must continue to recognise and
//! repair. They cover exact rewrite-set planning plus the staged, finalized,
//! and end-to-end PostgreSQL `refinery_schema_history` repair flow.

#[cfg(feature = "postgres")]
use std::collections::BTreeSet;

#[cfg(feature = "postgres")]
use super::{
    finalize_migration_history_rewrites, migration_history_rewrites,
    plan_migration_history_rewrites, repair_postgres_refinery_history,
    stage_migration_history_rewrites,
};
#[cfg(feature = "postgres")]
use crate::testing::test_utils::EnvVarsGuard;

#[cfg(feature = "postgres")]
mod fixtures;
#[cfg(feature = "postgres")]
mod postgres_testing;

#[cfg(feature = "postgres")]
use fixtures::{
    EXPECTED_REWRITE_TUPLES, RENUMBERED_RELEASE_WINDOW_ROWS, canonical_full_release_rows,
    canonical_release_window_rows, legacy_released_applied_rows,
    renumbered_release_window_applied_rows, staged_release_window_rows,
};
#[cfg(feature = "postgres")]
use postgres_testing::{
    create_temp_refinery_history_table, rewrite_tuple_set, rows_to_tuples, seed_history_rows,
    seed_legacy_released_rows,
};

#[cfg(feature = "postgres")]
#[test]
fn migration_history_rewrites_cover_the_known_released_mappings() {
    let rewrites = migration_history_rewrites().expect("released migration identities parse");

    assert_eq!(rewrites.len(), EXPECTED_REWRITE_TUPLES.len());
    for &(fv, fn_, fc, tv, tn, tc) in EXPECTED_REWRITE_TUPLES {
        assert!(
            rewrites.iter().any(|r| {
                r.from.version == fv
                    && r.from.name == fn_
                    && r.from.checksum == fc
                    && r.to.version == tv
                    && r.to.name == tn
                    && r.to.checksum == tc
            }),
            "missing rewrite from {}:{}:{} to {}:{}:{}",
            fv,
            fn_,
            fc,
            tv,
            tn,
            tc,
        );
    }
}

#[cfg(feature = "postgres")]
#[test]
fn plan_migration_history_rewrites_matches_fixed_released_rows() {
    let rewrites = plan_migration_history_rewrites(&legacy_released_applied_rows())
        .expect("released migration identities parse");
    let expected = EXPECTED_REWRITE_TUPLES
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();

    assert_eq!(rewrite_tuple_set(&rewrites), expected);
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore]
async fn stage_and_finalize_migration_history_rewrites_two_phases() {
    use crate::config::Config;
    use crate::history::Store;

    let _env_guard = EnvVarsGuard::new(&["DATABASE_URL"]);
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

    let _env_guard = EnvVarsGuard::new(&["DATABASE_URL"]);
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

    let _env_guard = EnvVarsGuard::new(&["DATABASE_URL"]);
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

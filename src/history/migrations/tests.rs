use chrono::Utc;

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

#[cfg(feature = "postgres")]
#[test]
fn migration_history_rewrites_cover_the_known_released_mappings() {
    let rewrites = migration_history_rewrites().expect("released migration identities parse");

    assert!(rewrites.iter().any(|rewrite| {
        rewrite.from.version == 12
            && rewrite.from.name == "wasm_wit_default_0_3_0"
            && rewrite.from.checksum == 17026967434177311328
            && rewrite.to.version == 12
            && rewrite.to.name == "wasm_wit_default_0_3_0"
            && rewrite.to.checksum == 6506104151552529421
    }));
    assert!(rewrites.iter().any(|rewrite| {
        rewrite.from.version == 12
            && rewrite.from.name == "job_token_budget"
            && rewrite.from.checksum == 13685500183340941819
            && rewrite.to.version == 13
            && rewrite.to.name == "job_token_budget"
            && rewrite.to.checksum == 8579391521996531151
    }));
    assert!(rewrites.iter().any(|rewrite| {
        rewrite.from.version == 13
            && rewrite.from.name == "drop_redundant_wasm_tools_name_index"
            && rewrite.from.checksum == 16100593955252925602
            && rewrite.to.version == 14
            && rewrite.to.name == "drop_redundant_wasm_tools_name_index"
            && rewrite.to.checksum == 16545681577522743559
    }));
    assert!(rewrites.iter().any(|rewrite| {
        rewrite.from.version == 14
            && rewrite.from.name == "wasm_wit_default_0_3_0"
            && rewrite.from.checksum == 9366402964940367356
            && rewrite.to.version == 12
            && rewrite.to.name == "wasm_wit_default_0_3_0"
            && rewrite.to.checksum == 6506104151552529421
    }));
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

    assert!(rewrites.iter().any(|rewrite| {
        rewrite.from.version == 12
            && rewrite.from.name == "wasm_wit_default_0_3_0"
            && rewrite.from.checksum == 17026967434177311328
            && rewrite.to.version == 12
    }));
    assert!(rewrites.iter().any(|rewrite| {
        rewrite.from.version == 12
            && rewrite.from.name == "job_token_budget"
            && rewrite.from.checksum == 13685500183340941819
            && rewrite.to.version == 13
    }));
    assert!(rewrites.iter().any(|rewrite| {
        rewrite.from.version == 13
            && rewrite.from.name == "drop_redundant_wasm_tools_name_index"
            && rewrite.from.checksum == 16100593955252925602
            && rewrite.to.version == 14
    }));
    assert!(rewrites.iter().any(|rewrite| {
        rewrite.from.version == 14
            && rewrite.from.name == "wasm_wit_default_0_3_0"
            && rewrite.from.checksum == 9366402964940367356
            && rewrite.to.version == 12
    }));
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore]
async fn repair_postgres_refinery_history_rewrites_released_rows_in_two_phases() {
    use crate::config::Config;
    use crate::history::Store;

    let _ = dotenvy::dotenv();
    let config = match Config::from_env().await {
        Ok(config) => config,
        Err(_) => return,
    };
    let store = Store::new(&config.database)
        .await
        .expect("Failed to connect to database");
    let mut client = store.conn().await.expect("Failed to get connection");

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

    for (version, name, checksum) in RENUMBERED_RELEASE_WINDOW_ROWS {
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
            .expect("Failed to seed legacy row");
    }

    let applied = RENUMBERED_RELEASE_WINDOW_ROWS
        .iter()
        .map(|(version, name, checksum)| (*version, (*name).to_string(), checksum.to_string()))
        .collect::<Vec<_>>();
    let rewrites =
        plan_migration_history_rewrites(&applied).expect("released migration identities parse");

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
    let staged_rows = staged
        .into_iter()
        .map(|row| {
            let version: i32 = row.get(0);
            let name: String = row.get(1);
            let checksum: String = row.get(2);
            (version, name, checksum)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        staged_rows,
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
    );

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
    let final_rows = final_rows
        .into_iter()
        .map(|row| {
            let version: i32 = row.get(0);
            let name: String = row.get(1);
            let checksum: String = row.get(2);
            (version, name, checksum)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        final_rows,
        CANONICAL_RELEASED_ROWS
            .iter()
            .map(|(version, name, checksum)| (*version, (*name).to_string(), checksum.to_string()))
            .collect::<Vec<_>>()
    );
    transaction
        .rollback()
        .await
        .expect("Failed to rollback staged tx");

    client
        .execute("TRUNCATE refinery_schema_history", &[])
        .await
        .expect("Failed to reset temp history table");
    for (version, name, checksum) in RENUMBERED_RELEASE_WINDOW_ROWS {
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
            .expect("Failed to reseed legacy row");
    }

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
    let repaired_rows = repaired_rows
        .into_iter()
        .map(|row| {
            let version: i32 = row.get(0);
            let name: String = row.get(1);
            let checksum: String = row.get(2);
            (version, name, checksum)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        repaired_rows,
        CANONICAL_RELEASED_ROWS
            .iter()
            .map(|(version, name, checksum)| (*version, (*name).to_string(), checksum.to_string()))
            .collect::<Vec<_>>()
    );
}

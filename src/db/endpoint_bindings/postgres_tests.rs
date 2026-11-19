//! PostgreSQL contract and schema tests for endpoint bindings.

use std::sync::Arc;

use super::PostgresEndpointBindingRepository;
use crate::db::NativeDatabase;
use crate::extensions::setup::{EndpointBindingRepository, binding_contract::run_binding_contract};
use crate::testing::postgres::try_test_pg_db;

#[tokio::test]
async fn postgres_repository_satisfies_binding_contract() {
    let Some(database) = try_test_pg_db().await.expect("Postgres test harness") else {
        return;
    };
    NativeDatabase::run_migrations(&*database)
        .await
        .expect("migrations");
    // Repeating the migration run proves the additive migration is idempotent.
    NativeDatabase::run_migrations(&*database)
        .await
        .expect("repeated migrations");

    let pool = database.pool();
    run_binding_contract(move || {
        Arc::new(PostgresEndpointBindingRepository::new(pool.clone()))
            as Arc<dyn EndpointBindingRepository>
    })
    .await;
}

#[tokio::test]
async fn endpoint_binding_table_has_only_ciphertext_value_columns() {
    let Some(database) = try_test_pg_db().await.expect("Postgres test harness") else {
        return;
    };
    let client = database.pool().get().await.expect("pooled client");

    let rows = client
        .query(
            r#"
            SELECT column_name
            FROM information_schema.columns
            WHERE table_name = 'endpoint_bindings'
            ORDER BY ordinal_position
            "#,
            &[],
        )
        .await
        .expect("column catalogue");

    let columns: Vec<String> = rows.iter().map(|row| row.get(0)).collect();
    assert!(columns.contains(&"encrypted_value".to_string()));
    assert!(columns.contains(&"key_salt".to_string()));
    assert!(columns.contains(&"endpoint_kind".to_string()));
    assert!(
        !columns
            .iter()
            .any(|column| column.contains("url") || column.contains("origin")),
        "no plaintext URL column may exist: {columns:?}"
    );
}

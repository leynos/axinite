//! PostgreSQL storage and gateway contracts for deployment feature flags.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::channels::web::handlers::{features, settings};
use crate::channels::web::test_helpers::TestGatewayBuilder;
use crate::db::postgres::PgBackend;
use crate::history::Store;
use crate::testing::postgres::try_test_pg_db;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn unique_deployment() -> String {
    format!("pg_test_{}", uuid::Uuid::new_v4().simple())
}

async fn remove_overrides(backend: &PgBackend, deployments: &[String]) -> TestResult {
    backend
        .pool()
        .get()
        .await?
        .execute(
            "DELETE FROM feature_flag_overrides WHERE deployment_id = ANY($1)",
            &[&deployments],
        )
        .await?;
    Ok(())
}

#[tokio::test]
async fn postgres_feature_flags_round_trip_upsert_order_and_isolation() -> TestResult {
    let Some(database) = try_test_pg_db().await? else {
        return Ok(());
    };
    let store = Store::from_pool(database.pool());
    store.run_migrations().await?;
    let deployment = unique_deployment();
    let other = unique_deployment();
    assert!(store.list_deployment_flags(&deployment).await?.is_empty());
    store
        .set_deployment_flag(&deployment, "route_jobs", false)
        .await?;
    store
        .set_deployment_flag(&deployment, "route_chat", true)
        .await?;
    store
        .set_deployment_flag(&deployment, "route_jobs", true)
        .await?;
    store
        .set_deployment_flag(&other, "route_jobs", false)
        .await?;
    assert_eq!(
        store.list_deployment_flags(&deployment).await?,
        vec![
            ("route_chat".to_owned(), true),
            ("route_jobs".to_owned(), true)
        ]
    );
    assert_eq!(
        store.list_deployment_flags(&other).await?,
        vec![("route_jobs".to_owned(), false)]
    );
    remove_overrides(&database, &[deployment, other]).await
}

async fn get_flags(
    router: &Router,
    deployment: &str,
) -> Result<BTreeMap<String, bool>, Box<dyn std::error::Error>> {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/features")
                .header("x-deployment-id", deployment)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await?;
    Ok(serde_json::from_slice(&body)?)
}

#[tokio::test]
async fn postgres_gateway_put_get_and_fresh_gateway_preserve_deployment_isolation() -> TestResult {
    let Some(database) = try_test_pg_db().await? else {
        return Ok(());
    };
    // Read rather than mutate the process environment shared by other tests.
    assert!(std::env::var_os("FEATURE_FLAG_ROUTE_MEMORY").is_none());
    let backend = Arc::new(PgBackend::from_pool(database.pool()));
    Store::from_pool(database.pool()).run_migrations().await?;
    let deployment = unique_deployment();
    let other = unique_deployment();
    let make_router = || {
        settings::routes()
            .merge(features::routes())
            .with_state(TestGatewayBuilder::new().store(backend.clone()).build())
    };
    let router = make_router();
    assert_eq!(
        get_flags(&router, &deployment).await?.get("route_memory"),
        Some(&true)
    );
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/settings/feature_flag:route_memory")
                .header("content-type", "application/json")
                .header("x-deployment-id", &deployment)
                .body(Body::from(r#"{"value":false}"#))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        get_flags(&router, &deployment).await?.get("route_memory"),
        Some(&false)
    );
    assert_eq!(
        get_flags(&router, &other).await?.get("route_memory"),
        Some(&true)
    );
    // A fresh registry proves that the SQL adapter persists the command.
    assert_eq!(
        get_flags(&make_router(), &deployment)
            .await?
            .get("route_memory"),
        Some(&false)
    );
    remove_overrides(&database, &[deployment, other]).await
}

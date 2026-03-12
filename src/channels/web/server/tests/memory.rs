#[cfg(feature = "libsql")]
use axum::{Router, body::Body, routing::get, routing::post};
#[cfg(feature = "libsql")]
use rstest::rstest;
#[cfg(feature = "libsql")]
use tower::ServiceExt;

use super::super::*;
use super::fixtures::{TestGatewayStateFactory, test_gateway_state};

#[cfg(feature = "libsql")]
#[rstest]
#[tokio::test]
async fn test_memory_search_results_round_trip_via_read_path(
    test_gateway_state: TestGatewayStateFactory,
) {
    let (db, _temp_dir) = crate::testing::test_db().await;
    let workspace = std::sync::Arc::new(Workspace::new_with_db("test", db));
    workspace
        .write("notes/test.md", "alpha needle beta")
        .await
        .expect("write workspace doc");

    let state = test_gateway_state.build(None, Some(workspace));
    let app = Router::new()
        .route("/api/memory/search", post(memory_search_handler))
        .route("/api/memory/read", get(memory_read_handler))
        .with_state(state);

    let search_req = axum::http::Request::builder()
        .method("POST")
        .uri("/api/memory/search")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"query":"needle","limit":10}"#))
        .expect("request");

    let search_resp = ServiceExt::<axum::http::Request<Body>>::oneshot(app.clone(), search_req)
        .await
        .expect("response");
    assert_eq!(search_resp.status(), StatusCode::OK);

    let search_body = axum::body::to_bytes(search_resp.into_body(), 1024 * 64)
        .await
        .expect("body");
    let search_json: serde_json::Value = serde_json::from_slice(&search_body).expect("search json");
    let result_path = search_json["results"][0]["path"]
        .as_str()
        .expect("search result path");
    assert_eq!(result_path, "notes/test.md");

    let read_req = axum::http::Request::builder()
        .uri(format!(
            "/api/memory/read?path={}",
            urlencoding::encode(result_path)
        ))
        .body(Body::empty())
        .expect("request");

    let read_resp = ServiceExt::<axum::http::Request<Body>>::oneshot(app, read_req)
        .await
        .expect("response");
    assert_eq!(read_resp.status(), StatusCode::OK);

    let read_body = axum::body::to_bytes(read_resp.into_body(), 1024 * 64)
        .await
        .expect("body");
    let read_json: serde_json::Value = serde_json::from_slice(&read_body).expect("read json");
    assert_eq!(read_json["content"], "alpha needle beta");
}

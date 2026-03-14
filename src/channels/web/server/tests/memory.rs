//! Tests for the web gateway memory search and read routes.

#[cfg(feature = "libsql")]
use axum::{Router, body::Body, routing::get, routing::post};
#[cfg(feature = "libsql")]
use rstest::{fixture, rstest};
#[cfg(feature = "libsql")]
use tempfile::TempDir;
#[cfg(feature = "libsql")]
use tower::ServiceExt;

#[cfg(feature = "libsql")]
use super::fixtures::{TestGatewayStateFactory, test_gateway_state};
#[cfg(feature = "libsql")]
use crate::channels::web::handlers::memory::{
    memory_read_handler, memory_search_handler, memory_tree_handler,
};
#[cfg(feature = "libsql")]
use crate::workspace::Workspace;
#[cfg(feature = "libsql")]
use axum::http::StatusCode;

#[cfg(feature = "libsql")]
type TestWorkspaceFixture = (std::sync::Arc<Workspace>, TempDir);

#[cfg(feature = "libsql")]
#[derive(Clone, Copy, Debug, Default)]
struct TestWorkspaceFactory;

#[cfg(feature = "libsql")]
impl TestWorkspaceFactory {
    async fn build(self) -> TestWorkspaceFixture {
        let (db, temp_dir) = crate::testing::test_db().await;
        (
            std::sync::Arc::new(Workspace::new_with_db("test", db)),
            temp_dir,
        )
    }
}

#[cfg(feature = "libsql")]
#[fixture]
fn test_workspace() -> TestWorkspaceFactory {
    TestWorkspaceFactory
}

#[cfg(feature = "libsql")]
#[rstest]
#[tokio::test]
async fn test_memory_search_results_round_trip_via_read_path(
    test_gateway_state: TestGatewayStateFactory,
    test_workspace: TestWorkspaceFactory,
) {
    let (workspace, _temp_dir) = test_workspace.build().await;
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
    let results = search_json["results"]
        .as_array()
        .expect("search results should be an array");
    assert!(
        !results.is_empty(),
        "search should return at least one result: {search_json}"
    );
    let result_path = results[0]["path"].as_str().expect("search result path");
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

#[cfg(feature = "libsql")]
#[rstest]
#[tokio::test]
async fn test_memory_tree_honours_depth_query(
    test_gateway_state: TestGatewayStateFactory,
    test_workspace: TestWorkspaceFactory,
) {
    let (workspace, _temp_dir) = test_workspace.build().await;
    workspace
        .write("notes/deep/test.md", "nested content")
        .await
        .expect("write nested workspace doc");

    let state = test_gateway_state.build(None, Some(workspace));
    let app = Router::new()
        .route("/api/memory/tree", get(memory_tree_handler))
        .with_state(state);

    let req = axum::http::Request::builder()
        .uri("/api/memory/tree?depth=2")
        .body(Body::empty())
        .expect("build memory tree request with depth");

    let resp = ServiceExt::<axum::http::Request<Body>>::oneshot(app, req)
        .await
        .expect("send memory tree request with depth");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 64)
        .await
        .expect("read memory tree response body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("memory tree json");
    let entries = json["entries"].as_array().expect("tree entries array");
    let paths: Vec<&str> = entries
        .iter()
        .map(|entry| entry["path"].as_str().expect("tree path"))
        .collect();

    assert!(paths.contains(&"notes"));
    assert!(paths.contains(&"notes/deep"));
    assert!(
        !paths.contains(&"notes/deep/test.md"),
        "depth-limited tree should omit deeper file entries: {paths:?}"
    );
}

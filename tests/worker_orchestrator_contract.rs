//! Contract tests verifying route-path and HTTP-method symmetry
//! between worker client paths and `OrchestratorApi` routes.

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};

use tokio::sync::Mutex;
use tower::ServiceExt;
use uuid::Uuid;

use ironclaw::llm::{
    CompletionRequest, CompletionResponse, NativeLlmProvider, ToolCompletionRequest,
    ToolCompletionResponse,
};
use ironclaw::orchestrator::api::{OrchestratorApi, OrchestratorState};
use ironclaw::orchestrator::auth::TokenStore;
use ironclaw::orchestrator::job_manager::{ContainerJobConfig, ContainerJobManager};
use ironclaw::tools::ToolRegistry;
use ironclaw::worker::api::{
    COMPLETE_ROUTE, CREDENTIALS_ROUTE, EVENT_ROUTE, JOB_ROUTE, LLM_COMPLETE_ROUTE,
    LLM_COMPLETE_WITH_TOOLS_ROUTE, PROMPT_ROUTE, REMOTE_TOOL_CATALOG_ROUTE,
    REMOTE_TOOL_EXECUTE_ROUTE, STATUS_ROUTE, WORKER_HEALTH_ROUTE, job_scoped_path,
};

// ---------------------------------------------------------------------------
// Minimal stub LLM for integration tests
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct StubLlm;

impl NativeLlmProvider for StubLlm {
    fn model_name(&self) -> &str {
        "stub"
    }

    fn cost_per_token(&self) -> (rust_decimal::Decimal, rust_decimal::Decimal) {
        (rust_decimal::Decimal::ZERO, rust_decimal::Decimal::ZERO)
    }

    async fn complete(
        &self,
        _req: CompletionRequest,
    ) -> Result<CompletionResponse, ironclaw::error::LlmError> {
        Ok(Default::default())
    }

    async fn complete_with_tools(
        &self,
        _req: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, ironclaw::error::LlmError> {
        Ok(Default::default())
    }
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn make_state() -> OrchestratorState {
    let token_store = TokenStore::new();
    let jm = ContainerJobManager::new(ContainerJobConfig::default(), token_store.clone());
    OrchestratorState {
        llm: Arc::new(StubLlm),
        tools: Arc::new(ToolRegistry::new()),
        job_manager: Arc::new(jm),
        token_store,
        job_event_tx: None,
        prompt_queue: Arc::new(Mutex::new(HashMap::new())),
        store: None,
        secrets_store: None,
        user_id: "default".to_string(),
    }
}

// ---------------------------------------------------------------------------
// 1. Route-path alignment
// ---------------------------------------------------------------------------

#[test]
fn worker_paths_match_route_constants() {
    use ironclaw::worker::api::{
        COMPLETE_PATH, CREDENTIALS_PATH, EVENT_PATH, JOB_PATH, LLM_COMPLETE_PATH,
        LLM_COMPLETE_WITH_TOOLS_PATH, PROMPT_PATH, REMOTE_TOOL_CATALOG_PATH,
        REMOTE_TOOL_EXECUTE_PATH, STATUS_PATH,
    };

    let pairs: &[(&str, &str)] = &[
        (JOB_PATH, JOB_ROUTE),
        (STATUS_PATH, STATUS_ROUTE),
        (COMPLETE_PATH, COMPLETE_ROUTE),
        (EVENT_PATH, EVENT_ROUTE),
        (PROMPT_PATH, PROMPT_ROUTE),
        (CREDENTIALS_PATH, CREDENTIALS_ROUTE),
        (LLM_COMPLETE_PATH, LLM_COMPLETE_ROUTE),
        (LLM_COMPLETE_WITH_TOOLS_PATH, LLM_COMPLETE_WITH_TOOLS_ROUTE),
        (REMOTE_TOOL_CATALOG_PATH, REMOTE_TOOL_CATALOG_ROUTE),
        (REMOTE_TOOL_EXECUTE_PATH, REMOTE_TOOL_EXECUTE_ROUTE),
    ];

    for (rel, route) in pairs {
        let job_id = Uuid::new_v4();
        let scoped = job_scoped_path(&job_id.to_string(), rel);
        let expected = route.replace("{job_id}", &job_id.to_string());
        assert_eq!(
            scoped.trim_end_matches('/'),
            expected.trim_end_matches('/'),
            "job_scoped_path for '{}' does not match route '{}'",
            rel,
            route,
        );
    }
}

#[test]
fn worker_job_url_produces_correct_path() {
    use ironclaw::worker::api::worker_job_url;

    let job_id = Uuid::new_v4();
    let url = worker_job_url("http://host:1234", &job_id.to_string(), "status");
    assert_eq!(url, format!("http://host:1234/worker/{}/status", job_id));
}

// ---------------------------------------------------------------------------
// 2. HTTP method correctness
// ---------------------------------------------------------------------------

/// Route-to-verb table built from the imported route constants so it stays in
/// sync with the orchestrator router definition in `src/orchestrator/api.rs`.
const ROUTE_METHOD_TABLE: &[(&str, &str)] = &[
    (WORKER_HEALTH_ROUTE, "GET"),
    (JOB_ROUTE, "GET"),
    (LLM_COMPLETE_ROUTE, "POST"),
    (LLM_COMPLETE_WITH_TOOLS_ROUTE, "POST"),
    (REMOTE_TOOL_CATALOG_ROUTE, "GET"),
    (REMOTE_TOOL_EXECUTE_ROUTE, "POST"),
    (STATUS_ROUTE, "POST"),
    (COMPLETE_ROUTE, "POST"),
    (EVENT_ROUTE, "POST"),
    (PROMPT_ROUTE, "GET"),
    (CREDENTIALS_ROUTE, "GET"),
];

#[tokio::test]
async fn wrong_method_yields_method_not_allowed() {
    let state = make_state();
    let job_id = Uuid::new_v4();
    let token = state.token_store.create_token(job_id).await;
    let router = OrchestratorApi::router(state);

    for &(route, expected) in ROUTE_METHOD_TABLE {
        let wrong = if expected == "GET" { "POST" } else { "GET" };
        let uri = route.replace("{job_id}", &job_id.to_string());
        let mut builder = Request::builder().method(wrong).uri(&uri);
        if route != WORKER_HEALTH_ROUTE {
            builder = builder.header("Authorization", format!("Bearer {}", token));
        }
        let resp = router
            .clone()
            .oneshot(builder.body(Body::empty()).expect("build request"))
            .await
            .expect("send request");
        assert_eq!(
            resp.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "wrong method {} on {} should yield 405",
            wrong,
            route,
        );
    }
}

// ---------------------------------------------------------------------------
// 3. Auth-header convention
// ---------------------------------------------------------------------------

async fn assert_all_authenticated_routes_yield_unauthorized(
    router: axum::Router,
    job_id: Uuid,
    auth_header: Option<String>,
) {
    for &(route, verb) in ROUTE_METHOD_TABLE
        .iter()
        .filter(|(r, _)| *r != WORKER_HEALTH_ROUTE)
    {
        let uri = route.replace("{job_id}", &job_id.to_string());
        let method = Method::from_bytes(verb.as_bytes()).expect("valid HTTP method");
        let mut builder = Request::builder().method(method).uri(&uri);
        if let Some(ref header) = auth_header {
            builder = builder.header("Authorization", header.as_str());
        }
        let resp = router
            .clone()
            .oneshot(builder.body(Body::empty()).expect("build request"))
            .await
            .expect("send request");
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "route {route} with {verb} should yield 401",
        );
    }
}

#[tokio::test]
async fn no_auth_header_yields_unauthorized() {
    let router = OrchestratorApi::router(make_state());
    let job_id = Uuid::new_v4();
    assert_all_authenticated_routes_yield_unauthorized(router, job_id, None).await;
}

#[tokio::test]
async fn wrong_bearer_token_yields_unauthorized() {
    let router = OrchestratorApi::router(make_state());
    let job_id = Uuid::new_v4();
    assert_all_authenticated_routes_yield_unauthorized(
        router,
        job_id,
        Some("Bearer totally-wrong-token".to_string()),
    )
    .await;
}

#[tokio::test]
async fn valid_token_wrong_job_yields_unauthorized() {
    let other_job = Uuid::new_v4();
    let state = make_state();
    let token = state.token_store.create_token(other_job).await;
    let router = OrchestratorApi::router(state);
    let target_job = Uuid::new_v4();
    assert_all_authenticated_routes_yield_unauthorized(
        router,
        target_job,
        Some(format!("Bearer {}", token)),
    )
    .await;
}

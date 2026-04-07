//! Contract tests verifying route-path and HTTP-method symmetry
//! between worker client paths and `OrchestratorApi` routes.

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use rstest::rstest;
use tokio::sync::Mutex;
use tower::ServiceExt;
use uuid::Uuid;

use ironclaw::llm::{
    CompletionRequest, CompletionResponse, FinishReason, NativeLlmProvider, ToolCompletionRequest,
    ToolCompletionResponse,
};
use ironclaw::orchestrator::api::{OrchestratorApi, OrchestratorState};
use ironclaw::orchestrator::auth::TokenStore;
use ironclaw::orchestrator::job_manager::{ContainerJobConfig, ContainerJobManager};
use ironclaw::tools::ToolRegistry;
use ironclaw::worker::api::{
    COMPLETE_PATH, COMPLETE_ROUTE, CREDENTIALS_PATH, CREDENTIALS_ROUTE, CompletionReport,
    CredentialResponse, EVENT_PATH, EVENT_ROUTE, JOB_PATH, JOB_ROUTE, JobDescription,
    JobEventPayload, JobEventType, LLM_COMPLETE_PATH, LLM_COMPLETE_ROUTE,
    LLM_COMPLETE_WITH_TOOLS_PATH, LLM_COMPLETE_WITH_TOOLS_ROUTE, PROMPT_PATH, PROMPT_ROUTE,
    PromptResponse, ProxyCompletionResponse, ProxyFinishReason, ProxyToolCompletionRequest,
    REMOTE_TOOL_CATALOG_PATH, REMOTE_TOOL_CATALOG_ROUTE, REMOTE_TOOL_EXECUTE_PATH,
    REMOTE_TOOL_EXECUTE_ROUTE, RemoteToolCatalogResponse, RemoteToolExecutionRequest, STATUS_PATH,
    STATUS_ROUTE, StatusUpdate, WorkerState, job_scoped_path, worker_job_url,
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
        // These transport types do not expose a canonical Default in the
        // library crate because `finish_reason` has no unambiguous default.
        Ok(CompletionResponse {
            content: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn complete_with_tools(
        &self,
        _req: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, ironclaw::error::LlmError> {
        Ok(ToolCompletionResponse {
            content: None,
            tool_calls: vec![],
            input_tokens: 0,
            output_tokens: 0,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
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
    let job_id = Uuid::new_v4();
    let url = worker_job_url("http://host:1234", &job_id.to_string(), "status");
    assert_eq!(url, format!("http://host:1234/worker/{}/status", job_id));
}

// ---------------------------------------------------------------------------
// 2. HTTP method correctness
// ---------------------------------------------------------------------------

const ROUTE_METHOD_TABLE: &[(&str, &str)] = &[
    ("/health", "GET"),
    ("/worker/{job_id}/job", "GET"),
    ("/worker/{job_id}/llm/complete", "POST"),
    ("/worker/{job_id}/llm/complete_with_tools", "POST"),
    ("/worker/{job_id}/tools/catalog", "GET"),
    ("/worker/{job_id}/tools/execute", "POST"),
    ("/worker/{job_id}/status", "POST"),
    ("/worker/{job_id}/complete", "POST"),
    ("/worker/{job_id}/event", "POST"),
    ("/worker/{job_id}/prompt", "GET"),
    ("/worker/{job_id}/credentials", "GET"),
];

#[rstest]
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
        if route != "/health" {
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

fn authenticated_routes() -> Vec<&'static str> {
    ROUTE_METHOD_TABLE
        .iter()
        .filter(|(r, _)| *r != "/health")
        .map(|(r, _)| *r)
        .collect()
}

async fn assert_all_authenticated_routes_yield_unauthorized(
    router: axum::Router,
    job_id: Uuid,
    auth_header: Option<String>,
) {
    for route in authenticated_routes() {
        let uri = route.replace("{job_id}", &job_id.to_string());
        let mut builder = Request::builder().method("GET").uri(&uri);
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
            "route {route} should yield 401",
        );
    }
}

#[rstest]
#[tokio::test]
async fn no_auth_header_yields_unauthorized() {
    let router = OrchestratorApi::router(make_state());
    let job_id = Uuid::new_v4();
    assert_all_authenticated_routes_yield_unauthorized(router, job_id, None).await;
}

#[rstest]
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

#[rstest]
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

// ---------------------------------------------------------------------------
// 4. JSON shape symmetry
// ---------------------------------------------------------------------------

#[test]
fn status_update_round_trips() {
    let original = StatusUpdate::new(WorkerState::InProgress, Some("working".into()), 42);
    let json = serde_json::to_string(&original).expect("serialize");
    let back: StatusUpdate = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.state, original.state);
    assert_eq!(back.message, original.message);
    assert_eq!(back.iteration, original.iteration);
}

#[test]
fn job_event_payload_round_trips() {
    let original = JobEventPayload {
        event_type: JobEventType::ToolUse,
        data: serde_json::json!({"tool": "bash"}),
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let back: JobEventPayload = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.event_type, original.event_type);
    assert_eq!(back.data, original.data);
}

#[test]
fn completion_report_round_trips() {
    let original = CompletionReport {
        success: true,
        message: Some("done".into()),
        iterations: 10,
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let back: CompletionReport = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.success, original.success);
    assert_eq!(back.message, original.message);
    assert_eq!(back.iterations, original.iterations);
}

#[test]
fn remote_tool_execution_request_round_trips() {
    let original = RemoteToolExecutionRequest {
        tool_name: "my_tool".into(),
        params: serde_json::json!({"key": "value"}),
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let back: RemoteToolExecutionRequest = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, original);
}

#[test]
fn proxy_tool_completion_request_round_trips() {
    let original = ProxyToolCompletionRequest {
        messages: vec![ironclaw::llm::ChatMessage::user("hello")],
        tools: vec![],
        model: None,
        max_tokens: None,
        temperature: None,
        tool_choice: Some("auto".into()),
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let back: ProxyToolCompletionRequest = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.tool_choice, original.tool_choice);
}

#[test]
fn proxy_completion_response_from_fixture() {
    let fixture = serde_json::json!({
        "content": "Hello",
        "input_tokens": 100,
        "output_tokens": 50,
        "finish_reason": "stop",
        "cache_read_input_tokens": 10,
        "cache_creation_input_tokens": 5
    });
    let parsed: ProxyCompletionResponse = serde_json::from_value(fixture).expect("parse");
    assert_eq!(parsed.content, "Hello");
    assert_eq!(parsed.input_tokens, 100);
    assert_eq!(parsed.finish_reason, ProxyFinishReason::Stop);

    let re = serde_json::to_string(&parsed).expect("serialise");
    let back: ProxyCompletionResponse = serde_json::from_str(&re).expect("re-parse");
    assert_eq!(back.content, parsed.content);
    assert_eq!(back.input_tokens, parsed.input_tokens);
}

#[test]
fn job_description_from_fixture() {
    let fixture = serde_json::json!({
        "title": "Test Job",
        "description": "Do something",
        "project_dir": "/tmp/project"
    });
    let parsed: JobDescription = serde_json::from_value(fixture).expect("parse");
    assert_eq!(parsed.title, "Test Job");
    assert_eq!(parsed.description, "Do something");
    assert_eq!(parsed.project_dir.as_deref(), Some("/tmp/project"));

    let re = serde_json::to_string(&parsed).expect("serialise");
    let back: JobDescription = serde_json::from_str(&re).expect("re-parse");
    assert_eq!(back.title, parsed.title);
    assert_eq!(back.description, parsed.description);
}

#[test]
fn remote_tool_catalog_response_from_fixture() {
    let fixture = serde_json::json!({
        "tools": [{"name": "t", "description": "d", "parameters": {"type": "object"}}],
        "toolset_instructions": ["Use bash carefully"],
        "catalog_version": 7
    });
    let parsed: RemoteToolCatalogResponse = serde_json::from_value(fixture).expect("parse");
    assert_eq!(parsed.catalog_version, 7);

    let re = serde_json::to_string(&parsed).expect("serialise");
    let back: RemoteToolCatalogResponse = serde_json::from_str(&re).expect("re-parse");
    assert_eq!(back, parsed);
}

#[test]
fn credential_response_from_fixture() {
    let fixture = serde_json::json!({"env_var": "API_KEY", "value": "secret123"});
    let parsed: CredentialResponse = serde_json::from_value(fixture).expect("parse");
    assert_eq!(parsed.env_var, "API_KEY");
    assert_eq!(parsed.value, "secret123");
}

#[test]
fn prompt_response_from_fixture() {
    let fixture = serde_json::json!({"content": "Continue?", "done": false});
    let parsed: PromptResponse = serde_json::from_value(fixture).expect("parse");
    assert_eq!(parsed.content, "Continue?");
    assert!(!parsed.done);
}

// ---------------------------------------------------------------------------
// 5. ProxyFinishReason aliases
// ---------------------------------------------------------------------------

#[test]
fn finish_reason_tool_calls_alias() {
    let reason: ProxyFinishReason =
        serde_json::from_value(serde_json::json!("tool_calls")).expect("parse");
    assert_eq!(reason, ProxyFinishReason::ToolUse);
}

#[test]
fn finish_reason_unknown_fallback() {
    let reason: ProxyFinishReason =
        serde_json::from_value(serde_json::json!("some_novel_reason")).expect("parse");
    assert_eq!(reason, ProxyFinishReason::Unknown);
}

//! Worker test harness for job module tests.
//!
//! Provides helpers for building workers with various configurations for testing.

use std::sync::Arc;
use std::time::Duration;

use crate::config::SafetyConfig;
use crate::context::{ContextManager, JobState};
use crate::db::Database;
use crate::hooks::HookRegistry;
use crate::llm::{CompletionRequest, CompletionResponse, NativeLlmProvider, ToolCompletionRequest, ToolCompletionResponse};
use crate::safety::SafetyLayer;
use crate::testing::null_db::{CapturingStore, EventCall, StatusCall};
use crate::tools::{ApprovalContext, Tool, ToolRegistry};
use crate::worker::Worker;
use crate::worker::job::WorkerDeps;

/// Stub LLM provider (never called in worker unit tests).
pub struct StubLlm;

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
    ) -> Result<CompletionResponse, crate::error::LlmError> {
        unimplemented!("stub")
    }
    async fn complete_with_tools(
        &self,
        _req: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, crate::error::LlmError> {
        unimplemented!("stub")
    }
}

/// Build a ToolRegistry containing the given tools.
pub async fn build_registry(tools: Vec<Arc<dyn Tool>>) -> ToolRegistry {
    let registry = ToolRegistry::new();
    for tool in tools {
        registry.register(tool).await;
    }
    registry
}

/// Build WorkerDeps with the given components.
pub fn base_deps(
    cm: Arc<ContextManager>,
    tools: Arc<ToolRegistry>,
    store: Option<Arc<dyn Database>>,
    approval_context: Option<ApprovalContext>,
) -> WorkerDeps {
    WorkerDeps {
        context_manager: cm,
        llm: Arc::new(StubLlm),
        safety: Arc::new(SafetyLayer::new(&SafetyConfig {
            max_output_length: 100_000,
            injection_check_enabled: false,
        })),
        tools,
        store,
        hooks: Arc::new(HookRegistry::new()),
        timeout: Duration::from_secs(30),
        use_planning: false,
        sse_tx: None,
        approval_context,
        http_interceptor: None,
    }
}

/// Build a Worker wired to a ToolRegistry containing the given tools.
pub async fn make_worker(tools: Vec<Arc<dyn Tool>>) -> Worker {
    let registry = Arc::new(build_registry(tools).await);
    let cm = Arc::new(ContextManager::new(5));
    let job_id = cm
        .create_job("test", "test job")
        .await
        .expect("failed to create job in ContextManager");
    let deps = base_deps(cm, registry, None, None);

    Worker::new(job_id, deps)
}

/// Build a Worker with a real database store (libsql feature required).
#[cfg(feature = "libsql")]
pub async fn make_worker_with_store(
    tools: Vec<Arc<dyn Tool>>,
) -> (Worker, Arc<dyn Database>, tempfile::TempDir) {
    use crate::db::libsql::LibSqlBackend;
    use tempfile::tempdir;

    let registry = Arc::new(build_registry(tools).await);
    let cm = Arc::new(ContextManager::new(5));
    let job_id = cm
        .create_job("test", "test job")
        .await
        .expect("failed to create job");
    let dir = tempdir().expect("failed to create tempdir");
    let path = dir.path().join("worker-test.db");
    let backend = LibSqlBackend::new_local(&path)
        .await
        .expect("failed to open libsql backend");
    backend
        .run_migrations()
        .await
        .expect("failed to run migrations");
    let store: Arc<dyn Database> = Arc::new(backend);
    let ctx = cm.get_context(job_id).await.expect("failed to get context");
    store.save_job(&ctx).await.expect("failed to save job");
    let deps = base_deps(cm, registry, Some(store.clone()), None);

    (Worker::new(job_id, deps), store, dir)
}

/// Build a Worker with a capturing store for characterisation tests.
pub async fn make_worker_with_capturing_store(
    tools: Vec<Arc<dyn Tool>>,
) -> (Worker, Arc<CapturingStore>) {
    let registry = Arc::new(build_registry(tools).await);
    let cm = Arc::new(ContextManager::new(5));
    let job_id = cm
        .create_job("test", "test job")
        .await
        .expect("failed to create job in ContextManager");

    let store = Arc::new(CapturingStore::new());
    let store_dyn: Arc<dyn Database> = store.clone();
    let deps = base_deps(cm, registry, Some(store_dyn), None);

    (Worker::new(job_id, deps), store)
}

/// Transition a worker's job to InProgress state.
pub async fn transition_to_in_progress(worker: &Worker) {
    worker
        .context_manager()
        .update_context(worker.job_id, |ctx| {
            ctx.transition_to(JobState::InProgress, None)
        })
        .await
        .expect("failed to transition to InProgress")
        .expect("job context should exist for InProgress transition");
}

/// Bundles the expected terminal-state outcome for persistence assertions.
pub struct TerminalPersistenceExpectation<'a> {
    pub state: JobState,
    pub status_str: &'a str,
    pub reason: Option<&'a str>,
}

/// Check captured persistence calls against expected values.
pub fn check_terminal_persistence_calls(
    status_call: &StatusCall,
    event_call: &EventCall,
    expected: &TerminalPersistenceExpectation<'_>,
) {
    assert_eq!(status_call.status, expected.state);
    if let Some(reason) = expected.reason {
        assert_eq!(status_call.reason.as_deref(), Some(reason));
    } else {
        assert!(
            status_call.reason.is_none(),
            "Expected no failure reason, but got {:?}",
            status_call.reason
        );
    }
    assert_eq!(event_call.event_type, "result");
    assert_eq!(event_call.data["status"], expected.status_str);
}

/// Assert terminal persistence state matches expected values.
pub async fn assert_terminal_persistence(
    store: &CapturingStore,
    expected_state: JobState,
    expected_status_str: &str,
    expected_reason: Option<&str>,
) {
    let status_call = store
        .calls()
        .last_status
        .lock()
        .await
        .clone()
        .expect("expected a status update");
    let event_call = store
        .calls()
        .last_event
        .lock()
        .await
        .clone()
        .expect("expected a job event");
    check_terminal_persistence_calls(
        &status_call,
        &event_call,
        &TerminalPersistenceExpectation {
            state: expected_state,
            status_str: expected_status_str,
            reason: expected_reason,
        },
    );
}

/// Assert terminal persistence state with snapshot testing.
pub async fn assert_terminal_persistence_with_snapshot(
    store: &CapturingStore,
    expected_state: JobState,
    expected_status_str: &str,
    expected_reason: Option<&str>,
) {
    let status_call = store
        .calls()
        .last_status
        .lock()
        .await
        .clone()
        .expect("expected a status update");
    let event_call = store
        .calls()
        .last_event
        .lock()
        .await
        .clone()
        .expect("expected a job event");
    check_terminal_persistence_calls(
        &status_call,
        &event_call,
        &TerminalPersistenceExpectation {
            state: expected_state,
            status_str: expected_status_str,
            reason: expected_reason,
        },
    );
    insta::assert_json_snapshot!(
        format!("terminal_persistence_result_{}", expected_status_str),
        &event_call.data
    );
}

/// Methods for driving terminal state transitions in tests.
pub enum TerminalMethod {
    Completed,
    Failed(&'static str),
    Stuck(&'static str),
}

impl TerminalMethod {
    /// Apply this terminal transition to a worker.
    pub async fn apply_transition(&self, worker: &Worker) {
        match self {
            TerminalMethod::Completed => {
                worker
                    .mark_completed()
                    .await
                    .expect("mark_completed should succeed");
            }
            TerminalMethod::Failed(reason) => {
                worker
                    .mark_failed(reason)
                    .await
                    .expect("mark_failed should succeed");
            }
            TerminalMethod::Stuck(reason) => {
                worker
                    .mark_stuck(reason)
                    .await
                    .expect("mark_stuck should succeed");
            }
        }
    }
}

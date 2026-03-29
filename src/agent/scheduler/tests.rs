//! Scheduler tests covering token-budget wiring, cancellation persistence, and
//! approval-gated tool execution with the scheduler's safety, LLM, tool, and
//! optional libSQL-backed dependencies.

use super::*;
use crate::config::SafetyConfig;
#[cfg(feature = "libsql")]
use crate::db::libsql::LibSqlBackend;
use crate::llm::{
    CompletionRequest, CompletionResponse, LlmError, LlmProvider, ToolCompletionRequest,
    ToolCompletionResponse,
};
use crate::safety::SafetyLayer;
use crate::tools::{ApprovalRequirement, NativeTool, ToolError, ToolOutput};
use rust_decimal_macros::dec;

/// Minimal LLM provider stub for scheduler tests that don't exercise LLM calls.
struct StubLlm;

impl crate::llm::NativeLlmProvider for StubLlm {
    fn model_name(&self) -> &str {
        "stub"
    }
    fn cost_per_token(&self) -> (rust_decimal::Decimal, rust_decimal::Decimal) {
        (dec!(0), dec!(0))
    }
    async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        Err(LlmError::RequestFailed {
            provider: "stub".into(),
            reason: "not implemented".into(),
        })
    }
    async fn complete_with_tools(
        &self,
        _req: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        Err(LlmError::RequestFailed {
            provider: "stub".into(),
            reason: "not implemented".into(),
        })
    }
}

fn make_test_config(max_tokens_per_job: u64) -> AgentConfig {
    AgentConfig {
        name: "test".to_string(),
        max_parallel_jobs: 5,
        job_timeout: std::time::Duration::from_secs(30),
        stuck_threshold: std::time::Duration::from_secs(300),
        repair_check_interval: std::time::Duration::from_secs(3600),
        max_repair_attempts: 0,
        use_planning: false,
        session_idle_timeout: std::time::Duration::from_secs(3600),
        allow_local_tools: true,
        max_cost_per_day_cents: None,
        max_actions_per_hour: None,
        max_tool_iterations: 10,
        auto_approve_tools: true,
        default_timezone: "UTC".to_string(),
        max_tokens_per_job,
    }
}

/// Create a Scheduler for token-budget tests. The LLM stub will fail if a
/// worker actually tries to call it, but `dispatch_job` sets the token budget
/// *before* spawning the worker so we can inspect the context immediately
/// after dispatch.
fn make_test_scheduler(max_tokens_per_job: u64) -> Scheduler {
    let config = make_test_config(max_tokens_per_job);
    let cm = Arc::new(ContextManager::new(5));
    let llm: Arc<dyn LlmProvider> = Arc::new(StubLlm);
    let safety = Arc::new(SafetyLayer::new(&SafetyConfig {
        max_output_length: 100_000,
        injection_check_enabled: false,
    }));
    let tools = Arc::new(ToolRegistry::new());
    let hooks = Arc::new(HookRegistry::default());

    Scheduler::new(config, cm, llm, safety, tools, None, hooks)
}

#[cfg(feature = "libsql")]
async fn make_test_scheduler_with_store(
    max_tokens_per_job: u64,
) -> (Scheduler, Arc<dyn Database>, tempfile::TempDir) {
    use tempfile::tempdir;

    let config = make_test_config(max_tokens_per_job);
    let cm = Arc::new(ContextManager::new(5));
    let llm: Arc<dyn LlmProvider> = Arc::new(StubLlm);
    let safety = Arc::new(SafetyLayer::new(&SafetyConfig {
        max_output_length: 100_000,
        injection_check_enabled: false,
    }));
    let tools = Arc::new(ToolRegistry::new());
    let hooks = Arc::new(HookRegistry::default());
    let dir = tempdir().expect("failed to create tempdir");
    let path = dir.path().join("scheduler-test.db");
    let backend = LibSqlBackend::new_local(&path)
        .await
        .expect("failed to open libsql backend");
    backend
        .run_migrations()
        .await
        .expect("failed to run migrations");
    let store: Arc<dyn Database> = Arc::new(backend);

    (
        Scheduler::new(config, cm, llm, safety, tools, Some(store.clone()), hooks),
        store,
        dir,
    )
}

#[cfg(feature = "libsql")]
async fn register_job_in_scheduler(sched: &Scheduler, store: &Arc<dyn Database>, job_id: Uuid) {
    let ctx = sched
        .context_manager
        .get_context(job_id)
        .await
        .expect("failed to get context");
    store
        .save_job(&ctx)
        .await
        .expect("failed to save job to store");

    let (tx, mut rx) = mpsc::channel(1);
    let handle = tokio::spawn(async move {
        let _ = rx.recv().await;
        tokio::time::sleep(Duration::from_secs(60)).await;
    });
    sched
        .jobs
        .write()
        .await
        .insert(job_id, ScheduledJob { handle, tx });
}

#[tokio::test]
async fn test_dispatch_job_caps_user_max_tokens() {
    let sched = make_test_scheduler(1000);
    let meta = serde_json::json!({ "max_tokens": 5000 });
    let job_id = sched
        .dispatch_job("user1", "test", "desc", Some(meta))
        .await
        .expect("dispatch_job failed for caps test");

    let ctx = sched
        .context_manager
        .get_context(job_id)
        .await
        .expect("context_manager.get_context missing for caps test");
    assert_eq!(ctx.max_tokens, 1000, "should cap at configured limit");
}

#[tokio::test]
async fn test_dispatch_job_unlimited_config_preserves_user_tokens() {
    let sched = make_test_scheduler(0); // 0 = unlimited
    let meta = serde_json::json!({ "max_tokens": 5000 });
    let job_id = sched
        .dispatch_job("user1", "test", "desc", Some(meta))
        .await
        .expect("dispatch_job failed for unlimited test");

    let ctx = sched
        .context_manager
        .get_context(job_id)
        .await
        .expect("context_manager.get_context missing for unlimited test");
    assert_eq!(
        ctx.max_tokens, 5000,
        "unlimited config should preserve user value"
    );
}

#[tokio::test]
async fn test_dispatch_job_no_user_tokens_uses_config() {
    let sched = make_test_scheduler(2000);
    let job_id = sched
        .dispatch_job("user1", "test", "desc", None)
        .await
        .expect("dispatch_job failed for default tokens test");

    let ctx = sched
        .context_manager
        .get_context(job_id)
        .await
        .expect("context_manager.get_context missing for default tokens test");
    assert_eq!(
        ctx.max_tokens, 2000,
        "should use config default when no user value"
    );
}

#[tokio::test]
async fn test_dispatch_job_atomic_metadata_and_tokens() {
    let sched = make_test_scheduler(10_000);
    let meta = serde_json::json!({
        "max_tokens": 3000,
        "custom_key": "custom_value"
    });
    let job_id = sched
        .dispatch_job("user1", "test", "desc", Some(meta))
        .await
        .expect("dispatch_job failed for metadata test");

    let ctx = sched
        .context_manager
        .get_context(job_id)
        .await
        .expect("context_manager.get_context missing for metadata test");
    assert_eq!(ctx.max_tokens, 3000, "should use user value within limit");
    assert_eq!(
        ctx.metadata.get("custom_key").and_then(|v| v.as_str()),
        Some("custom_value"),
        "metadata should be set atomically with token budget"
    );
}

#[cfg(feature = "libsql")]
#[tokio::test]
async fn test_stop_persists_cancellation_before_returning() {
    let (sched, store, _dir) = make_test_scheduler_with_store(1000).await;
    let job_id = sched
        .context_manager
        .create_job_for_user("user1", "test", "desc")
        .await
        .expect("failed to create job");
    sched
        .context_manager
        .update_context(job_id, |ctx| ctx.transition_to(JobState::InProgress, None))
        .await
        .expect("failed to update context")
        .expect("failed to transition to in-progress");

    register_job_in_scheduler(&sched, &store, job_id).await;

    sched
        .stop(job_id, "Stopped by scheduler")
        .await
        .expect("failed to stop job");

    let job = store
        .get_job(job_id)
        .await
        .expect("failed to load job")
        .expect("job should exist");
    assert_eq!(job.state, JobState::Cancelled);
}

#[cfg(feature = "libsql")]
#[tokio::test]
async fn test_stop_does_not_overwrite_completed_jobs() {
    let (sched, store, _dir) = make_test_scheduler_with_store(1000).await;
    let job_id = sched
        .context_manager
        .create_job_for_user("user1", "test", "desc")
        .await
        .expect("failed to create job");
    sched
        .context_manager
        .update_context(job_id, |ctx| {
            ctx.transition_to(JobState::InProgress, None)
                .expect("failed to transition to in-progress");
            ctx.transition_to(JobState::Completed, None)
        })
        .await
        .expect("failed to update context")
        .expect("failed to transition to completed");

    register_job_in_scheduler(&sched, &store, job_id).await;

    let error = sched
        .stop(job_id, "Cancelled by user")
        .await
        .expect_err("completed job should reject cancellation");
    assert!(matches!(
        error,
        JobError::InvalidTransition {
            target,
            ..
        } if target == JobState::Cancelled.to_string()
    ));

    let job = store
        .get_job(job_id)
        .await
        .expect("failed to load job")
        .expect("job should exist");
    assert_eq!(job.state, JobState::Completed);
}

#[tokio::test]
async fn test_stop_returns_not_found_for_unknown_job() {
    let sched = make_test_scheduler(1000);
    let job_id = Uuid::new_v4();

    let error = sched
        .stop(job_id, "Stopped by scheduler")
        .await
        .expect_err("unknown job should not stop successfully");
    assert!(matches!(error, JobError::NotFound { id } if id == job_id));
}

struct TestApprovalTool {
    name: &'static str,
    description: &'static str,
    output_text: &'static str,
    approval_requirement: ApprovalRequirement,
}

impl NativeTool for TestApprovalTool {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        self.description
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }
    async fn execute(
        &self,
        _params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::text(
            self.output_text,
            std::time::Instant::now().elapsed(),
        ))
    }
    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        self.approval_requirement
    }
    fn requires_sanitization(&self) -> bool {
        false
    }
}

async fn setup_tools_and_job() -> (
    Arc<ToolRegistry>,
    Arc<ContextManager>,
    Arc<SafetyLayer>,
    Uuid,
) {
    let registry = ToolRegistry::new();
    registry
        .register(Arc::new(TestApprovalTool {
            name: "soft_gate",
            description: "needs soft approval",
            output_text: "soft_ok",
            approval_requirement: ApprovalRequirement::UnlessAutoApproved,
        }))
        .await;
    registry
        .register(Arc::new(TestApprovalTool {
            name: "hard_gate",
            description: "needs hard approval",
            output_text: "hard_ok",
            approval_requirement: ApprovalRequirement::Always,
        }))
        .await;

    let cm = Arc::new(ContextManager::new(5));
    let job_id = cm
        .create_job("test", "approval test")
        .await
        .expect("failed to create test job in setup_tools_and_job");
    cm.update_context(job_id, |ctx| ctx.transition_to(JobState::InProgress, None))
        .await
        .expect("failed to update test job context in setup_tools_and_job")
        .expect("failed to transition test job to JobState::InProgress in setup_tools_and_job");

    let safety = Arc::new(SafetyLayer::new(&SafetyConfig {
        max_output_length: 100_000,
        injection_check_enabled: false,
    }));

    (Arc::new(registry), cm, safety, job_id)
}

#[tokio::test]
async fn test_execute_tool_task_blocks_without_context() {
    let (tools, cm, safety, job_id) = setup_tools_and_job().await;

    // Without approval context, UnlessAutoApproved is blocked
    let result = Scheduler::execute_tool_task(
        tools.clone(),
        cm.clone(),
        safety.clone(),
        None,
        job_id,
        "soft_gate",
        serde_json::json!({}),
    )
    .await;
    assert!(
        result.is_err(),
        "soft_gate should be blocked without context"
    );

    // Always is also blocked
    let result = Scheduler::execute_tool_task(
        tools,
        cm,
        safety,
        None,
        job_id,
        "hard_gate",
        serde_json::json!({}),
    )
    .await;
    assert!(
        result.is_err(),
        "hard_gate should be blocked without context"
    );
}

#[tokio::test]
async fn test_execute_tool_task_autonomous_unblocks_soft() {
    let (tools, cm, safety, job_id) = setup_tools_and_job().await;

    // Autonomous context auto-approves UnlessAutoApproved
    let result = Scheduler::execute_tool_task(
        tools.clone(),
        cm.clone(),
        safety.clone(),
        Some(ApprovalContext::autonomous()),
        job_id,
        "soft_gate",
        serde_json::json!({}),
    )
    .await;
    assert!(
        result.is_ok(),
        "soft_gate should pass with autonomous context"
    );

    // But still blocks Always
    let result = Scheduler::execute_tool_task(
        tools,
        cm,
        safety,
        Some(ApprovalContext::autonomous()),
        job_id,
        "hard_gate",
        serde_json::json!({}),
    )
    .await;
    assert!(
        result.is_err(),
        "hard_gate should still be blocked without explicit permission"
    );
}

#[tokio::test]
async fn test_execute_tool_task_autonomous_with_permissions() {
    let (tools, cm, safety, job_id) = setup_tools_and_job().await;

    // Autonomous context with explicit permission for hard_gate
    let ctx = ApprovalContext::autonomous_with_tools(["hard_gate".to_string()]);

    let result = Scheduler::execute_tool_task(
        tools.clone(),
        cm.clone(),
        safety.clone(),
        Some(ctx.clone()),
        job_id,
        "soft_gate",
        serde_json::json!({}),
    )
    .await;
    assert!(result.is_ok(), "soft_gate should pass");

    let result = Scheduler::execute_tool_task(
        tools,
        cm,
        safety,
        Some(ctx),
        job_id,
        "hard_gate",
        serde_json::json!({}),
    )
    .await;
    assert!(
        result.is_ok(),
        "hard_gate should pass with explicit permission"
    );
}

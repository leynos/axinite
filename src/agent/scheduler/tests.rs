//! Scheduler tests covering token-budget wiring, cancellation persistence, and
//! approval-gated tool execution with the scheduler's safety, LLM, tool, and
//! optional libSQL-backed dependencies.

use super::*;
use crate::config::SafetyConfig;
use crate::llm::{
    CompletionRequest, CompletionResponse, LlmError, LlmProvider, ToolCompletionRequest,
    ToolCompletionResponse,
};
use crate::safety::SafetyLayer;
use rstest::rstest;
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

#[rstest]
#[case(
    1000,
    Some(serde_json::json!({ "max_tokens": 5000 })),
    1000,
    "should cap at configured limit"
)]
#[case(
    0,
    Some(serde_json::json!({ "max_tokens": 5000 })),
    5000,
    "unlimited config should preserve user value"
)]
#[case(2000, None, 2000, "should use config default when no user value")]
#[tokio::test]
async fn test_dispatch_job_token_budget(
    #[case] max_tokens_per_job: u64,
    #[case] meta: Option<serde_json::Value>,
    #[case] expected_max_tokens: u64,
    #[case] msg: &'static str,
) {
    let sched = make_test_scheduler(max_tokens_per_job);
    let job_id = sched
        .dispatch_job("user1", "test", "desc", meta)
        .await
        .expect("dispatch_job should succeed");
    let ctx = sched
        .context_manager
        .get_context(job_id)
        .await
        .expect("get_context should succeed");
    assert_eq!(ctx.max_tokens, expected_max_tokens, "{msg}");
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

#[path = "tests/approval.rs"]
mod approval;
#[path = "tests/persistence.rs"]
mod persistence;

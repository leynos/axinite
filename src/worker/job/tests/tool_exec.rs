//! Tests for tool execution: parallelism, result ordering, and approval.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::context::JobContext;
use crate::llm::ToolSelection;
use crate::testing::worker_harness::*;
use crate::tools::{NativeTool, Tool, ToolError as ToolExecError, ToolOutput};
use crate::worker::job::Worker;

/// A test tool that sleeps for a configurable duration before returning.
struct SlowTool {
    tool_name: String,
    delay: Duration,
    current_active: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
}

impl NativeTool for SlowTool {
    fn name(&self) -> &str {
        &self.tool_name
    }
    fn description(&self) -> &str {
        "Test tool with configurable delay"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }
    async fn execute(
        &self,
        _params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolExecError> {
        let active = self.current_active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(active, Ordering::SeqCst);
        let start = std::time::Instant::now();
        tokio::time::sleep(self.delay).await;
        self.current_active.fetch_sub(1, Ordering::SeqCst);
        Ok(ToolOutput::text(
            format!("done_{}", self.tool_name),
            start.elapsed(),
        ))
    }
    fn requires_sanitization(&self) -> bool {
        false
    }
}

#[test]
fn test_tool_selection_preserves_call_id() {
    let selection = ToolSelection {
        tool_name: "memory_search".to_string(),
        parameters: serde_json::json!({"query": "test"}),
        reasoning: "Need to search memory".to_string(),
        alternatives: vec![],
        tool_call_id: "call_abc123".to_string(),
    };

    assert_eq!(selection.tool_call_id, "call_abc123");
    assert_ne!(
        selection.tool_call_id, "tool_call_id",
        "tool_call_id must not be the hardcoded placeholder string"
    );
}

// Completion detection tests live in src/util.rs (the canonical location).
// See: test_completion_signals, test_completion_negative, etc.

#[tokio::test]
async fn test_parallel_speedup() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let current_active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let tools: Vec<Arc<dyn Tool>> = (0..3)
        .map(|i| {
            Arc::new(SlowTool {
                tool_name: format!("slow_{}", i),
                delay: Duration::from_millis(200),
                current_active: Arc::clone(&current_active),
                max_active: Arc::clone(&max_active),
            }) as Arc<dyn Tool>
        })
        .collect();

    let worker = make_worker(tools).await?;

    let selections: Vec<ToolSelection> = (0..3)
        .map(|i| ToolSelection {
            tool_name: format!("slow_{}", i),
            parameters: serde_json::json!({}),
            reasoning: String::new(),
            alternatives: vec![],
            tool_call_id: format!("call_{}", i),
        })
        .collect();

    let results = worker.execute_tools_parallel(&selections).await;

    assert_eq!(results.len(), 3);
    for r in &results {
        assert!(r.result.is_ok(), "Tool should succeed");
    }
    assert!(
        max_active.load(Ordering::SeqCst) > 1,
        "Expected parallel tool execution to overlap, but max concurrency was {}",
        max_active.load(Ordering::SeqCst)
    );
    Ok(())
}

fn slow_tool(
    name: &str,
    delay_ms: u64,
    current: &Arc<AtomicUsize>,
    max: &Arc<AtomicUsize>,
) -> Arc<dyn Tool> {
    Arc::new(SlowTool {
        tool_name: name.into(),
        delay: Duration::from_millis(delay_ms),
        current_active: Arc::clone(current),
        max_active: Arc::clone(max),
    })
}

fn tool_selection(name: &str, call_id: &str) -> ToolSelection {
    ToolSelection {
        tool_name: name.into(),
        parameters: serde_json::json!({}),
        reasoning: String::new(),
        alternatives: vec![],
        tool_call_id: call_id.into(),
    }
}

#[tokio::test]
async fn test_result_ordering_preserved() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let current_active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));

    let tools: Vec<Arc<dyn Tool>> = vec![
        slow_tool("tool_a", 300, &current_active, &max_active),
        slow_tool("tool_b", 100, &current_active, &max_active),
        slow_tool("tool_c", 200, &current_active, &max_active),
    ];

    let worker = make_worker(tools).await?;

    let selections = vec![
        tool_selection("tool_a", "call_a"),
        tool_selection("tool_b", "call_b"),
        tool_selection("tool_c", "call_c"),
    ];

    let results = worker.execute_tools_parallel(&selections).await;

    for (i, (result, expected)) in results
        .iter()
        .zip(["done_tool_a", "done_tool_b", "done_tool_c"])
        .enumerate()
    {
        let result_str = result
            .result
            .as_ref()
            .map_err(|error| format!("tool {i} should return a captured result: {error}"))?
            .clone();
        assert!(
            result_str.contains(expected),
            "result[{i}] should contain '{expected}'",
        );
    }
    Ok(())
}

#[tokio::test]
async fn test_missing_tool_produces_error_not_panic()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let worker = make_worker(vec![]).await?;

    let selections = vec![ToolSelection {
        tool_name: "nonexistent_tool".into(),
        parameters: serde_json::json!({}),
        reasoning: String::new(),
        alternatives: vec![],
        tool_call_id: "call_x".into(),
    }];

    let results = worker.execute_tools_parallel(&selections).await;
    assert_eq!(results.len(), 1);
    assert!(
        results[0].result.is_err(),
        "Missing tool should produce an error, not a panic"
    );
    Ok(())
}

/// Build a Worker with the given approval context.
async fn make_worker_with_approval(
    tools: Vec<Arc<dyn Tool>>,
    approval_context: Option<crate::tools::ApprovalContext>,
) -> Result<Worker, Box<dyn std::error::Error + Send + Sync>> {
    let registry = Arc::new(build_registry(tools).await);
    let cm = Arc::new(crate::context::ContextManager::new(5));
    let job_id = cm.create_job("test", "test job").await?;
    let deps = base_deps(cm, registry, None, approval_context);

    Ok(Worker::new(job_id, deps))
}

/// A tool that requires approval (UnlessAutoApproved).
struct ApprovalTool;

impl NativeTool for ApprovalTool {
    fn name(&self) -> &str {
        "needs_approval"
    }
    fn description(&self) -> &str {
        "Tool requiring approval"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }
    async fn execute(
        &self,
        _params: serde_json::Value,
        _ctx: &crate::context::JobContext,
    ) -> Result<ToolOutput, crate::tools::ToolError> {
        Ok(ToolOutput::text(
            "approved",
            std::time::Instant::now().elapsed(),
        ))
    }
    fn requires_approval(&self, _params: &serde_json::Value) -> crate::tools::ApprovalRequirement {
        crate::tools::ApprovalRequirement::UnlessAutoApproved
    }
    fn requires_sanitization(&self) -> bool {
        false
    }
}

/// A tool that always requires approval.
struct AlwaysApprovalTool;

impl NativeTool for AlwaysApprovalTool {
    fn name(&self) -> &str {
        "always_approval"
    }
    fn description(&self) -> &str {
        "Tool always requiring approval"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }
    async fn execute(
        &self,
        _params: serde_json::Value,
        _ctx: &crate::context::JobContext,
    ) -> Result<ToolOutput, crate::tools::ToolError> {
        Ok(ToolOutput::text(
            "always",
            std::time::Instant::now().elapsed(),
        ))
    }
    fn requires_approval(&self, _params: &serde_json::Value) -> crate::tools::ApprovalRequirement {
        crate::tools::ApprovalRequirement::Always
    }
    fn requires_sanitization(&self) -> bool {
        false
    }
}

#[tokio::test]
async fn test_approval_context_unblocks_unless_auto_approved()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let worker_blocked = make_worker_with_approval(vec![Arc::new(ApprovalTool)], None).await?;
    let result = worker_blocked
        .execute_tool("needs_approval", &serde_json::json!({}))
        .await;
    assert!(
        result.is_err(),
        "Should be blocked without approval context"
    );

    let worker_allowed = make_worker_with_approval(
        vec![Arc::new(ApprovalTool)],
        Some(crate::tools::ApprovalContext::autonomous()),
    )
    .await?;
    let result = worker_allowed
        .execute_tool("needs_approval", &serde_json::json!({}))
        .await;
    assert!(result.is_ok(), "Should be allowed with autonomous context");
    Ok(())
}

#[tokio::test]
async fn test_approval_context_blocks_always_unless_permitted()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let worker_blocked = make_worker_with_approval(
        vec![Arc::new(AlwaysApprovalTool)],
        Some(crate::tools::ApprovalContext::autonomous()),
    )
    .await?;
    let result = worker_blocked
        .execute_tool("always_approval", &serde_json::json!({}))
        .await;
    assert!(
        result.is_err(),
        "Always tool should be blocked without permission"
    );

    let worker_allowed = make_worker_with_approval(
        vec![Arc::new(AlwaysApprovalTool)],
        Some(crate::tools::ApprovalContext::autonomous_with_tools([
            "always_approval".to_string(),
        ])),
    )
    .await?;
    let result = worker_allowed
        .execute_tool("always_approval", &serde_json::json!({}))
        .await;
    assert!(
        result.is_ok(),
        "Always tool should be allowed with permission"
    );
    Ok(())
}

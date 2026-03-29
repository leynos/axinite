use super::*;
use crate::error::{Error, ToolError as AppToolError};
use crate::tools::{ApprovalRequirement, NativeTool, ToolError, ToolOutput};

struct TestApprovalTool {
    name: &'static str,
    description: &'static str,
    output_text: &'static str,
    approval_requirement: ApprovalRequirement,
}

struct ToolGatingFixture {
    tools: Arc<ToolRegistry>,
    cm: Arc<ContextManager>,
    safety: Arc<SafetyLayer>,
    job_id: Uuid,
}

impl ToolGatingFixture {
    async fn run(
        &self,
        approval_ctx: Option<ApprovalContext>,
        tool_name: &'static str,
    ) -> Result<TaskOutput, Error> {
        Scheduler::execute_tool_task(
            self.tools.clone(),
            self.cm.clone(),
            self.safety.clone(),
            approval_ctx,
            self.job_id,
            tool_name,
            serde_json::json!({}),
        )
        .await
    }
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

async fn setup_tools_and_job() -> ToolGatingFixture {
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

    ToolGatingFixture {
        tools: Arc::new(registry),
        cm,
        safety,
        job_id,
    }
}

fn assert_auth_required(
    result: Result<TaskOutput, Error>,
    tool_name: &'static str,
    msg: &'static str,
) {
    match result.expect_err(msg) {
        Error::Tool(AppToolError::AuthRequired { name }) => assert_eq!(name, tool_name),
        other => panic!("{msg}: unexpected error {other}"),
    }
}

fn assert_executed(
    result: Result<TaskOutput, Error>,
    expected_text: &'static str,
    msg: &'static str,
) {
    let output = result.expect(msg);
    assert_eq!(output.result.as_str(), Some(expected_text), "{msg}");
}

#[tokio::test]
async fn test_execute_tool_task_blocks_without_context() {
    let f = setup_tools_and_job().await;
    assert_auth_required(
        f.run(None, "soft_gate").await,
        "soft_gate",
        "soft_gate should be blocked without context",
    );
    assert_auth_required(
        f.run(None, "hard_gate").await,
        "hard_gate",
        "hard_gate should be blocked without context",
    );
}

#[tokio::test]
async fn test_execute_tool_task_autonomous_unblocks_soft() {
    let f = setup_tools_and_job().await;
    let ctx = ApprovalContext::autonomous();
    assert_executed(
        f.run(Some(ctx.clone()), "soft_gate").await,
        "soft_ok",
        "soft_gate should pass with autonomous context",
    );
    assert_auth_required(
        f.run(Some(ctx), "hard_gate").await,
        "hard_gate",
        "hard_gate should still be blocked without explicit permission",
    );
}

#[tokio::test]
async fn test_execute_tool_task_autonomous_with_permissions() {
    let f = setup_tools_and_job().await;
    let ctx = ApprovalContext::autonomous_with_tools(["hard_gate".to_string()]);

    assert_executed(
        f.run(Some(ctx.clone()), "soft_gate").await,
        "soft_ok",
        "soft_gate should pass",
    );
    assert_executed(
        f.run(Some(ctx), "hard_gate").await,
        "hard_ok",
        "hard_gate should pass with explicit permission",
    );
}

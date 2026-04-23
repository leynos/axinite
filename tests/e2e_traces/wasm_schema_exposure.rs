//! End-to-end coverage for proactive WASM schema advertisement on the first
//! LLM request.

use std::sync::Arc;

use anyhow::Context as _;
use rstest::{fixture, rstest};
use rust_decimal::Decimal;

use crate::fixtures::DEFAULT_TIMEOUT;
use crate::support::test_rig::TestRigBuilder;

use ironclaw::error::LlmError;
use ironclaw::llm::{
    CompletionRequest, CompletionResponse, FinishReason, LlmProvider, NativeLlmProvider,
    ToolCompletionRequest, ToolCompletionResponse, ToolDefinition,
};
use ironclaw::testing::github_wasm_wrapper;
use ironclaw::tools::Tool;

/// A pre-built GitHub WASM tool and its registration-time
/// [`ToolDefinition`], shared across schema-exposure assertions.
#[derive(Clone)]
struct GithubWasmFixture {
    tool: Arc<dyn Tool>,
    definition: ToolDefinition,
}

/// A test-only [`NativeLlmProvider`] that records every
/// [`ToolCompletionRequest`] it receives and returns a deterministic
/// no-op response.
#[derive(Default)]
struct CapturingToolLlm {
    requests: tokio::sync::Mutex<Vec<ToolCompletionRequest>>,
}

impl CapturingToolLlm {
    /// Returns a snapshot of all [`ToolCompletionRequest`]s recorded so far.
    async fn captured_requests(&self) -> Vec<ToolCompletionRequest> {
        self.requests.lock().await.clone()
    }
}

impl NativeLlmProvider for CapturingToolLlm {
    fn model_name(&self) -> &str {
        "capturing-tool-llm"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        Ok(CompletionResponse {
            content: "text fallback".to_string(),
            input_tokens: 1,
            output_tokens: 1,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn complete_with_tools(
        &self,
        request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        self.requests.lock().await.push(request);
        Ok(ToolCompletionResponse {
            content: Some("No tool use needed.".to_string()),
            tool_calls: Vec::new(),
            input_tokens: 1,
            output_tokens: 1,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }
}

/// rstest fixture that builds and returns a [`GithubWasmFixture`] backed
/// by the shared GitHub WASM test artifact.
#[fixture]
async fn github_wasm_fixture() -> anyhow::Result<GithubWasmFixture> {
    let wrapper = github_wasm_wrapper()
        .await
        .context("build shared github WASM wrapper")?;
    let definition = ToolDefinition {
        name: wrapper.name().to_string(),
        description: wrapper.description().to_string(),
        parameters: wrapper.parameters_schema(),
    };

    Ok(GithubWasmFixture {
        tool: Arc::new(wrapper),
        definition,
    })
}

/// rstest fixture that returns a fresh [`CapturingToolLlm`] wrapped in
/// an [`Arc`].
#[fixture]
fn capturing_llm() -> Arc<CapturingToolLlm> {
    Arc::new(CapturingToolLlm::default())
}

#[rstest]
#[tokio::test]
async fn first_llm_request_includes_advertised_schema_for_active_wasm_tool(
    #[future] github_wasm_fixture: anyhow::Result<GithubWasmFixture>,
    capturing_llm: Arc<CapturingToolLlm>,
) -> anyhow::Result<()> {
    let github_wasm_fixture = github_wasm_fixture.await?;
    let llm: Arc<dyn LlmProvider> = capturing_llm.clone();
    let rig = TestRigBuilder::new()
        .with_llm(llm)
        .with_extra_tools(vec![Arc::clone(&github_wasm_fixture.tool)])
        .build()
        .await
        .context("build end-to-end rig with real github WASM tool")?;

    rig.send_message("Review the available tool surface before taking any action.")
        .await;
    let responses = rig.wait_for_responses(1, DEFAULT_TIMEOUT).await;
    assert_eq!(responses.len(), 1, "expected a single assistant response");
    assert!(
        rig.tool_calls_started().is_empty(),
        "capturing LLM should not have triggered tool execution before assertion"
    );

    let captured_requests = capturing_llm.captured_requests().await;
    let first_request = captured_requests
        .first()
        .expect("expected one captured tool-capable LLM request");
    let github = first_request
        .tools
        .iter()
        .find(|tool| tool.name == github_wasm_fixture.definition.name)
        .expect("first request should advertise the active github WASM tool");

    assert_eq!(
        github, &github_wasm_fixture.definition,
        "the first LLM request must carry the same WASM schema advertised at registration time"
    );
    assert_eq!(github.parameters["type"], serde_json::json!("object"));
    assert!(
        github.parameters["required"]
            .as_array()
            .expect("github schema should expose required fields")
            .iter()
            .any(|value| value == "action"),
        "github schema should keep the required action field on the first request"
    );
    assert!(
        github.parameters["properties"]["owner"].is_object(),
        "github schema should keep real guest-defined properties on the first request"
    );
    assert_ne!(
        github.parameters,
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": true
        }),
        "first request must not fall back to the placeholder WASM schema"
    );

    rig.shutdown();
    Ok(())
}

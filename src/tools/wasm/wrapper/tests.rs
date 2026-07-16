//! Unit tests for the WASM tool wrapper: fallback guidance snapshots and
//! themed submodules for credential handling and validation.

mod basics;
mod injection;
mod resolve;
mod validation;

use insta::assert_snapshot;
use rstest::{fixture, rstest};

use super::*;
use crate::testing::github_wasm_wrapper;

#[fixture]
async fn github_wrapper() -> anyhow::Result<WasmToolWrapper> {
    github_wasm_wrapper().await
}

#[rstest]
#[tokio::test]
async fn malformed_first_call_returns_fallback_guidance(
    #[future] github_wrapper: anyhow::Result<WasmToolWrapper>,
) -> anyhow::Result<()> {
    let wrapper = github_wrapper.await?;
    let error = wrapper
        .execute_sync(serde_json::json!({}), None, Vec::new())
        .expect_err("missing required action should fail");

    match error {
        WasmError::ToolReturnedError { message, hint } => {
            assert!(
                !message.trim().is_empty(),
                "tool error should preserve a parameter failure message"
            );
            let message_lower = message.to_lowercase();
            assert!(
                message_lower.contains("parameter")
                    || message_lower.contains("invalid")
                    || message_lower.contains("validation"),
                "tool error should signal parameter validation failure: {message_lower}"
            );
            assert!(hint.contains("Retry using the advertised tool schema"));
            assert!(hint.contains("`github`"));
            assert!(hint.contains("Advertised schema excerpt"));
            assert!(!hint.contains("Tool usage hint"));
            assert_snapshot!(
                "malformed_first_call_tool_returned_error",
                format!("message: {message}\n\nhint:\n{hint}")
            );
        }
        other => panic!("expected ToolReturnedError, got {other:?}"),
    }

    Ok(())
}

#[rstest]
#[tokio::test]
async fn malformed_first_call_uses_wrapper_advertised_schema_in_fallback_guidance(
    #[future] github_wrapper: anyhow::Result<WasmToolWrapper>,
) -> anyhow::Result<()> {
    let wrapper = github_wrapper.await?.with_schema(serde_json::json!({
        "type": "object",
        "properties": {
            "operation": { "type": "string" }
        },
        "required": ["operation"],
        "additionalProperties": false
    }));
    let error = wrapper
        .execute_sync(serde_json::json!({}), None, Vec::new())
        .expect_err("missing required action should fail");

    match error {
        WasmError::ToolReturnedError { hint, .. } => {
            assert!(hint.contains("`github`"));
            assert!(hint.contains("\"operation\""));
            assert!(!hint.contains("\"action\""));
            assert_snapshot!("malformed_first_call_wrapper_advertised_schema_hint", hint);
        }
        other => panic!("expected ToolReturnedError, got {other:?}"),
    }

    Ok(())
}

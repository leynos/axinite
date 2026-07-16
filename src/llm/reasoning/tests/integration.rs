//! StubLlm-backed integration tests for the reasoning flows: complete,
//! respond_with_tools, plan, and evaluate_success (issue #789).

use super::*;

// ---- Issue #789: StubLlm integration tests ----

#[tokio::test]
async fn test_complete_truncates_tool_tags_from_response() {
    use crate::testing::StubLlm;
    let response = "The server has 3 endpoints.\n<tool_call>{\"name\": \"read_file\"}";
    let llm = Arc::new(StubLlm::new(response));
    let reasoning = Reasoning::new(llm);

    let request = CompletionRequest::new(vec![ChatMessage::user("describe the server")]);
    let (result, _usage) = reasoning.complete(request).await.unwrap();
    assert_eq!(result, "The server has 3 endpoints.");
}

#[tokio::test]
async fn test_complete_with_only_tool_tag_returns_empty() {
    use crate::testing::StubLlm;
    let response = "<tool_call>{\"name\": \"search\", \"arguments\": {}}";
    let llm = Arc::new(StubLlm::new(response));
    let reasoning = Reasoning::new(llm);

    let request = CompletionRequest::new(vec![ChatMessage::user("hello")]);
    let (result, _usage) = reasoning.complete(request).await.unwrap();
    assert!(result.trim().is_empty());
}

#[tokio::test]
async fn test_respond_with_tools_force_text_truncates_tool_tags() {
    use crate::testing::StubLlm;
    let response = "Here is my analysis of the code.\n<tool_call>{\"name\": \"read_file\", \"arguments\": {\"path\": \"main.rs\"}}";
    let llm = Arc::new(StubLlm::new(response));
    let reasoning = Reasoning::new(llm);

    let mut context = ReasoningContext::new().with_message(ChatMessage::user("analyse the code"));
    context.force_text = true;

    let output = reasoning.respond_with_tools(&context).await.unwrap();
    match output.result {
        RespondResult::Text(text) => {
            assert_eq!(text, "Here is my analysis of the code.");
        }
        RespondResult::ToolCalls { .. } => {
            panic!("Expected text result in force_text mode");
        }
    }
}

#[tokio::test]
async fn test_respond_with_tools_force_text_only_tag_uses_fallback() {
    use crate::testing::StubLlm;
    let response = "<tool_call>{\"name\": \"search\"}";
    let llm = Arc::new(StubLlm::new(response));
    let reasoning = Reasoning::new(llm);

    let mut context = ReasoningContext::new().with_message(ChatMessage::user("hi"));
    context.force_text = true;

    let output = reasoning.respond_with_tools(&context).await.unwrap();
    match output.result {
        RespondResult::Text(text) => {
            assert_eq!(text, "I'm not sure how to respond to that.");
        }
        RespondResult::ToolCalls { .. } => {
            panic!("Expected fallback text, not tool calls");
        }
    }
}

#[tokio::test]
async fn test_plan_truncates_tool_tags_before_json() {
    use crate::testing::StubLlm;
    let response = r#"<think>Let me plan</think>{"goal": "Test goal", "actions": [{"tool_name": "search", "parameters": {}, "reasoning": "find files", "expected_outcome": "results"}], "confidence": 0.9}
<tool_call>{"name": "search"}"#;
    let llm = Arc::new(StubLlm::new(response));
    let reasoning = Reasoning::new(llm);

    let context = ReasoningContext::new()
        .with_message(ChatMessage::user("plan a search"))
        .with_job("Search for relevant files");

    let plan = reasoning.plan(&context).await.unwrap();
    assert_eq!(plan.goal, "Test goal");
    assert!(!plan.actions.is_empty());
}

// ---- Issue #789: evaluate_success integration test ----

#[tokio::test]
async fn test_evaluate_success_truncates_tool_tags() {
    use crate::testing::StubLlm;
    let response = r#"<think>evaluating</think>{"success": true, "confidence": 0.85, "reasoning": "Task completed", "issues": [], "suggestions": []}
<tool_call>{"name": "verify"}"#;
    let llm = Arc::new(StubLlm::new(response));
    let reasoning = Reasoning::new(llm);

    let context = ReasoningContext::new().with_job("Test task");
    let eval = reasoning
        .evaluate_success(&context, "The job is done")
        .await
        .unwrap();
    assert!(eval.success);
    assert_eq!(eval.confidence, 0.85);
}

// ---- Issue #789: respond_with_tools recovered tool calls path ----

#[tokio::test]
async fn test_respond_with_tools_recovered_tool_calls_preserves_text() {
    use crate::testing::StubLlm;
    // StubLlm returns empty tool_calls + content with XML tool tags.
    // The recovery path should parse the tool call AND preserve text before it.
    let response = "Let me search for that.\n<tool_call>{\"name\": \"tool_list\", \"arguments\": {}}</tool_call>";
    let llm = Arc::new(StubLlm::new(response));
    let reasoning = Reasoning::new(llm);

    let context = ReasoningContext::new()
        .with_message(ChatMessage::user("list tools"))
        .with_tools(vec![ToolDefinition {
            name: "tool_list".to_string(),
            description: "Lists tools".to_string(),
            parameters: serde_json::json!({}),
        }]);

    let output = reasoning.respond_with_tools(&context).await.unwrap();
    match output.result {
        RespondResult::ToolCalls {
            tool_calls,
            content,
        } => {
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0].name, "tool_list");
            // Text before the tag should be preserved
            assert_eq!(content.as_deref(), Some("Let me search for that."));
        }
        RespondResult::Text(_) => {
            panic!("Expected recovered tool calls, got text");
        }
    }
}

#[tokio::test]
async fn test_respond_with_tools_recovered_only_tag_content_is_none() {
    use crate::testing::StubLlm;
    // Content is ONLY a tool call tag — after truncation+cleaning, content should be None
    let response = "<tool_call>{\"name\": \"tool_list\", \"arguments\": {}}</tool_call>";
    let llm = Arc::new(StubLlm::new(response));
    let reasoning = Reasoning::new(llm);

    let context = ReasoningContext::new()
        .with_message(ChatMessage::user("list tools"))
        .with_tools(vec![ToolDefinition {
            name: "tool_list".to_string(),
            description: "Lists tools".to_string(),
            parameters: serde_json::json!({}),
        }]);

    let output = reasoning.respond_with_tools(&context).await.unwrap();
    match output.result {
        RespondResult::ToolCalls {
            tool_calls,
            content,
        } => {
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0].name, "tool_list");
            assert!(
                content.is_none(),
                "Content should be None when only tool tags present"
            );
        }
        RespondResult::Text(_) => {
            panic!("Expected recovered tool calls, got text");
        }
    }
}

// ---- Issue #789: OpenAI reasoning models negative test ----

#[test]
fn test_openai_reasoning_models_not_detected() {
    use crate::llm::reasoning_models::has_native_thinking;
    assert!(!has_native_thinking("o1"));
    assert!(!has_native_thinking("o1-mini"));
    assert!(!has_native_thinking("o1-preview"));
    assert!(!has_native_thinking("o3-mini"));
    assert!(!has_native_thinking("o4-mini"));
}

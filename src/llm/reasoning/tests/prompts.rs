//! Tests for system prompt assembly (tools sections, native-thinking
//! variants) and system-message merging.

use super::*;

// ---- System prompt building tests (issue #565) ----

fn make_test_reasoning() -> Reasoning {
    use crate::testing::StubLlm;
    let llm = Arc::new(StubLlm::new("test"));
    Reasoning::new(llm)
}

#[test]
fn test_system_prompt_with_tools_contains_tools_section() {
    let reasoning = make_test_reasoning();
    let tool_defs = vec![ToolDefinition {
        name: "echo".to_string(),
        description: "Echoes input".to_string(),
        parameters: serde_json::json!({}),
    }];

    let prompt = reasoning.build_system_prompt_with_tools(&tool_defs);
    assert!(
        prompt.contains("## Available Tools"),
        "Prompt with tools should contain Available Tools section"
    );
    assert!(
        prompt.contains("echo: Echoes input"),
        "Prompt with tools should list the echo tool"
    );
}

// ---- merge_system_messages: duplicate system message regression (Bug #597) ----

#[test]
fn test_merge_system_messages_no_system_in_context() {
    let messages = vec![
        ChatMessage::user("Hello"),
        ChatMessage::assistant("Hi there"),
    ];
    let result = merge_system_messages("primary prompt".into(), &messages);
    assert_eq!(result, "primary prompt");
}

#[test]
fn test_merge_system_messages_merges_worker_system() {
    let messages = vec![
        ChatMessage::system("You are an autonomous agent working on a job.\n\nJob: Test Job"),
        ChatMessage::user("Do the thing"),
    ];
    let result = merge_system_messages("planning prompt".into(), &messages);
    assert!(
        result.contains("planning prompt"),
        "must contain the primary prompt"
    );
    assert!(
        result.contains("autonomous agent"),
        "must contain worker system text"
    );
    assert!(
        result.contains("Test Job"),
        "must contain job description from worker system message"
    );
}

#[test]
fn test_merge_system_messages_multiple_system() {
    let messages = vec![
        ChatMessage::system("First system instruction"),
        ChatMessage::system("Second system instruction"),
        ChatMessage::user("Hello"),
    ];
    let result = merge_system_messages("primary".into(), &messages);
    assert!(result.contains("primary"), "must contain primary prompt");
    assert!(
        result.contains("First system instruction"),
        "must contain first system message"
    );
    assert!(
        result.contains("Second system instruction"),
        "must contain second system message"
    );
}

#[test]
fn test_system_prompt_without_tools_omits_tools_section() {
    let reasoning = make_test_reasoning();

    let prompt = reasoning.build_system_prompt_with_tools(&[]);
    assert!(
        !prompt.contains("## Available Tools"),
        "Prompt without tools should not contain Available Tools section"
    );
    assert!(
        !prompt.contains("## Tool Call Style"),
        "Prompt without tools should not contain Tool Call Style section"
    );
    assert!(
        !prompt.contains("Call tools when they would help"),
        "Prompt without tools should not contain tool-calling guidance"
    );
}

#[test]
fn test_system_prompt_with_tools_contains_tool_guidance() {
    let reasoning = make_test_reasoning();
    let tool_defs = vec![ToolDefinition {
        name: "echo".to_string(),
        description: "Echoes input".to_string(),
        parameters: serde_json::json!({}),
    }];

    let prompt = reasoning.build_system_prompt_with_tools(&tool_defs);
    assert!(
        prompt.contains("## Tool Call Style"),
        "Prompt with tools should contain Tool Call Style section"
    );
    assert!(
        prompt.contains("Call tools when they would help"),
        "Prompt with tools should contain tool-calling guidance"
    );
}

#[test]
fn test_system_prompt_is_deterministic() {
    let reasoning = make_test_reasoning();
    let tool_defs = vec![ToolDefinition {
        name: "echo".to_string(),
        description: "Echoes input".to_string(),
        parameters: serde_json::json!({}),
    }];

    let first = reasoning.build_system_prompt_with_tools(&tool_defs);
    let second = reasoning.build_system_prompt_with_tools(&tool_defs);
    assert_eq!(first, second, "System prompt should be deterministic");
}

#[test]
fn test_context_system_prompt_overrides_build() {
    // When system_prompt is set on ReasoningContext, respond_with_tools
    // should use it instead of building from Reasoning state.
    let ctx = ReasoningContext::new().with_system_prompt("custom prompt".to_string());
    assert_eq!(ctx.system_prompt.as_deref(), Some("custom prompt"));
}

// ---- Issue #789: conditional system prompt tests ----

fn make_reasoning_with_model(model: &str) -> Reasoning {
    use crate::testing::StubLlm;
    Reasoning::new(Arc::new(StubLlm::new("test"))).with_model_name(model.to_string())
}

#[test]
fn test_system_prompt_skips_think_final_for_native_thinking() {
    let reasoning = make_reasoning_with_model("qwen3-8b");
    let prompt = reasoning.build_system_prompt_with_tools(&[]);
    assert!(
        !prompt.contains("<think>"),
        "Native thinking model should NOT have <think> in system prompt"
    );
    assert!(prompt.contains("Respond directly with your answer"));
}

#[test]
fn test_system_prompt_includes_think_final_for_regular_model() {
    let reasoning = make_reasoning_with_model("llama-3.1-70b");
    let prompt = reasoning.build_system_prompt_with_tools(&[]);
    assert!(prompt.contains("<think>"));
    assert!(prompt.contains("<final>"));
}

#[test]
fn test_system_prompt_defaults_to_think_final_when_no_model() {
    use crate::testing::StubLlm;
    let reasoning = Reasoning::new(Arc::new(StubLlm::new("test")));
    let prompt = reasoning.build_system_prompt_with_tools(&[]);
    assert!(prompt.contains("<think>"));
    assert!(prompt.contains("<final>"));
}

#[test]
fn test_system_prompt_deepseek_r1_skips_think_final() {
    let reasoning = make_reasoning_with_model("deepseek-r1-distill-qwen-32b");
    let prompt = reasoning.build_system_prompt_with_tools(&[]);
    assert!(!prompt.contains("CRITICAL"));
    assert!(prompt.contains("Respond directly"));
}

// ---- Issue #789: model name propagation test ----

#[tokio::test]
async fn test_with_model_name_affects_system_prompt() {
    use crate::testing::StubLlm;
    // StubLlm model_name is "stub-model" by default, but Reasoning.model_name
    // is what matters for system prompt building.
    let llm = Arc::new(StubLlm::new("test").with_model_name("qwen3-8b"));
    let reasoning = Reasoning::new(llm.clone()).with_model_name("qwen3-8b".to_string());

    let prompt = reasoning.build_system_prompt_with_tools(&[]);
    assert!(
        !prompt.contains("<think>"),
        "Qwen3 model should get native thinking system prompt"
    );
    assert!(prompt.contains("Respond directly"));

    // Now create reasoning WITHOUT with_model_name — should get default prompt
    let reasoning_no_model = Reasoning::new(llm);
    let prompt2 = reasoning_no_model.build_system_prompt_with_tools(&[]);
    assert!(
        prompt2.contains("<think>"),
        "Without model name, should get default think/final prompt"
    );
}

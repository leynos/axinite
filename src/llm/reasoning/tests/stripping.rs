//! Tests for stripping reasoning and tool tags from responses: tag name
//! variants, case/whitespace/attribute tolerance, and unclosed-tag handling.

use super::*;

// ---- Basic thinking tag stripping ----

#[test]
fn test_strip_thinking_tags_basic() {
    let input = "<thinking>Let me think about this...</thinking>Hello, user!";
    assert_eq!(clean_response(input), "Hello, user!");
}

#[test]
fn test_strip_thinking_tags_multiple() {
    let input = "<thinking>First thought</thinking>Hello<thinking>Second thought</thinking> world!";
    assert_eq!(clean_response(input), "Hello world!");
}

#[test]
fn test_strip_thinking_tags_multiline() {
    let input = "<thinking>\nI need to consider:\n1. What the user wants\n2. How to respond\n</thinking>\nHere is my response to your question.";
    assert_eq!(
        clean_response(input),
        "Here is my response to your question."
    );
}

#[test]
fn test_strip_thinking_tags_no_tags() {
    let input = "Just a normal response without thinking tags.";
    assert_eq!(clean_response(input), input);
}

#[test]
fn test_strip_thinking_tags_unclosed() {
    // Strict mode: unclosed tag discards trailing text
    let input = "Hello <thinking>this never closes";
    assert_eq!(clean_response(input), "Hello");
}

// ---- Different tag names ----

#[test]
fn test_strip_think_tags() {
    let input = "<think>Let me reason about this...</think>The answer is 42.";
    assert_eq!(clean_response(input), "The answer is 42.");
}

#[test]
fn test_strip_thought_tags() {
    let input = "<thought>The user wants X.</thought>Sure, here you go.";
    assert_eq!(clean_response(input), "Sure, here you go.");
}

#[test]
fn test_strip_thoughts_tags() {
    let input = "<thoughts>Multiple thoughts...</thoughts>Result.";
    assert_eq!(clean_response(input), "Result.");
}

#[test]
fn test_strip_reasoning_tags() {
    let input = "<reasoning>Analyzing the request...</reasoning>\n\nHere's what I found.";
    assert_eq!(clean_response(input), "Here's what I found.");
}

#[test]
fn test_strip_reflection_tags() {
    let input = "<reflection>Am I answering correctly? Yes.</reflection>The capital is Paris.";
    assert_eq!(clean_response(input), "The capital is Paris.");
}

#[test]
fn test_strip_scratchpad_tags() {
    let input =
        "<scratchpad>Step 1: check memory\nStep 2: respond</scratchpad>\n\nI found the answer.";
    assert_eq!(clean_response(input), "I found the answer.");
}

#[test]
fn test_strip_inner_monologue_tags() {
    let input = "<inner_monologue>Processing query...</inner_monologue>Done!";
    assert_eq!(clean_response(input), "Done!");
}

#[test]
fn test_strip_antthinking_tags() {
    let input = "<antthinking>Claude reasoning here</antthinking>Visible answer.";
    assert_eq!(clean_response(input), "Visible answer.");
}

// ---- Regex flexibility: whitespace, case, attributes ----

#[test]
fn test_whitespace_in_tags() {
    let input = "< think >reasoning</ think >Answer.";
    assert_eq!(clean_response(input), "Answer.");
}

#[test]
fn test_case_insensitive_tags() {
    let input = "<THINKING>Upper case reasoning</THINKING>Visible.";
    assert_eq!(clean_response(input), "Visible.");
}

#[test]
fn test_mixed_case_tags() {
    let input = "<Think>Mixed case</Think>Output.";
    assert_eq!(clean_response(input), "Output.");
}

#[test]
fn test_tags_with_attributes() {
    let input = "<thinking type=\"deep\" level=\"3\">reasoning</thinking>Answer.";
    assert_eq!(clean_response(input), "Answer.");
}

// ---- Tool call tags ----

#[test]
fn test_strip_tool_call_tags() {
    let input = "<tool_call>tool_list</tool_call>";
    assert_eq!(clean_response(input), "");
}

#[test]
fn test_strip_tool_call_with_surrounding_text() {
    let input = "Here is my answer.\n\n<tool_call>\n{\"name\": \"search\", \"arguments\": {}}\n</tool_call>";
    assert_eq!(clean_response(input), "Here is my answer.");
}

#[test]
fn test_strip_function_call_tags() {
    let input = "Response text<function_call>{\"name\": \"foo\"}</function_call>";
    assert_eq!(clean_response(input), "Response text");
}

#[test]
fn test_strip_tool_calls_plural() {
    let input = "<tool_calls>[{\"id\": \"1\"}]</tool_calls>Actual response.";
    assert_eq!(clean_response(input), "Actual response.");
}

#[test]
fn test_strip_xml_tag_with_attributes() {
    let input = "<tool_call type=\"function\">search()</tool_call>Done.";
    assert_eq!(clean_response(input), "Done.");
}

// ---- Pipe-delimited tags ----

#[test]
fn test_strip_pipe_delimited_tags() {
    let input = "<|tool_call|>{\"name\": \"search\"}<|/tool_call|>Hello!";
    assert_eq!(clean_response(input), "Hello!");
}

#[test]
fn test_strip_pipe_delimited_thinking() {
    let input = "<|thinking|>reasoning here<|/thinking|>The answer is 42.";
    assert_eq!(clean_response(input), "The answer is 42.");
}

#[test]
fn test_strip_pipe_delimited_think() {
    let input = "<|think|>reasoning here<|/think|>The answer is 42.";
    assert_eq!(clean_response(input), "The answer is 42.");
}

// ---- Mixed tags ----

#[test]
fn test_strip_multiple_internal_tags() {
    let input = "<thinking>Let me think</thinking>Hello!\n<tool_call>some_tool</tool_call>";
    assert_eq!(clean_response(input), "Hello!");
}

#[test]
fn test_strip_multiple_reasoning_tag_types() {
    let input = "<think>Initial analysis</think>Intermediate.\n<reflection>Double-check</reflection>Final answer.";
    assert_eq!(clean_response(input), "Intermediate.\nFinal answer.");
}

#[test]
fn test_clean_response_preserves_normal_content() {
    let input = "The function tool_call_handler works great. No tags here!";
    assert_eq!(clean_response(input), input);
}

#[test]
fn test_clean_response_thinking_tags_with_trailing_text() {
    let input = "<thinking>Internal thought</thinking>Some text.\n\nHere's the answer.";
    assert_eq!(clean_response(input), "Some text.\n\nHere's the answer.");
}

#[test]
fn test_clean_response_thinking_tags_reasoning_properly_tagged() {
    let input = "<thinking>The user is asking about my name.</thinking>\n\nI'm IronClaw, a secure personal AI assistant.";
    assert_eq!(
        clean_response(input),
        "I'm IronClaw, a secure personal AI assistant."
    );
}

// ---- Unclosed think before final (Bug #564-3) ----

#[test]
fn test_unclosed_think_before_final() {
    assert_eq!(
        clean_response("<think>reasoning no close tag <final>actual answer</final>"),
        "actual answer"
    );
}

#[test]
fn test_unclosed_thinking_before_final() {
    assert_eq!(
        clean_response("<thinking>long reasoning... <final>the real answer</final>"),
        "the real answer"
    );
}

#[test]
fn test_unclosed_think_before_final_with_prefix() {
    assert_eq!(
        clean_response("Hello <think>reasoning <final>world</final>"),
        "Hello world"
    );
}

#[test]
fn test_unclosed_think_no_final_still_discards() {
    assert_eq!(clean_response("Hello <thinking>this never closes"), "Hello");
}

#[test]
fn test_clean_response_strips_bracket_tool_calls() {
    let input = "Let me fetch that.\n[Called tool `http` with arguments: {\"method\":\"GET\",\"url\":\"https://example.com\"}]\nHere are the results.";
    let cleaned = clean_response(input);
    assert!(!cleaned.contains("[Called tool"));
    assert!(cleaned.contains("Let me fetch that."));
    assert!(cleaned.contains("Here are the results."));
}

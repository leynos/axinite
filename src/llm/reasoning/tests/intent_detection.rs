//! Tests for tool-intent detection in LLM response prose, including
//! exclusion phrases and code/quote awareness.

use super::*;

#[test]
fn test_llm_signals_tool_intent_true_positives() {
    assert!(llm_signals_tool_intent("Let me search for that file."));
    assert!(llm_signals_tool_intent("I'll fetch the data now."));
    assert!(llm_signals_tool_intent("I'm going to check the logs."));
    assert!(llm_signals_tool_intent("Let me add it now."));
    assert!(llm_signals_tool_intent("I will run the tests to verify."));
    assert!(llm_signals_tool_intent("I'll look up the documentation."));
    assert!(llm_signals_tool_intent("Let me read the file contents."));
    assert!(llm_signals_tool_intent("I'm going to execute the command."));
}

#[test]
fn test_llm_signals_tool_intent_true_negatives_conversational() {
    assert!(!llm_signals_tool_intent("Let me explain how this works."));
    assert!(!llm_signals_tool_intent(
        "Let me know if you need anything."
    ));
    assert!(!llm_signals_tool_intent("Let me think about this."));
    assert!(!llm_signals_tool_intent("Let me summarize the findings."));
    assert!(!llm_signals_tool_intent("Let me clarify what I mean."));
}

#[test]
fn test_llm_signals_tool_intent_exclusion_takes_precedence() {
    // Exclusion phrase present alongside intent → false
    assert!(!llm_signals_tool_intent(
        "Let me explain the approach, then I'll search for the file."
    ));
}

#[test]
fn test_llm_signals_tool_intent_ignores_code_blocks() {
    let with_code = "Here's the updated code:\n\n```\nfn main() {\n    println!(\"Let me search the database\");\n}\n```";
    assert!(!llm_signals_tool_intent(with_code));
}

#[test]
fn test_llm_signals_tool_intent_ignores_indented_code() {
    let with_indent = "Here's the code:\n\n    println!(\"I'll fetch the data\");\n\nThat's it.";
    assert!(!llm_signals_tool_intent(with_indent));
}

#[test]
fn test_llm_signals_tool_intent_ignores_plain_text() {
    assert!(!llm_signals_tool_intent("The task is complete."));
    assert!(!llm_signals_tool_intent(
        "Here are the results you asked for."
    ));
    assert!(!llm_signals_tool_intent("I found 3 matching files."));
}

#[test]
fn test_llm_signals_tool_intent_quoted_string_in_code_block() {
    let text = "The button text should say:\n```\n\"I will create your account\"\n```";
    assert!(!llm_signals_tool_intent(text));
}

#[test]
fn test_llm_signals_tool_intent_quoted_string_outside_code_block() {
    // Quoted intent phrase in prose should not trigger.
    let text = "The button says \"Let me search the database\" to the user.";
    assert!(!llm_signals_tool_intent(text));
    // But unquoted intent in the same line should still trigger.
    let text = "I'll fetch the results for you.";
    assert!(llm_signals_tool_intent(text));
}

#[test]
fn test_llm_signals_tool_intent_shadowed_prefix() {
    // An earlier non-intent "let me" should not shadow a later real intent.
    let text = "Sure, let me think about it. Actually, let me search for the file.";
    // "let me think" is an exclusion, so this returns false despite the second "let me search".
    assert!(!llm_signals_tool_intent(text));

    // But without an exclusion phrase, multiple prefixes should be checked.
    let text = "I said let me be clear, then let me fetch the data.";
    assert!(llm_signals_tool_intent(text));
}

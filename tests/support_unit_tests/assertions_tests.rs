//! Assertions helper tests.

use crate::support::assertions::*;

#[test]
fn all_tools_succeeded_passes_when_all_true() {
    let completed = vec![("echo".to_string(), true), ("time".to_string(), true)];
    assert_all_tools_succeeded(&completed);
}

#[test]
fn all_tools_succeeded_passes_on_empty() {
    assert_all_tools_succeeded(&[]);
}

#[test]
#[should_panic(expected = "Expected all tools to succeed")]
fn all_tools_succeeded_panics_on_failure() {
    let completed = vec![("echo".to_string(), true), ("shell".to_string(), false)];
    assert_all_tools_succeeded(&completed);
}

#[test]
fn tool_succeeded_passes_when_present_and_true() {
    let completed = vec![("echo".to_string(), true), ("time".to_string(), false)];
    assert_tool_succeeded(&completed, "echo");
}

#[test]
#[should_panic(expected = "Expected 'echo' to complete successfully")]
fn tool_succeeded_panics_when_tool_missing() {
    let completed = vec![("time".to_string(), true)];
    assert_tool_succeeded(&completed, "echo");
}

#[test]
#[should_panic(expected = "Expected 'shell' to complete successfully")]
fn tool_succeeded_panics_when_tool_failed() {
    let completed = vec![("shell".to_string(), false)];
    assert_tool_succeeded(&completed, "shell");
}

#[test]
fn tool_order_passes_for_correct_order() {
    let started: Vec<String> = vec!["write_file", "echo", "read_file"]
        .into_iter()
        .map(String::from)
        .collect();
    assert_tool_order(&started, &["write_file", "read_file"]);
}

#[test]
fn tool_order_passes_for_consecutive() {
    let started: Vec<String> = vec!["write_file", "read_file"]
        .into_iter()
        .map(String::from)
        .collect();
    assert_tool_order(&started, &["write_file", "read_file"]);
}

#[test]
#[should_panic(expected = "assert_tool_order")]
fn tool_order_panics_for_wrong_order() {
    let started: Vec<String> = vec!["read_file", "write_file"]
        .into_iter()
        .map(String::from)
        .collect();
    assert_tool_order(&started, &["write_file", "read_file"]);
}

#[test]
#[should_panic(expected = "assert_tool_order")]
fn tool_order_panics_for_missing_tool() {
    let started: Vec<String> = vec!["echo".to_string()];
    assert_tool_order(&started, &["echo", "write_file"]);
}

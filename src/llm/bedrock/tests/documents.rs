//! Unit tests for JSON ↔ Smithy document conversion and token usage
//! extraction.

use crate::llm::bedrock::documents::{document_to_json, json_to_document};
use crate::llm::bedrock::response::extract_token_usage;

#[test]
fn test_json_to_document_round_trip() {
    let json = serde_json::json!({
        "name": "test",
        "count": 42,
        "negative": -7,
        "ratio": 3.125,
        "active": true,
        "nothing": null,
        "tags": ["a", "b"],
        "nested": {"x": 1}
    });

    let doc = json_to_document(&json);
    let back = document_to_json(&doc);

    assert_eq!(json, back);
}

#[test]
fn test_json_to_document_empty_object() {
    let json = serde_json::json!({});
    let doc = json_to_document(&json);
    let back = document_to_json(&doc);
    assert_eq!(json, back);
}

#[test]
fn test_json_to_document_nested_arrays() {
    let json = serde_json::json!([[1, 2], [3, 4]]);
    let doc = json_to_document(&json);
    let back = document_to_json(&doc);
    assert_eq!(json, back);
}

#[test]
fn test_json_to_document_large_numbers() {
    let json = serde_json::json!({
        "big_pos": u64::MAX,
        "big_neg": i64::MIN,
    });
    let doc = json_to_document(&json);
    let back = document_to_json(&doc);
    assert_eq!(json, back);
}

#[test]
fn test_extract_token_usage_present() {
    let usage = aws_sdk_bedrockruntime::types::TokenUsage::builder()
        .input_tokens(150)
        .output_tokens(42)
        .total_tokens(192)
        .build()
        .unwrap();
    let (input, output) = extract_token_usage(Some(&usage));
    assert_eq!(input, 150);
    assert_eq!(output, 42);
}

#[test]
fn test_extract_token_usage_none() {
    let (input, output) = extract_token_usage(None);
    assert_eq!(input, 0);
    assert_eq!(output, 0);
}

#[test]
fn test_extract_token_usage_negative_clamps_to_zero() {
    // Bedrock uses i32; negative values should not panic
    let usage = aws_sdk_bedrockruntime::types::TokenUsage::builder()
        .input_tokens(-1)
        .output_tokens(-5)
        .total_tokens(0)
        .build()
        .unwrap();
    let (input, output) = extract_token_usage(Some(&usage));
    assert_eq!(input, 0);
    assert_eq!(output, 0);
}

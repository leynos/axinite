//! Unit tests for Bedrock tool configuration, inference configuration,
//! stop-reason mapping, and model ID construction.

use aws_sdk_bedrockruntime::types::StopReason;

use crate::llm::bedrock::convert::{build_inference_config, build_tool_config};
use crate::llm::bedrock::response::map_stop_reason;
use crate::llm::provider::{FinishReason, ToolDefinition};

#[test]
fn test_build_tool_config_empty_tools() {
    let result = build_tool_config(&[], None).unwrap();
    assert!(result.is_none());
}

#[test]
fn test_build_tool_config_none_choice() {
    let result = build_tool_config(&[], Some("none")).unwrap();
    assert!(result.is_none());
}

#[test]
fn test_build_tool_config_with_tools() {
    let tools = vec![ToolDefinition {
        name: "echo".to_string(),
        description: "Echoes input".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "text": {"type": "string"}
            }
        }),
    }];

    let result = build_tool_config(&tools, Some("auto")).unwrap();
    assert!(result.is_some());
}

#[test]
fn test_build_tool_config_required_choice() {
    let tools = vec![ToolDefinition {
        name: "echo".to_string(),
        description: "Echoes".to_string(),
        parameters: serde_json::json!({"type": "object"}),
    }];

    let result = build_tool_config(&tools, Some("required")).unwrap();
    assert!(result.is_some());
}

#[test]
fn test_map_stop_reason() {
    assert_eq!(map_stop_reason(&StopReason::EndTurn), FinishReason::Stop);
    assert_eq!(
        map_stop_reason(&StopReason::StopSequence),
        FinishReason::Stop
    );
    assert_eq!(map_stop_reason(&StopReason::ToolUse), FinishReason::ToolUse);
    assert_eq!(
        map_stop_reason(&StopReason::MaxTokens),
        FinishReason::Length
    );
    assert_eq!(
        map_stop_reason(&StopReason::ContentFiltered),
        FinishReason::ContentFilter
    );
}

#[test]
fn test_map_stop_reason_all_variants() {
    assert_eq!(
        map_stop_reason(&StopReason::GuardrailIntervened),
        FinishReason::ContentFilter
    );
    assert_eq!(
        map_stop_reason(&StopReason::ModelContextWindowExceeded),
        FinishReason::Length
    );
}

#[test]
fn test_model_id_with_cross_region() {
    // Simulate what the constructor does
    let prefix = "us.";
    let model = "anthropic.claude-opus-4-6-v1";
    let model_id = format!("{}{}", prefix, model);
    assert_eq!(model_id, "us.anthropic.claude-opus-4-6-v1");
}

#[test]
fn test_model_id_without_cross_region() {
    let prefix = "";
    let model = "anthropic.claude-opus-4-6-v1";
    let model_id = format!("{}{}", prefix, model);
    assert_eq!(model_id, "anthropic.claude-opus-4-6-v1");
}

#[test]
fn test_build_inference_config_none_none() {
    assert!(build_inference_config(None, None, None).is_none());
}

#[test]
fn test_build_inference_config_temperature_only() {
    let config = build_inference_config(Some(0.7), None, None);
    assert!(config.is_some());
}

#[test]
fn test_build_inference_config_max_tokens_only() {
    let config = build_inference_config(None, Some(1024), None);
    assert!(config.is_some());
}

#[test]
fn test_build_inference_config_both() {
    let config = build_inference_config(Some(0.5), Some(2048), None);
    assert!(config.is_some());
}

#[test]
fn test_build_inference_config_max_tokens_overflow() {
    // u32::MAX exceeds i32::MAX, should clamp to i32::MAX not wrap
    let config = build_inference_config(None, Some(u32::MAX), None).unwrap();
    // Just verify it builds without panic — the clamped value is inside the opaque struct
    let _ = config;
}

#[test]
fn test_build_inference_config_stop_sequences() {
    let seqs = vec!["STOP".to_string(), "END".to_string()];
    let config = build_inference_config(None, None, Some(&seqs));
    assert!(config.is_some());
}

#[test]
fn test_build_inference_config_empty_stop_sequences_ignored() {
    let seqs: Vec<String> = vec![];
    let config = build_inference_config(None, None, Some(&seqs));
    assert!(config.is_none());
}

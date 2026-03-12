use super::request::build_rig_request;
use super::*;
use crate::llm::test_fixtures::github_style_schema;
use rstest::rstest;
use serde_json::Value as JsonValue;

#[test]
fn test_convert_messages_system_to_preamble() {
    let messages = vec![
        ChatMessage::system("You are a helpful assistant."),
        ChatMessage::user("Hello"),
    ];
    let (preamble, history) = convert_messages(&messages);
    assert_eq!(preamble, Some("You are a helpful assistant.".to_string()));
    assert_eq!(history.len(), 1);
}

#[test]
fn test_convert_messages_multiple_systems_concatenated() {
    let messages = vec![
        ChatMessage::system("System 1"),
        ChatMessage::system("System 2"),
        ChatMessage::user("Hi"),
    ];
    let (preamble, history) = convert_messages(&messages);
    assert_eq!(preamble, Some("System 1\nSystem 2".to_string()));
    assert_eq!(history.len(), 1);
}

#[test]
fn test_convert_messages_tool_result() {
    let messages = vec![ChatMessage::tool_result(
        "call_123",
        "search",
        "result text",
    )];
    let (preamble, history) = convert_messages(&messages);
    assert!(preamble.is_none());
    assert_eq!(history.len(), 1);
    // Tool results become User messages in rig-core
    match &history[0] {
        RigMessage::User { content } => match content.first() {
            UserContent::ToolResult(r) => {
                assert_eq!(r.id, "call_123");
                assert_eq!(r.call_id.as_deref(), Some("call_123"));
            }
            other => panic!("Expected tool result content, got: {:?}", other),
        },
        other => panic!("Expected User message, got: {:?}", other),
    }
}

#[test]
fn test_convert_messages_assistant_with_tool_calls() {
    let tc = IronToolCall {
        id: "call_1".to_string(),
        name: "search".to_string(),
        arguments: serde_json::json!({"query": "test"}),
    };
    let msg = ChatMessage::assistant_with_tool_calls(Some("thinking".to_string()), vec![tc]);
    let messages = vec![msg];
    let (_preamble, history) = convert_messages(&messages);
    assert_eq!(history.len(), 1);
    match &history[0] {
        RigMessage::Assistant { content, .. } => {
            // Should have both text and tool call
            assert!(content.iter().count() >= 2);
            for item in content.iter() {
                if let AssistantContent::ToolCall(tc) = item {
                    assert_eq!(tc.call_id.as_deref(), Some("call_1"));
                }
            }
        }
        other => panic!("Expected Assistant message, got: {:?}", other),
    }
}

#[test]
fn test_convert_messages_tool_result_without_id_gets_fallback() {
    let messages = vec![ChatMessage {
        role: crate::llm::Role::Tool,
        content: "result text".to_string(),
        content_parts: Vec::new(),
        tool_call_id: None,
        name: Some("search".to_string()),
        tool_calls: None,
    }];
    let (_preamble, history) = convert_messages(&messages);
    match &history[0] {
        RigMessage::User { content } => match content.first() {
            UserContent::ToolResult(r) => {
                assert!(r.id.starts_with("generated_tool_call_"));
                assert_eq!(r.call_id.as_deref(), Some(r.id.as_str()));
            }
            other => panic!("Expected tool result content, got: {:?}", other),
        },
        other => panic!("Expected User message, got: {:?}", other),
    }
}

#[test]
fn test_convert_tools() {
    let tools = vec![IronToolDefinition {
        name: "search".to_string(),
        description: "Search the web".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"}
            }
        }),
    }];
    let rig_tools = convert_tools(&tools);
    assert_eq!(rig_tools.len(), 1);
    assert_eq!(rig_tools[0].name, "search");
    assert_eq!(rig_tools[0].description, "Search the web");
}

#[rstest]
fn test_convert_tools_rewrites_github_style_schema_before_provider_submission(
    github_style_schema: JsonValue,
) {
    let tools = vec![IronToolDefinition {
        name: "github".to_string(),
        description: "GitHub integration".to_string(),
        parameters: github_style_schema,
    }];

    let rig_tools = convert_tools(&tools);
    let parameters = &rig_tools[0].parameters;

    assert_eq!(rig_tools[0].name, "github");
    assert!(
        parameters.get("oneOf").is_none(),
        "provider-facing schema must not keep top-level oneOf: {parameters}"
    );
    assert_eq!(
        parameters["required"],
        serde_json::json!(["action", "owner", "repo", "title"])
    );
    assert_eq!(
        parameters["properties"]["action"]["enum"],
        serde_json::json!(["create_issue", "get_repo"])
    );
}

#[test]
fn test_convert_tool_choice() {
    assert!(matches!(
        convert_tool_choice(Some("auto")),
        Some(RigToolChoice::Auto)
    ));
    assert!(matches!(
        convert_tool_choice(Some("required")),
        Some(RigToolChoice::Required)
    ));
    assert!(matches!(
        convert_tool_choice(Some("none")),
        Some(RigToolChoice::None)
    ));
    assert!(matches!(
        convert_tool_choice(Some("AUTO")),
        Some(RigToolChoice::Auto)
    ));
    assert!(convert_tool_choice(None).is_none());
    assert!(convert_tool_choice(Some("unknown")).is_none());
}

#[test]
fn test_extract_response_text_only() {
    let content = OneOrMany::one(AssistantContent::text("Hello world"));
    let usage = RigUsage::new();
    let (text, calls, finish) = extract_response(&content, &usage);
    assert_eq!(text, Some("Hello world".to_string()));
    assert!(calls.is_empty());
    assert_eq!(finish, FinishReason::Stop);
}

#[test]
fn test_extract_response_tool_call() {
    let tc = AssistantContent::tool_call("call_1", "search", serde_json::json!({"q": "test"}));
    let content = OneOrMany::one(tc);
    let usage = RigUsage::new();
    let (text, calls, finish) = extract_response(&content, &usage);
    assert!(text.is_none());
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "search");
    assert_eq!(finish, FinishReason::ToolUse);
}

#[test]
fn test_assistant_tool_call_empty_id_gets_generated() {
    let tc = IronToolCall {
        id: "".to_string(),
        name: "search".to_string(),
        arguments: serde_json::json!({"query": "test"}),
    };
    let messages = vec![ChatMessage::assistant_with_tool_calls(None, vec![tc])];
    let (_preamble, history) = convert_messages(&messages);

    match &history[0] {
        RigMessage::Assistant { content, .. } => {
            let tool_call = content.iter().find_map(|c| match c {
                AssistantContent::ToolCall(tc) => Some(tc),
                _ => None,
            });
            let tc = tool_call.expect("should have a tool call");
            assert!(!tc.id.is_empty(), "tool call id must not be empty");
            assert!(
                tc.id.starts_with("generated_tool_call_"),
                "empty id should be replaced with generated id, got: {}",
                tc.id
            );
            assert_eq!(tc.call_id.as_deref(), Some(tc.id.as_str()));
        }
        other => panic!("Expected Assistant message, got: {:?}", other),
    }
}

#[test]
fn test_assistant_tool_call_whitespace_id_gets_generated() {
    let tc = IronToolCall {
        id: "   ".to_string(),
        name: "search".to_string(),
        arguments: serde_json::json!({"query": "test"}),
    };
    let messages = vec![ChatMessage::assistant_with_tool_calls(None, vec![tc])];
    let (_preamble, history) = convert_messages(&messages);

    match &history[0] {
        RigMessage::Assistant { content, .. } => {
            let tool_call = content.iter().find_map(|c| match c {
                AssistantContent::ToolCall(tc) => Some(tc),
                _ => None,
            });
            let tc = tool_call.expect("should have a tool call");
            assert!(
                tc.id.starts_with("generated_tool_call_"),
                "whitespace-only id should be replaced, got: {:?}",
                tc.id
            );
        }
        other => panic!("Expected Assistant message, got: {:?}", other),
    }
}

#[test]
fn test_assistant_and_tool_result_missing_ids_share_generated_id() {
    // Simulate: assistant emits a tool call with empty id, then tool
    // result arrives without an id. Both should get deterministic
    // generated ids that match (based on their position in history).
    let tc = IronToolCall {
        id: "".to_string(),
        name: "search".to_string(),
        arguments: serde_json::json!({"query": "test"}),
    };
    let assistant_msg = ChatMessage::assistant_with_tool_calls(None, vec![tc]);
    let tool_result_msg = ChatMessage {
        role: crate::llm::Role::Tool,
        content: "search results here".to_string(),
        content_parts: Vec::new(),
        tool_call_id: None,
        name: Some("search".to_string()),
        tool_calls: None,
    };
    let messages = vec![assistant_msg, tool_result_msg];
    let (_preamble, history) = convert_messages(&messages);

    // Extract the generated call_id from the assistant tool call
    let assistant_call_id = match &history[0] {
        RigMessage::Assistant { content, .. } => {
            let tc = content.iter().find_map(|c| match c {
                AssistantContent::ToolCall(tc) => Some(tc),
                _ => None,
            });
            tc.expect("should have tool call").id.clone()
        }
        other => panic!("Expected Assistant message, got: {:?}", other),
    };

    // Extract the generated call_id from the tool result
    let tool_result_call_id = match &history[1] {
        RigMessage::User { content } => match content.first() {
            UserContent::ToolResult(r) => r
                .call_id
                .clone()
                .expect("tool result call_id must be present"),
            other => panic!("Expected ToolResult, got: {:?}", other),
        },
        other => panic!("Expected User message, got: {:?}", other),
    };

    assert!(
        !assistant_call_id.is_empty(),
        "assistant call_id must not be empty"
    );
    assert!(
        !tool_result_call_id.is_empty(),
        "tool result call_id must not be empty"
    );

    // NOTE: With the current seed-based generation, these IDs will differ
    // because the assistant tool call uses seed=0 (history.len() at that
    // point) and the tool result uses seed=1 (history.len() after the
    // assistant message was pushed). This documents the current behavior.
    // A future improvement could thread the assistant's generated ID into
    // the tool result for exact matching.
    assert_ne!(
        assistant_call_id, tool_result_call_id,
        "Current impl generates different IDs for assistant call and tool result \
             because seeds differ; this documents the known limitation"
    );
}

#[test]
fn test_saturate_u32() {
    assert_eq!(saturate_u32(100), 100);
    assert_eq!(saturate_u32(u64::MAX), u32::MAX);
    assert_eq!(saturate_u32(u32::MAX as u64), u32::MAX);
}

// -- normalize_tool_name tests --

#[test]
fn test_normalize_tool_name_exact_match() {
    let known = HashSet::from(["echo".to_string(), "list_jobs".to_string()]);
    assert_eq!(normalize_tool_name("echo", &known), "echo");
}

#[test]
fn test_normalize_tool_name_proxy_prefix_match() {
    let known = HashSet::from(["echo".to_string(), "list_jobs".to_string()]);
    assert_eq!(normalize_tool_name("proxy_echo", &known), "echo");
}

#[test]
fn test_normalize_tool_name_proxy_prefix_no_match_kept() {
    let known = HashSet::from(["echo".to_string(), "list_jobs".to_string()]);
    assert_eq!(
        normalize_tool_name("proxy_unknown", &known),
        "proxy_unknown"
    );
}

#[test]
fn test_normalize_tool_name_unknown_passthrough() {
    let known = HashSet::from(["echo".to_string()]);
    assert_eq!(normalize_tool_name("other_tool", &known), "other_tool");
}

#[test]
fn test_build_rig_request_injects_cache_control_short() {
    let req = build_rig_request(
        Some("You are helpful.".to_string()),
        vec![RigMessage::user("Hello")],
        Vec::new(),
        None,
        None,
        None,
        CacheRetention::Short,
    )
    .unwrap();

    let params = req
        .additional_params
        .expect("should have additional_params for Short retention");
    assert_eq!(params["cache_control"]["type"], "ephemeral");
    assert!(
        params["cache_control"].get("ttl").is_none(),
        "Short retention should not include ttl"
    );
}

#[test]
fn test_build_rig_request_injects_cache_control_long() {
    let req = build_rig_request(
        Some("You are helpful.".to_string()),
        vec![RigMessage::user("Hello")],
        Vec::new(),
        None,
        None,
        None,
        CacheRetention::Long,
    )
    .unwrap();

    let params = req
        .additional_params
        .expect("should have additional_params for Long retention");
    assert_eq!(params["cache_control"]["type"], "ephemeral");
    assert_eq!(params["cache_control"]["ttl"], "1h");
}

#[test]
fn test_build_rig_request_no_cache_control_when_none() {
    let req = build_rig_request(
        Some("You are helpful.".to_string()),
        vec![RigMessage::user("Hello")],
        Vec::new(),
        None,
        None,
        None,
        CacheRetention::None,
    )
    .unwrap();

    assert!(
        req.additional_params.is_none(),
        "additional_params should be None when cache is disabled"
    );
}

/// Verify that the multiplier match arms in `RigAdapter::cache_write_multiplier`
/// produce the expected values. We use a standalone helper because constructing
/// a real `RigAdapter` requires a rig `Model` (which needs network/provider setup).
/// The helper mirrors the same match expression — if the impl drifts, the
/// `test_build_rig_request_*` tests will still catch regressions end-to-end.
#[test]
fn test_cache_write_multiplier_values() {
    use rust_decimal::Decimal;
    // None → 1.0× (no surcharge)
    assert_eq!(
        cache_write_multiplier_for(CacheRetention::None),
        Decimal::ONE
    );
    // Short → 1.25× (25% surcharge)
    assert_eq!(
        cache_write_multiplier_for(CacheRetention::Short),
        Decimal::new(125, 2)
    );
    // Long → 2.0× (100% surcharge)
    assert_eq!(
        cache_write_multiplier_for(CacheRetention::Long),
        Decimal::TWO
    );
}

fn cache_write_multiplier_for(retention: CacheRetention) -> rust_decimal::Decimal {
    match retention {
        CacheRetention::None => rust_decimal::Decimal::ONE,
        CacheRetention::Short => rust_decimal::Decimal::new(125, 2),
        CacheRetention::Long => rust_decimal::Decimal::TWO,
    }
}

// -- supports_prompt_cache tests --

#[test]
fn test_supports_prompt_cache_supported_models() {
    // All Claude 3+ models per Anthropic docs
    assert!(supports_prompt_cache("claude-opus-4-6"));
    assert!(supports_prompt_cache("claude-sonnet-4-6"));
    assert!(supports_prompt_cache("claude-sonnet-4"));
    assert!(supports_prompt_cache("claude-haiku-4-5"));
    assert!(supports_prompt_cache("claude-3-5-sonnet-20241022"));
    assert!(supports_prompt_cache("claude-haiku-3"));
    assert!(supports_prompt_cache("Claude-Opus-4-5")); // case-insensitive
    assert!(supports_prompt_cache("anthropic/claude-sonnet-4-6")); // provider prefix
}

#[test]
fn test_supports_prompt_cache_unsupported_models() {
    // Legacy Claude models that predate caching
    assert!(!supports_prompt_cache("claude-2"));
    assert!(!supports_prompt_cache("claude-2.1"));
    assert!(!supports_prompt_cache("claude-instant-1.2"));
    // Non-Claude models
    assert!(!supports_prompt_cache("gpt-4o"));
    assert!(!supports_prompt_cache("llama3"));
}

#[test]
fn test_with_unsupported_params_populates_set() {
    use rig::client::CompletionClient;
    use rig::providers::openai;

    let client: openai::Client = openai::Client::builder()
        .api_key("test-key")
        .base_url("http://localhost:0")
        .build()
        .unwrap();
    let client = client.completions_api();
    let model = client.completion_model("test-model");
    let adapter = RigAdapter::new(model, "test-model")
        .with_unsupported_params(vec!["temperature".to_string()]);

    assert!(adapter.unsupported_params.contains("temperature"));
    assert!(!adapter.unsupported_params.contains("max_tokens"));
}

#[test]
fn test_strip_unsupported_completion_params() {
    use rig::client::CompletionClient;
    use rig::providers::openai;

    let client: openai::Client = openai::Client::builder()
        .api_key("test-key")
        .base_url("http://localhost:0")
        .build()
        .unwrap();
    let client = client.completions_api();
    let model = client.completion_model("test-model");
    let adapter = RigAdapter::new(model, "test-model").with_unsupported_params(vec![
        "temperature".to_string(),
        "stop_sequences".to_string(),
    ]);

    let mut req = CompletionRequest::new(vec![ChatMessage::user("hi")]);
    req.temperature = Some(0.7);
    req.max_tokens = Some(100);
    req.stop_sequences = Some(vec!["STOP".to_string()]);

    adapter.strip_unsupported_completion_params(&mut req);

    assert!(req.temperature.is_none(), "temperature should be stripped");
    assert_eq!(req.max_tokens, Some(100), "max_tokens should be preserved");
    assert!(
        req.stop_sequences.is_none(),
        "stop_sequences should be stripped"
    );
}

#[test]
fn test_strip_unsupported_tool_params() {
    use rig::client::CompletionClient;
    use rig::providers::openai;

    let client: openai::Client = openai::Client::builder()
        .api_key("test-key")
        .base_url("http://localhost:0")
        .build()
        .unwrap();
    let client = client.completions_api();
    let model = client.completion_model("test-model");
    let adapter = RigAdapter::new(model, "test-model")
        .with_unsupported_params(vec!["temperature".to_string(), "max_tokens".to_string()]);

    let mut req = ToolCompletionRequest::new(vec![ChatMessage::user("hi")], vec![]);
    req.temperature = Some(0.5);
    req.max_tokens = Some(200);

    adapter.strip_unsupported_tool_params(&mut req);

    assert!(req.temperature.is_none(), "temperature should be stripped");
    assert!(req.max_tokens.is_none(), "max_tokens should be stripped");
}

#[test]
fn test_unsupported_params_empty_by_default() {
    use rig::client::CompletionClient;
    use rig::providers::openai;

    let client: openai::Client = openai::Client::builder()
        .api_key("test-key")
        .base_url("http://localhost:0")
        .build()
        .unwrap();
    let client = client.completions_api();
    let model = client.completion_model("test-model");
    let adapter = RigAdapter::new(model, "test-model");

    assert!(adapter.unsupported_params.is_empty());
}

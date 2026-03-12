use super::*;

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
    assert_ne!(
        assistant_call_id, tool_result_call_id,
        "Current impl generates different IDs for assistant call and tool result \
             because seeds differ; this documents the known limitation"
    );
}

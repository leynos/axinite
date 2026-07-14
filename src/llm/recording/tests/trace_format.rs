//! Tests for trace-file serialization, flushing, and format compatibility.

use super::super::*;
use crate::testing::StubLlm;

#[tokio::test]
async fn flush_writes_valid_json_with_all_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("trace.json");

    let stub = Arc::new(StubLlm::new("response"));
    let recorder = RecordingLlm::new(stub, path.clone(), "flush-test".to_string());

    // Simulate a memory snapshot
    recorder
        .memory_snapshot
        .lock()
        .await
        .push(MemorySnapshotEntry {
            path: "context/test.md".to_string(),
            content: "test content".to_string(),
        });

    // Simulate an HTTP exchange
    NativeHttpInterceptor::after_response(
        &*recorder.http_interceptor,
        &HttpExchangeRequest {
            method: "GET".to_string(),
            url: "https://api.example.com/data".to_string(),
            headers: Vec::new(),
            body: None,
        },
        &HttpExchangeResponse {
            status: 200,
            headers: Vec::new(),
            body: r#"{"ok": true}"#.to_string(),
        },
    )
    .await;

    let request = CompletionRequest::new(vec![ChatMessage::user("hello")]);
    recorder.complete(request).await.unwrap();
    recorder.flush().await.unwrap();

    let content = tokio::fs::read_to_string(&path).await.unwrap();
    let trace: TraceFile = serde_json::from_str(&content).unwrap();
    assert_eq!(trace.model_name, "flush-test");
    assert_eq!(trace.memory_snapshot.len(), 1);
    assert_eq!(trace.memory_snapshot[0].path, "context/test.md");
    assert_eq!(trace.http_exchanges.len(), 1);
    assert_eq!(trace.http_exchanges[0].response.status, 200);
    assert_eq!(trace.steps.len(), 2);
}

#[test]
fn serde_roundtrip_extended_format() {
    let trace = TraceFile {
        model_name: "test".to_string(),
        memory_snapshot: vec![MemorySnapshotEntry {
            path: "context/vision.md".to_string(),
            content: "Be helpful.".to_string(),
        }],
        http_exchanges: vec![HttpExchange {
            request: HttpExchangeRequest {
                method: "GET".to_string(),
                url: "https://api.example.com".to_string(),
                headers: vec![("Accept".to_string(), "application/json".to_string())],
                body: None,
            },
            response: HttpExchangeResponse {
                status: 200,
                headers: Vec::new(),
                body: "{}".to_string(),
            },
        }],
        steps: vec![
            TraceStep {
                request_hint: None,
                response: TraceResponse::UserInput {
                    content: "hello".to_string(),
                },
                expected_tool_results: Vec::new(),
            },
            TraceStep {
                request_hint: Some(RequestHint {
                    last_user_message_contains: Some("hello".to_string()),
                    min_message_count: Some(2),
                }),
                response: TraceResponse::ToolCalls {
                    tool_calls: vec![TraceToolCall {
                        id: "call_1".to_string(),
                        name: "echo".to_string(),
                        arguments: serde_json::json!({"message": "hi"}),
                    }],
                    input_tokens: 50,
                    output_tokens: 20,
                },
                expected_tool_results: Vec::new(),
            },
            TraceStep {
                request_hint: None,
                response: TraceResponse::Text {
                    content: "done".to_string(),
                    input_tokens: 80,
                    output_tokens: 10,
                },
                expected_tool_results: vec![ExpectedToolResult {
                    tool_call_id: "call_1".to_string(),
                    name: "echo".to_string(),
                    content: "hi".to_string(),
                }],
            },
        ],
    };

    let json = serde_json::to_string_pretty(&trace).unwrap();
    let parsed: TraceFile = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.model_name, "test");
    assert_eq!(parsed.memory_snapshot.len(), 1);
    assert_eq!(parsed.http_exchanges.len(), 1);
    assert_eq!(parsed.steps.len(), 3);
    assert_eq!(parsed.steps[2].expected_tool_results.len(), 1);
}

#[test]
fn backward_compatible_with_old_format() {
    // Old format without memory_snapshot, http_exchanges, expected_tool_results
    let json = r#"{
        "model_name": "old-trace",
        "steps": [
            {
                "response": {
                    "type": "text",
                    "content": "hello",
                    "input_tokens": 10,
                    "output_tokens": 5
                }
            }
        ]
    }"#;
    let trace: TraceFile = serde_json::from_str(json).unwrap();
    assert_eq!(trace.model_name, "old-trace");
    assert!(trace.memory_snapshot.is_empty());
    assert!(trace.http_exchanges.is_empty());
    assert!(trace.steps[0].expected_tool_results.is_empty());
}

use rstest::rstest;

use super::*;

#[rstest]
#[case(
    r#"{"ok":true,"items":[1,2]}"#,
    serde_json::json!({"ok": true, "items": [1, 2]})
)]
#[case("plain text", serde_json::Value::String("plain text".to_string()))]
#[case("[1,2,3]", serde_json::json!([1, 2, 3]))]
#[case("{bad", serde_json::Value::String("{bad".to_string()))]
#[case("[bad", serde_json::Value::String("[bad".to_string()))]
#[case("   {\"ok\":true}", serde_json::json!({"ok": true}))]
fn record_tool_result_content_cases(
    #[case] raw_content: &str,
    #[case] expected: serde_json::Value,
) {
    let mut turn = Turn::new(1, "input");
    turn.record_tool_call("json", serde_json::json!({}));
    turn.record_tool_result_content(raw_content);

    assert_eq!(turn.tool_calls[0].result, Some(expected));
}

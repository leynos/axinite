use super::*;

fn assert_default_completion_response(r: &CompletionResponse) {
    assert!(
        r.content.is_empty()
            && r.input_tokens == 0
            && r.output_tokens == 0
            && r.finish_reason == FinishReason::Stop
            && r.cache_read_input_tokens == 0
            && r.cache_creation_input_tokens == 0,
        "default CompletionResponse mismatch: content={:?}, in={}, out={}, finish_reason={:?}, cache_read={}, cache_create={}",
        r.content,
        r.input_tokens,
        r.output_tokens,
        r.finish_reason,
        r.cache_read_input_tokens,
        r.cache_creation_input_tokens
    );
}

fn assert_default_tool_completion_response(r: &ToolCompletionResponse) {
    assert!(
        r.content.is_none()
            && r.tool_calls.is_empty()
            && r.input_tokens == 0
            && r.output_tokens == 0
            && r.finish_reason == FinishReason::Stop
            && r.cache_read_input_tokens == 0
            && r.cache_creation_input_tokens == 0,
        "default ToolCompletionResponse mismatch: content={:?}, tool_calls_len={}, in={}, out={}, finish_reason={:?}, cache_read={}, cache_create={}",
        r.content,
        r.tool_calls.len(),
        r.input_tokens,
        r.output_tokens,
        r.finish_reason,
        r.cache_read_input_tokens,
        r.cache_creation_input_tokens
    );
}

fn assert_finish_reason_is_stop(fr: FinishReason) {
    assert!(
        fr == FinishReason::Stop,
        "FinishReason::default() should be Stop, got: {:?}",
        fr
    );
}

#[test]
fn default_finish_reason_is_stop() {
    assert_finish_reason_is_stop(FinishReason::default());
}

#[test]
fn default_completion_response_matches_contract() {
    assert_default_completion_response(&CompletionResponse::default());
}

#[test]
fn default_tool_completion_response_matches_contract() {
    assert_default_tool_completion_response(&ToolCompletionResponse::default());
}

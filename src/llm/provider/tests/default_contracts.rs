use super::*;

macro_rules! assert_llm_defaults {
    ($resp:expr, content_ok = $content_ok:expr, tool_calls_len = $tc_len:expr) => {{
        let r = &$resp;
        assert!(
            $content_ok
                && $tc_len == 0
                && r.input_tokens == 0
                && r.output_tokens == 0
                && r.finish_reason == FinishReason::Stop
                && r.cache_read_input_tokens == 0
                && r.cache_creation_input_tokens == 0,
            "default {} mismatch: content_ok={}, tool_calls_len={}, in={}, out={}, finish_reason={:?}, cache_read={}, cache_create={}",
            std::any::type_name_of_val(r),
            $content_ok,
            $tc_len,
            r.input_tokens,
            r.output_tokens,
            r.finish_reason,
            r.cache_read_input_tokens,
            r.cache_creation_input_tokens
        );
    }};
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
    let r = CompletionResponse::default();
    assert_llm_defaults!(r, content_ok = r.content.is_empty(), tool_calls_len = 0);
}

#[test]
fn default_tool_completion_response_matches_contract() {
    let r = ToolCompletionResponse::default();
    assert_llm_defaults!(
        r,
        content_ok = r.content.is_none(),
        tool_calls_len = r.tool_calls.len()
    );
}

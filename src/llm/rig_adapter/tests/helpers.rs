//! Tests for rig adapter helper functions and cache-related utilities.

use super::*;

#[test]
fn test_saturate_u32() {
    assert_eq!(saturate_u32(100), 100);
    assert_eq!(saturate_u32(u64::MAX), u32::MAX);
    assert_eq!(saturate_u32(u32::MAX as u64), u32::MAX);
}

#[rstest]
#[case("echo", "echo")]
#[case("proxy_echo", "echo")]
#[case("proxy_unknown", "proxy_unknown")]
#[case("other_tool", "other_tool")]
fn test_normalize_tool_name_cases(#[case] raw: &str, #[case] expected: &str) {
    let known = HashSet::from(["echo".to_string(), "list_jobs".to_string()]);
    assert_eq!(normalize_tool_name(raw, &known), expected);
}

#[rstest]
#[case("claude-opus-4-6", true)]
#[case("claude-sonnet-4-6", true)]
#[case("claude-sonnet-4", true)]
#[case("claude-haiku-4-5", true)]
#[case("claude-3-5-sonnet-20241022", true)]
#[case("claude-haiku-3", true)]
#[case("Claude-Opus-4-5", true)]
#[case("anthropic/claude-sonnet-4-6", true)]
#[case("claude-2", false)]
#[case("claude-2.1", false)]
#[case("claude-instant-1.2", false)]
#[case("gpt-4o", false)]
#[case("llama3", false)]
fn test_supports_prompt_cache_cases(#[case] model: &str, #[case] expected: bool) {
    assert_eq!(supports_prompt_cache(model), expected);
}

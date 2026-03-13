//! Tests for rig adapter helper functions and cache-related utilities.

use super::*;

#[test]
fn test_saturate_u32() {
    assert_eq!(saturate_u32(100), 100);
    assert_eq!(saturate_u32(u64::MAX), u32::MAX);
    assert_eq!(saturate_u32(u32::MAX as u64), u32::MAX);
}

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
fn test_cache_write_multiplier_values() {
    use rust_decimal::Decimal;

    assert_eq!(
        cache_write_multiplier_for(CacheRetention::None),
        Decimal::ONE
    );
    assert_eq!(
        cache_write_multiplier_for(CacheRetention::Short),
        Decimal::new(125, 2)
    );
    assert_eq!(
        cache_write_multiplier_for(CacheRetention::Long),
        Decimal::TWO
    );
}

#[test]
fn test_supports_prompt_cache_supported_models() {
    assert!(supports_prompt_cache("claude-opus-4-6"));
    assert!(supports_prompt_cache("claude-sonnet-4-6"));
    assert!(supports_prompt_cache("claude-sonnet-4"));
    assert!(supports_prompt_cache("claude-haiku-4-5"));
    assert!(supports_prompt_cache("claude-3-5-sonnet-20241022"));
    assert!(supports_prompt_cache("claude-haiku-3"));
    assert!(supports_prompt_cache("Claude-Opus-4-5"));
    assert!(supports_prompt_cache("anthropic/claude-sonnet-4-6"));
}

#[test]
fn test_supports_prompt_cache_unsupported_models() {
    assert!(!supports_prompt_cache("claude-2"));
    assert!(!supports_prompt_cache("claude-2.1"));
    assert!(!supports_prompt_cache("claude-instant-1.2"));
    assert!(!supports_prompt_cache("gpt-4o"));
    assert!(!supports_prompt_cache("llama3"));
}

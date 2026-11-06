//! Unit tests for per-user tool rate limiting windows.

use super::*;
use crate::tools::tool::ToolRateLimitConfig;

#[tokio::test]
async fn test_allowed_within_limits() {
    let limiter = RateLimiter::new();
    let config = ToolRateLimitConfig::new(10, 100);

    let result = limiter.check_and_record("user1", "shell", &config).await;

    match result {
        RateLimitResult::Allowed {
            remaining_minute,
            remaining_hour,
        } => {
            assert_eq!(remaining_minute, 9);
            assert_eq!(remaining_hour, 99);
        }
        _ => panic!("Expected allowed"),
    }
}

#[tokio::test]
async fn test_minute_limit_exceeded() {
    let limiter = RateLimiter::new();
    let config = ToolRateLimitConfig::new(2, 100);

    // Use up the minute limit
    limiter.check_and_record("user1", "shell", &config).await;
    limiter.check_and_record("user1", "shell", &config).await;

    // Third request should be limited
    let result = limiter.check_and_record("user1", "shell", &config).await;

    match result {
        RateLimitResult::Limited {
            limit_type,
            retry_after,
        } => {
            assert_eq!(limit_type, LimitType::PerMinute);
            assert!(retry_after.as_secs() <= 60);
        }
        _ => panic!("Expected limited"),
    }
}

#[tokio::test]
async fn test_hour_limit_exceeded() {
    let limiter = RateLimiter::new();
    let config = ToolRateLimitConfig::new(100, 2);

    // Use up the hour limit
    limiter.check_and_record("user1", "shell", &config).await;
    limiter.check_and_record("user1", "shell", &config).await;

    // Third request should be limited
    let result = limiter.check_and_record("user1", "shell", &config).await;

    match result {
        RateLimitResult::Limited { limit_type, .. } => {
            assert_eq!(limit_type, LimitType::PerHour);
        }
        _ => panic!("Expected limited"),
    }
}

#[tokio::test]
async fn test_user_isolation() {
    let limiter = RateLimiter::new();
    let config = ToolRateLimitConfig::new(1, 10);

    // User1 uses their limit
    limiter.check_and_record("user1", "shell", &config).await;
    let result1 = limiter.check_and_record("user1", "shell", &config).await;

    // User2 should still have their limit
    let result2 = limiter.check_and_record("user2", "shell", &config).await;

    assert!(!result1.is_allowed());
    assert!(result2.is_allowed());
}

#[tokio::test]
async fn test_tool_isolation() {
    let limiter = RateLimiter::new();
    let config = ToolRateLimitConfig::new(1, 10);

    // shell uses its limit
    limiter.check_and_record("user1", "shell", &config).await;
    let result1 = limiter.check_and_record("user1", "shell", &config).await;

    // http should still have its limit
    let result2 = limiter.check_and_record("user1", "http", &config).await;

    assert!(!result1.is_allowed());
    assert!(result2.is_allowed());
}

#[tokio::test]
async fn test_get_usage() {
    let limiter = RateLimiter::new();
    let config = ToolRateLimitConfig::new(30, 300);

    limiter.check_and_record("user1", "shell", &config).await;
    limiter.check_and_record("user1", "shell", &config).await;
    limiter.check_and_record("user1", "shell", &config).await;

    let usage = limiter.get_usage("user1", "shell").await;
    assert_eq!(usage, Some((3, 3)));
}

#[tokio::test]
async fn test_clear() {
    let limiter = RateLimiter::new();
    let config = ToolRateLimitConfig::new(1, 10);

    limiter.check_and_record("user1", "shell", &config).await;
    let result1 = limiter.check_and_record("user1", "shell", &config).await;
    assert!(!result1.is_allowed());

    limiter.clear("user1", "shell").await;

    let result2 = limiter.check_and_record("user1", "shell", &config).await;
    assert!(result2.is_allowed());
}

#[tokio::test]
async fn test_read_only_tools_have_no_config() {
    // Read-only tools return None from rate_limit_config() —
    // verified in the individual tool tests, but assert the config
    // type we'd use for write tools has sensible defaults here.
    let write_config = ToolRateLimitConfig::new(20, 200);
    assert_eq!(write_config.requests_per_minute, 20);
    assert_eq!(write_config.requests_per_hour, 200);
}

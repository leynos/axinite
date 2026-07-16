//! Tests for approval requirements and approval-context blocking.

use super::super::*;
use super::EchoTool;

#[test]
fn test_requires_approval_default() {
    let tool = EchoTool;
    assert_eq!(
        NativeTool::requires_approval(&tool, &serde_json::json!({"message": "hi"})),
        ApprovalRequirement::Never
    );
    assert_eq!(
        NativeTool::hosted_tool_eligibility(&tool),
        HostedToolEligibility::Eligible
    );
    assert!(!ApprovalRequirement::Never.is_required());
    assert!(ApprovalRequirement::UnlessAutoApproved.is_required());
    assert!(ApprovalRequirement::Always.is_required());
}

#[test]
fn test_approval_context_autonomous_allows_unless_auto_approved() {
    let ctx = ApprovalContext::autonomous();
    assert!(!ctx.is_blocked("shell", ApprovalRequirement::Never));
    assert!(!ctx.is_blocked("shell", ApprovalRequirement::UnlessAutoApproved));
    assert!(ctx.is_blocked("shell", ApprovalRequirement::Always));
}

#[test]
fn test_approval_context_autonomous_with_tools_allows_always() {
    let ctx = ApprovalContext::autonomous_with_tools(["shell".to_string(), "message".to_string()]);
    assert!(!ctx.is_blocked("shell", ApprovalRequirement::Always));
    assert!(!ctx.is_blocked("message", ApprovalRequirement::Always));
    assert!(ctx.is_blocked("http", ApprovalRequirement::Always));
}

#[test]
fn test_approval_context_never_is_not_blocked() {
    let ctx = ApprovalContext::autonomous();
    assert!(!ctx.is_blocked("any_tool", ApprovalRequirement::Never));
}

#[test]
fn test_is_blocked_or_default_with_none_uses_legacy() {
    assert!(!ApprovalContext::is_blocked_or_default(
        &None,
        "any",
        ApprovalRequirement::Never
    ));
    assert!(ApprovalContext::is_blocked_or_default(
        &None,
        "any",
        ApprovalRequirement::UnlessAutoApproved
    ));
    assert!(ApprovalContext::is_blocked_or_default(
        &None,
        "any",
        ApprovalRequirement::Always
    ));
}

#[test]
fn test_is_blocked_or_default_with_some_delegates() {
    let ctx = Some(ApprovalContext::autonomous_with_tools(
        ["shell".to_string()],
    ));
    assert!(!ApprovalContext::is_blocked_or_default(
        &ctx,
        "shell",
        ApprovalRequirement::Always
    ));
    assert!(ApprovalContext::is_blocked_or_default(
        &ctx,
        "other",
        ApprovalRequirement::Always
    ));
    assert!(!ApprovalContext::is_blocked_or_default(
        &ctx,
        "any",
        ApprovalRequirement::UnlessAutoApproved
    ));
}

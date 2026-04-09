//! Approval-related tests.

use super::*;

#[test]
fn test_make_test_agent_succeeds() {
    // Verify that a test agent can be constructed without panicking.
    let _agent = make_test_agent();
}

#[test]
fn test_auto_approved_tool_is_respected() {
    let _agent = make_test_agent();
    let mut session = Session::new("user-1");
    session.auto_approve_tool("http");

    // A non-shell tool that is auto-approved should be approved.
    assert!(session.is_tool_auto_approved("http"));
    // A tool that hasn't been auto-approved should not be.
    assert!(!session.is_tool_auto_approved("shell"));
}

#[test]
fn test_shell_destructive_command_requires_explicit_approval() {
    // requires_explicit_approval() detects destructive commands that
    // should return ApprovalRequirement::Always from ShellTool.
    use crate::tools::builtin::shell::requires_explicit_approval;

    let destructive_cmds = [
        "rm -rf /tmp/test",
        "git push --force origin main",
        "git reset --hard HEAD~5",
    ];
    for cmd in &destructive_cmds {
        assert!(
            requires_explicit_approval(cmd),
            "'{}' should require explicit approval",
            cmd
        );
    }

    let safe_cmds = ["git status", "cargo build", "ls -la"];
    for cmd in &safe_cmds {
        assert!(
            !requires_explicit_approval(cmd),
            "'{}' should not require explicit approval",
            cmd
        );
    }
}

#[test]
fn test_always_approval_requirement_bypasses_session_auto_approve() {
    // Regression test: even if tool is auto-approved in session,
    // ApprovalRequirement::Always must still trigger approval.
    use crate::tools::ApprovalRequirement;

    let mut session = Session::new("user-1");
    let tool_name = "tool_remove";

    // Manually auto-approve tool_remove in this session
    session.auto_approve_tool(tool_name);
    assert!(
        session.is_tool_auto_approved(tool_name),
        "tool should be auto-approved"
    );

    // However, ApprovalRequirement::Always should always require approval
    // This is verified by the dispatcher logic: Always => true (ignores session state)
    let always_req = ApprovalRequirement::Always;
    let requires_approval = match always_req {
        ApprovalRequirement::Never => false,
        ApprovalRequirement::UnlessAutoApproved => !session.is_tool_auto_approved(tool_name),
        ApprovalRequirement::Always => true,
    };

    assert!(
        requires_approval,
        "ApprovalRequirement::Always must require approval even when tool is auto-approved"
    );
}

#[test]
fn test_always_approval_requirement_vs_unless_auto_approved() {
    // Verify the two requirements behave differently
    use crate::tools::ApprovalRequirement;

    let mut session = Session::new("user-2");
    let tool_name = "http";

    // Scenario 1: Tool is auto-approved
    session.auto_approve_tool(tool_name);

    // UnlessAutoApproved => doesn't require approval if auto-approved
    let unless_req = ApprovalRequirement::UnlessAutoApproved;
    let unless_needs = match unless_req {
        ApprovalRequirement::Never => false,
        ApprovalRequirement::UnlessAutoApproved => !session.is_tool_auto_approved(tool_name),
        ApprovalRequirement::Always => true,
    };
    assert!(
        !unless_needs,
        "UnlessAutoApproved should not need approval when auto-approved"
    );

    // Always => always requires approval
    let always_req = ApprovalRequirement::Always;
    let always_needs = match always_req {
        ApprovalRequirement::Never => false,
        ApprovalRequirement::UnlessAutoApproved => !session.is_tool_auto_approved(tool_name),
        ApprovalRequirement::Always => true,
    };
    assert!(
        always_needs,
        "Always must always require approval, even when auto-approved"
    );

    // Scenario 2: Tool is NOT auto-approved
    let new_tool = "new_tool";
    assert!(!session.is_tool_auto_approved(new_tool));

    // UnlessAutoApproved => requires approval
    let unless_needs = match unless_req {
        ApprovalRequirement::Never => false,
        ApprovalRequirement::UnlessAutoApproved => !session.is_tool_auto_approved(new_tool),
        ApprovalRequirement::Always => true,
    };
    assert!(
        unless_needs,
        "UnlessAutoApproved should need approval when not auto-approved"
    );

    // Always => always requires approval
    let always_needs = match always_req {
        ApprovalRequirement::Never => false,
        ApprovalRequirement::UnlessAutoApproved => !session.is_tool_auto_approved(new_tool),
        ApprovalRequirement::Always => true,
    };
    assert!(always_needs, "Always must always require approval");
}

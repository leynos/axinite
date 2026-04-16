//! Legacy approval helper retained for dispatcher approval tests.

/// Return `true` if a tool invocation requires interactive approval.
pub(in crate::agent::dispatcher) fn approval_requirement_needs_approval(
    requirement: crate::tools::ApprovalRequirement,
    session: &crate::agent::session::Session,
    tool_name: &str,
) -> bool {
    use crate::tools::ApprovalRequirement;

    match requirement {
        ApprovalRequirement::Never => false,
        ApprovalRequirement::UnlessAutoApproved => !session.is_tool_auto_approved(tool_name),
        ApprovalRequirement::Always => true,
    }
}

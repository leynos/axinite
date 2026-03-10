//! Exported GitHub tool schema used by [`SCHEMA`].
//!
//! The large embedded JSON lives in this companion module so `lib.rs` stays
//! focused on tool logic while tests and reviewers can inspect the schema
//! independently.

pub(super) const SCHEMA: &str = r#"{
    "type": "object",
    "additionalProperties": false,
    "properties": {
        "action": {
            "type": "string",
            "enum": [
                "get_repo",
                "list_issues",
                "create_issue",
                "get_issue",
                "list_pull_requests",
                "get_pull_request",
                "get_pull_request_files",
                "create_pr_review",
                "list_repos",
                "get_file_content",
                "trigger_workflow",
                "get_workflow_runs"
            ],
            "description": "GitHub action to execute. Repository actions use owner/repo, list_repos uses username, create_pr_review needs pr_number/body/event, get_file_content needs path, and trigger_workflow needs workflow_id/ref."
        },
        "owner": {
            "type": "string",
            "description": "Repository owner or organization. Required for repository, issue, pull request, and workflow actions."
        },
        "repo": {
            "type": "string",
            "description": "Repository name. Required for repository, issue, pull request, and workflow actions."
        },
        "username": {
            "type": "string",
            "description": "GitHub username whose repositories should be listed. Used by action=list_repos."
        },
        "title": {
            "type": "string",
            "description": "Issue title. Used by action=create_issue."
        },
        "body": {
            "type": "string",
            "description": "Issue body or pull-request review comment."
        },
        "labels": {
            "type": "array",
            "description": "Issue labels for action=create_issue.",
            "items": { "type": "string" }
        },
        "issue_number": {
            "type": "integer",
            "description": "Issue number. Used by action=get_issue."
        },
        "state": {
            "type": "string",
            "enum": ["open", "closed", "all"],
            "default": "open",
            "description": "Issue or pull-request state filter."
        },
        "page": {
            "type": "integer",
            "description": "1-based results page for list actions."
        },
        "limit": {
            "type": "integer",
            "default": 30,
            "description": "Maximum items to return for list actions."
        },
        "pr_number": {
            "type": "integer",
            "description": "Pull request number."
        },
        "event": {
            "type": "string",
            "enum": ["APPROVE", "REQUEST_CHANGES", "COMMENT"],
            "description": "Review event for action=create_pr_review."
        },
        "path": {
            "type": "string",
            "description": "Repository file path for action=get_file_content."
        },
        "ref": {
            "type": "string",
            "description": "Branch, tag, or commit ref. Required by action=trigger_workflow and optional for action=get_file_content."
        },
        "workflow_id": {
            "type": "string",
            "description": "Workflow filename or numeric ID."
        },
        "inputs": {
            "type": "object",
            "description": "Workflow dispatch inputs for action=trigger_workflow.",
            "additionalProperties": { "type": "string" }
        }
    },
    "required": ["action"],
    "description": "Parameters for the GitHub tool. The required combination of fields depends on the selected action."
}"#;

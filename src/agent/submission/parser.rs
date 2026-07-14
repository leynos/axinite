//! Parsing of raw user input into [`Submission`] values, covering control
//! commands, system commands, job commands, and approval responses.

use uuid::Uuid;

use super::types::Submission;

/// Parses user input into Submission types.
pub struct SubmissionParser;

impl SubmissionParser {
    /// Parse message content into a Submission.
    pub fn parse(content: &str) -> Submission {
        let trimmed = content.trim();
        let lower = trimmed.to_lowercase();
        tracing::debug!("[SubmissionParser::parse] Parsing input: {:?}", trimmed);

        // Control commands (exact match or prefix)
        if lower == "/undo" {
            return Submission::Undo;
        }
        if lower == "/redo" {
            return Submission::Redo;
        }
        if lower == "/interrupt" || lower == "/stop" {
            return Submission::Interrupt;
        }
        if lower == "/compact" {
            return Submission::Compact;
        }
        if lower == "/clear" {
            return Submission::Clear;
        }
        if lower == "/heartbeat" {
            return Submission::Heartbeat;
        }
        if lower == "/summarize" || lower == "/summary" {
            return Submission::Summarize;
        }
        if lower == "/suggest" {
            return Submission::Suggest;
        }
        if lower == "/thread new" || lower == "/new" {
            return Submission::NewThread;
        }
        // System commands (bypass thread-state checks)
        if lower == "/help" || lower == "/?" {
            return Submission::SystemCommand {
                command: "help".to_string(),
                args: vec![],
            };
        }
        if lower == "/version" {
            return Submission::SystemCommand {
                command: "version".to_string(),
                args: vec![],
            };
        }
        if lower == "/tools" {
            return Submission::SystemCommand {
                command: "tools".to_string(),
                args: vec![],
            };
        }
        if lower == "/skills" {
            return Submission::SystemCommand {
                command: "skills".to_string(),
                args: vec![],
            };
        }
        if lower.starts_with("/skills ") {
            let args: Vec<String> = trimmed
                .split_whitespace()
                .skip(1)
                .map(|s| s.to_string())
                .collect();
            return Submission::SystemCommand {
                command: "skills".to_string(),
                args,
            };
        }
        if lower == "/ping" {
            return Submission::SystemCommand {
                command: "ping".to_string(),
                args: vec![],
            };
        }
        if lower == "/debug" {
            return Submission::SystemCommand {
                command: "debug".to_string(),
                args: vec![],
            };
        }
        if lower == "/restart" {
            tracing::debug!("[SubmissionParser::parse] Recognized /restart command");
            return Submission::SystemCommand {
                command: "restart".to_string(),
                args: vec![],
            };
        }
        if lower.starts_with("/model") {
            let args: Vec<String> = trimmed
                .split_whitespace()
                .skip(1)
                .map(|s| s.to_string())
                .collect();
            return Submission::SystemCommand {
                command: "model".to_string(),
                args,
            };
        }

        if is_quit_command(&lower) {
            return Submission::Quit;
        }

        // Job commands
        if lower == "/status" || lower == "/progress" {
            return Submission::JobStatus { job_id: None };
        }
        if let Some(rest) = lower
            .strip_prefix("/status ")
            .or_else(|| lower.strip_prefix("/progress "))
        {
            let id = rest.trim().to_string();
            if !id.is_empty() {
                return Submission::JobStatus { job_id: Some(id) };
            }
        }
        if lower == "/list" {
            return Submission::JobStatus { job_id: None };
        }
        if let Some(rest) = lower.strip_prefix("/cancel ") {
            let id = rest.trim().to_string();
            if !id.is_empty() {
                return Submission::JobCancel { job_id: id };
            }
        }

        // /thread <uuid> - switch thread
        if let Some(rest) = lower.strip_prefix("/thread ") {
            let rest = rest.trim();
            if rest != "new"
                && let Ok(id) = Uuid::parse_str(rest)
            {
                return Submission::SwitchThread { thread_id: id };
            }
        }

        // /resume <uuid> - resume from checkpoint
        if let Some(rest) = lower.strip_prefix("/resume ")
            && let Ok(id) = Uuid::parse_str(rest.trim())
        {
            return Submission::Resume { checkpoint_id: id };
        }

        // Try structured JSON approval (from web gateway's /api/chat/approval endpoint)
        if let Some(submission) = parse_exec_approval(trimmed) {
            return submission;
        }

        // Approval responses (simple yes/no/always for pending approvals)
        // These are short enough to check explicitly
        match lower.as_str() {
            "yes" | "y" | "approve" | "ok" | "/approve" | "/yes" | "/y" => {
                return Submission::ApprovalResponse {
                    approved: true,
                    always: false,
                };
            }
            "always" | "a" | "yes always" | "approve always" | "/always" | "/a" => {
                return Submission::ApprovalResponse {
                    approved: true,
                    always: true,
                };
            }
            "no" | "n" | "deny" | "reject" | "cancel" | "/deny" | "/no" | "/n" => {
                return Submission::ApprovalResponse {
                    approved: false,
                    always: false,
                };
            }
            _ => {}
        }

        // Default: user input
        Submission::UserInput {
            content: content.to_string(),
        }
    }
}

/// Return `true` when the lowercased input is one of the quit commands.
fn is_quit_command(lower: &str) -> bool {
    matches!(lower, "/quit" | "/exit" | "/shutdown")
}

/// Parse a structured JSON exec-approval submission, returning `None` for
/// anything else so ordinary JSON-looking text falls through to user input.
fn parse_exec_approval(input: &str) -> Option<Submission> {
    if !input.starts_with('{') {
        return None;
    }
    let submission = serde_json::from_str::<Submission>(input).ok()?;
    matches!(submission, Submission::ExecApproval { .. }).then_some(submission)
}

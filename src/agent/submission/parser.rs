//! Parsing of raw user input into [`Submission`] values, covering control
//! commands, system commands, job commands, and approval responses.

use uuid::Uuid;

use super::types::Submission;

/// Parses user input into Submission types.
pub struct SubmissionParser;

impl SubmissionParser {
    /// Parse message content into a Submission.
    ///
    /// Tries each command family in turn — control, system, job, thread,
    /// structured approval, then plain approval words — and falls back to
    /// ordinary user input.
    pub fn parse(content: &str) -> Submission {
        let trimmed = content.trim();
        let lower = trimmed.to_lowercase();
        let input = Input {
            trimmed,
            lower: &lower,
        };
        tracing::debug!("[SubmissionParser::parse] Parsing input: {:?}", trimmed);

        parse_control_command(&input)
            .or_else(|| parse_system_command(&input))
            .or_else(|| parse_job_command(&input))
            .or_else(|| parse_thread_command(&input))
            .or_else(|| parse_exec_approval(&input))
            .or_else(|| parse_approval_response(&input))
            .unwrap_or_else(|| Submission::UserInput {
                content: content.to_string(),
            })
    }
}

/// User input in the two forms the command parsers match against.
struct Input<'a> {
    /// Original input with surrounding whitespace removed.
    trimmed: &'a str,
    /// Lowercased form used for case-insensitive command matching.
    lower: &'a str,
}

/// Build a `SystemCommand` submission.
fn system_command(command: &str, args: Vec<String>) -> Submission {
    Submission::SystemCommand {
        command: command.to_string(),
        args,
    }
}

/// Extract the whitespace-separated arguments after a slash command.
fn command_args(trimmed: &str) -> Vec<String> {
    trimmed
        .split_whitespace()
        .skip(1)
        .map(|s| s.to_string())
        .collect()
}

/// Parse exact-match control commands (undo, redo, interrupt, and friends).
fn parse_control_command(input: &Input<'_>) -> Option<Submission> {
    let lower = input.lower;
    let submission = match lower {
        "/undo" => Submission::Undo,
        "/redo" => Submission::Redo,
        "/interrupt" | "/stop" => Submission::Interrupt,
        "/compact" => Submission::Compact,
        "/clear" => Submission::Clear,
        "/heartbeat" => Submission::Heartbeat,
        "/summarize" | "/summary" => Submission::Summarize,
        "/suggest" => Submission::Suggest,
        "/thread new" | "/new" => Submission::NewThread,
        _ if is_quit_command(lower) => Submission::Quit,
        _ => return None,
    };
    Some(submission)
}

/// Parse system commands that bypass thread-state checks.
fn parse_system_command(input: &Input<'_>) -> Option<Submission> {
    let Input { trimmed, lower } = *input;
    let simple = match lower {
        "/help" | "/?" => Some("help"),
        "/version" => Some("version"),
        "/tools" => Some("tools"),
        "/skills" => Some("skills"),
        "/ping" => Some("ping"),
        "/debug" => Some("debug"),
        "/restart" => Some("restart"),
        _ => None,
    };
    if let Some(command) = simple {
        if command == "restart" {
            tracing::debug!("[SubmissionParser::parse] Recognized /restart command");
        }
        return Some(system_command(command, vec![]));
    }
    if lower.starts_with("/skills ") {
        return Some(system_command("skills", command_args(trimmed)));
    }
    if lower.starts_with("/model") {
        return Some(system_command("model", command_args(trimmed)));
    }
    None
}

/// Extract a non-empty, trimmed job id following one of the given prefixes.
fn job_id_after(lower: &str, prefixes: &[&str]) -> Option<String> {
    let rest = prefixes.iter().find_map(|p| lower.strip_prefix(p))?;
    let id = rest.trim();
    (!id.is_empty()).then(|| id.to_string())
}

/// Parse job status and cancellation commands.
fn parse_job_command(input: &Input<'_>) -> Option<Submission> {
    let lower = input.lower;
    if matches!(lower, "/status" | "/progress" | "/list") {
        return Some(Submission::JobStatus { job_id: None });
    }
    if let Some(id) = job_id_after(lower, &["/status ", "/progress "]) {
        return Some(Submission::JobStatus { job_id: Some(id) });
    }
    if let Some(id) = job_id_after(lower, &["/cancel "]) {
        return Some(Submission::JobCancel { job_id: id });
    }
    None
}

/// Parse thread switching and checkpoint resume commands.
fn parse_thread_command(input: &Input<'_>) -> Option<Submission> {
    let lower = input.lower;
    // /thread <uuid> - switch thread
    if let Some(rest) = lower.strip_prefix("/thread ") {
        let rest = rest.trim();
        if rest != "new"
            && let Ok(id) = Uuid::parse_str(rest)
        {
            return Some(Submission::SwitchThread { thread_id: id });
        }
    }

    // /resume <uuid> - resume from checkpoint
    if let Some(rest) = lower.strip_prefix("/resume ")
        && let Ok(id) = Uuid::parse_str(rest.trim())
    {
        return Some(Submission::Resume { checkpoint_id: id });
    }
    None
}

/// Parse simple yes/no/always responses to pending approvals.
fn parse_approval_response(input: &Input<'_>) -> Option<Submission> {
    match input.lower {
        "yes" | "y" | "approve" | "ok" | "/approve" | "/yes" | "/y" => {
            Some(Submission::ApprovalResponse {
                approved: true,
                always: false,
            })
        }
        "always" | "a" | "yes always" | "approve always" | "/always" | "/a" => {
            Some(Submission::ApprovalResponse {
                approved: true,
                always: true,
            })
        }
        "no" | "n" | "deny" | "reject" | "cancel" | "/deny" | "/no" | "/n" => {
            Some(Submission::ApprovalResponse {
                approved: false,
                always: false,
            })
        }
        _ => None,
    }
}

/// Return `true` when the lowercased input is one of the quit commands.
fn is_quit_command(lower: &str) -> bool {
    matches!(lower, "/quit" | "/exit" | "/shutdown")
}

/// Parse a structured JSON exec-approval submission, returning `None` for
/// anything else so ordinary JSON-looking text falls through to user input.
fn parse_exec_approval(input: &Input<'_>) -> Option<Submission> {
    let trimmed = input.trimmed;
    if !trimmed.starts_with('{') {
        return None;
    }
    let submission = serde_json::from_str::<Submission>(trimmed).ok()?;
    matches!(submission, Submission::ExecApproval { .. }).then_some(submission)
}

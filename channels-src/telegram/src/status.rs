use crate::exports::near::agent::channel::{StatusType, StatusUpdate};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TelegramStatusAction {
    Typing,
    Notify(String),
}

pub(crate) const TELEGRAM_STATUS_MAX_CHARS: usize = 600;

pub(crate) fn truncate_status_message(input: &str, max_chars: usize) -> String {
    let mut iter = input.chars();
    let truncated: String = iter.by_ref().take(max_chars).collect();
    if iter.next().is_some() {
        format!("{}...", truncated)
    } else {
        truncated
    }
}

pub(crate) fn status_message_for_user(update: &StatusUpdate) -> Option<String> {
    let message = update.message.trim();
    if message.is_empty() {
        None
    } else {
        Some(truncate_status_message(message, TELEGRAM_STATUS_MAX_CHARS))
    }
}

fn is_terminal_text_status(message: &str) -> bool {
    let msg = message.trim();

    ["Done", "Interrupted", "Awaiting approval", "Rejected"]
        .iter()
        .any(|terminal| msg.eq_ignore_ascii_case(terminal))
}

fn notify_status_for_user(update: &StatusUpdate) -> Option<TelegramStatusAction> {
    status_message_for_user(update).map(TelegramStatusAction::Notify)
}

pub(crate) fn classify_status_update(update: &StatusUpdate) -> Option<TelegramStatusAction> {
    match update.status {
        StatusType::Thinking => Some(TelegramStatusAction::Typing),
        StatusType::Done | StatusType::Interrupted => None,
        // Tool telemetry can be noisy in chat; keep it as typing-only UX.
        StatusType::ToolStarted | StatusType::ToolCompleted | StatusType::ToolResult => None,
        StatusType::Status => {
            if is_terminal_text_status(&update.message) {
                None
            } else {
                notify_status_for_user(update)
            }
        }
        StatusType::ApprovalNeeded
        | StatusType::JobStarted
        | StatusType::AuthRequired
        | StatusType::AuthCompleted => notify_status_for_user(update),
    }
}

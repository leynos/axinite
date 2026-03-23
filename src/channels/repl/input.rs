//! Rustyline helper types for the REPL: tab-completion, hints, and Esc handling.

use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rustyline::completion::Completer;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{
    Cmd as ReadlineCmd, ConditionalEventHandler, Event, EventContext, Helper, RepeatCount,
};

/// Slash commands available in the REPL.
pub(super) const SLASH_COMMANDS: &[&str] = &[
    "/help",
    "/quit",
    "/exit",
    "/debug",
    "/model",
    "/undo",
    "/redo",
    "/clear",
    "/compact",
    "/new",
    "/interrupt",
    "/version",
    "/tools",
    "/ping",
    "/job",
    "/status",
    "/cancel",
    "/list",
    "/heartbeat",
    "/summarize",
    "/suggest",
    "/thread",
    "/resume",
];

/// Rustyline helper for slash-command tab completion.
pub(super) struct ReplHelper;

impl Completer for ReplHelper {
    type Candidate = String;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> rustyline::Result<(usize, Vec<String>)> {
        if !line.starts_with('/') {
            return Ok((0, vec![]));
        }

        let prefix = &line[..pos];
        let matches: Vec<String> = SLASH_COMMANDS
            .iter()
            .filter(|cmd| cmd.starts_with(prefix))
            .map(|cmd| cmd.to_string())
            .collect();

        Ok((0, matches))
    }
}

impl Hinter for ReplHelper {
    type Hint = String;

    fn hint(&self, line: &str, pos: usize, _ctx: &rustyline::Context<'_>) -> Option<String> {
        if !line.starts_with('/') || pos < line.len() {
            return None;
        }

        SLASH_COMMANDS
            .iter()
            .find(|cmd| cmd.starts_with(line) && **cmd != line)
            .map(|cmd| cmd[line.len()..].to_string())
    }
}

impl Highlighter for ReplHelper {
    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Cow::Owned(format!("\x1b[90m{hint}\x1b[0m"))
    }
}

impl Validator for ReplHelper {}
impl Helper for ReplHelper {}

pub(super) struct EscInterruptHandler {
    pub(super) triggered: Arc<AtomicBool>,
}

impl ConditionalEventHandler for EscInterruptHandler {
    fn handle(
        &self,
        _evt: &Event,
        _n: RepeatCount,
        _positive: bool,
        _ctx: &EventContext,
    ) -> Option<ReadlineCmd> {
        self.triggered.store(true, Ordering::Relaxed);
        Some(ReadlineCmd::Interrupt)
    }
}

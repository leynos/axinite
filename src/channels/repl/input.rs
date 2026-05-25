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

/// Slash command entry with name, description, and help grouping metadata.
#[derive(Clone, Copy)]
pub(super) struct SlashCommand {
    pub(super) name: &'static str,
    pub(super) description: &'static str,
    pub(super) group: &'static str,
}

/// Slash commands available in the REPL.
pub(super) const SLASH_COMMANDS: &[SlashCommand] = &[
    SlashCommand {
        name: "/help",
        description: "show this help",
        group: "Commands",
    },
    SlashCommand {
        name: "/quit",
        description: "exit the repl",
        group: "Commands",
    },
    SlashCommand {
        name: "/exit",
        description: "exit the repl",
        group: "Commands",
    },
    SlashCommand {
        name: "/debug",
        description: "toggle verbose output",
        group: "Commands",
    },
    SlashCommand {
        name: "/model",
        description: "show or change model",
        group: "Configuration",
    },
    SlashCommand {
        name: "/undo",
        description: "undo the last turn",
        group: "Conversation",
    },
    SlashCommand {
        name: "/redo",
        description: "redo an undone turn",
        group: "Conversation",
    },
    SlashCommand {
        name: "/clear",
        description: "clear conversation",
        group: "Conversation",
    },
    SlashCommand {
        name: "/compact",
        description: "compact context window",
        group: "Conversation",
    },
    SlashCommand {
        name: "/new",
        description: "new conversation thread",
        group: "Conversation",
    },
    SlashCommand {
        name: "/interrupt",
        description: "stop current operation",
        group: "Conversation",
    },
    SlashCommand {
        name: "/version",
        description: "show version information",
        group: "Configuration",
    },
    SlashCommand {
        name: "/tools",
        description: "list available tools",
        group: "Configuration",
    },
    SlashCommand {
        name: "/ping",
        description: "test connection",
        group: "Diagnostics",
    },
    SlashCommand {
        name: "/job",
        description: "manage background jobs",
        group: "Jobs",
    },
    SlashCommand {
        name: "/status",
        description: "show system status",
        group: "Diagnostics",
    },
    SlashCommand {
        name: "/cancel",
        description: "cancel current operation",
        group: "Jobs",
    },
    SlashCommand {
        name: "/list",
        description: "list conversations",
        group: "Threads",
    },
    SlashCommand {
        name: "/heartbeat",
        description: "send heartbeat",
        group: "Diagnostics",
    },
    SlashCommand {
        name: "/summarize",
        description: "summarize conversation",
        group: "Threads",
    },
    SlashCommand {
        name: "/suggest",
        description: "get suggestions",
        group: "Threads",
    },
    SlashCommand {
        name: "/thread",
        description: "manage conversation threads",
        group: "Threads",
    },
    SlashCommand {
        name: "/resume",
        description: "resume a conversation",
        group: "Threads",
    },
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
            .filter(|cmd| cmd.name.starts_with(prefix))
            .map(|cmd| cmd.name.to_string())
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
            .find(|cmd| cmd.name.starts_with(line) && cmd.name != line)
            .map(|cmd| cmd.name[line.len()..].to_string())
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

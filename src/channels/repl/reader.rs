//! The blocking input thread of the REPL: line editing, local commands and
//! forwarding of typed input to the agent.

use std::io::IsTerminal;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rustyline::config::Config;
use rustyline::error::ReadlineError;
use rustyline::history::DefaultHistory;
use rustyline::{CompletionType, Editor, EventHandler, KeyCode, KeyEvent, Modifiers};
use tokio::sync::mpsc;

use super::formatting::print_help;
use super::history_path;
use super::input::{EscInterruptHandler, ReplHelper};
use crate::channels::IncomingMessage;

/// The line editor type used by the REPL.
type ReplEditor = Editor<ReplHelper, DefaultHistory>;

/// State shared between the REPL channel and its input thread.
pub(super) struct ReplReader {
    /// Where typed input goes.
    pub(super) tx: mpsc::Sender<IncomingMessage>,
    /// Debug mode flag (toggled by `/debug`).
    pub(super) debug_mode: Arc<AtomicBool>,
    /// When true, the one-liner startup banner is suppressed.
    pub(super) suppress_banner: Arc<AtomicBool>,
    /// Set by the Esc key handler to distinguish Esc from Ctrl+C.
    pub(super) esc_interrupt_triggered: Arc<AtomicBool>,
}

impl ReplReader {
    /// Run the input thread to completion.
    ///
    /// With `single_message` set, send it and a shutdown request and return.
    pub(super) fn run(self, single_message: Option<String>) {
        let sys_tz = crate::timezone::detect_system_timezone().name().to_string();

        if let Some(msg) = single_message {
            self.send_single_message(&msg, &sys_tz);
            return;
        }

        let Some(mut rl) = self.build_editor() else {
            return;
        };

        let hist_path = history_path();
        if let Some(parent) = hist_path.parent() {
            let _ = ambient_fs::create_dir_all(parent);
        }
        let _ = rl.load_history(&hist_path);

        if !self.suppress_banner.load(Ordering::Relaxed) {
            println!("\x1b[1mAxinite\x1b[0m  /help for commands, /quit to exit");
            println!();
        }

        self.read_loop(&mut rl, &sys_tz);

        // Save history on exit
        let _ = rl.save_history(&history_path());
    }

    /// Single message mode: send the message, then ask the agent to exit after
    /// exactly one turn, even when other channels (gateway/http) are enabled.
    fn send_single_message(&self, msg: &str, sys_tz: &str) {
        let incoming = IncomingMessage::new("repl", "default", msg).with_timezone(sys_tz);
        let _ = self.tx.blocking_send(incoming);
        let _ = self
            .tx
            .blocking_send(IncomingMessage::new("repl", "default", "/quit"));
    }

    /// Configure the line editor, its helper and the Esc binding.
    ///
    /// `history_ignore_dups` returns `Result` (it validates the underlying
    /// config value), while the remaining builder methods and `.build()` are
    /// infallible. Failures are reported on stderr and yield `None`.
    fn build_editor(&self) -> Option<ReplEditor> {
        let config = match Config::builder().history_ignore_dups(true) {
            Ok(b) => b
                .auto_add_history(true)
                .completion_type(CompletionType::List)
                .build(),
            Err(e) => {
                eprintln!("Failed to configure line editor: {e}");
                return None;
            }
        };

        let mut rl = match Editor::with_config(config) {
            Ok(editor) => editor,
            Err(e) => {
                eprintln!("Failed to initialize line editor: {e}");
                return None;
            }
        };

        rl.set_helper(Some(ReplHelper));
        rl.bind_sequence(
            KeyEvent(KeyCode::Esc, Modifiers::NONE),
            EventHandler::Conditional(Box::new(EscInterruptHandler {
                triggered: Arc::clone(&self.esc_interrupt_triggered),
            })),
        );
        Some(rl)
    }

    /// Read lines until the user quits, the channel closes or input ends.
    fn read_loop(&self, rl: &mut ReplEditor, sys_tz: &str) {
        loop {
            let prompt = if self.debug_mode.load(Ordering::Relaxed) {
                "\x1b[33m[debug]\x1b[0m \x1b[1;36m\u{203A}\x1b[0m "
            } else {
                "\x1b[1;36m\u{203A}\x1b[0m "
            };

            let flow = match rl.readline(prompt) {
                Ok(line) => self.handle_line(line.trim(), sys_tz),
                Err(ReadlineError::Interrupted) => self.handle_interrupt(sys_tz),
                Err(ReadlineError::Eof) => self.handle_eof(sys_tz),
                Err(e) => {
                    eprintln!("Input error: {e}");
                    ControlFlow::Break(())
                }
            };
            if flow.is_break() {
                break;
            }
        }
    }

    /// Handle one typed line: local commands stay here, anything else goes to
    /// the agent.
    fn handle_line(&self, line: &str, sys_tz: &str) -> ControlFlow<()> {
        if line.is_empty() {
            return ControlFlow::Continue(());
        }

        // Only commands that need immediate local handling stay here.
        match line.to_lowercase().as_str() {
            "/quit" | "/exit" => {
                // Forward shutdown command so the agent loop exits even
                // when other channels (e.g. web gateway) are still active.
                self.send_quit(sys_tz);
                return ControlFlow::Break(());
            }
            "/help" => {
                print_help();
                return ControlFlow::Continue(());
            }
            "/debug" => {
                self.toggle_debug();
                return ControlFlow::Continue(());
            }
            _ => {}
        }

        let msg = IncomingMessage::new("repl", "default", line).with_timezone(sys_tz);
        if self.tx.blocking_send(msg).is_err() {
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    }

    /// Flip debug mode and say which way it went.
    fn toggle_debug(&self) {
        let current = self.debug_mode.load(Ordering::Relaxed);
        self.debug_mode.store(!current, Ordering::Relaxed);
        if !current {
            println!("\x1b[90mdebug mode on\x1b[0m");
        } else {
            println!("\x1b[90mdebug mode off\x1b[0m");
        }
    }

    /// Esc interrupts the current operation and keeps the REPL open; Ctrl+C
    /// (VINTR) requests a graceful shutdown.
    fn handle_interrupt(&self, sys_tz: &str) -> ControlFlow<()> {
        if self.esc_interrupt_triggered.swap(false, Ordering::Relaxed) {
            let msg = IncomingMessage::new("repl", "default", "/interrupt").with_timezone(sys_tz);
            if self.tx.blocking_send(msg).is_err() {
                return ControlFlow::Break(());
            }
            return ControlFlow::Continue(());
        }
        self.send_quit(sys_tz);
        ControlFlow::Break(())
    }

    /// Ctrl+D in interactive mode requests a graceful shutdown. In daemon mode
    /// (stdin = /dev/null, no TTY) EOF arrives immediately, so the REPL thread
    /// just ends silently and the other channels (gateway, telegram, ...) keep
    /// running.
    fn handle_eof(&self, sys_tz: &str) -> ControlFlow<()> {
        if std::io::stdin().is_terminal() {
            self.send_quit(sys_tz);
        }
        ControlFlow::Break(())
    }

    /// Ask the agent loop to shut down, ignoring a closed channel.
    fn send_quit(&self, sys_tz: &str) {
        let msg = IncomingMessage::new("repl", "default", "/quit").with_timezone(sys_tz);
        let _ = self.tx.blocking_send(msg);
    }
}

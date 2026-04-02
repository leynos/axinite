//! Interactive REPL channel with line editing and markdown rendering.
//!
//! Provides the primary CLI interface for interacting with the agent.
//! Uses rustyline for line editing, history, and tab-completion.
//! Uses termimad for rendering markdown responses inline.
//!
//! ## Commands
//!
//! - `/help` - Show available commands
//! - `/quit` or `/exit` - Exit the REPL
//! - `/debug` - Toggle debug mode (verbose tool output)
//! - `/undo` - Undo the last turn
//! - `/redo` - Redo an undone turn
//! - `/clear` - Clear the conversation
//! - `/compact` - Compact the context
//! - `/new` - Start a new thread
//! - `yes`/`no`/`always` - Respond to tool approval prompts
//! - `Esc` - Interrupt current operation

mod formatting;
mod input;
mod status_output;
use std::io::IsTerminal;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rustyline::config::Config;
use rustyline::error::ReadlineError;
use rustyline::{CompletionType, Editor, EventHandler, KeyCode, KeyEvent, Modifiers};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::bootstrap::ironclaw_base_dir;
use crate::channels::{
    IncomingMessage, MessageStream, NativeChannel, OutgoingResponse, StatusUpdate,
};
use crate::error::ChannelError;

use formatting::{ToolApprovalRequest, make_skin, print_help};
use input::{EscInterruptHandler, ReplHelper};
use status_output::{
    print_approval_needed, print_auth_completed, print_auth_required, print_image_generated,
    print_job_started, print_status, print_stream_chunk, print_thinking, print_tool_completed,
    print_tool_result, print_tool_started,
};

/// REPL channel with line editing and markdown rendering.
pub struct ReplChannel {
    /// Optional single message to send (for -m flag).
    single_message: Option<String>,
    /// Debug mode flag (shared with input thread).
    debug_mode: Arc<AtomicBool>,
    /// Whether we're currently streaming (chunks have been printed without a trailing newline).
    is_streaming: Arc<AtomicBool>,
    /// When true, the one-liner startup banner is suppressed (boot screen shown instead).
    suppress_banner: Arc<AtomicBool>,
}

impl ReplChannel {
    /// Create a new REPL channel.
    pub fn new() -> Self {
        Self {
            single_message: None,
            debug_mode: Arc::new(AtomicBool::new(false)),
            is_streaming: Arc::new(AtomicBool::new(false)),
            suppress_banner: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create a REPL channel that sends a single message and exits.
    pub fn with_message(message: String) -> Self {
        Self {
            single_message: Some(message),
            debug_mode: Arc::new(AtomicBool::new(false)),
            is_streaming: Arc::new(AtomicBool::new(false)),
            suppress_banner: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Suppress the one-liner startup banner (boot screen will be shown instead).
    pub fn suppress_banner(&self) {
        self.suppress_banner.store(true, Ordering::Relaxed);
    }

    fn is_debug(&self) -> bool {
        self.debug_mode.load(Ordering::Relaxed)
    }
}

impl Default for ReplChannel {
    fn default() -> Self {
        Self::new()
    }
}

/// Get the history file path (~/.ironclaw/history).
fn history_path() -> std::path::PathBuf {
    ironclaw_base_dir().join("history")
}

impl NativeChannel for ReplChannel {
    fn name(&self) -> &str {
        "repl"
    }

    async fn start(&self) -> Result<MessageStream, ChannelError> {
        let (tx, rx) = mpsc::channel(32);
        let single_message = self.single_message.clone();
        let debug_mode = Arc::clone(&self.debug_mode);
        let suppress_banner = Arc::clone(&self.suppress_banner);
        let esc_interrupt_triggered_for_thread = Arc::new(AtomicBool::new(false));

        std::thread::spawn(move || {
            let sys_tz = crate::timezone::detect_system_timezone().name().to_string();

            // Single message mode: send it and return
            if let Some(msg) = single_message {
                let incoming = IncomingMessage::new("repl", "default", &msg).with_timezone(&sys_tz);
                let _ = tx.blocking_send(incoming);
                // Ensure the agent exits after handling exactly one turn in -m mode,
                // even when other channels (gateway/http) are enabled.
                let _ = tx.blocking_send(IncomingMessage::new("repl", "default", "/quit"));
                return;
            }

            // Set up rustyline
            //
            // `history_ignore_dups` returns `Result` (it validates the
            // underlying config value), while the remaining builder methods
            // and `.build()` are infallible.
            let config = match Config::builder().history_ignore_dups(true) {
                Ok(b) => b
                    .auto_add_history(true)
                    .completion_type(CompletionType::List)
                    .build(),
                Err(e) => {
                    eprintln!("Failed to configure line editor: {e}");
                    return;
                }
            };

            let mut rl = match Editor::with_config(config) {
                Ok(editor) => editor,
                Err(e) => {
                    eprintln!("Failed to initialize line editor: {e}");
                    return;
                }
            };

            rl.set_helper(Some(ReplHelper));

            rl.bind_sequence(
                KeyEvent(KeyCode::Esc, Modifiers::NONE),
                EventHandler::Conditional(Box::new(EscInterruptHandler {
                    triggered: Arc::clone(&esc_interrupt_triggered_for_thread),
                })),
            );

            // Load history
            let hist_path = history_path();
            if let Some(parent) = hist_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = rl.load_history(&hist_path);

            if !suppress_banner.load(Ordering::Relaxed) {
                println!("\x1b[1mIronClaw\x1b[0m  /help for commands, /quit to exit");
                println!();
            }

            loop {
                let prompt = if debug_mode.load(Ordering::Relaxed) {
                    "\x1b[33m[debug]\x1b[0m \x1b[1;36m\u{203A}\x1b[0m "
                } else {
                    "\x1b[1;36m\u{203A}\x1b[0m "
                };

                match rl.readline(prompt) {
                    Ok(line) => {
                        let line = line.trim();
                        if line.is_empty() {
                            continue;
                        }

                        // Handle local REPL commands (only commands that need
                        // immediate local handling stay here)
                        match line.to_lowercase().as_str() {
                            "/quit" | "/exit" => {
                                // Forward shutdown command so the agent loop exits even
                                // when other channels (e.g. web gateway) are still active.
                                let msg = IncomingMessage::new("repl", "default", "/quit")
                                    .with_timezone(&sys_tz);
                                let _ = tx.blocking_send(msg);
                                break;
                            }
                            "/help" => {
                                print_help();
                                continue;
                            }
                            "/debug" => {
                                let current = debug_mode.load(Ordering::Relaxed);
                                debug_mode.store(!current, Ordering::Relaxed);
                                if !current {
                                    println!("\x1b[90mdebug mode on\x1b[0m");
                                } else {
                                    println!("\x1b[90mdebug mode off\x1b[0m");
                                }
                                continue;
                            }
                            _ => {}
                        }

                        let msg =
                            IncomingMessage::new("repl", "default", line).with_timezone(&sys_tz);
                        if tx.blocking_send(msg).is_err() {
                            break;
                        }
                    }
                    Err(ReadlineError::Interrupted) => {
                        if esc_interrupt_triggered_for_thread.swap(false, Ordering::Relaxed) {
                            // Esc: interrupt current operation and keep REPL open.
                            let msg = IncomingMessage::new("repl", "default", "/interrupt")
                                .with_timezone(&sys_tz);
                            if tx.blocking_send(msg).is_err() {
                                break;
                            }
                        } else {
                            // Ctrl+C (VINTR): request graceful shutdown.
                            let msg = IncomingMessage::new("repl", "default", "/quit")
                                .with_timezone(&sys_tz);
                            let _ = tx.blocking_send(msg);
                            break;
                        }
                    }
                    Err(ReadlineError::Eof) => {
                        // Ctrl+D in interactive mode: graceful shutdown.
                        // In daemon mode (stdin = /dev/null, no TTY), EOF arrives
                        // immediately — just drop the REPL thread silently so other
                        // channels (gateway, telegram, …) keep running.
                        if std::io::stdin().is_terminal() {
                            let msg = IncomingMessage::new("repl", "default", "/quit")
                                .with_timezone(&sys_tz);
                            let _ = tx.blocking_send(msg);
                        }
                        break;
                    }
                    Err(e) => {
                        eprintln!("Input error: {e}");
                        break;
                    }
                }
            }

            // Save history on exit
            let _ = rl.save_history(&history_path());
        });

        Ok(Box::pin(ReceiverStream::new(rx)))
    }

    async fn respond(
        &self,
        _msg: &IncomingMessage,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        let width = crossterm::terminal::size()
            .map(|(w, _)| w as usize)
            .unwrap_or(80);

        // If we were streaming, the content was already printed via StreamChunk.
        // Just finish the line and reset.
        if self.is_streaming.swap(false, Ordering::Relaxed) {
            println!();
            println!();
            return Ok(());
        }

        // Dim separator line before the response
        let sep_width = width.min(80);
        eprintln!("\x1b[90m{}\x1b[0m", "\u{2500}".repeat(sep_width));

        // Render markdown
        let skin = make_skin();
        let text = termimad::FmtText::from(&skin, &response.content, Some(width));

        print!("{text}");
        println!();
        Ok(())
    }

    async fn send_status(
        &self,
        status: StatusUpdate,
        _metadata: &serde_json::Value,
    ) -> Result<(), ChannelError> {
        match status {
            StatusUpdate::Thinking(msg) => print_thinking(&msg),
            StatusUpdate::ToolStarted { name } => print_tool_started(&name),
            StatusUpdate::ToolCompleted { name, success, .. } => {
                print_tool_completed(&name, success);
            }
            StatusUpdate::ToolResult { name: _, preview } => print_tool_result(&preview),
            StatusUpdate::StreamChunk(chunk) => print_stream_chunk(&self.is_streaming, &chunk),
            StatusUpdate::JobStarted {
                job_id,
                title,
                browse_url,
            } => {
                print_job_started(&job_id, &title, &browse_url);
            }
            StatusUpdate::Status(msg) => print_status(self.is_debug(), &msg),
            StatusUpdate::ApprovalNeeded {
                request_id,
                tool_name,
                description,
                parameters,
            } => {
                let request = ToolApprovalRequest {
                    request_id: &request_id,
                    tool_name: &tool_name,
                    description: &description,
                };
                print_approval_needed(&request, &parameters);
            }
            StatusUpdate::AuthRequired {
                extension_name,
                instructions,
                setup_url,
                ..
            } => print_auth_required(
                &extension_name,
                instructions.as_deref(),
                setup_url.as_deref(),
            ),
            StatusUpdate::AuthCompleted {
                extension_name,
                success,
                message,
            } => {
                print_auth_completed(&extension_name, success, &message);
            }
            StatusUpdate::ImageGenerated { path, .. } => {
                print_image_generated(path.as_deref());
            }
        }
        Ok(())
    }

    async fn broadcast(
        &self,
        _user_id: &str,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        let skin = make_skin();
        let width = crossterm::terminal::size()
            .map(|(w, _)| w as usize)
            .unwrap_or(80);

        eprintln!("\x1b[34m\u{25CF}\x1b[0m notification");
        let text = termimad::FmtText::from(&skin, &response.content, Some(width));
        eprint!("{text}");
        eprintln!();
        Ok(())
    }

    async fn health_check(&self) -> Result<(), ChannelError> {
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), ChannelError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use futures::StreamExt;

    use super::*;

    #[tokio::test]
    async fn single_message_mode_sends_message_then_quit() {
        let repl = ReplChannel::with_message("hi".to_string());
        let mut stream = repl.start().await.expect("repl start should succeed");

        let first = stream.next().await.expect("first message missing");
        assert_eq!(first.channel, "repl");
        assert_eq!(first.content, "hi");

        let second = stream.next().await.expect("quit message missing");
        assert_eq!(second.channel, "repl");
        assert_eq!(second.content, "/quit");

        assert!(
            stream.next().await.is_none(),
            "stream should end after /quit"
        );
    }
}

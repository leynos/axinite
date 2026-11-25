//! The helper's protocol session.
//!
//! One process serves exactly one run: it announces itself, accepts one start
//! request, mediates any host callbacks, and reports one terminal outcome. The
//! parent owns every side effect; this module only translates between bounded
//! JSON frames and Monty's suspension points.

use std::collections::BTreeSet;
use std::io::{self, BufRead, Write};
use std::mem;

use monty::{FunctionCall, RunProgress};
use monty_protocol::{
    CallOutcome, DenialCode, GuestOutput, GuestStream, HelperToParent, HostCall, ParentToHelper,
    ResumeRun, StartRun, Terminal, TerminalOutcome, hello, validate_start_run, write_frame,
};
use monty_types::{
    CollectedStreams, MontyException, MontyObject, NameLookupResult, PrintWriter,
};
use serde_json::Value;

use self::failures::{abort, concurrent_denied, os_denied};
use self::output::{OutputBudget, split_prefix, truncate};
use crate::convert;
use crate::limits;

mod failures;
mod output;
mod run;

use self::run::{Failure, ReplyOutcome, completed, guest_stream, read_parent, run_session};

/// A fatal helper-side failure, reported through the process exit status.
pub type Fatal = i32;

/// Exit status for a malformed or unexpected parent message.
pub const EXIT_PROTOCOL: i32 = 2;
/// Exit status for a failure to establish the helper's own boundaries.
pub const EXIT_SETUP: i32 = 3;
/// Exit status for a failed write to the parent.
pub const EXIT_IO: i32 = 4;

/// Largest guest-output text placed in one frame.
///
/// JSON escaping can expand a byte sixfold, so this stays well below the
/// protocol's frame ceiling.
const OUTPUT_CHUNK_BYTES: usize = 16 * 1024;

/// Serves one helper process over standard input and output.
pub fn serve_stdio() -> i32 {
    if let Err(error) = limits::impose_address_space_limit() {
        let _ = writeln!(
            io::stderr(),
            "axinite-monty-helper: cannot impose the address-space limit: {error}"
        );
        return EXIT_SETUP;
    }
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = io::BufReader::new(stdin.lock());
    let mut writer = io::BufWriter::new(stdout.lock());
    match serve(&mut reader, &mut writer) {
        Ok(()) => 0,
        Err(code) => code,
    }
}

fn serve<R: BufRead, W: Write>(reader: &mut R, writer: &mut W) -> Result<(), Fatal> {
    write_frame(writer, &hello()).map_err(|_| EXIT_IO)?;
    let request = match read_parent(reader) {
        Some(ParentToHelper::StartRun(request)) => request,
        _ => return Err(EXIT_PROTOCOL),
    };
    if let Err(message) = validate_start_run(&request) {
        return reject(writer, request.run_id, message);
    }
    if let Err(message) = limits::check_guest_memory(request.limits.max_memory_bytes) {
        return reject(writer, request.run_id, message);
    }
    if let Err(message) = limits::arm_allocator(request.limits.max_memory_bytes) {
        return reject(writer, request.run_id, message);
    }
    run_session(reader, writer, request)
}

fn reject<W: Write>(writer: &mut W, run_id: String, message: String) -> Result<(), Fatal> {
    let terminal = Terminal {
        run_id,
        outcome: TerminalOutcome::RuntimeCrash {
            message: truncate(&message),
        },
    };
    write_frame(writer, &HelperToParent::Terminal(terminal)).map_err(|_| EXIT_IO)
}

/// The outcome of handling one suspended host call.
enum CallReply {
    /// The parent answered with a value the guest may continue with.
    Resume {
        /// The suspended call, ready to settle.
        call: Box<FunctionCall>,
        /// The converted result value.
        value: MontyObject,
    },
    /// The run must end without resuming the guest.
    Finished(TerminalOutcome),
}

struct Session<'w, W: Write> {
    writer: &'w mut W,
    run_id: String,
    tools: BTreeSet<String>,
    budget: OutputBudget,
    next_call_id: u64,
}

impl<'w, W: Write> Session<'w, W> {
    fn new(writer: &'w mut W, request: &StartRun) -> Self {
        Self {
            writer,
            run_id: request.run_id.clone(),
            tools: request.tools.iter().map(|tool| tool.name.clone()).collect(),
            budget: OutputBudget::new(
                usize::try_from(request.limits.max_output_bytes).unwrap_or(usize::MAX),
            ),
            next_call_id: 0,
        }
    }

    fn send(&mut self, message: &HelperToParent) -> Result<(), Fatal> {
        write_frame(self.writer, message).map_err(|_| EXIT_IO)
    }

    fn printer<'a>(&self, streams: &'a mut CollectedStreams) -> PrintWriter<'a> {
        PrintWriter::CollectStreams(streams, Some(self.budget.remaining()))
    }

    /// Sends all output collected since the last drain, in bounded frames.
    fn drain(&mut self, streams: &mut CollectedStreams) -> Result<(), Fatal> {
        for (stream, text) in mem::take(streams).into_entries() {
            let stream = guest_stream(stream);
            self.send_text(stream, text.as_str())?;
        }
        Ok(())
    }

    /// Send one stream's text in chunks that fit the budget and the frame.
    fn send_text(&mut self, stream: GuestStream, mut rest: &str) -> Result<(), Fatal> {
        while !rest.is_empty() && self.budget.remaining() > 0 {
            let limit = self.budget.remaining().min(OUTPUT_CHUNK_BYTES);
            let Some(chunk) = self.send_chunk(stream, rest, limit)? else {
                break;
            };
            rest = &rest[chunk..];
        }
        Ok(())
    }

    /// Send at most `limit` bytes of `rest`, returning how many were sent.
    ///
    /// `None` means nothing could be sent, which ends the chunking loop.
    fn send_chunk(
        &mut self,
        stream: GuestStream,
        rest: &str,
        limit: usize,
    ) -> Result<Option<usize>, Fatal> {
        let (chunk, _) = split_prefix(rest, limit);
        if chunk.is_empty() {
            return Ok(None);
        }
        self.budget.consume(chunk.len());
        self.send(&HelperToParent::GuestOutput(GuestOutput {
            run_id: self.run_id.clone(),
            stream,
            text: chunk.to_owned(),
        }))?;
        Ok(Some(chunk.len()))
    }

    fn finish(&mut self, outcome: TerminalOutcome) -> Result<(), Fatal> {
        let terminal = Terminal {
            run_id: self.run_id.clone(),
            outcome,
        };
        self.send(&HelperToParent::Terminal(terminal))
    }

    /// Abort `call` and report the failure as the run's terminal state.
    fn abort_with(
        &mut self,
        call: FunctionCall,
        streams: &mut CollectedStreams,
        failure: Failure,
    ) -> CallReply {
        let remaining = self.budget.remaining();
        abort(call, streams, remaining, &failure.message);
        CallReply::Finished(failure.outcome)
    }

    /// Advance the run by one progress step.
    ///
    /// `None` means the parent cancelled the run, which ends it quietly; the
    /// caller then finishes without a terminal message of its own.
    fn step<R: BufRead>(
        &mut self,
        reader: &mut R,
        streams: &mut CollectedStreams,
        step: RunProgress,
    ) -> Result<Option<Result<RunProgress, MontyException>>, Fatal> {
        let print = self.printer(streams);
        Ok(match step {
            RunProgress::Complete(value) => {
                self.finish(completed(&value))?;
                None
            }
            RunProgress::FunctionCall(call) => {
                self.answer_call(reader, streams, call)?
            }
            RunProgress::NameLookup(lookup) => {
                Some(lookup.resume(NameLookupResult::Undefined, print))
            }
            RunProgress::OsCall(call) => Some(call.abort(os_denied(), print)),
            RunProgress::ResolveFutures(state) => {
                Some(state.abort(concurrent_denied(), print))
            }
        })
    }

    /// Answer one callback, returning the continuation or ending the run.
    fn answer_call<R: BufRead>(
        &mut self,
        reader: &mut R,
        streams: &mut CollectedStreams,
        call: FunctionCall,
    ) -> Result<Option<Result<RunProgress, MontyException>>, Fatal> {
        match self.handle_call(reader, streams, call)? {
            CallReply::Resume { call, value } => {
                let print = self.printer(streams);
                Ok(Some(call.resume_eager(Ok(value), print)))
            }
            CallReply::Finished(outcome) => {
                self.drain(streams)?;
                self.finish(outcome)?;
                Ok(None)
            }
        }
    }

    /// Answers one tool callback, reading the parent's reply.
    fn handle_call<R: BufRead>(
        &mut self,
        reader: &mut R,
        streams: &mut CollectedStreams,
        call: FunctionCall,
    ) -> Result<CallReply, Fatal> {
        let name = call.function_name.clone();
        if call.object_id.is_some() || !self.tools.contains(&name) {
            let message = format!("tool '{name}' is not available to this run");
            let outcome = TerminalOutcome::ToolError {
                code: DenialCode::NotAllowed,
                message: message.clone(),
            };
            return Ok(self.abort_with(call, streams, Failure::new(outcome, &message)));
        }
        let arguments = match convert::call_arguments(&call.args) {
            Ok(arguments) => arguments,
            Err(error) => {
                let message = truncate(error.message());
                let outcome = TerminalOutcome::ToolError {
                    code: DenialCode::ContractError,
                    message: message.clone(),
                };
                return Ok(self.abort_with(call, streams, Failure::new(outcome, &message)));
            }
        };
        self.next_call_id = self.next_call_id.checked_add(1).ok_or(EXIT_PROTOCOL)?;
        let call_id = self.next_call_id;
        self.send(&HelperToParent::HostCall(HostCall {
            run_id: self.run_id.clone(),
            call_id,
            name,
            arguments,
        }))?;

        let outcome = self.read_reply(reader, call_id)?;
        match outcome {
            ReplyOutcome::Cancelled => {
                Ok(self.abort_with(
                    call,
                    streams,
                    Failure::new(
                        TerminalOutcome::ScriptError {
                            message: "run cancelled by the parent".to_owned(),
                        },
                        "run cancelled by the parent",
                    ),
                ))
            }
            ReplyOutcome::Settled(outcome) => {
                Ok(self.settle(call, streams, *outcome))
            }
        }
    }

    /// Read the parent's answer to `call_id`, refusing any other message.
    fn read_reply<R: BufRead>(
        &self,
        reader: &mut R,
        call_id: u64,
    ) -> Result<ReplyOutcome, Fatal> {
        match read_parent(reader).ok_or(EXIT_PROTOCOL)? {
            ParentToHelper::ResumeRun(ResumeRun {
                run_id,
                call_id: answered,
                outcome,
            }) => {
                if run_id != self.run_id || answered != call_id {
                    return Err(EXIT_PROTOCOL);
                }
                Ok(ReplyOutcome::Settled(Box::new(outcome)))
            }
            ParentToHelper::CancelRun(cancel) if cancel.run_id == self.run_id => {
                Ok(ReplyOutcome::Cancelled)
            }
            _ => Err(EXIT_PROTOCOL),
        }
    }

    /// Turn the parent's outcome into the next guest state.
    fn settle(
        &mut self,
        call: FunctionCall,
        streams: &mut CollectedStreams,
        outcome: CallOutcome,
    ) -> CallReply {
        match outcome {
            CallOutcome::Value { value } => self.resume_with(call, streams, &value),
            CallOutcome::Terminate { code, message } => {
                let message = truncate(&message);
                let outcome = TerminalOutcome::ToolError { code, message: message.clone() };
                self.abort_with(call, streams, Failure::new(outcome, &message))
            }
        }
    }

    /// Resume the guest with the value the parent returned.
    fn resume_with(
        &mut self,
        call: FunctionCall,
        streams: &mut CollectedStreams,
        value: &Value,
    ) -> CallReply {
        let value = match convert::json_to_monty(value, 0) {
            Ok(value) => value,
            Err(error) => {
                let message = truncate(error.message());
                let outcome = TerminalOutcome::RuntimeCrash { message: message.clone() };
                return self.abort_with(call, streams, Failure::new(outcome, &message));
            }
        };
        if !call.allow_eager_await {
            let message = "concurrent tool calls are not supported by exec_code";
            let outcome = TerminalOutcome::ScriptError {
                message: message.to_owned(),
            };
            return self.abort_with(call, streams, Failure::new(outcome, message));
        }
        CallReply::Resume {
            call: Box::new(call),
            value,
        }
    }
}


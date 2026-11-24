//! The helper's protocol session.
//!
//! One process serves exactly one run: it announces itself, accepts one start
//! request, mediates any host callbacks, and reports one terminal outcome. The
//! parent owns every side effect; this module only translates between bounded
//! JSON frames and Monty's suspension points.

use std::collections::BTreeSet;
use std::io::{self, BufRead, Write};
use std::mem;

use monty::{FunctionCall, MontyRun, RunProgress};
use monty_protocol::{
    CallOutcome, DenialCode, GuestOutput, GuestStream, HelperToParent, HostCall, ParentToHelper,
    ResumeRun, StartRun, Terminal, TerminalOutcome, decode_frame, hello, read_frame,
    validate_start_run, write_frame,
};
use monty_types::{
    CollectedStreams, CompileOptions, MontyObject, NameLookupResult, PrintStream, PrintWriter,
    ResourceTracker,
};

use self::failures::{abort, concurrent_denied, os_denied, script_failure};
use self::output::{OutputBudget, split_prefix, truncate};
use crate::convert;
use crate::limits;
use crate::source;

mod failures;
mod output;

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
    /// The parent cancelled the run.
    Cancelled,
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
        let entries = mem::take(streams).into_entries();
        for (stream, text) in entries {
            let stream = match stream {
                PrintStream::Stdout => GuestStream::Stdout,
                PrintStream::Stderr => GuestStream::Stderr,
            };
            let mut rest = text.as_str();
            while !rest.is_empty() && self.budget.remaining() > 0 {
                let limit = self.budget.remaining().min(OUTPUT_CHUNK_BYTES);
                let (chunk, tail) = split_prefix(rest, limit);
                if chunk.is_empty() {
                    break;
                }
                self.budget.consume(chunk.len());
                self.send(&HelperToParent::GuestOutput(GuestOutput {
                    run_id: self.run_id.clone(),
                    stream,
                    text: chunk.to_owned(),
                }))?;
                rest = tail;
            }
        }
        Ok(())
    }

    fn finish(&mut self, outcome: TerminalOutcome) -> Result<(), Fatal> {
        let terminal = Terminal {
            run_id: self.run_id.clone(),
            outcome,
        };
        self.send(&HelperToParent::Terminal(terminal))
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
            let outcome = TerminalOutcome::ToolError {
                code: DenialCode::NotAllowed,
                message: format!("tool '{name}' is not available to this run"),
            };
            let remaining = self.budget.remaining();
            abort(
                call,
                streams,
                remaining,
                "tool is not available to this run",
            );
            return Ok(CallReply::Finished(outcome));
        }
        let arguments = match convert::call_arguments(&call.args) {
            Ok(arguments) => arguments,
            Err(error) => {
                let message = truncate(error.message());
                let remaining = self.budget.remaining();
                abort(call, streams, remaining, &message);
                let outcome = TerminalOutcome::ToolError {
                    code: DenialCode::ContractError,
                    message,
                };
                return Ok(CallReply::Finished(outcome));
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

        let message = read_parent(reader).ok_or(EXIT_PROTOCOL)?;
        let (run_id, answered, outcome) = match message {
            ParentToHelper::ResumeRun(ResumeRun {
                run_id,
                call_id,
                outcome,
            }) => (run_id, call_id, outcome),
            ParentToHelper::CancelRun(cancel) if cancel.run_id == self.run_id => {
                let remaining = self.budget.remaining();
                abort(call, streams, remaining, "run cancelled by the parent");
                return Ok(CallReply::Cancelled);
            }
            _ => return Err(EXIT_PROTOCOL),
        };
        if run_id != self.run_id || answered != call_id {
            return Err(EXIT_PROTOCOL);
        }
        match outcome {
            CallOutcome::Value { value } => {
                let value = match convert::json_to_monty(&value, 0) {
                    Ok(value) => value,
                    Err(error) => {
                        let message = truncate(error.message());
                        let remaining = self.budget.remaining();
                        abort(call, streams, remaining, &message);
                        let outcome = TerminalOutcome::RuntimeCrash { message };
                        return Ok(CallReply::Finished(outcome));
                    }
                };
                if !call.allow_eager_await {
                    let remaining = self.budget.remaining();
                    abort(
                        call,
                        streams,
                        remaining,
                        "concurrent tool calls are not supported by exec_code",
                    );
                    let outcome = TerminalOutcome::ScriptError {
                        message: "concurrent tool calls are not supported by exec_code".to_owned(),
                    };
                    return Ok(CallReply::Finished(outcome));
                }
                Ok(CallReply::Resume {
                    call: Box::new(call),
                    value,
                })
            }
            CallOutcome::Terminate { code, message } => {
                let message = truncate(&message);
                let remaining = self.budget.remaining();
                abort(call, streams, remaining, &message);
                Ok(CallReply::Finished(TerminalOutcome::ToolError {
                    code,
                    message,
                }))
            }
        }
    }
}

fn run_session<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    request: StartRun,
) -> Result<(), Fatal> {
    let mut session = Session::new(writer, &request);
    let wrapped = source::wrapper(&request.code, &request.entrypoint);
    let mut input_names = vec!["params".to_owned(), "state".to_owned()];
    input_names.extend(request.tools.iter().map(|tool| tool.name.clone()));
    let run = match MontyRun::new(
        wrapped,
        source::SCRIPT_NAME,
        input_names,
        CompileOptions::default(),
    ) {
        Ok(run) => run,
        Err(error) => return session.finish(script_failure(&error)),
    };
    let inputs = match build_inputs(&request) {
        Ok(inputs) => inputs,
        Err(message) => return session.finish(TerminalOutcome::ScriptError { message }),
    };
    let tracker = ResourceTracker::new(limits::resource_limits(&request.limits));
    let mut streams = CollectedStreams::default();
    let mut progress = {
        let print = session.printer(&mut streams);
        run.start(inputs, tracker, print)
    };
    loop {
        session.drain(&mut streams)?;
        let step = match progress {
            Ok(progress) => progress,
            Err(error) => return session.finish(script_failure(&error)),
        };
        progress = match step {
            RunProgress::Complete(value) => {
                let outcome = match convert::completed_members(&value) {
                    Ok((result, state)) => TerminalOutcome::Completed { result, state },
                    Err(error) => TerminalOutcome::ScriptError {
                        message: truncate(error.message()),
                    },
                };
                return session.finish(outcome);
            }
            RunProgress::FunctionCall(call) => {
                match session.handle_call(reader, &mut streams, call)? {
                    CallReply::Resume { call, value } => {
                        let print = session.printer(&mut streams);
                        call.resume_eager(Ok(value), print)
                    }
                    CallReply::Finished(outcome) => {
                        session.drain(&mut streams)?;
                        return session.finish(outcome);
                    }
                    CallReply::Cancelled => {
                        session.drain(&mut streams)?;
                        return Ok(());
                    }
                }
            }
            RunProgress::NameLookup(lookup) => {
                let print = session.printer(&mut streams);
                lookup.resume(NameLookupResult::Undefined, print)
            }
            RunProgress::OsCall(call) => {
                let print = session.printer(&mut streams);
                call.abort(os_denied(), print)
            }
            RunProgress::ResolveFutures(state) => {
                let print = session.printer(&mut streams);
                state.abort(concurrent_denied(), print)
            }
        };
    }
}

fn build_inputs(request: &StartRun) -> Result<Vec<MontyObject>, String> {
    let params =
        convert::json_to_monty(&request.params, 0).map_err(|error| error.message().to_owned())?;
    let state =
        convert::json_to_monty(&request.state, 0).map_err(|error| error.message().to_owned())?;
    let mut inputs = Vec::with_capacity(2 + request.tools.len());
    inputs.push(params);
    inputs.push(state);
    inputs.extend(request.tools.iter().map(|tool| {
        MontyObject::async_function(tool.name.clone(), Some(tool.description.clone()))
    }));
    Ok(inputs)
}

fn read_parent<R: BufRead>(reader: &mut R) -> Option<ParentToHelper> {
    let payload = read_frame(reader).ok()?;
    decode_frame(&payload).ok()
}

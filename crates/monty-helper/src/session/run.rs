//! One run: drive the guest from start to terminal.
//!
//! `run_session` compiles the guest, seeds its inputs, then hands each progress
//! step to [`Session::step`]. The types here pair a terminal outcome with the
//! message the guest is told and record what the parent did with a callback.

use std::io::{BufRead, Write};

use monty::MontyRun;
use monty_protocol::{
    CallOutcome, GuestStream, ParentToHelper, StartRun, TerminalOutcome, decode_frame, read_frame,
};
use monty_types::{CollectedStreams, CompileOptions, MontyObject, PrintStream, ResourceTracker};

use super::output::truncate;
use super::{Fatal, Session, failures::script_failure};
use crate::convert;
use crate::limits;
use crate::source;

/// What the parent did with a callback.
pub(super) enum ReplyOutcome {
    /// The parent answered with an outcome to apply.
    Settled(Box<CallOutcome>),
    /// The parent cancelled the run instead of answering.
    Cancelled,
}

/// A terminal outcome together with the message the guest is told about it.
pub(super) struct Failure {
    pub(super) outcome: TerminalOutcome,
    pub(super) message: String,
}

impl Failure {
    /// Pair `outcome` with the `message` that explains it.
    pub(super) fn new(outcome: TerminalOutcome, message: &str) -> Self {
        Self {
            outcome,
            message: message.to_owned(),
        }
    }
}


pub(super) fn run_session<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    request: StartRun,
) -> Result<(), Fatal> {
    let mut session = Session::new(writer, &request);
    let run = match compile(&request) {
        Ok(run) => run,
        Err(outcome) => return session.finish(outcome),
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
        let Some(next) = session.step(reader, &mut streams, step)? else {
            return Ok(());
        };
        progress = next;
    }
}

/// Map Monty's print stream to the wire stream the parent reads.
pub(super) fn guest_stream(stream: PrintStream) -> GuestStream {
    match stream {
        PrintStream::Stdout => GuestStream::Stdout,
        PrintStream::Stderr => GuestStream::Stderr,
    }
}

/// Compile the guest program, or describe why it could not be compiled.
fn compile(request: &StartRun) -> Result<MontyRun, TerminalOutcome> {
    let mut input_names = vec!["params".to_owned(), "state".to_owned()];
    input_names.extend(request.tools.iter().map(|tool| tool.name.clone()));
    MontyRun::new(
        source::wrapper(&request.code, &request.entrypoint),
        source::SCRIPT_NAME,
        input_names,
        CompileOptions::default(),
    )
    .map_err(|error| script_failure(&error))
}

/// Describe a completed run, or the contract error in its returned members.
pub(super) fn completed(value: &MontyObject) -> TerminalOutcome {
    match convert::completed_members(value) {
        Ok((result, state)) => TerminalOutcome::Completed { result, state },
        Err(error) => TerminalOutcome::ScriptError {
            message: truncate(error.message()),
        },
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

pub(super) fn read_parent<R: BufRead>(reader: &mut R) -> Option<ParentToHelper> {
    let payload = read_frame(reader).ok()?;
    decode_frame(&payload).ok()
}

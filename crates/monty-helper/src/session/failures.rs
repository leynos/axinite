//! Monty exceptions and terminal outcomes the helper reports.

use monty::FunctionCall;
use monty_protocol::TerminalOutcome;
use monty_types::{CollectedStreams, ExcType, MontyException, PrintWriter};

use super::output::truncate;

pub(super) fn abort(
    call: FunctionCall,
    streams: &mut CollectedStreams,
    remaining: usize,
    message: &str,
) {
    let print = PrintWriter::CollectStreams(streams, Some(remaining));
    let exception = MontyException::new(ExcType::RuntimeError, Some(message.to_owned()));
    let _ = call.abort(exception, print);
}

pub(super) fn os_denied() -> MontyException {
    MontyException::new(
        ExcType::RuntimeError,
        Some("operating-system access is not available in exec_code".to_owned()),
    )
}

pub(super) fn concurrent_denied() -> MontyException {
    MontyException::new(
        ExcType::RuntimeError,
        Some("concurrent tool calls are not supported by exec_code".to_owned()),
    )
}

pub(super) fn script_failure(error: &MontyException) -> TerminalOutcome {
    let message = truncate(&error.summary());
    match error.exc_type() {
        ExcType::MemoryError | ExcType::RecursionError | ExcType::TimeoutError => {
            TerminalOutcome::ResourceLimitExceeded { message }
        }
        _ => TerminalOutcome::ScriptError { message },
    }
}

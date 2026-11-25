//! Supervises one helper process per run.
//!
//! The supervisor owns admission, the process, its pipes and the run deadline.
//! Every exit path either sees the helper terminate or kills and reaps it
//! before returning, and a dropped future kills the child rather than orphaning
//! it. Side effects already performed by a tool callback are never replayed.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use monty_protocol::{
    CallOutcome, GuestLimits, HelperToParent, HostCall, MAX_FRAME_BYTES, PROTOCOL_VERSION,
    ParentToHelper, ResumeRun, StartRun, TerminalOutcome,
};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::Semaphore;

use self::bounds::{append_bounded, truncate};
use self::process::{drain_stderr, finish_child, read_message, spawn, terminate, write_message};
use crate::context::JobContext;
use crate::tools::monty::broker::{BrokerContext, BrokerDenial};

mod bounds;
mod process;
mod program;

pub use self::program::{HELPER_ENV, HELPER_NAME, HelperProgram};

/// Concurrent helper processes admitted across the whole process.
pub const MAX_CONCURRENT_RUNS: usize = 4;

/// Bytes of guest output retained before further output is discarded.
pub const MAX_CAPTURED_OUTPUT_BYTES: usize = 64 * 1024;

/// The largest result or state value the helper may return.
const MAX_TERMINAL_BYTES: usize = MAX_FRAME_BYTES;

/// Limits the parent asks the helper to enforce, plus its own deadline.
#[derive(Debug, Clone, Copy)]
pub struct RunLimits {
    /// Guest allocator ceiling.
    pub max_memory_bytes: u64,
    /// Python call-stack depth.
    pub max_recursion_depth: u64,
    /// Host suspensions the run may service.
    pub max_suspensions: u64,
    /// One feed's execution budget.
    pub max_feed_millis: u64,
    /// One host turn's execution budget.
    pub max_turn_millis: u64,
    /// Cumulative guest output.
    pub max_output_bytes: u64,
    /// Total wall-clock budget from admission.
    pub deadline: Duration,
}

impl Default for RunLimits {
    fn default() -> Self {
        Self {
            max_memory_bytes: 64 * 1024 * 1024,
            max_recursion_depth: 256,
            max_suspensions: 4096,
            max_feed_millis: 30_000,
            max_turn_millis: 30_000,
            max_output_bytes: MAX_CAPTURED_OUTPUT_BYTES as u64,
            deadline: Duration::from_secs(120),
        }
    }
}

impl RunLimits {
    fn guest(&self) -> GuestLimits {
        GuestLimits {
            max_memory_bytes: self.max_memory_bytes,
            max_recursion_depth: self.max_recursion_depth,
            max_suspensions: self.max_suspensions,
            max_feed_millis: self.max_feed_millis,
            max_turn_millis: self.max_turn_millis,
            max_output_bytes: self.max_output_bytes,
        }
    }
}

/// Everything the helper needs to start one run.
#[derive(Debug, Clone)]
pub struct RunRequest {
    /// Parent-issued run identity.
    pub run_id: String,
    /// The guest entrypoint body.
    pub code: String,
    /// The entrypoint name.
    pub entrypoint: String,
    /// Input parameters.
    pub params: Value,
    /// Initial state object.
    pub state: Value,
    /// Guest-side limits.
    pub limits: RunLimits,
}

/// A successful run's observable result.
#[derive(Debug, Clone, PartialEq)]
pub struct RunOutcome {
    /// The guest's result member, or null.
    pub result: Value,
    /// The guest's returned state object.
    pub state: Value,
    /// Bounded guest print output.
    pub output: String,
}

/// Why a run did not produce a result.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// The helper executable could not be found.
    #[error("the Monty helper is not available")]
    HelperUnavailable,
    /// Admission is saturated.
    #[error("too many code runs are already active")]
    Overloaded,
    /// The process could not be started.
    #[error("cannot start the Monty helper")]
    Spawn(#[source] std::io::Error),
    /// The helper broke the protocol or exited unexpectedly.
    #[error("the Monty helper failed: {0}")]
    RuntimeCrash(String),
    /// The guest raised an exception.
    #[error("the guest script failed: {0}")]
    ScriptError(String),
    /// A host callback was refused.
    #[error("a tool callback was refused: {0}")]
    ToolDenied(BrokerDenial),
    /// Monty reported an internal limit.
    #[error("the guest exceeded a resource limit: {0}")]
    ResourceLimitExceeded(String),
    /// The run exceeded its wall-clock budget.
    #[error("the code run exceeded its deadline")]
    Timeout,
}

/// The process-wide admission pool.
fn permits() -> Arc<Semaphore> {
    static PERMITS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    Arc::clone(PERMITS.get_or_init(|| Arc::new(Semaphore::new(MAX_CONCURRENT_RUNS))))
}

/// Admission and supervision for one-shot helper runs.
pub struct Supervisor {
    permits: Arc<Semaphore>,
    program: HelperProgram,
}

impl Supervisor {
    /// Creates a supervisor for the resolved helper.
    ///
    /// # Errors
    ///
    /// Returns [`RunError`] when no executable is available.
    pub fn resolve() -> Result<Self, RunError> {
        Ok(Self {
            permits: permits(),
            program: HelperProgram::resolve()?,
        })
    }

    /// Creates a supervisor for an explicit executable.
    pub fn with_program(program: HelperProgram) -> Self {
        Self {
            permits: permits(),
            program,
        }
    }

    /// Runs one request, admitting it or failing fast when saturated.
    ///
    /// # Errors
    ///
    /// Returns [`RunError`] for admission, spawn, protocol, timeout, script
    /// and callback failures.
    pub async fn run(
        &self,
        request: RunRequest,
        broker: &Arc<BrokerContext>,
        job_ctx: &JobContext,
    ) -> Result<RunOutcome, RunError> {
        let permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| RunError::Overloaded)?;
        // The deadline is enforced inside `run_admitted`, which owns the child.
        // The permit is therefore released only after the helper has been
        // killed and reaped, never while a timed-out run is still winding down.
        let result = self.run_admitted(request, broker, job_ctx).await;
        drop(permit);
        result
    }

    async fn run_admitted(
        &self,
        request: RunRequest,
        broker: &Arc<BrokerContext>,
        job_ctx: &JobContext,
    ) -> Result<RunOutcome, RunError> {
        let guest_limits = request.limits.guest();
        let deadline = request.limits.deadline;
        let mut child = spawn(&self.program).await?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| RunError::RuntimeCrash("helper stdin was not piped".to_string()))?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| RunError::RuntimeCrash("helper stdout was not piped".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| RunError::RuntimeCrash("helper stderr was not piped".to_string()))?;
        let stderr_task = tokio::spawn(drain_stderr(stderr));

        let run = tokio::time::timeout(deadline, async {
            match read_message(&mut stdout).await? {
                HelperToParent::Hello(hello) if hello.protocol_version == PROTOCOL_VERSION => {}
                _ => {
                    return Err(RunError::RuntimeCrash(
                        "unexpected helper greeting".to_string(),
                    ));
                }
            }
            let start = StartRun {
                run_id: request.run_id.clone(),
                code: request.code.clone(),
                entrypoint: request.entrypoint.clone(),
                params: request.params.clone(),
                state: request.state.clone(),
                tools: broker.table().definitions(),
                limits: guest_limits,
            };
            write_message(&mut stdin, &ParentToHelper::StartRun(start)).await?;
            Session {
                stdin: &mut stdin,
                stdout: &mut stdout,
                request: &request,
                broker,
                job_ctx,
            }
            .drive()
            .await
        })
        .await;

        stderr_task.abort();
        match run {
            Ok(Ok(outcome)) => {
                let _ = stdin.shutdown().await;
                finish_child(&mut child).await;
                Ok(outcome)
            }
            Ok(Err(error)) => {
                terminate(&mut child, &request.run_id, &mut stdin).await;
                Err(error)
            }
            Err(_) => {
                terminate(&mut child, &request.run_id, &mut stdin).await;
                Err(RunError::Timeout)
            }
        }
    }
}

/// The pipes one run talks over, and the authority it talks with.
struct Session<'a, W, R> {
    stdin: &'a mut W,
    stdout: &'a mut R,
    request: &'a RunRequest,
    broker: &'a Arc<BrokerContext>,
    job_ctx: &'a JobContext,
}

impl<W: AsyncWrite + Unpin, R: AsyncRead + Unpin> Session<'_, W, R> {
    /// Drive one run to its terminal message.
    async fn drive(&mut self) -> Result<RunOutcome, RunError> {
        let mut output = String::new();
        let mut next_call_id = 0_u64;
        loop {
            match read_message(self.stdout).await? {
                HelperToParent::GuestOutput(chunk) => {
                    self.check_run(&chunk.run_id, "output for another run")?;
                    append_bounded(&mut output, &chunk.text, MAX_CAPTURED_OUTPUT_BYTES);
                }
                HelperToParent::HostCall(call) => {
                    // `next_call_id` starts at zero, so `call_id <=
                    // next_call_id` already rejects the reserved id zero as
                    // well as any repeat.
                    if call.call_id <= next_call_id {
                        return Err(RunError::RuntimeCrash(
                            "unexpected host callback".to_string(),
                        ));
                    }
                    self.check_run(&call.run_id, "callback for another run")?;
                    next_call_id = call.call_id;
                    self.answer(call).await?;
                }
                HelperToParent::Terminal(terminal) => {
                    self.check_run(&terminal.run_id, "terminal for another run")?;
                    return terminal_outcome(terminal.outcome, output);
                }
                HelperToParent::Hello(_) => {
                    return Err(RunError::RuntimeCrash(
                        "duplicate helper greeting".to_string(),
                    ));
                }
            }
        }
    }

    /// Refuse a message that belongs to a different run.
    fn check_run(&self, run_id: &str, message: &str) -> Result<(), RunError> {
        if run_id == self.request.run_id {
            return Ok(());
        }
        Err(RunError::RuntimeCrash(message.to_string()))
    }

    /// Mediate one callback and send the parent's outcome back.
    async fn answer(&mut self, call: HostCall) -> Result<(), RunError> {
        let outcome = match self
            .broker
            .mediate(&call.name, &call.arguments, self.job_ctx)
            .await
        {
            Ok(value) => CallOutcome::Value { value },
            Err(denial) => CallOutcome::Terminate {
                code: denial.code,
                message: truncate(denial.message),
            },
        };
        write_message(
            self.stdin,
            &ParentToHelper::ResumeRun(ResumeRun {
                run_id: self.request.run_id.clone(),
                call_id: call.call_id,
                outcome,
            }),
        )
        .await
    }
}

fn terminal_outcome(outcome: TerminalOutcome, output: String) -> Result<RunOutcome, RunError> {
    match outcome {
        TerminalOutcome::Completed { result, state } => {
            if serde_json::to_vec(&result).map_or(true, |bytes| bytes.len() > MAX_TERMINAL_BYTES)
                || serde_json::to_vec(&state).map_or(true, |bytes| bytes.len() > MAX_TERMINAL_BYTES)
            {
                return Err(RunError::RuntimeCrash(
                    "helper returned an oversized result".to_string(),
                ));
            }
            Ok(RunOutcome {
                result,
                state,
                output,
            })
        }
        TerminalOutcome::ScriptError { message } => Err(RunError::ScriptError(truncate(message))),
        TerminalOutcome::ToolError { code, message } => Err(RunError::ToolDenied(BrokerDenial {
            code,
            message: truncate(message),
        })),
        TerminalOutcome::ResourceLimitExceeded { message } => {
            Err(RunError::ResourceLimitExceeded(truncate(message)))
        }
        TerminalOutcome::RuntimeCrash { message } => Err(RunError::RuntimeCrash(truncate(message))),
    }
}

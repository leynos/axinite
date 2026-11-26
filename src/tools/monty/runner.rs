//! Supervises one helper process per run.
//!
//! The supervisor owns admission, the process, its pipes and the run deadline.
//! Every exit path either sees the helper terminate or kills and reaps it
//! before returning, and a dropped future kills the child rather than orphaning
//! it. Side effects already performed by a tool callback are never replayed.

use std::sync::{Arc, OnceLock};

use monty_protocol::{
    CallOutcome, GuestLimits, HelperToParent, HostCall, MAX_FRAME_BYTES, PROTOCOL_VERSION,
    ParentToHelper, ResumeRun, StartRun, TerminalOutcome,
};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use self::bounds::{append_bounded, truncate};
use self::process::{
    ChildGuard, Pipes, drain_stderr, finish_child, read_message, spawn, terminate, write_message,
};
use crate::context::JobContext;
use crate::tools::monty::broker::{BrokerContext, BrokerDenial};

mod bounds;
mod process;
mod program;
mod request;

pub use request::{RunError, RunLimits, RunOutcome, RunRequest};

pub use self::program::{HELPER_ENV, HELPER_NAME, HelperProgram};

/// Concurrent helper processes admitted across the whole process.
pub const MAX_CONCURRENT_RUNS: usize = 4;

/// Bytes of guest output retained before further output is discarded.
pub const MAX_CAPTURED_OUTPUT_BYTES: usize = 64 * 1024;

/// The largest result or state value the helper may return.
const MAX_TERMINAL_BYTES: usize = MAX_FRAME_BYTES;

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
        // The permit moves into the guard, so it is released by the same
        // cleanup that reaps the child — including when this future is
        // cancelled. `run_admitted` therefore cannot release it early.
        self.run_admitted(request, broker, job_ctx, permit).await
    }

    async fn run_admitted(
        &self,
        request: RunRequest,
        broker: &Arc<BrokerContext>,
        job_ctx: &JobContext,
        permit: OwnedSemaphorePermit,
    ) -> Result<RunOutcome, RunError> {
        let guest_limits = request.limits.guest();
        let deadline = request.limits.deadline;
        let spawned = spawn(&self.program).await?;
        // The guard reaps the child if this future is dropped mid-run, so the
        // permit above is never released while a helper is still exiting.
        let mut guarded = ChildGuard::new(spawned, permit);
        let Pipes {
            mut stdin,
            mut stdout,
            stderr,
        } = guarded.take_pipes()?;
        let stderr_task = tokio::spawn(drain_stderr(stderr));

        let conversation = Conversation {
            request: &request,
            broker,
            job_ctx,
        };
        let run = tokio::time::timeout(deadline, async {
            greet(&mut stdout).await?;
            write_message(
                &mut stdin,
                &ParentToHelper::StartRun(conversation.start(guest_limits)),
            )
            .await?;
            Session {
                stdin: &mut stdin,
                stdout: &mut stdout,
                request: conversation.request,
                broker: conversation.broker,
                job_ctx: conversation.job_ctx,
            }
            .drive()
            .await
        })
        .await;

        stderr_task.abort();
        let settled = Settled {
            run,
            guarded,
            stdin,
        };
        settled.finish(&request.run_id).await
    }
}

/// The caller-side values one run's conversation carries.
struct Conversation<'a> {
    request: &'a RunRequest,
    broker: &'a Arc<BrokerContext>,
    job_ctx: &'a JobContext,
}

impl Conversation<'_> {
    /// The start message this run sends.
    fn start(&self, limits: GuestLimits) -> StartRun {
        StartRun {
            run_id: self.request.run_id.clone(),
            code: self.request.code.clone(),
            entrypoint: self.request.entrypoint.clone(),
            params: self.request.params.clone(),
            state: self.request.state.clone(),
            tools: self.broker.table().definitions(),
            limits,
        }
    }
}

/// Refuse a helper that does not greet with this protocol's version.
async fn greet<R: AsyncRead + Unpin>(stdout: &mut R) -> Result<(), RunError> {
    match read_message(stdout).await? {
        HelperToParent::Hello(hello) if hello.protocol_version == PROTOCOL_VERSION => Ok(()),
        _ => Err(RunError::RuntimeCrash(
            "unexpected helper greeting".to_string(),
        )),
    }
}

/// Everything one run owns once its conversation has finished.
struct Settled<W> {
    run: Result<Result<RunOutcome, RunError>, tokio::time::error::Elapsed>,
    guarded: ChildGuard,
    stdin: W,
}

impl<W: AsyncWrite + Unpin> Settled<W> {
    /// Close the run out, reaping the child on every path.
    async fn finish(mut self, run_id: &str) -> Result<RunOutcome, RunError> {
        match self.run {
            Ok(Ok(outcome)) => {
                let _ = self.stdin.shutdown().await;
                finish_child(self.guarded.child()).await;
                self.guarded.reap().await;
                Ok(outcome)
            }
            Ok(Err(error)) => {
                terminate(self.guarded.child(), run_id, &mut self.stdin).await;
                self.guarded.reap().await;
                Err(error)
            }
            Err(_) => {
                terminate(self.guarded.child(), run_id, &mut self.stdin).await;
                self.guarded.reap().await;
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

/// The run-wide callback budgets the parent enforces itself.
///
/// The helper counts suspensions too, but the parent must not rely on a
/// process it is isolating from: a non-conforming helper could otherwise make
/// unbounded callbacks and unbounded bytes cross the boundary.
struct CallbackBudget {
    remaining_calls: u64,
    remaining_bytes: u64,
}

impl CallbackBudget {
    /// Take one callback and its encoded size out of the budget.
    fn charge(&mut self, call: &HostCall) -> Result<(), RunError> {
        if self.remaining_calls == 0 {
            return Err(RunError::RuntimeCrash(
                "run exceeded its callback budget".to_string(),
            ));
        }
        let size = encoded_size(call);
        self.charge_bytes(size)?;
        self.remaining_calls -= 1;
        Ok(())
    }

    /// Charge the value the parent returned for one callback.
    ///
    /// Both directions count: a helper that made many small calls could
    /// otherwise return far more than the budget intended, because the request
    /// side alone is bounded.
    fn charge_result(&mut self, value: &serde_json::Value) -> Result<(), RunError> {
        let size = encoded_size(value);
        self.charge_bytes(size)
    }

    /// Deduct `size` from the byte budget, refusing the run if it overflows.
    fn charge_bytes(&mut self, size: u64) -> Result<(), RunError> {
        if size > self.remaining_bytes {
            return Err(RunError::RuntimeCrash(
                "run exceeded its callback byte budget".to_string(),
            ));
        }
        self.remaining_bytes -= size;
        Ok(())
    }
}

/// The encoded size of a value, saturating so a serialization failure is
/// charged as the whole budget rather than silently free.
fn encoded_size<T: serde::Serialize>(value: &T) -> u64 {
    serde_json::to_vec(value)
        .map(|encoded| encoded.len() as u64)
        .unwrap_or(u64::MAX)
}

impl<W: AsyncWrite + Unpin, R: AsyncRead + Unpin> Session<'_, W, R> {
    /// Drive one run to its terminal message.
    ///
    /// A refusal sent back to the helper ends the run: continuing to read
    /// would let a non-conforming helper keep asking after its denial.
    async fn drive(&mut self) -> Result<RunOutcome, RunError> {
        let mut output = String::new();
        let mut next_call_id = 0_u64;
        let mut budget = CallbackBudget {
            remaining_calls: self.request.limits.max_suspensions,
            remaining_bytes: u64::try_from(MAX_FRAME_BYTES).unwrap_or(u64::MAX),
        };
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
                    budget.charge(&call)?;
                    next_call_id = call.call_id;
                    if let Some(denial) = self.answer(call, &mut budget).await? {
                        return Err(denial);
                    }
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
    ///
    /// A denial is terminal: the outcome is sent so the guest sees the typed
    /// refusal, and the run ends there rather than waiting for another
    /// message. `None` means the callback succeeded and the run continues.
    async fn answer(
        &mut self,
        call: HostCall,
        budget: &mut CallbackBudget,
    ) -> Result<Option<RunError>, RunError> {
        let (outcome, denial) = match self
            .broker
            .mediate(&call.name, &call.arguments, self.job_ctx)
            .await
        {
            Ok(value) => {
                budget.charge_result(&value)?;
                (CallOutcome::Value { value }, None)
            }
            Err(denial) => {
                let reported = RunError::ToolDenied(denial.clone());
                (
                    CallOutcome::Terminate {
                        code: denial.code,
                        message: truncate(denial.message),
                    },
                    Some(reported),
                )
            }
        };
        write_message(
            self.stdin,
            &ParentToHelper::ResumeRun(ResumeRun {
                run_id: self.request.run_id.clone(),
                call_id: call.call_id,
                outcome,
            }),
        )
        .await?;
        Ok(denial)
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

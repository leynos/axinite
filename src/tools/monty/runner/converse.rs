//! One run's caller-side values, greeting and close-out.
//!
//! `run_admitted` drives these, so the supervisor's own file stays a state
//! machine over the helper's frames rather than a mix of protocol, process and
//! message construction.

use std::sync::Arc;

use monty_protocol::{GuestLimits, HelperToParent, PROTOCOL_VERSION, StartRun};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

use super::process::{ChildGuard, finish_child, read_message, terminate};
use crate::context::JobContext;

use super::{BrokerContext, RunError, RunOutcome, RunRequest};

/// The caller-side values one run's conversation carries.
pub(super) struct Conversation<'a> {
    pub(super) request: &'a RunRequest,
    pub(super) broker: &'a Arc<BrokerContext>,
    pub(super) job_ctx: &'a JobContext,
}

impl Conversation<'_> {
    /// The start message this run sends.
    pub(super) fn start(&self, limits: GuestLimits) -> StartRun {
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
pub(super) async fn greet<R: AsyncRead + Unpin>(stdout: &mut R) -> Result<(), RunError> {
    match read_message(stdout).await? {
        HelperToParent::Hello(hello) if hello.protocol_version == PROTOCOL_VERSION => Ok(()),
        _ => Err(RunError::RuntimeCrash(
            "unexpected helper greeting".to_string(),
        )),
    }
}

/// Everything one run owns once its conversation has finished.
pub(super) struct Settled<W> {
    pub(super) run: Result<Result<RunOutcome, RunError>, tokio::time::error::Elapsed>,
    pub(super) guarded: ChildGuard,
    pub(super) stdin: W,
}

impl<W: AsyncWrite + Unpin> Settled<W> {
    /// Close the run out, reaping the child on every path.
    pub(super) async fn finish(mut self, run_id: &str) -> Result<RunOutcome, RunError> {
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

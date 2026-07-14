//! Routine execution engine.
//!
//! Handles loading routines, checking triggers, enforcing guardrails,
//! and executing both lightweight (single LLM call) and full-job routines.
//!
//! The engine runs two independent loops:
//! - A **cron ticker** that polls the DB every N seconds for due cron routines
//! - An **event matcher** called synchronously from the agent main loop
//!
//! Lightweight routines execute inline (single LLM call, no scheduler slot).
//! Full-job routines are delegated to the existing `Scheduler`.
//!
//! Module layout:
//! - [`engine`]: the `RoutineEngine` type, guardrails, and the cron ticker
//! - [`triggers`]: event/cron trigger matching and firing
//! - [`execution`]: run execution, finalization, and notifications
//! - [`lightweight`] / [`lightweight_tools`]: lightweight routine execution

mod engine;
mod execution;
mod lightweight;
mod lightweight_tools;
mod triggers;

pub use engine::{RoutineEngine, spawn_cron_ticker};

#[cfg(test)]
mod tests;

//! Test-only database doubles and captured-call helpers.
//!
//! [`NullDatabase`] provides null defaults across the `Native*Store` traits for
//! bespoke mocks, while [`CapturingStore`] wraps that baseline with captured
//! [`Calls`], [`EventCall`], [`EventCallWithId`], [`StatusCall`], and
//! [`StatusCallWithId`] records for persistence assertions.
//!
//! Choose the right testing abstraction for the job: use
//! [`crate::testing::TestHarnessBuilder`] for persistence testing with a real
//! database, [`CapturingStore`] for verifying calls without durable storage,
//! or [`NullDatabase`] when a test needs a custom mock with null behaviour.

mod capturing_store;
mod null_database;

pub use capturing_store::{
    Calls, CapturingStore, EventCall, EventCallWithId, StatusCall, StatusCallWithId,
};
pub use null_database::NullDatabase;

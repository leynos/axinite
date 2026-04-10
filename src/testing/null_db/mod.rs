//! Null database helper for tests.
//!
//! Provides a [`NullDatabase`] struct that implements all `Native*Store` traits
//! with no-op methods returning default values. Useful as a baseline for
//! test doubles that need to override only specific methods.

mod capturing_store;
mod null_database;

pub use capturing_store::{
    Calls, CapturingStore, EventCall, EventCallWithId, StatusCall, StatusCallWithId,
};
pub use null_database::NullDatabase;

//! Unit tests for LLM provider failover behaviour.
//!
//! - [`mocks`] — mock providers shared across the test modules
//! - [`basic`] — core failover sequencing and model tracking tests
//! - [`cooldown`] — cooldown activation, expiry, and threshold tests
//! - [`chaos`] — provider chaos and edge-case tests

mod basic;
mod chaos;
mod cooldown;
mod mocks;

//! Unit tests for session reuse and per-channel thread resolution.
//!
//! Cases are grouped by feature area: thread resolution, hydrated thread
//! registration, stale-session pruning, and concurrency stress tests.

mod concurrency;
mod pruning;
mod registration;
mod resolution;

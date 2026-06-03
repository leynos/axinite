//! Monotonic clocks for measuring build tool execution duration.
//!
//! [`BuildSoftwareTool`](super::BuildSoftwareTool) needs elapsed durations for
//! tool output, but wall-clock timestamps can move backwards when the host
//! clock is adjusted. [`MonotonicClock`] is the narrow port that keeps duration
//! measurement on [`Instant`] while still allowing tests to inject deterministic
//! instants.
//!
//! [`StdMonotonicClock`] is the production adapter over [`Instant::now`].
//! [`FixedMonotonicClock`] is compiled only for tests and supplies a queued
//! pair of instants so wrapper tests can assert the exact elapsed duration
//! without depending on real time. The fixed clock is synchronized with a mutex,
//! so it satisfies the same `Send + Sync` contract as the production clock.

use std::time::Instant;

/// Clock abstraction for monotonic elapsed-time measurements.
pub trait MonotonicClock: Send + Sync {
    /// Returns the current monotonic instant.
    fn now(&self) -> Instant;
}

/// Monotonic clock backed by [`Instant::now`].
pub struct StdMonotonicClock;

impl MonotonicClock for StdMonotonicClock {
    /// Returns the current monotonic instant via [`Instant::now`].
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// Deterministic monotonic clock for tests.
///
/// The clock returns queued instants in insertion order. Exhausting the queue
/// is a test configuration error and panics immediately, after releasing the
/// queue lock, so under-provisioned tests fail at the call site that consumed
/// too many instants.
#[cfg(test)]
pub struct FixedMonotonicClock {
    instants: std::sync::Mutex<std::collections::VecDeque<Instant>>,
}

#[cfg(test)]
impl FixedMonotonicClock {
    /// Creates a fixed clock that reports `elapsed` between two `now` calls.
    pub fn with_elapsed(elapsed: std::time::Duration) -> Self {
        let start = Instant::now();
        Self {
            instants: std::sync::Mutex::new(std::collections::VecDeque::from([
                start,
                start + elapsed,
            ])),
        }
    }
}

#[cfg(test)]
impl MonotonicClock for FixedMonotonicClock {
    /// Pops and returns the next pre-seeded instant.
    ///
    /// Panics if the queue is exhausted, signalling a test misconfiguration.
    fn now(&self) -> Instant {
        let next = self
            .instants
            .lock()
            .expect("fixed monotonic clock mutex should not be poisoned")
            .pop_front();
        next.unwrap_or_else(|| {
            panic!("deterministic clock queue exhausted: no instant available - test misconfigured")
        })
    }
}

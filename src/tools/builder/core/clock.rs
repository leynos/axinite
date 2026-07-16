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
/// is a test configuration error; the clock logs the misconfiguration and
/// repeats the final queued instant so elapsed measurements stay
/// deterministic rather than panicking.
#[cfg(test)]
pub struct FixedMonotonicClock {
    instants: std::sync::Mutex<std::collections::VecDeque<Instant>>,
    /// Instant repeated once the queue is exhausted.
    fallback: Instant,
}

#[cfg(test)]
impl FixedMonotonicClock {
    /// Creates a fixed clock that reports `elapsed` between two `now` calls.
    pub fn with_elapsed(elapsed: std::time::Duration) -> Self {
        let start = Instant::now();
        let last = start + elapsed;
        Self {
            instants: std::sync::Mutex::new(std::collections::VecDeque::from([start, last])),
            fallback: last,
        }
    }
}

#[cfg(test)]
impl MonotonicClock for FixedMonotonicClock {
    /// Pops and returns the next pre-seeded instant.
    ///
    /// Once the queue is exhausted, logs the misconfiguration and repeats the
    /// final queued instant so measurements remain deterministic.
    fn now(&self) -> Instant {
        let next = self
            .instants
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front();
        let Some(instant) = next else {
            tracing::error!(
                "deterministic clock queue exhausted: no instant available - test misconfigured"
            );
            return self.fallback;
        };
        instant
    }
}

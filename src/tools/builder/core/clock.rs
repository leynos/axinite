//! Monotonic clocks for measuring build tool execution duration.

use std::time::Instant;

/// Clock abstraction for monotonic elapsed-time measurements.
pub trait MonotonicClock: Send + Sync {
    /// Returns the current monotonic instant.
    fn now(&self) -> Instant;
}

/// Monotonic clock backed by [`Instant::now`].
pub struct StdMonotonicClock;

impl MonotonicClock for StdMonotonicClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

#[cfg(test)]
pub struct FixedMonotonicClock {
    instants: std::sync::Mutex<std::collections::VecDeque<Instant>>,
}

#[cfg(test)]
impl FixedMonotonicClock {
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
    fn now(&self) -> Instant {
        self.instants
            .lock()
            .expect("fixed monotonic clock mutex should not be poisoned")
            .pop_front()
            .unwrap_or_else(|| {
                panic!(
                    "deterministic clock queue exhausted: no instant available - test misconfigured"
                )
            })
    }
}

//! In-process metrics recorder for asserting emitted counters in tests.
//!
//! Only compiled when the `metrics` feature is enabled. The recorder is
//! installed once as the process-global recorder, but it only records while a
//! [`record`] guard is alive on the current thread. That keeps tests that
//! assert on counters isolated from unrelated tests in the same process that
//! also reach a telemetry call site: an increment from another thread is
//! ignored even while a guard is held elsewhere.
//!
//! The repository's `#[tokio::test]` tests use the current-thread runtime, so
//! the production path they drive runs on the thread that holds the guard.
//! A test that moves the production path onto a multi-thread runtime must
//! acquire the guard on the thread that performs the work.
//!
//! ```ignore
//! let _guard = metrics::record();
//! production_path().await;
//! assert_eq!(metrics::counter("axinite.startup.config_load", "success"), 1);
//! ```

use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, Once, OnceLock};

use metrics::{
    Counter, CounterFn, Gauge, Histogram, Key, KeyName, Metadata, Recorder, SharedString, Unit,
};

thread_local! {
    /// Whether the current thread is inside a recording guard.
    static RECORDING: Cell<bool> = const { Cell::new(false) };
}

/// Serializes recording guards so counter snapshots are not interleaved.
static RECORD_LOCK: Mutex<()> = Mutex::new(());

/// Installs the test recorder exactly once.
static INSTALL: Once = Once::new();

type Counters = BTreeMap<String, u64>;

fn counters() -> &'static Mutex<Counters> {
    static COUNTERS: OnceLock<Mutex<Counters>> = OnceLock::new();
    COUNTERS.get_or_init(|| Mutex::new(Counters::new()))
}

fn counters_lock() -> MutexGuard<'static, Counters> {
    counters()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Whether the current thread is inside a recording guard.
fn recording() -> bool {
    RECORDING.with(Cell::get)
}

/// Install the recorder and begin capturing counters on this thread.
///
/// Only one guard can be live at a time; a second call blocks until the first
/// guard is dropped. Counters are cleared when the guard is acquired.
pub fn record() -> RecordingGuard {
    INSTALL.call_once(|| {
        // A recorder may already be installed by another harness; in that case
        // this test cannot capture counters and the assertion fails loudly.
        let _ = metrics::set_global_recorder(TestRecorder);
    });
    let lock = RECORD_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    counters_lock().clear();
    RECORDING.with(|flag| flag.set(true));
    RecordingGuard { _lock: lock }
}

/// Read the recorded value of `name` with the `outcome` label.
///
/// Returns `0` when the counter has not been recorded since the guard was
/// acquired.
pub fn counter(name: &str, outcome: &str) -> u64 {
    counters_lock()
        .get(&format!("{name}{{outcome={outcome}}}"))
        .copied()
        .unwrap_or(0)
}

/// Exclusive recording scope. Counters stop being captured when dropped.
pub struct RecordingGuard {
    _lock: MutexGuard<'static, ()>,
}

impl Drop for RecordingGuard {
    fn drop(&mut self) {
        RECORDING.with(|flag| flag.set(false));
    }
}

struct TestRecorder;

impl Recorder for TestRecorder {
    fn describe_counter(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}

    fn describe_gauge(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}

    fn describe_histogram(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}

    fn register_counter(&self, key: &Key, _: &Metadata<'_>) -> Counter {
        Counter::from_arc(Arc::new(TestCounter {
            key: render_key(key),
        }))
    }

    fn register_gauge(&self, _: &Key, _: &Metadata<'_>) -> Gauge {
        Gauge::noop()
    }

    fn register_histogram(&self, _: &Key, _: &Metadata<'_>) -> Histogram {
        Histogram::noop()
    }
}

struct TestCounter {
    key: String,
}

impl CounterFn for TestCounter {
    fn increment(&self, value: u64) {
        if !recording() {
            return;
        }
        *counters_lock().entry(self.key.clone()).or_default() += value;
    }

    fn absolute(&self, value: u64) {
        if !recording() {
            return;
        }
        counters_lock().insert(self.key.clone(), value);
    }
}

/// Render a metric key as `name{label=value,...}` with labels sorted so the
/// order is stable regardless of registration order.
fn render_key(key: &Key) -> String {
    let mut labels: Vec<String> = key
        .labels()
        .map(|label| format!("{}={}", label.key(), label.value()))
        .collect();
    labels.sort();
    format!("{}{{{}}}", key.name(), labels.join(","))
}

#[cfg(test)]
mod tests {
    //! Tests for the in-process metrics recorder.

    use super::*;

    #[test]
    fn increments_on_the_recording_thread_are_captured() {
        let _guard = record();

        ::metrics::counter!("axinite.test.recorder", "outcome" => "self").increment(1);

        assert_eq!(counter("axinite.test.recorder", "self"), 1);
    }

    #[test]
    fn increments_from_other_threads_are_ignored() {
        let _guard = record();

        std::thread::spawn(|| {
            ::metrics::counter!("axinite.test.recorder", "outcome" => "other").increment(1);
        })
        .join()
        .expect("emitter thread should finish");

        assert_eq!(
            counter("axinite.test.recorder", "other"),
            0,
            "an increment from a thread without the guard must not be captured"
        );
    }
}

//! Loom checks for shared singleton state transitions.
//!
//! These tests model `shared_singleton_core` under deterministic thread
//! interleavings. They verify that concurrent callers initialize the shared
//! singleton at most once, observe one cached success or failure, and never see
//! a torn intermediate state while reusing the same core state-machine logic as
//! `shared_singleton.rs`.

use loom::{
    sync::{
        Arc,
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

use super::shared_singleton_core::{SharedInitState, get_or_try_init_shared};

const FIRST_FAILURE: usize = 13;
const CACHED_FAILURE: usize = 17;

struct LoomSharedState {
    state: Mutex<SharedInitState<usize, usize>>,
    initializations: AtomicUsize,
}

impl LoomSharedState {
    fn new() -> Self {
        Self {
            state: Mutex::new(SharedInitState::Uninitialized),
            initializations: AtomicUsize::new(0),
        }
    }

    fn get(&self, initialization: Result<usize, (usize, usize)>) -> Result<usize, usize> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        get_or_try_init_shared(
            &mut state,
            || {
                self.initializations.fetch_add(1, Ordering::SeqCst);
                initialization
            },
            |cached| *cached,
        )
    }
}

fn run_loom_model<F>(f: F)
where
    F: Fn() + Send + Sync + 'static,
{
    let mut builder = loom::model::Builder::new();
    builder.max_threads = 3;
    builder.max_branches = 64;
    builder.preemption_bound = Some(3);
    builder.check(f);
}

fn assert_two_callers_share_initialization(
    initialization: Result<usize, (usize, usize)>,
    assert_results: fn(Result<usize, usize>, Result<usize, usize>),
) {
    run_loom_model(move || {
        let state = Arc::new(LoomSharedState::new());
        let first = Arc::clone(&state);
        let second = Arc::clone(&state);

        let first_thread = thread::spawn(move || first.get(initialization));
        let second_thread = thread::spawn(move || second.get(initialization));

        let Ok(first_result) = first_thread.join() else {
            panic!("first thread should join");
        };
        let Ok(second_result) = second_thread.join() else {
            panic!("second thread should join");
        };

        assert_results(first_result, second_result);
        assert_eq!(state.initializations.load(Ordering::SeqCst), 1);
    });
}

fn assert_success_results(first: Result<usize, usize>, second: Result<usize, usize>) {
    assert_eq!(first, Ok(7));
    assert_eq!(second, Ok(7));
}

#[test]
#[ignore = "requires Loom model checking"]
fn shared_singleton_initializes_once_for_concurrent_callers() {
    assert_two_callers_share_initialization(Ok(7), assert_success_results);
}

#[test]
#[ignore = "requires Loom model checking"]
fn shared_singleton_caches_failed_initialization() {
    assert_two_callers_share_initialization(
        Err((CACHED_FAILURE, FIRST_FAILURE)),
        assert_cached_failure_translation,
    );
}

fn assert_cached_failure_translation(
    first_result: Result<usize, usize>,
    second_result: Result<usize, usize>,
) {
    let Err(first_error) = first_result else {
        panic!("first caller should observe failure");
    };
    let Err(second_error) = second_result else {
        panic!("second caller should observe failure");
    };
    let mut errors = [first_error, second_error];
    errors.sort_unstable();
    assert_eq!(errors, [FIRST_FAILURE, CACHED_FAILURE]);
}

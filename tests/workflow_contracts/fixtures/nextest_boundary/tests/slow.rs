//! A test that outlasts any allowance the boundary contract gives it.
//!
//! The contract runs this binary under a short `slow-timeout` built
//! from the real configuration's own fields, and asserts nextest
//! terminates it. Thirty seconds is long enough that a run which did
//! not terminate is unmistakable, and short enough that such a run
//! ends by itself rather than hanging the suite.

#[test]
fn sleeps_past_every_allowance() {
    std::thread::sleep(std::time::Duration::from_secs(30));
}

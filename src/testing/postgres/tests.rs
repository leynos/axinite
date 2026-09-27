//! Unit tests for the decision a fixture makes when Postgres is absent.
//!
//! The table has four cells: the database is reachable or not, and the run
//! promised one or did not. Only one of them changed, and these tests say
//! which, so a later edit that quietly restores the skip has something to
//! fail against.
//!
//! A sibling module rather than an inline one, so that neither this file nor
//! the helper it tests exceeds the repository's 400-line limit.

use super::{PostgresRequirement, is_database_unavailable, skip_is_allowed};
use crate::error::DatabaseError;
use rstest::rstest;
use std::ffi::{OsStr, OsString};

/// A port nothing listens on, so a connection there is refused at once.
///
/// Port 1 is privileged and unassigned, and the address is the loopback
/// interface, so the attempt neither leaves the machine nor waits on a
/// name server.
#[cfg(unix)]
const UNREACHABLE_URL: &str = "postgresql://127.0.0.1:1/axinite_test";

/// Build an environment value the platform cannot render as Unicode.
#[cfg(unix)]
fn not_unicode() -> OsString {
    use std::os::unix::ffi::OsStringExt as _;

    // A lone continuation byte: valid in an environment value, not UTF-8.
    OsString::from_vec(vec![b'1', 0x80])
}

/// Build an environment value the platform cannot render as Unicode.
#[cfg(windows)]
fn not_unicode() -> OsString {
    use std::os::windows::ffi::OsStringExt as _;

    // An unpaired high surrogate: storable as UTF-16, not valid Unicode.
    OsString::from_wide(&[0x0031, 0xD800])
}

/// An error that means nothing is listening on the other end.
///
/// `DatabaseError::Pool` carries its message as a string and is one of the
/// variants `is_database_unavailable` considers, so it stands in for the
/// pool errors a real absent Postgres produces without needing a real one.
fn unavailable() -> DatabaseError {
    DatabaseError::Pool("connection refused (os error 111)".to_string())
}

/// An error that means the database answered and refused us.
///
/// Authentication and configuration mistakes were never skippable. They
/// are here so the requirement is shown to change one cell of the table
/// rather than the whole of it.
fn misconfigured() -> DatabaseError {
    DatabaseError::Pool("password authentication failed for user".to_string())
}

#[rstest]
#[case::unset(None, PostgresRequirement::Optional)]
#[case::empty(Some(""), PostgresRequirement::Optional)]
#[case::whitespace(Some("  "), PostgresRequirement::Optional)]
#[case::one(Some("1"), PostgresRequirement::Required)]
#[case::any_value(Some("yes"), PostgresRequirement::Required)]
fn the_requirement_is_read_from_the_value(
    #[case] value: Option<&str>,
    #[case] expected: PostgresRequirement,
) {
    assert_eq!(PostgresRequirement::from_value(value), expected);
}

/// Without a promise of a database, an absent one skips.
///
/// This is the behaviour a developer depends on: no local Postgres, and
/// the rest of the suite still runs.
#[test]
fn an_unreachable_database_is_skippable_when_nobody_promised_one() {
    assert!(skip_is_allowed(
        &unavailable(),
        PostgresRequirement::Optional
    ));
}

/// The cell this change exists for.
///
/// A lane that starts a Postgres service and points the tests at it sets
/// the requirement. If the database is then unreachable, the run must
/// fail. Before this, it skipped: every Postgres test reported success
/// without connecting, and the lane published coverage measured without
/// them.
#[test]
fn an_unreachable_database_is_not_skippable_when_the_lane_promised_one() {
    assert!(!skip_is_allowed(
        &unavailable(),
        PostgresRequirement::Required
    ));
}

/// The requirement changes one cell, not the whole table.
///
/// A misconfiguration was already a failure under either requirement, so a
/// test that only checked the required column would pass with the
/// requirement ignored entirely.
#[rstest]
#[case(PostgresRequirement::Optional)]
#[case(PostgresRequirement::Required)]
fn a_misconfigured_database_was_never_skippable(#[case] requirement: PostgresRequirement) {
    assert!(!is_database_unavailable(&misconfigured()));
    assert!(!skip_is_allowed(&misconfigured(), requirement));
}

/// The raw reading agrees with the string reading where a string exists.
///
/// `from_os_value` is what the process actually calls, so the decision
/// table above would be describing an unused function if the two readings
/// could disagree on the ordinary cases.
#[rstest]
#[case::unset(None, PostgresRequirement::Optional)]
#[case::empty(Some(""), PostgresRequirement::Optional)]
#[case::whitespace(Some("  "), PostgresRequirement::Optional)]
#[case::one(Some("1"), PostgresRequirement::Required)]
fn the_raw_reading_agrees_with_the_string_reading(
    #[case] value: Option<&str>,
    #[case] expected: PostgresRequirement,
) {
    assert_eq!(
        PostgresRequirement::from_os_value(value.map(OsStr::new)),
        expected
    );
}

/// A value the platform cannot read as Unicode still promises a database.
///
/// This is the cell `std::env::var(..).ok()` got wrong: it folded the read
/// failure into the unset case, so a lane that set the variable to
/// something unreadable would have got its skip back without saying so.
#[test]
fn a_value_that_is_not_unicode_still_requires_a_database() {
    assert_eq!(
        PostgresRequirement::from_os_value(Some(not_unicode().as_os_str())),
        PostgresRequirement::Required
    );
}

/// The production path skips or fails against a real refused connection.
///
/// The decision-table tests above call `skip_is_allowed` directly, so they
/// would all pass with `try_test_pg_db` ignoring it. This drives the whole
/// path instead: a pool that cannot connect, the error classification, and
/// the branch that turns them into `Ok(None)` or `Err`.
///
/// Unix only. A refused connection is reported by the platform, and
/// Windows words it differently from the transport failures
/// `UNAVAILABLE_PATTERNS` lists; the tests that read this decision run on
/// Linux, and asserting it on Windows would be asserting a different
/// thing.
#[cfg(unix)]
#[rstest]
#[case::nobody_promised_one(PostgresRequirement::Optional, true)]
#[case::the_lane_promised_one(PostgresRequirement::Required, false)]
#[tokio::test]
async fn an_unreachable_endpoint_skips_only_when_nobody_promised_one(
    #[case] requirement: PostgresRequirement,
    #[case] skips: bool,
) {
    match super::try_pg_db_at(UNREACHABLE_URL.to_string(), requirement).await {
        Ok(None) => assert!(skips, "{requirement:?} skipped an unreachable database"),
        Err(error) => {
            assert!(!skips, "{requirement:?} failed on an unreachable database");
            assert!(
                is_database_unavailable(&error),
                "the refusal was not read as an unavailable database: {error}"
            );
        }
        Ok(Some(_)) => panic!("something answered on {UNREACHABLE_URL}"),
    }
}

/// The full path of the child test the wiring cases below run.
///
/// Spelled out rather than derived: `module_path!()` would agree with
/// whatever the module is called, including after a rename that left the
/// child unreachable, and an unreachable child test makes every case below
/// pass on a process that ran nothing.
#[cfg(unix)]
const WIRING_CHILD: &str = "testing::postgres::tests::the_entry_point_reads_its_own_environment";

/// Drive `try_test_pg_db` itself, in a process whose environment is its own.
///
/// Everything above tests a decision handed its inputs. This is the one that
/// says the production entry point composes those inputs: it reads
/// `TEST_DATABASE_URL`, it reads `AXINITE_REQUIRE_POSTGRES`, and it hands
/// both to the decision. Replacing either reading with a constant leaves
/// every other test in this module passing.
///
/// It is `#[ignore]`d because the parent cases below supply its environment,
/// and it is driven in a child process because setting a variable in this one
/// would have to be serialized against every other test in the binary, which
/// the environment is not a structural reason to do.
///
/// Passing means a skip. A failure to connect that was not skipped panics
/// here, so the child's exit status is the decision, read by the parent.
#[cfg(unix)]
#[tokio::test]
#[ignore = "run by the_environment_reaches_the_production_decision, which supplies its environment"]
async fn the_entry_point_reads_its_own_environment() {
    let skipped = super::try_test_pg_db()
        .await
        .expect("the lane promised a database, so an unreachable one is a failure")
        .is_none();
    assert!(skipped, "something answered on the unreachable test URL");
}

/// Run the wiring child once, with `declared` as the lane's promise.
///
/// Takes an `OsStr` rather than a `str` so that the case which matters most
/// can be stated: a value the platform cannot render as Unicode. That is the
/// cell `std::env::var(..).ok()` gets wrong, and it cannot be written with a
/// `&str` at all.
///
/// # Returns
///
/// Whether the fixture skipped, which is what the child's exit status means:
/// the child passes when `try_test_pg_db` returned `Ok(None)` and fails when
/// it returned the error the requirement asks for.
///
/// # Errors
///
/// If this process cannot name its own binary, or the child cannot be
/// started. Arranging a test can fail, and arranging is what this does, so the
/// failure is returned rather than raised: only the test body may treat one as
/// a verdict.
///
/// # Panics
///
/// If the child ran no test. An unreachable child makes a harness exit
/// non-zero on an argument error rather than on the decision, which reads as
/// "failed" for every case and would pass the cases expecting one.
#[cfg(unix)]
fn the_fixture_skips_with(declared: Option<&OsStr>) -> std::io::Result<bool> {
    let binary = std::env::current_exe()?;
    let mut child = std::process::Command::new(binary);
    child
        .args(["--exact", "--ignored", "--nocapture", WIRING_CHILD])
        .env("TEST_DATABASE_URL", UNREACHABLE_URL)
        .env_remove(super::REQUIRE_POSTGRES_ENV);
    if let Some(value) = declared {
        child.env(super::REQUIRE_POSTGRES_ENV, value);
    }
    let output = child.output()?;
    let ran = String::from_utf8_lossy(&output.stdout);
    assert!(
        ran.contains("1 passed") || ran.contains("1 failed"),
        "the child ran no test, so the caller asserts nothing. \
         Check that {WIRING_CHILD} still exists. Its output was:\n{ran}"
    );
    Ok(output.status.success())
}

/// The environment a lane sets must reach the decision the fixture makes.
///
/// This is the wiring, and it is the part no other test in this module
/// reaches: `try_test_pg_db` passing `PostgresRequirement::Optional` instead
/// of `from_env()` would restore the skip on exactly the lane that asked for
/// it to be gone, and would leave the whole decision table above green.
///
/// Each case runs the ignored test above in a child process with the variable
/// set as a lane would set it, and reads the child's exit status: success is a
/// skip, failure is the error the requirement asks for.
///
/// Unix only, for the same reason the endpoint case is: Windows words a
/// refused connection differently from the transport failures
/// `UNAVAILABLE_PATTERNS` lists, so asserting it there would assert a
/// different thing.
#[cfg(unix)]
#[rstest]
#[case::nobody_promised_one(None, true)]
#[case::an_empty_promise_is_not_one(Some(""), true)]
#[case::whitespace_is_not_a_promise(Some("   "), true)]
#[case::the_lane_promised_one(Some("1"), false)]
#[case::any_value_is_a_promise(Some("false"), false)]
fn the_environment_reaches_the_production_decision(
    #[case] declared: Option<&str>,
    #[case] skips: bool,
) {
    assert_eq!(
        the_fixture_skips_with(declared.map(OsStr::new))
            .expect("the child test process must start"),
        skips,
        "with {} the fixture should {}",
        declared.map_or_else(
            || format!("{} unset", super::REQUIRE_POSTGRES_ENV),
            |value| format!("{}={value:?}", super::REQUIRE_POSTGRES_ENV)
        ),
        if skips { "skip" } else { "fail" }
    );
}

/// A value the platform cannot read must reach the decision as a promise too.
///
/// The cases above all carry valid Unicode, so every one of them survives
/// `from_env` going back to `std::env::var(..).ok()`: that reading maps a
/// valid value correctly and only gets the unreadable one wrong. And
/// `a_value_that_is_not_unicode_still_requires_a_database` calls
/// `from_os_value` directly, which the production path does not.
///
/// So this is the case that closes the loop. It is the same end-to-end run as
/// the cases above, with the one value that tells the two readings apart: a
/// lane that set the variable to something unreadable must still be refused
/// its skip.
#[cfg(unix)]
#[test]
fn a_value_that_cannot_be_read_reaches_the_decision_as_a_promise() {
    let skipped = the_fixture_skips_with(Some(not_unicode().as_os_str()))
        .expect("the child test process must start");
    assert!(
        !skipped,
        "the lane set {} to a value that is not Unicode, which is still a \
         promise; the fixture skipped instead of failing, so the read failure \
         was folded onto the same `None` as an unset variable",
        super::REQUIRE_POSTGRES_ENV
    );
}

//! Postgres-specific test helpers.

use crate::config::{DatabaseBackend, DatabaseConfig, SslMode};
use crate::db::postgres::PgBackend;
use crate::error::DatabaseError;
use secrecy::SecretString;
use std::ffi::OsStr;

// These substrings are limited to concrete local transport and name-resolution
// failures observed when a test Postgres instance is absent. We intentionally
// exclude generic timeout wording so TLS, authentication, and other
// misconfiguration-related delays still fail loudly instead of being skipped.
const UNAVAILABLE_PATTERNS: &[&str] = &[
    "connection refused",
    "failed to lookup address information",
    "name or service not known",
    "temporary failure in name resolution",
    "network is unreachable",
    "no such file or directory",
    "could not connect to server",
];

/// Create a PostgreSQL-backed test database.
///
/// Reads the test database URL from the `TEST_DATABASE_URL` environment
/// variable, or falls back to a default local Postgres instance.
/// Returns the `PgBackend` instance for testing, propagating any
/// connection or pool errors to the caller.
///
/// # Examples
///
/// ```no_run
/// use axinite::testing::postgres::test_pg_db;
///
/// async fn example() -> Result<(), Box<dyn std::error::Error>> {
///     let db = test_pg_db().await?;
///     let _ = db;
///     Ok(())
/// }
/// ```
pub async fn test_pg_db() -> Result<PgBackend, DatabaseError> {
    let url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://localhost/axinite_test".to_string());

    let config = DatabaseConfig {
        backend: DatabaseBackend::Postgres,
        url: SecretString::from(url),
        pool_size: 5,
        ssl_mode: SslMode::Prefer,
        libsql_path: None,
        libsql_url: None,
        libsql_auth_token: None,
    };

    PgBackend::new(&config).await
}

/// Set by a CI lane that provides Postgres on purpose.
///
/// A developer without a local Postgres should still be able to run the rest
/// of the suite, so an unreachable database is a skip by default. A lane that
/// starts a Postgres service and points the tests at it has no such excuse: a
/// skip there reports success for tests that never ran, which is how
/// `coverage.yml` stayed green while publishing coverage measured without
/// them. The lane says so by setting this, and the skip stops being available.
const REQUIRE_POSTGRES_ENV: &str = "AXINITE_REQUIRE_POSTGRES";

/// Whether a run may carry on without a database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PostgresRequirement {
    /// Nobody promised a database, so its absence skips the Postgres tests.
    Optional,
    /// A database was provided on purpose, so its absence is a failure.
    Required,
}

impl PostgresRequirement {
    /// Read the requirement from an environment value.
    ///
    /// Split from `from_env` so the reading is a pure function: a test that
    /// set the variable would have to be serialized against every other test
    /// in the process, and the environment is not a structural reason to
    /// serialize.
    fn from_value(value: Option<&str>) -> Self {
        match value {
            Some(raw) if !raw.trim().is_empty() => Self::Required,
            _ => Self::Optional,
        }
    }

    /// Read the requirement from the raw value the platform holds.
    ///
    /// A value that is not valid Unicode is still a value: the lane set the
    /// variable, so the reading that keeps the promise is `Required`. Going
    /// through `std::env::var(..).ok()` instead would map that read failure
    /// onto the same `None` as an unset variable, restoring the skip on
    /// exactly the lane that asked for it to be gone, and silently.
    fn from_os_value(value: Option<&OsStr>) -> Self {
        match value {
            None => Self::Optional,
            Some(raw) => raw
                .to_str()
                .map_or(Self::Required, |text| Self::from_value(Some(text))),
        }
    }

    /// Read the requirement this process was started with.
    fn from_env() -> Self {
        Self::from_os_value(std::env::var_os(REQUIRE_POSTGRES_ENV).as_deref())
    }
}

/// Report whether an unavailable database may be skipped over.
///
/// Both halves have to hold. The error must be one of the transport and
/// name-resolution failures that mean "nothing is listening", which is what
/// keeps an authentication or configuration mistake loud; and the run must not
/// have declared that it provides a database.
fn skip_is_allowed(error: &DatabaseError, requirement: PostgresRequirement) -> bool {
    requirement == PostgresRequirement::Optional && is_database_unavailable(error)
}

/// Attempt to create a test `PgBackend`, returning `None` only when the
/// database is unavailable and this run did not promise one.
///
/// Use this in test fixtures that should be skipped when no local Postgres
/// instance is available, while still surfacing configuration and
/// authentication mistakes. A lane that sets `AXINITE_REQUIRE_POSTGRES` gets no
/// skip at all: an unreachable database fails there rather than reporting
/// success for tests that never ran.
pub async fn try_test_pg_db() -> Result<Option<PgBackend>, DatabaseError> {
    match test_pg_db().await {
        Ok(db) => Ok(Some(db)),
        Err(error) if skip_is_allowed(&error, PostgresRequirement::from_env()) => {
            eprintln!("Skipping Postgres test (database unavailable): {error}");
            Ok(None)
        }
        Err(error) => {
            if is_database_unavailable(&error) {
                eprintln!(
                    concat!(
                        "Postgres is unreachable and {env} is set, so this ",
                        "is a failure rather than a skip: {error}"
                    ),
                    env = REQUIRE_POSTGRES_ENV,
                    error = error
                );
            }
            Err(error)
        }
    }
}

fn is_database_unavailable(error: &DatabaseError) -> bool {
    let lowered = format!("{error:?} {error}").to_lowercase();

    matches!(
        error,
        DatabaseError::Postgres(_)
            | DatabaseError::Pool(_)
            | DatabaseError::PoolBuild(_)
            | DatabaseError::PoolRuntime(_)
    ) && UNAVAILABLE_PATTERNS.iter().any(|p| lowered.contains(p))
}

#[cfg(test)]
mod tests {
    //! Unit tests for the decision a fixture makes when Postgres is absent.
    //!
    //! The table has four cells: the database is reachable or not, and the run
    //! promised one or did not. Only one of them changed, and these tests say
    //! which, so a later edit that quietly restores the skip has something to
    //! fail against.

    use super::{PostgresRequirement, is_database_unavailable, skip_is_allowed};
    use crate::error::DatabaseError;
    use rstest::rstest;
    use std::ffi::{OsStr, OsString};

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
}

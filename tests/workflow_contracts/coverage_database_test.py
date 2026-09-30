"""Contracts binding the coverage job's database URL to the name tests read.

`src/testing/postgres.rs` reads `TEST_DATABASE_URL`, and falls back to
`postgresql://localhost/axinite_test` when it is absent. That fallback carries
no user and no password, so the pool fails with
`kind: Config, cause: "password missing"`.

The failure is loud by design rather than by accident: `is_database_unavailable`
lists only transport and name-resolution failures, deliberately excluding
authentication and configuration errors, so a misconfigured job fails instead
of quietly reporting coverage for tests that never ran.

That is what makes the export worth a contract. It was renamed to
`DATABASE_URL` in #243 on 2026-07-14, which nothing on the test path reads, and
every push to `main` failed from two days later until this was fixed. Nothing
caught it, because the workflow still exported something plausible and the
`libsql-only` leg, which needs no database, stayed green. See issue #350.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import re
import typing as typ
from urllib.parse import urlsplit

import pytest
from _cargo_features import POSTGRES_FEATURE, _enables_postgres
from _workflow_files import load
from _workflow_policy import WORKFLOW_DIR, step_text

WORKFLOW: typ.Final[str] = "coverage.yml"

#: The variable `src/testing/postgres.rs` reads. Renaming the export without
#: renaming the reader is the exact mistake this file exists to catch, so the
#: constant is spelled out here rather than derived from the workflow.
TEST_URL_VARIABLE: typ.Final[str] = "TEST_DATABASE_URL"

#: The job that runs the Postgres-bearing coverage legs.
JOB: typ.Final[str] = "coverage"

#: Matches the shell assignment of a URL to a variable, so the value tied to
#: `TEST_DATABASE_URL` can be checked rather than any credentialed URL that
#: happens to appear in the same script. A passwordless `TEST_DATABASE_URL`
#: beside a credentialed `DATABASE_URL` reproduces the original failure exactly,
#: and a contract that searched the joined script would pass it.
ASSIGNMENT_RE: typ.Final[re.Pattern[str]] = re.compile(
    r"^\s*(?P<name>[A-Za-z_][A-Za-z0-9_]*)=\"?(?P<value>[^\"\n]+?)\"?\s*$",
    re.MULTILINE,
)

#: Matches `echo "NAME=${shell_var}" >> "$GITHUB_ENV"`, which is how a value
#: reaches later steps. The exported name and the shell variable holding the
#: value are both captured, so the two halves can be joined.
#:
#: The redirection target is part of the pattern on purpose. A line redirecting
#: to an ordinary file looks identical up to the `>>`, and would satisfy every
#: assertion here while later steps received nothing and the helper fell back to
#: the unauthenticated URL.
EXPORT_RE: typ.Final[re.Pattern[str]] = re.compile(
    r"echo\s+\"(?P<name>[A-Za-z_][A-Za-z0-9_]*)=\$\{(?P<source>[A-Za-z_]"
    r"[A-Za-z0-9_]*)\}\"\s*>>\s*\"?\$(?:\{)?GITHUB_ENV(?:\})?\"?",
)


def _postgres_job() -> dict[str, object]:
    """Return the coverage job, failing loudly if it is renamed away."""
    jobs = load(WORKFLOW_DIR / WORKFLOW).get("jobs")
    assert isinstance(jobs, dict), f"{WORKFLOW} must declare a jobs mapping"
    job = jobs.get(JOB)
    assert isinstance(job, dict), f"{WORKFLOW} must declare a {JOB!r} job"
    return job


def _steps(job: dict[str, object]) -> list[dict[str, object]]:
    """Return the job's step mappings."""
    steps = job.get("steps")
    assert isinstance(steps, list), f"{JOB} must declare steps"
    return [step for step in steps if isinstance(step, dict)]


def _service_env() -> dict[str, object]:
    """Return the Postgres service container's environment."""
    services = _postgres_job().get("services")
    assert isinstance(services, dict), f"{JOB} must declare a services mapping"
    postgres = services.get("postgres")
    assert isinstance(postgres, dict), f"{JOB} must declare a postgres service"
    env = postgres.get("env")
    assert isinstance(env, dict), "the postgres service must declare env"
    return env


def _exporting_step() -> dict[str, object]:
    """Return the single step that exports the test database URL."""
    matches = [
        step
        for step in _steps(_postgres_job())
        if f"{TEST_URL_VARIABLE}=" in step_text(step)
    ]
    assert len(matches) == 1, (
        f"expected exactly one step exporting {TEST_URL_VARIABLE}, found {len(matches)}"
    )
    return matches[0]


def _exported_test_url() -> str:
    """Return the URL value the step exports as `TEST_DATABASE_URL`."""
    # The script assigns the URL to a shell variable and then exports that
    # variable, so both halves are resolved rather than assumed. Following the
    # indirection is the point: it is what ties the credentials being asserted
    # to the name the tests read.
    script = step_text(_exporting_step())
    exports = {m["name"]: m["source"] for m in EXPORT_RE.finditer(script)}
    source = exports.get(TEST_URL_VARIABLE)
    assert source is not None, (
        f"the step must export {TEST_URL_VARIABLE} from a shell variable, as "
        f'echo "{TEST_URL_VARIABLE}=${{...}}" >> "$GITHUB_ENV"'
    )
    assignments = {
        m["name"]: m["value"]
        for m in ASSIGNMENT_RE.finditer(script)
        if not m.group(0).lstrip().startswith("echo")
    }
    value = assignments.get(source)
    assert value is not None, (
        f"{TEST_URL_VARIABLE} is exported from ${source}, which the step never assigns"
    )
    return value


def test_the_coverage_job_exports_the_variable_the_tests_read() -> None:
    """Export the name the code reads, not one that merely looks right.

    A plausible but unread name is worse than no export at all: the job still
    runs, the tests still fail, and the workflow reports a database problem
    rather than a configuration one.
    """
    script = step_text(_exporting_step())
    assert f"{TEST_URL_VARIABLE}=" in script, (
        f"{JOB} must export {TEST_URL_VARIABLE}, which "
        "src/testing/postgres.rs reads. Without it the helper falls back to a "
        "URL with no credentials and every Postgres test fails on "
        "'password missing'."
    )


def test_the_exported_test_url_matches_the_service_container() -> None:
    """Check the URL bound to `TEST_DATABASE_URL`, not any URL nearby.

    Searching the whole script would pass a passwordless `TEST_DATABASE_URL`
    sitting beside a credentialed `DATABASE_URL`, which reproduces the original
    failure exactly while every other assertion here holds. The expected values
    come from the service container rather than being written out again, so the
    two cannot drift apart.
    """
    env = _service_env()
    parsed = urlsplit(_exported_test_url())
    assert parsed.username == env.get("POSTGRES_USER"), (
        f"the {TEST_URL_VARIABLE} value must carry the service's "
        f"POSTGRES_USER, got {parsed.username!r}"
    )
    assert parsed.password == env.get("POSTGRES_PASSWORD"), (
        f"the {TEST_URL_VARIABLE} value must carry the service's "
        "POSTGRES_PASSWORD. Without a password the pool fails with "
        '`kind: Config, cause: "password missing"`, which is the failure '
        "this contract exists to prevent."
    )
    assert parsed.path.lstrip("/") == env.get("POSTGRES_DB"), (
        f"the {TEST_URL_VARIABLE} value must name the service's POSTGRES_DB, "
        f"got {parsed.path!r}"
    )
    assert parsed.hostname == "localhost", (
        f"the service container is published on localhost; got {parsed.hostname!r}"
    )


#: The job that runs the one leg with no database.
LIBSQL_JOB: typ.Final[str] = "coverage-libsql"


def _libsql_job() -> dict[str, object]:
    """Return the libsql-only coverage job, failing loudly if it is renamed."""
    jobs = load(WORKFLOW_DIR / WORKFLOW).get("jobs")
    assert isinstance(jobs, dict), f"{WORKFLOW} must declare a jobs mapping"
    job = jobs.get(LIBSQL_JOB)
    assert isinstance(job, dict), f"{WORKFLOW} must declare a {LIBSQL_JOB!r} job"
    return job


def test_the_libsql_job_is_handed_no_database() -> None:
    """The leg with no database is its own job, so it can never be given one.

    The libsql-only leg used to be a cell of the `coverage` matrix, kept away
    from the database by a `matrix.has_postgres` guard on every step that
    advertised one. It is a job of its own now, so the guarantee is
    structural: no service container, and no step that exports the URL or the
    promise of a database. Either would point a leg built with
    `--no-default-features --features libsql` at a database it does not use.
    """
    job = _libsql_job()
    assert "services" not in job, f"{LIBSQL_JOB} must not start a Postgres service"
    leaking = [
        step.get("name")
        for step in _steps(job)
        if TEST_URL_VARIABLE in step_text(step) or REQUIRE_VARIABLE in step_text(step)
    ]
    assert not leaking, (
        f"{LIBSQL_JOB} must not export {TEST_URL_VARIABLE} or {REQUIRE_VARIABLE}, "
        f"found them in {leaking}"
    )


def test_the_libsql_job_compiles_without_postgres() -> None:
    """Keep the libsql-only job libsql-only, or its promise of no database lies.

    `postgres` is a default feature, so the job stays database-free only while
    the action turns the defaults off and names no `postgres` feature.
    """
    generating = [
        step
        for step in _steps(_libsql_job())
        if "generate-coverage" in str(step.get("uses", ""))
    ]
    assert len(generating) == 1, f"{LIBSQL_JOB} must run generate-coverage once"
    inputs = generating[0].get("with")
    assert isinstance(inputs, dict), "generate-coverage must declare its inputs"
    assert str(inputs.get("with-default-features")).lower() == "false", (
        "the libsql-only job must turn the default features off, or it gets "
        "`postgres` and needs the database it is not given"
    )
    features = set(re.split(r"[,\s]+", str(inputs.get("features", ""))))
    assert POSTGRES_FEATURE not in features, (
        "the libsql-only job must not name the `postgres` feature"
    )


#: The variable a lane sets to say that Postgres is not optional on this run.
#: `src/testing/postgres.rs` skips its Postgres tests when the database is
#: unreachable, which is what a developer without one needs. On a lane that
#: starts a Postgres service and points the tests at it, the same skip reports
#: success for tests that never connected.
REQUIRE_VARIABLE: typ.Final[str] = "AXINITE_REQUIRE_POSTGRES"

#: `echo "NAME=value" >> "$GITHUB_ENV"`, anchored so the redirection names
#: that file and not a variable whose name merely starts the same way.
#: `$GITHUB_ENV_UNUSED`, `$GITHUB_ENVIRONMENT` and `$GITHUB_ENV.backup` are all
#: plausible typos, none of them reaches a later step, and a pattern that
#: stopped at `GITHUB_ENV` would accept every one while the tests silently
#: skipped.
REQUIRE_APPEND_RE: typ.Final[re.Pattern[str]] = re.compile(
    rf'echo\s+"{REQUIRE_VARIABLE}=[^"\n]+"\s*>>\s*'
    r'(?:"\$\{GITHUB_ENV\}"|"\$GITHUB_ENV"|\$\{GITHUB_ENV\}|\$GITHUB_ENV)'
    r"\s*$",
    re.MULTILINE,
)


def _legs() -> list[dict[str, object]]:
    """Return the coverage job's matrix legs."""
    strategy = _postgres_job().get("strategy")
    assert isinstance(strategy, dict), f"{JOB} must declare a strategy"
    matrix = strategy.get("matrix")
    assert isinstance(matrix, dict), f"{JOB} must declare a matrix"
    include = matrix.get("include")
    assert isinstance(include, list), f"{JOB}'s matrix must declare include"
    return [leg for leg in include if isinstance(leg, dict)]


def test_every_leg_of_the_postgres_job_compiles_postgres() -> None:
    """Every leg of the `coverage` matrix must bear Postgres, and say so.

    The job starts a Postgres service, runs the migrations and exports the
    database URL for every leg, unconditionally. A leg that did not compile
    `postgres` would pay for a service it cannot use, and belongs in
    `coverage-libsql`. `postgres` is a default feature, so a leg gets it unless
    it passes `--no-default-features`. The export is unconditional for the same
    reason: a guard on it could turn it off for a leg that needs it.
    """
    for leg in _legs():
        name = leg.get("name")
        assert _enables_postgres(str(leg.get("flags", ""))), (
            f"the {name!r} leg's flags {leg.get('flags')!r} do not compile "
            "postgres, so it belongs in coverage-libsql, not in the job that "
            "starts the database"
        )
    assert "if" not in _exporting_step(), (
        f"the {TEST_URL_VARIABLE} export must be unconditional: every leg of "
        f"{JOB} needs it"
    )


def test_the_leg_that_provides_postgres_tells_the_tests_it_is_not_optional() -> None:
    """The database URL and the promise of a database ship in one step.

    Separating them is the failure this guards. A lane that exports the URL
    without the promise leaves the skip available, so a Postgres service that
    failed to start reports success for every test that needed it; a lane that
    exports the promise without the URL fails on the passwordless fallback,
    which is issue #350 again.
    """
    step = _exporting_step()
    script = step_text(step)
    assert f"{REQUIRE_VARIABLE}=" in script, (
        f"the step exporting {TEST_URL_VARIABLE} must also export "
        f"{REQUIRE_VARIABLE}, so a leg cannot receive the database without "
        "also being told that the database is not optional"
    )
    assert REQUIRE_APPEND_RE.search(script), (
        f"{REQUIRE_VARIABLE} must be appended to $GITHUB_ENV; setting it in "
        "the step's own shell reaches nothing that runs the tests"
    )


def test_the_promise_is_unconditional_in_the_postgres_job() -> None:
    """Every leg with a database must be told it is mandatory.

    A guard on the step would leave the skip available on a leg it fell off,
    which reports success for tests that never connected.
    """
    exporting = [
        step
        for step in _steps(_postgres_job())
        if f"{REQUIRE_VARIABLE}=" in step_text(step)
    ]
    assert len(exporting) == 1, (
        f"expected exactly one step exporting {REQUIRE_VARIABLE}, found "
        f"{len(exporting)}"
    )
    assert "if" not in exporting[0], (
        f"the step exporting {REQUIRE_VARIABLE} must be unconditional, found "
        f"{exporting[0].get('if')!r}"
    )


@pytest.mark.parametrize(
    ("flags", "expected"),
    [
        pytest.param("--all-features", True, id="all-features"),
        pytest.param("", True, id="nothing-named-gets-the-defaults"),
        pytest.param("--features libsql", True, id="defaults-plus-a-name"),
        pytest.param(
            "--no-default-features --features libsql", False, id="narrow-and-named"
        ),
        pytest.param(
            "--no-default-features --features postgres", True, id="narrow-but-postgres"
        ),
        pytest.param(
            "--no-default-features --features libsql,postgres",
            True,
            id="comma-separated",
        ),
        pytest.param(
            "--no-default-features --features 'libsql postgres'",
            True,
            id="whitespace-separated-in-one-argument",
        ),
        pytest.param(
            "--no-default-features --features=libsql,postgres",
            True,
            id="joined-with-equals",
        ),
        pytest.param("--no-default-features -F postgres", True, id="short-flag"),
        pytest.param(
            "--no-default-features -F=postgres", True, id="short-flag-with-equals"
        ),
        pytest.param("--no-default-features -Fpostgres", True, id="short-flag-joined"),
        pytest.param(
            "--no-default-features -F libsql -F postgres", True, id="repeated-flags"
        ),
    ],
)
def test_every_spelling_of_a_feature_list_is_read(
    flags: str, *, expected: bool
) -> None:
    """Read Cargo's feature syntax, not one spelling of it.

    A leg whose features the reader cannot see resolves to the default set, so
    a narrow leg naming `postgres` explicitly would read as not bearing it and
    `test_every_postgres_bearing_leg_declares_it` would reject a correct
    declaration. The `narrow-and-named` case is the one that keeps this honest:
    it is the only narrow case expected to be false, so a reader that answered
    true for everything would fail it.
    """
    assert _enables_postgres(flags) is expected, (
        f"{flags!r} should read as postgres={expected}"
    )

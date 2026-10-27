"""Contracts binding the coverage jobs' Postgres legs to the embedded cluster.

`src/testing/postgres.rs` takes its database from `TEST_DATABASE_URL` when one
is named, and otherwise, with the `embedded-postgres` feature on Linux, from an
embedded cluster the test process owns. The coverage job used to start a
Postgres service container and name it through `TEST_DATABASE_URL`; it now runs
every Postgres-bearing leg on the embedded cluster, so the contracts here are
the reverse of the old ones.

The failure to keep out is quiet. A leg that compiles `postgres` but not
`embedded-postgres`, and exports no URL, has no database source, and its
Postgres tests skip, so the leg reports coverage for tests that never ran. A
leg that still exports a URL would bypass the cluster. See issue #350 for the
history of this job's database wiring.

The libsql-only leg is a job of its own (`coverage-libsql`), the one CodeScene
publisher, so its freedom from a database is structural: no step of it can
advertise one, and the `coverage` matrix has no leg left that needs a guard.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import re
import typing as typ

import pytest
from _cargo_features import (
    EMBEDDED_FEATURE,
    POSTGRES_FEATURE,
    _enables_postgres,
    _has_embedded_cluster,
)
from _workflow_files import load
from _workflow_policy import WORKFLOW_DIR, step_text

WORKFLOW: typ.Final[str] = "coverage.yml"

#: The variable `src/testing/postgres.rs` reads. Renaming the export without
#: renaming the reader is the exact mistake this file exists to catch, so the
#: constant is spelled out here rather than derived from the workflow.
TEST_URL_VARIABLE: typ.Final[str] = "TEST_DATABASE_URL"

#: The runtime's own name for the same thing, held to the same rule.
RUNTIME_URL_VARIABLE: typ.Final[str] = "DATABASE_URL"

#: The job whose matrix runs the Postgres-bearing coverage legs.
JOB: typ.Final[str] = "coverage"

#: The job that runs the one leg with no database, and publishes to CodeScene.
LIBSQL_JOB: typ.Final[str] = "coverage-libsql"

#: The variable a lane sets to say that Postgres is not optional on this run.
#: `src/testing/postgres.rs` skips its Postgres tests when no database can be
#: had, which is what a developer without one needs. On a leg meant to run the
#: database tests the same skip reports success for tests that never ran.
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


def _job(name: str) -> dict[str, object]:
    """Return one job of the publisher workflow, failing loudly if renamed."""
    jobs = load(WORKFLOW_DIR / WORKFLOW).get("jobs")
    assert isinstance(jobs, dict), f"{WORKFLOW} must declare a jobs mapping"
    job = jobs.get(name)
    assert isinstance(job, dict), f"{WORKFLOW} must declare a {name!r} job"
    return job


def _steps(job: dict[str, object]) -> list[dict[str, object]]:
    """Return the job's step mappings."""
    steps = job.get("steps")
    assert isinstance(steps, list), "the job must declare steps"
    return [step for step in steps if isinstance(step, dict)]


def _database_url_environment_scopes(
    job_name: str,
) -> list[tuple[str, object | None]]:
    """Return workflow, job and step environments for a coverage job."""
    workflow = load(WORKFLOW_DIR / WORKFLOW)
    job = _job(job_name)
    scopes = [("workflow", workflow.get("env")), ("job", job.get("env"))]
    scopes.extend(
        (f"step {step.get('name')!r}", step.get("env")) for step in _steps(job)
    )
    return scopes


def _assert_no_database_url_environment(
    scope: str, environment: object | None, job_name: str
) -> None:
    """Reject database URL declarations in one workflow environment scope."""
    if environment is None:
        return
    assert isinstance(environment, dict), f"{scope} env must be a mapping"
    for variable in (TEST_URL_VARIABLE, RUNTIME_URL_VARIABLE):
        assert variable not in environment, (
            f"{scope} of {job_name} sets {variable}; Postgres legs must use "
            "the embedded cluster"
        )


def _assert_no_database_url_in_script(job_name: str, step: dict[str, object]) -> None:
    """Reject shell assignments that would override the embedded cluster."""
    script = step_text(step)
    for variable in (TEST_URL_VARIABLE, RUNTIME_URL_VARIABLE):
        assert f"{variable}=" not in script, (
            f"the step {step.get('name')!r} of {job_name} exports "
            f"{variable}; Postgres legs must use the embedded cluster"
        )


def _legs() -> list[dict[str, object]]:
    """Return the coverage job's matrix legs."""
    strategy = _job(JOB).get("strategy")
    assert isinstance(strategy, dict), f"{JOB} must declare a strategy"
    matrix = strategy.get("matrix")
    assert isinstance(matrix, dict), f"{JOB} must declare a matrix"
    include = matrix.get("include")
    assert isinstance(include, list), f"{JOB}'s matrix must declare include"
    return [leg for leg in include if isinstance(leg, dict)]


@pytest.mark.parametrize("job_name", [JOB, LIBSQL_JOB])
def test_no_coverage_job_starts_a_database_service(job_name: str) -> None:
    """The coverage jobs provision their database through pg-embed, not a service.

    A service container is a second source of truth for the database and the
    reason this job needed a migration step and a credentialed URL. Neither
    exists any more, and a service that crept back would start for every leg,
    and for the libsql-only job, which has no database to use it.
    """
    assert "services" not in _job(job_name), (
        f"{job_name} must not declare a services mapping: its Postgres legs run "
        "on the embedded cluster"
    )


@pytest.mark.parametrize("job_name", [JOB, LIBSQL_JOB])
def test_no_coverage_job_names_a_database(job_name: str) -> None:
    """Workflow, job and step environments must not name a database."""
    for scope, environment in _database_url_environment_scopes(job_name):
        _assert_no_database_url_environment(scope, environment, job_name)


@pytest.mark.parametrize("job_name", [JOB, LIBSQL_JOB])
def test_no_coverage_job_script_exports_a_database_url(job_name: str) -> None:
    """Coverage step scripts must leave database selection to the cluster."""
    for step in _steps(_job(job_name)):
        _assert_no_database_url_in_script(job_name, step)


def test_every_leg_of_the_postgres_job_compiles_postgres() -> None:
    """Every leg of the `coverage` matrix must bear Postgres, and say nothing else.

    The libsql-only leg left the matrix when it became the publisher, so a leg
    that compiles no `postgres` belongs in `coverage-libsql`, not here. The leg
    needs no `has_postgres` flag: nothing is guarded on it, and a leftover key
    would suggest a leg exists that bears no database.
    """
    for leg in _legs():
        name = leg.get("name")
        assert _enables_postgres(str(leg.get("flags", ""))), (
            f"the {name!r} leg's flags {leg.get('flags')!r} do not compile "
            "postgres, so it belongs in coverage-libsql"
        )
        assert "has_postgres" not in leg, (
            f"the {name!r} leg carries has_postgres; every leg of {JOB} bears "
            "Postgres, so the key guards nothing"
        )


def test_every_postgres_leg_enables_the_embedded_cluster() -> None:
    """A leg compiles `embedded-postgres` exactly when it bears Postgres.

    `--all-features` carries it implicitly. Without the feature a Postgres leg
    has no database source at all and skips its database tests.
    """
    for leg in _legs():
        flags = str(leg.get("flags", ""))
        assert _has_embedded_cluster(flags), (
            f"the {leg.get('name')!r} leg's flags {flags!r} do not resolve to "
            f"{EMBEDDED_FEATURE}; a Postgres-bearing leg without the embedded "
            "cluster skips its database tests"
        )


def test_the_postgres_job_tells_the_tests_the_database_is_not_optional() -> None:
    """The promise of a database reaches the step that runs the tests.

    Without it a leg whose cluster cannot be had reports success for every test
    that needed it. It is appended to `$GITHUB_ENV`: setting it in the step's
    own shell reaches nothing that runs the tests. It is unconditional because
    every leg of the job bears Postgres, and a guard could switch it off for a
    leg it fell off.
    """
    steps = [
        step for step in _steps(_job(JOB)) if f"{REQUIRE_VARIABLE}=" in step_text(step)
    ]
    assert len(steps) == 1, (
        f"expected exactly one step exporting {REQUIRE_VARIABLE} in {JOB}, found "
        f"{len(steps)}"
    )
    assert REQUIRE_APPEND_RE.search(step_text(steps[0])), (
        f"{REQUIRE_VARIABLE} must be appended to $GITHUB_ENV; setting it in "
        "the step's own shell reaches nothing that runs the tests"
    )
    assert "if" not in steps[0], (
        f"the step exporting {REQUIRE_VARIABLE} must be unconditional, found "
        f"{steps[0].get('if')!r}"
    )


def test_the_libsql_job_is_not_told_a_database_is_mandatory() -> None:
    """The leg with no database must keep its skip.

    Exporting the requirement there would fail the libsql-only job, which is
    meant to run without one. It is a job of its own, so this is structural:
    nothing in it may export the promise.
    """
    leaking = [
        step.get("name")
        for step in _steps(_job(LIBSQL_JOB))
        if REQUIRE_VARIABLE in step_text(step)
    ]
    assert not leaking, (
        f"{LIBSQL_JOB} must not export {REQUIRE_VARIABLE}, found it in {leaking}"
    )
    for scope, environment in _database_url_environment_scopes(LIBSQL_JOB):
        if environment is None:
            continue
        assert isinstance(environment, dict), f"{scope} env must be a mapping"
        assert REQUIRE_VARIABLE not in environment, (
            f"the {scope} env of {LIBSQL_JOB} sets {REQUIRE_VARIABLE}; "
            "the libSQL-only job must not require Postgres"
        )


def test_the_libsql_job_compiles_without_postgres() -> None:
    """Keep the libsql-only job libsql-only, or its promise of no database lies.

    `postgres` is a default feature, so the job stays database-free only while
    the action turns the defaults off and names neither `postgres` nor the
    embedded cluster that implies it.
    """
    generating = [
        step
        for step in _steps(_job(LIBSQL_JOB))
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
    assert not features & {POSTGRES_FEATURE, EMBEDDED_FEATURE}, (
        "the libsql-only job must name neither the `postgres` feature nor the "
        "embedded cluster that implies it"
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
    `test_every_leg_of_the_postgres_job_compiles_postgres` would reject a
    correct declaration. The `narrow-and-named` case is the one that keeps this honest:
    it is the only narrow case expected to be false, so a reader that answered
    true for everything would fail it.
    """
    assert _enables_postgres(flags) is expected, (
        f"{flags!r} should read as postgres={expected}"
    )

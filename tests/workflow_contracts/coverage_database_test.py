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

import pytest
from _workflow_policy import WORKFLOW_DIR, load, step_text

WORKFLOW = "coverage.yml"

#: The variable `src/testing/postgres.rs` reads. Renaming the export without
#: renaming the reader is the exact mistake this file exists to catch, so the
#: constant is spelled out here rather than derived from the workflow.
TEST_URL_VARIABLE = "TEST_DATABASE_URL"

#: The job that runs the Postgres-bearing coverage legs.
JOB = "coverage"


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


def _exporting_steps() -> list[dict[str, object]]:
    """Return every step whose script writes to `GITHUB_ENV`."""
    return [step for step in _steps(_postgres_job()) if "GITHUB_ENV" in step_text(step)]


def test_the_coverage_job_exports_the_variable_the_tests_read() -> None:
    """Export the name the code reads, not one that merely looks right.

    A plausible but unread name is worse than no export at all: the job still
    runs, the tests still fail, and the workflow reports a database problem
    rather than a configuration one.
    """
    exports = "\n".join(step_text(step) for step in _exporting_steps())
    assert exports, f"{JOB} must export a database URL for the Postgres legs"
    assert f"{TEST_URL_VARIABLE}=" in exports, (
        f"{JOB} must export {TEST_URL_VARIABLE}, which "
        "src/testing/postgres.rs reads. Without it the helper falls back to a "
        "URL with no credentials and every Postgres test fails on "
        "'password missing'."
    )


def test_the_exported_url_carries_credentials() -> None:
    """A URL without a user and password is the failure being prevented."""
    exports = "\n".join(step_text(step) for step in _exporting_steps())
    assert "postgres://postgres:postgres@" in exports, (
        f"{JOB} must export a URL carrying the user and password the service "
        "container declares; a bare host is what the unset fallback already "
        "supplies, and it is what fails"
    )


def test_the_export_is_guarded_to_the_postgres_legs() -> None:
    """The libsql-only leg has no service container to point at."""
    steps = [
        step
        for step in _exporting_steps()
        if f"{TEST_URL_VARIABLE}=" in step_text(step)
    ]
    assert len(steps) == 1, (
        f"expected exactly one step exporting {TEST_URL_VARIABLE}, found {len(steps)}"
    )
    assert steps[0].get("if") == "matrix.has_postgres", (
        f"the {TEST_URL_VARIABLE} export must be guarded on "
        "matrix.has_postgres, so the libsql-only leg does not advertise a "
        "database it has not been given"
    )


@pytest.mark.parametrize("variable", ["POSTGRES_USER", "POSTGRES_PASSWORD"])
def test_the_service_container_declares_its_credentials(variable: str) -> None:
    """Keep the exported URL and the service's own credentials in one place."""
    services = _postgres_job().get("services")
    assert isinstance(services, dict), f"{JOB} must declare a services mapping"
    postgres = services.get("postgres")
    assert isinstance(postgres, dict), f"{JOB} must declare a postgres service"
    env = postgres.get("env")
    assert isinstance(env, dict), "the postgres service must declare env"
    assert env.get(variable) == "postgres", (
        f"the postgres service must set {variable} to the value the exported "
        "URL uses; if one changes the other has to change with it"
    )

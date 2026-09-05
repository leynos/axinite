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

from _workflow_policy import WORKFLOW_DIR, load, step_text

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
EXPORT_RE: typ.Final[re.Pattern[str]] = re.compile(
    r"echo\s+\"(?P<name>[A-Za-z_][A-Za-z0-9_]*)=\$\{(?P<source>[A-Za-z_]"
    r"[A-Za-z0-9_]*)\}\"\s*>>",
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
    """Return the URL value the step exports as `TEST_DATABASE_URL`.

    The script assigns the URL to a shell variable and then exports that
    variable, so both halves are resolved rather than assumed. Following the
    indirection is the point: it is what ties the credentials being asserted to
    the name the tests read.
    """
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


def test_the_export_is_guarded_to_the_postgres_legs() -> None:
    """Only the legs with a database configured may advertise one.

    `services` is declared at job level, so the container starts for every
    matrix leg including `libsql-only`. That leg is built with
    `--no-default-features --features libsql` and must not be pointed at a
    database it does not use, which is what `has_postgres` expresses.
    """
    assert _exporting_step().get("if") == "matrix.has_postgres", (
        f"the {TEST_URL_VARIABLE} export must be guarded on "
        "matrix.has_postgres, so the libsql-only leg is not handed a database "
        "URL its feature set does not use"
    )

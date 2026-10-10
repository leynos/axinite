"""Contracts binding the PostgreSQL test legs to the embedded cluster.

The fixture in `src/testing/postgres/embedded.rs` is behind the
`embedded-postgres` feature, so that the libSQL-only legs do not build the
pg-embed dependency graph. The cost of the split is that a leg which compiles
the `postgres` feature but forgets to enable `embedded-postgres` has no
database source: its PostgreSQL tests skip and the leg reports success for
tests that never ran. These contracts make that omission a failure.

`coverage.yml` is the other way to give a leg a database, a service container
named by `TEST_DATABASE_URL`, and `coverage_database_test.py` binds it.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import shlex
import typing as typ

import pytest
from _trigger_reading import matrix_legs
from _workflow_files import jobs_of
from _workflow_policy import Job
from coverage_database_test import (
    REQUIRE_VARIABLE,
    _enables_postgres,
    _features_named_by,
)

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from _estate import Estate

EMBEDDED_FEATURE: typ.Final[str] = "embedded-postgres"

#: Every event the `tests` job's matrix can resolve for.
EVENTS: typ.Final[tuple[str, ...]] = (
    "pull_request",
    "push",
    "schedule",
    "workflow_dispatch",
)

#: How the `Run Tests` step derives the promise from the leg's own flags, so a
#: leg that carries the embedded cluster is also told it is not optional.
REQUIRE_EXPRESSION: typ.Final[str] = (
    "${{ contains(matrix.flags, 'embedded-postgres') && '1' || '' }}"
)


def _named_features(flags: str) -> set[str]:
    """Return the features a command line's flags name explicitly."""
    tokens = shlex.split(flags, comments=False, posix=True)
    named: set[str] = set()
    for index, token in enumerate(tokens):
        following = tokens[index + 1] if index + 1 < len(tokens) else None
        named |= _features_named_by(token, following)
    return named


def _tests_job(estate: Estate) -> Job:
    """Return the `tests` job of `test.yml`."""
    return next(
        job for job in jobs_of("test.yml", estate["test.yml"]) if job.job_id == "tests"
    )


@pytest.mark.parametrize("event", EVENTS, ids=str)
def test_every_postgres_leg_enables_the_embedded_cluster(
    event: str, estate: Estate
) -> None:
    """A leg compiles `embedded-postgres` exactly when it compiles `postgres`.

    The wide direction is the silent skip this file exists to prevent. The
    narrow one keeps the libSQL-only legs light: `embedded-postgres` implies
    `postgres`, so a libSQL-only leg that named it would have PostgreSQL after
    all.
    """
    for leg in matrix_legs(_tests_job(estate), event):
        flags = leg.get("flags", "")
        postgres = _enables_postgres(flags)
        embedded = EMBEDDED_FEATURE in _named_features(flags)
        assert postgres == embedded, (
            f"on {event} the {leg.get('name')!r} leg has flags {flags!r}: "
            f"postgres={postgres} but {EMBEDDED_FEATURE} named={embedded}; a "
            "PostgreSQL leg without the embedded cluster skips its database "
            "tests, and a libSQL-only one must not build it"
        )


def test_the_tests_step_promises_a_database_where_the_cluster_is_enabled(
    estate: Estate,
) -> None:
    """`Run Tests` derives `AXINITE_REQUIRE_POSTGRES` from the leg's flags.

    Together with the contract above, a PostgreSQL leg that has the cluster
    also has the promise, so an unreachable database fails rather than skips,
    and a leg without one keeps the skip.
    """
    steps = _tests_job(estate).body.get("steps")
    assert isinstance(steps, list), "the tests job must declare steps"
    run_tests = [
        step
        for step in steps
        if isinstance(step, dict) and step.get("name") == "Run Tests"
    ]
    assert len(run_tests) == 1, f"expected one Run Tests step, found {len(run_tests)}"
    env = run_tests[0].get("env")
    assert isinstance(env, dict), "Run Tests must declare an env mapping"
    assert env.get(REQUIRE_VARIABLE) == REQUIRE_EXPRESSION, (
        f"Run Tests sets {REQUIRE_VARIABLE} to {env.get(REQUIRE_VARIABLE)!r}, "
        f"not {REQUIRE_EXPRESSION!r}"
    )

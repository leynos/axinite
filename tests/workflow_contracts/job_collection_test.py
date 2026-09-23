"""Unit tests for how per-job contracts collect their jobs.

`pytest_generate_tests` in `conftest.py` calls a module's `JOB_SELECTOR`
during collection. The reason it exists is the failure path: a selector
that raises `SourceError` must become a failing test naming the file, not a
collection error. The estate's own workflows are readable, so that path is
driven here with a stub collector and a selector that raises.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import types
import typing as typ
from pathlib import Path

import pytest
from _sources import SourceError
from _workflow_policy import Job
from conftest import job_or_failure, pytest_generate_tests


class _Collector:
    """A stand-in for `pytest.Metafunc` that records what it is asked."""

    def __init__(self, selector: object, fixturenames: tuple[str, ...]) -> None:
        """Record the module's selector and the test's arguments."""
        self.module = types.SimpleNamespace(JOB_SELECTOR=selector)
        self.fixturenames = fixturenames
        self.calls: list[dict[str, object]] = []

    def parametrize(
        self, argname: str, values: list[object], **options: object
    ) -> None:
        """Record one parametrization."""
        self.calls.append({"argname": argname, "values": values, **options})


def _unreadable() -> tuple[Job, ...]:
    """Stand for a selector whose workflow could not be read."""
    raise SourceError(Path("/tree/broken.yml"), "is not valid YAML (stray colon)")


def test_a_selector_that_cannot_read_becomes_one_named_parameter() -> None:
    """The source error is carried into the test, identified by its file."""
    collector = _Collector(_unreadable, ("job",))
    pytest_generate_tests(typ.cast("pytest.Metafunc", collector))
    assert len(collector.calls) == 1, (
        f"expected one parametrization; got {collector.calls}"
    )
    (call,) = collector.calls
    (value,) = typ.cast("list[object]", call["values"])
    assert isinstance(value, SourceError), (
        f"the parameter should be the error; got {value!r}"
    )
    assert call["ids"] == ["unreadable-broken.yml"], (
        f"the parameter should be identified by the file; got {call['ids']}"
    )
    assert call["indirect"] is True, "the job fixture must see the parameter first"


def test_the_error_fails_the_test_with_the_path() -> None:
    """The fixture turns the carried error into a failure naming the file."""
    error = SourceError(Path("/tree/broken.yml"), "is not valid YAML (stray colon)")
    with pytest.raises(pytest.fail.Exception, match="broken.yml"):
        job_or_failure(error)


def test_a_readable_selector_parametrizes_each_job_by_name() -> None:
    """The working path: one parameter per job, identified as `file:job`."""
    jobs = (Job("ci.yml", "build", {}), Job("ci.yml", "lint", {}))
    collector = _Collector(lambda: jobs, ("job",))
    pytest_generate_tests(typ.cast("pytest.Metafunc", collector))
    (call,) = collector.calls
    assert list(typ.cast("tuple[Job, ...]", call["values"])) == list(jobs), (
        f"every job should be a parameter; got {call}"
    )
    assert call["ids"] == [str(job) for job in jobs], (
        f"ids should name the jobs; got {call}"
    )
    assert job_or_failure(jobs[0]) is jobs[0], (
        "a job parameter passes through unchanged"
    )


def test_a_test_without_a_job_argument_is_left_alone() -> None:
    """Only tests asking for `job` are parametrized."""
    collector = _Collector(_unreadable, ("estate",))
    pytest_generate_tests(typ.cast("pytest.Metafunc", collector))
    assert collector.calls == [], (
        f"nothing should be parametrized; got {collector.calls}"
    )

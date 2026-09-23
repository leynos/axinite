"""The entry point at which these contracts read the files they judge.

Everything the suite contracts do with a workflow, a Makefile or a manifest is
a pure function of text. The reading is here, in two session fixtures, so that
each source is read and parsed once per run and so that a file which is
missing or malformed fails the contracts that asked for it, by name and with
the path in the message.

The alternative, and what these fixtures replaced, was a module-level snapshot
taken during import. A bad file then raises while pytest is collecting, and a
contract directory that fails to collect reports no failures at all, which
reads exactly like a clean run.

`_sources.py` holds the reading, and `source_boundary_test.py` states each
failure it converts.

A contract that asserts one job per test needs its jobs while pytest is
collecting, because that is when a parameter list and its identifiers are
fixed, so it cannot take a fixture. Such a module names a `JOB_SELECTOR`, a
function returning its jobs, and `pytest_generate_tests` below calls it and
parametrizes the `job` argument. A `SourceError` raised by the selector does
not escape as a collection error: it becomes the one parameter, and the `job`
fixture fails that test with the error, naming the file.
"""

from __future__ import annotations

import typing as typ

import pytest
from _estate import isolated, read_estate
from _sources import SourceError
from _suite_targets import read_default_features

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Callable
    from pathlib import Path

    from _estate import Estate
    from _workflow_policy import Job

#: The module attribute naming a contract's job selector, and the argument
#: it parametrizes.
JOB_SELECTOR = "JOB_SELECTOR"
JOB_ARGUMENT = "job"


def pytest_generate_tests(metafunc: pytest.Metafunc) -> None:
    """Parametrize `job` over the jobs a module's `JOB_SELECTOR` returns.

    The selector runs here, during collection, and nowhere earlier: the
    module only names it. A selector that raises `SourceError` yields one
    parameter holding the error, identified by the file it names, so the
    failure is a test failure with the path in it rather than a collection
    error.

    Parameters
    ----------
    metafunc
        pytest's view of the test being collected.
    """
    select = getattr(metafunc.module, JOB_SELECTOR, None)
    if select is None or JOB_ARGUMENT not in metafunc.fixturenames:
        return
    try:
        jobs: tuple[object, ...] = tuple(select())
    except SourceError as error:
        metafunc.parametrize(
            JOB_ARGUMENT, [error], ids=[f"unreadable-{error.path.name}"], indirect=True
        )
        return
    metafunc.parametrize(
        JOB_ARGUMENT, jobs, ids=[str(job) for job in jobs], indirect=True
    )


@pytest.fixture
def job(request: pytest.FixtureRequest) -> Job:
    """Return the job this test was parametrized with.

    Parameters
    ----------
    request
        pytest's request, carrying the parameter `pytest_generate_tests`
        chose.

    Returns
    -------
    Job
        The job under test.
    """
    return job_or_failure(request.param)


def job_or_failure(parameter: object) -> Job:
    """Return a parametrized job, or fail the test with the source error.

    Parameters
    ----------
    parameter
        What `pytest_generate_tests` parametrized the test with: a job, or
        the `SourceError` the selector raised.

    Returns
    -------
    Job
        The parameter, when it is not an error.

    Raises
    ------
    pytest.fail.Exception
        When the parameter is a `SourceError`, with its message, which names
        the file.
    """
    if isinstance(parameter, SourceError):
        pytest.fail(f"the workflow estate could not be read: {parameter}")
    return typ.cast("Job", parameter)


@pytest.fixture(scope="session")
def defaults() -> frozenset[str]:
    """Return the root manifest's `default` feature list.

    Cargo enables these on every command that does not pass
    `--no-default-features`, so they are part of what a command runs whether
    it names them or not.

    Returns
    -------
    frozenset of str
        Every feature in the root manifest's `default` list.
    """
    return read_default_features()


@pytest.fixture(scope="session")
def _estate_source() -> Estate:
    """Read and parse the estate once per run.

    Private, because a contract that took this would share one structure with
    every other contract. `estate` hands out an isolated copy of it.

    Returns
    -------
    Mapping
        The parsed documents, keyed by file name.
    """
    return read_estate()


@pytest.fixture
def estate(_estate_source: Estate) -> Estate:
    """Return every workflow this repository declares, parsed by file name.

    Each test gets its own deep copy. The outer mapping is a proxy, so a
    contract cannot add or replace a workflow; but a proxy is shallow, and
    the documents beneath it are ordinary dictionaries and lists. A contract
    appending to one job's `steps` would otherwise change what a later
    contract judges, and the failure would name the later contract, which had
    done nothing wrong, about a reading that was gone by the time anyone
    looked.

    Parsing stays at session scope, since that is the part that costs
    anything; only the copy is per test.

    Returns
    -------
    Mapping
        The parsed documents, isolated from every other test's copy.
    """
    return isolated(_estate_source)


@pytest.fixture
def workflow_directory(tmp_path: Path) -> Callable[[str, bytes], Path]:
    """Return a factory that writes one workflow into a fresh directory.

    For the boundary cases, which state a fault against a temporary tree
    rather than against the estate. The contents are bytes so that an
    undecodable file can be written as such.

    Parameters
    ----------
    tmp_path
        pytest's per-test temporary directory, under which each call
        creates the `workflows` directory.

    Returns
    -------
    Callable
        Given a file name and its raw contents, writes that one file into a
        new `workflows` directory under the test's temporary path and returns
        the directory.
    """

    def write(name: str, body: bytes) -> Path:
        """Write one workflow file into a fresh directory and return it."""
        directory = tmp_path / "workflows"
        directory.mkdir()
        (directory / name).write_bytes(body)
        return directory

    return write

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
"""

from __future__ import annotations

import typing as typ

import pytest
from _estate import isolated, read_estate
from _suite_targets import read_default_features

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Callable
    from pathlib import Path

    from _estate import Estate


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

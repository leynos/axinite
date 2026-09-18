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
from _suite_reader import read_estate
from _suite_targets import read_default_features

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from _suite_reader import Estate


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
def estate() -> Estate:
    """Return every workflow this repository declares, parsed by file name.

    Returns
    -------
    Mapping
        The parsed documents, in a mapping no contract can alter: one test
        mutating the estate would change what a later one judges, and the
        failure would name the later test.
    """
    return read_estate()

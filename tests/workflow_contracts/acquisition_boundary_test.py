"""The acquisition functions report a failure rather than return nothing.

Every sweep in this suite is shaped like a query: a name and a return
type that promise a value, over a body that reaches the filesystem. That
shape is the hazard. A directory that cannot be listed, or a file that
cannot be read, would otherwise come back as an empty result, the caller
would read it as a tree with no workflows and no compile contracts, and
every assertion over it would pass over nothing while reporting success.

So the boundary is asserted here rather than only documented. Each entry
point is driven at a directory that does not exist, and each must raise
`SourceReadError` carrying the path rather than return an empty result.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import os
import sys
import typing as typ
from pathlib import Path

import pytest
from _workflow_policy import declared_jobs, jobs_in, load, workflow_paths
from contract_sources import SourceReadError, matching_entries, read_source
from suite_lanes import suite_lanes_of
from timeout_budgets import compile_contract_binaries

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    import collections.abc as cabc


def _absent(tmp_path: Path) -> Path:
    """Return a path under the temporary tree that does not exist.

    Parameters
    ----------
    tmp_path
        pytest's per-test temporary directory.

    Returns
    -------
    Path
        A path that is guaranteed absent.
    """
    missing = tmp_path / "no-such-tree"
    assert not missing.exists(), "the fixture must name a path that is absent"
    return missing


#: Each acquisition entry point, as a callable taking the absent path.
#: Named individually rather than discovered, because the point of the
#: table is that somebody had to decide each of these is acquisition.
ENTRY_POINTS: typ.Final[dict[str, cabc.Callable[[Path], object]]] = {
    "workflow_paths": workflow_paths,
    "suite_lanes_of": suite_lanes_of,
    "compile_contract_binaries": compile_contract_binaries,
    "matching_entries": lambda path: matching_entries(path, "*.rs"),
    "load": lambda path: load(path / "ci.yml"),
    "declared_jobs": lambda path: declared_jobs(path / "ci.yml"),
    "jobs_in": lambda path: list(jobs_in(path / "ci.yml")),
    "read_source": lambda path: read_source(path / "ci.yml"),
}


@pytest.mark.parametrize("name", sorted(ENTRY_POINTS))
def test_an_unreadable_source_is_reported_not_swallowed(
    name: str, tmp_path: Path
) -> None:
    """Assert each entry point raises rather than returning an empty result.

    The empty return is the dangerous answer rather than the loud one:
    nothing distinguishes "no workflows here" from "this directory could
    not be read", and the assertions downstream cannot tell either.
    """
    missing = _absent(tmp_path)
    with pytest.raises(SourceReadError) as raised:
        ENTRY_POINTS[name](missing)
    assert missing in (raised.value.path, *raised.value.path.parents), (
        f"{name} must report the path it could not read; it reported "
        f"{raised.value.path}"
    )


def test_the_error_is_an_os_error() -> None:
    """Assert the domain error keeps `OSError` as a base.

    These are operating-system failures, and a caller that already
    catches `OSError` around a read should keep catching them. Narrowing
    the base would silently stop such a caller working while every test
    that catches the domain type kept passing.
    """
    assert issubclass(SourceReadError, OSError), (
        "SourceReadError must remain an OSError, so a caller catching that "
        "family keeps catching this one"
    )


def test_a_readable_tree_still_returns_its_contents(tmp_path: Path) -> None:
    """Assert the refusal above is narrow as well as sufficient.

    A boundary that raised on every input would satisfy every case in
    the table and make the whole suite unrunnable, so the working path
    is pinned beside the failing one.
    """
    workflow = tmp_path / "ci.yml"
    workflow.write_text("name: controlled\non: push\njobs: {}\n", encoding="utf-8")
    assert workflow_paths(tmp_path) == [workflow], (
        "a readable directory must still yield its workflow files"
    )
    assert load(workflow)["name"] == "controlled", (
        "a readable workflow must still parse"
    )


#: The sweeps that reach the filesystem through a directory listing, as
#: callables taking the directory. `read_source` is absent because it
#: takes a file, and a file inside an unreadable directory cannot be
#: named to open in the first place.
DIRECTORY_SWEEPS: typ.Final[dict[str, cabc.Callable[[Path], object]]] = {
    "workflow_paths": workflow_paths,
    "suite_lanes_of": suite_lanes_of,
    "compile_contract_binaries": compile_contract_binaries,
    "matching_entries": lambda path: matching_entries(path, "*.rs"),
    "matching_entries_nested": lambda path: matching_entries(path, "*/main.rs"),
}


@pytest.mark.skipif(
    sys.platform == "win32",
    reason="POSIX permission bits; Windows chmod only toggles read-only",
)
@pytest.mark.parametrize("name", sorted(DIRECTORY_SWEEPS))
def test_an_unreadable_directory_fails_closed(name: str, tmp_path: Path) -> None:
    """Assert a directory that exists but cannot be read is reported.

    This is the sharper half of the empty-result hazard and the reason
    the sweeps enumerate rather than glob. From Python 3.13 ``Path.glob``
    suppresses the errors raised while scanning, so an unreadable
    directory passes ``is_dir`` and yields nothing, which is
    indistinguishable from a directory with no matches. Neither a
    pre-check nor a ``try`` around the call can see it: there is no
    exception left to catch and nothing about the result to doubt.

    A source file is planted first, so the directory is one that would
    have matched. A sweep reporting an empty set here is reporting the
    absence of a file that is there.
    """
    if os.geteuid() == 0:
        pytest.skip("root reads a directory whatever its mode bits say")
    unreadable = tmp_path / "unreadable"
    nested = unreadable / "contract"
    nested.mkdir(parents=True)
    (unreadable / "ui.rs").write_text("trybuild::TestCases", encoding="utf-8")
    (nested / "main.rs").write_text("trybuild::TestCases", encoding="utf-8")
    unreadable.chmod(0o000)
    try:
        with pytest.raises(SourceReadError) as raised:
            DIRECTORY_SWEEPS[name](unreadable)
    finally:
        unreadable.chmod(0o700)
    assert unreadable in (raised.value.path, *raised.value.path.parents), (
        f"{name} must report the directory it could not read; it reported "
        f"{raised.value.path}"
    )


@pytest.mark.skipif(
    sys.platform == "win32",
    reason="POSIX permission bits; Windows chmod only toggles read-only",
)
def test_a_readable_nested_tree_is_still_matched(tmp_path: Path) -> None:
    """Assert the nested walk still finds what it should.

    The refusal above is satisfied by a walk that reports nothing ever,
    so the shape the sweeps actually use is pinned: a source beside the
    directory and a source one level down, both found.
    """
    nested = tmp_path / "contract"
    nested.mkdir()
    flat_source = tmp_path / "ui.rs"
    flat_source.write_text("trybuild::TestCases", encoding="utf-8")
    nested_source = nested / "main.rs"
    nested_source.write_text("trybuild::TestCases", encoding="utf-8")
    assert matching_entries(tmp_path, "*.rs") == [flat_source], (
        "the flat pattern matches the source beside the directory"
    )
    assert matching_entries(tmp_path, "*/main.rs") == [nested_source], (
        "the nested pattern matches one level down"
    )
    assert compile_contract_binaries(tmp_path) == frozenset({"ui", "contract"}), (
        "cargo names a target after tests/<name>.rs or tests/<name>/main.rs, "
        "and both forms must be discovered"
    )


def test_a_pattern_this_reader_cannot_walk_is_refused(tmp_path: Path) -> None:
    """Assert a deeper pattern is refused rather than under-matched.

    One separator is what this repository's sweeps use. A deeper pattern
    would match nothing here while a glob would have matched, and a
    reader that cannot walk a pattern must not report on it.
    """
    with pytest.raises(SourceReadError, match=r"nests deeper"):
        matching_entries(tmp_path, "*/*/main.rs")

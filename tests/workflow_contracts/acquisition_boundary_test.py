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

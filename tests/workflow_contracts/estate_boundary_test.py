"""Contracts for every public reading of the workflow estate.

`source_boundary_test.py` states the boundary through `read_estate`. The estate
has four more ways in: `read_workflows`, the unfiltered scan; `estate_source`
and `estate_jobs`, the cached readings the collection-time contracts use; and
`read_workflow`, for a contract that judges one named file. A fault converted
on one of those paths and not another would escape exactly where nobody was
looking, so each fault is stated against every path here.

The cached readings carry a second obligation. They parse once per process and
hand the result to many contracts, so a contract that altered what it was
given would change what every later one judged. Each call therefore returns a
copy, and the cases below alter one and read again.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import typing as typ

import pytest
from _estate import (
    estate_jobs,
    estate_source,
    isolated,
    read_estate,
    read_workflow,
    read_workflows,
)
from _sources import SourceError

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Callable
    from pathlib import Path

#: Every public reading that scans a directory, by the name a failure reports.
DIRECTORY_READERS: tuple[tuple[str, Callable[[Path], object]], ...] = (
    ("read_workflows", read_workflows),
    ("read_estate", read_estate),
    ("estate_source", estate_source),
    ("estate_jobs", estate_jobs),
)

#: A workflow with one job and one step, the smallest shape the isolation
#: cases can alter at every level they care about.
MINIMAL_WORKFLOW = (
    "on: [push]\n"
    "jobs:\n"
    "  build:\n"
    "    runs-on: ubuntu-latest\n"
    "    steps:\n"
    "      - run: make test\n"
)

#: File contents that fail the boundary, each with the phrase its
#: `SourceError` must carry.
FAULTY_WORKFLOWS: tuple[tuple[str, bytes, str], ...] = (
    ("undecodable", b"on: [push]\njobs: \xff\xfe\n", "is not valid UTF-8"),
    ("malformed-yaml", b"on: [push]\njobs:\n  - :\n  :", "is not valid YAML"),
    ("a-sequence", b"[]", "is not a workflow"),
    ("a-scalar", b"just a string", "is not a workflow"),
    ("empty", b"", "is not a workflow"),
)


def _directory_with(tmp_path: Path, name: str, body: bytes) -> Path:
    """Return a fresh workflow directory holding one file.

    Parameters
    ----------
    tmp_path
        The test's temporary directory.
    name
        The workflow's file name.
    body
        Its raw contents, so an undecodable file can be written as such.

    Returns
    -------
    Path
        The directory, to hand to a reader.
    """
    directory = tmp_path / "workflows"
    directory.mkdir()
    (directory / name).write_bytes(body)
    return directory


@pytest.mark.parametrize(
    ("reader_name", "reader"),
    [pytest.param(name, reader, id=name) for name, reader in DIRECTORY_READERS],
)
@pytest.mark.parametrize(
    ("body", "reason"),
    [pytest.param(body, reason, id=case) for case, body, reason in FAULTY_WORKFLOWS],
)
def test_a_faulty_workflow_is_named_on_every_directory_reading(
    tmp_path: Path,
    reader_name: str,
    reader: Callable[[Path], object],
    body: bytes,
    reason: str,
) -> None:
    """Every scan converts the same faults, and names the file.

    The cached readings are where this matters most: they run while pytest
    is collecting, and an `OSError`, a `UnicodeDecodeError` or a `YAMLError`
    escaping there is a collection error, which reports no failures at all.
    """
    directory = _directory_with(tmp_path, "odd.yml", body)
    with pytest.raises(SourceError, match=reason) as raised:
        reader(directory)
    assert "odd.yml" in str(raised.value), (
        f"{reader_name} must name the file it could not read; it said "
        f"{raised.value}"
    )


@pytest.mark.parametrize(
    ("reader_name", "reader"),
    [pytest.param(name, reader, id=name) for name, reader in DIRECTORY_READERS],
)
def test_a_missing_directory_is_named_on_every_directory_reading(
    tmp_path: Path, reader_name: str, reader: Callable[[Path], object]
) -> None:
    """A scan of nothing is a failure, not an estate of no workflows.

    An empty estate satisfies every contract that asks whether anything in
    the estate is wrong, so it is the fault that reads most like success.
    """
    absent = tmp_path / "workflows"
    with pytest.raises(SourceError, match="cannot be scanned") as raised:
        reader(absent)
    assert str(absent) in str(raised.value), (
        f"{reader_name} must name the directory it could not scan; it said "
        f"{raised.value}"
    )


@pytest.mark.parametrize(
    ("body", "reason"),
    [pytest.param(body, reason, id=case) for case, body, reason in FAULTY_WORKFLOWS],
)
def test_a_faulty_workflow_is_named_when_read_alone(
    tmp_path: Path, body: bytes, reason: str
) -> None:
    """The single-file reading converts the same faults as the scans."""
    path = _directory_with(tmp_path, "odd.yml", body) / "odd.yml"
    with pytest.raises(SourceError, match=reason) as raised:
        read_workflow(path)
    assert "odd.yml" in str(raised.value), (
        f"read_workflow must name the file it could not read; it said "
        f"{raised.value}"
    )


def test_a_missing_workflow_is_named_when_read_alone(tmp_path: Path) -> None:
    """A named workflow that is not there fails with its path."""
    absent = tmp_path / "test.yml"
    with pytest.raises(SourceError, match="cannot be read") as raised:
        read_workflow(absent)
    assert str(absent) in str(raised.value), (
        f"read_workflow must name the missing file; it said {raised.value}"
    )


def test_a_workflow_read_alone_is_its_parsed_document(tmp_path: Path) -> None:
    """The positive half, without which the refusals prove nothing.

    A reader that raised for everything would pass every case above.
    """
    path = _directory_with(tmp_path, "ci.yml", MINIMAL_WORKFLOW.encode()) / "ci.yml"
    document = read_workflow(path)
    assert list(document.get("jobs", {})) == ["build"], (
        f"read_workflow should return the parsed document; it returned {document}"
    )


def test_estate_source_hands_each_caller_its_own_copy(tmp_path: Path) -> None:
    """A nested change to one reading is invisible to the next.

    The outer mapping is a proxy, but a proxy is shallow: without the copy,
    appending to a job's `steps` would alter the cached parse and every
    contract that read it afterwards.
    """
    directory = _directory_with(tmp_path, "ci.yml", MINIMAL_WORKFLOW.encode())
    first = estate_source(directory)
    first["ci.yml"]["jobs"]["build"]["steps"].append({"run": "invented"})
    second = estate_source(directory)
    steps = second["ci.yml"]["jobs"]["build"]["steps"]
    assert steps == [{"run": "make test"}], (
        "a change made to one estate_source reading reached the next one: "
        f"{steps}"
    )


def test_estate_jobs_hands_each_caller_its_own_copy(tmp_path: Path) -> None:
    """A change to one job body is invisible to the next reading of it."""
    directory = _directory_with(tmp_path, "ci.yml", MINIMAL_WORKFLOW.encode())
    (first,) = estate_jobs(directory)
    first.body["runs-on"] = "ubicloud-standard-8"
    (second,) = estate_jobs(directory)
    assert second.body["runs-on"] == "ubuntu-latest", (
        "a change made to one estate_jobs reading reached the next one: "
        f"{second.body['runs-on']}"
    )


def test_an_isolated_copy_shares_nothing_with_its_original() -> None:
    """The copy is deep, and the proxy refuses a new workflow outright."""
    original = {"ci.yml": {"jobs": {"build": {"steps": [{"run": "make"}]}}}}
    copied = isolated(original)
    copied["ci.yml"]["jobs"]["build"]["steps"].append({"run": "invented"})
    assert original["ci.yml"]["jobs"]["build"]["steps"] == [{"run": "make"}], (
        f"isolated shared a nested list with its original: {original}"
    )
    with pytest.raises(TypeError):
        copied["invented.yml"] = {}  # type: ignore[index]

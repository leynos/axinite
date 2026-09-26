"""Reading workflow files into documents and jobs.

Split from `_workflow_policy.py`, which says what a job declares; this module
turns workflow text into the documents and `Job` values those questions are
asked of, and holds the thin file-reading edge that lists the workflow
directory and reads one file.

The pure readers, `parse_workflow`, `declared_jobs_in` and `jobs_of`, take text
or an already-parsed mapping, so a test can exercise them without writing a
file. `workflow_paths`, `load`, `declared_jobs`, `jobs_in` and `jobs` read the
filesystem and do nothing but read and delegate. A contract that reads the
estate should prefer `_estate.py`, which reads through this edge and converts
every failure into a `SourceError` naming the file.
"""

from __future__ import annotations

import typing as typ
from pathlib import Path

import yaml
from _workflow_policy import WORKFLOW_DIR, Job
from contract_sources import (
    SourceReadError,
    directory_entries,
    is_regular_file,
    read_source,
)

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Iterator

#: Extensions GitHub accepts for a workflow file. Scanning only `.yml` would
#: silently exempt a `.yaml` workflow from every contract in this directory,
#: which is the same vacuous pass an unread `runs-on` produces.
WORKFLOW_SUFFIXES: tuple[str, ...] = (".yml", ".yaml")

#: Re-exported so a caller of the acquisition functions below can catch
#: what they raise without importing a second module. The boundary is
#: only useful if the error it reports is reachable from the same place
#: as the functions that raise it.
__all__ = ["SourceReadError"]



def workflow_paths(directory: Path = WORKFLOW_DIR) -> list[Path]:
    """Return every workflow file in a directory.

    Acquisition, not a query: this lists a directory. The distinction is
    stated on every function below that touches the filesystem, because
    a name and a return type that read like a query are exactly what
    hides the fact.

    Parameters
    ----------
    directory
        Directory to scan. It defaults to the repository's workflow
        directory; the parameter exists so a test can point the same scan at
        a temporary tree instead of the estate.

    Returns
    -------
    list of Path
        Workflow paths sorted by name, so parameterized tests report in a
        stable order. Both extensions GitHub accepts are included.

    Raises
    ------
    SourceReadError
        If the directory cannot be listed, or an entry in it cannot be
        classified as a regular file or not.
    """
    # Classified through `is_regular_file` rather than `Path.is_file`.
    # `Path.is_file` answers False for an entry it could not stat, so a
    # workflow directory that lists but does not stat would drop an
    # existing workflow here and a lane reading would certify a set with
    # that lane missing from it. The extension test comes second, so an
    # entry of the wrong suffix costs no stat at all.
    return sorted(
        path
        for path in directory_entries(directory)
        if path.suffix in WORKFLOW_SUFFIXES and is_regular_file(path)
    )


def parse_workflow(text: str, name: str) -> dict[str, object]:
    """Parse workflow text into a mapping.

    Parameters
    ----------
    text
        The workflow document's YAML source.
    name
        File name to quote in the failure message. It identifies the
        document and is not used to read anything.

    Returns
    -------
    dict
        The parsed workflow document.

    Raises
    ------
    AssertionError
        If the text does not parse as a mapping, which means it is not a
        workflow at all.
    """
    document = yaml.safe_load(text)
    if not isinstance(document, dict):
        message = f"{name} must parse as a mapping"
        raise AssertionError(message)
    return document


def declared_jobs_in(document: dict[str, object]) -> dict[str, object]:
    """Return a parsed workflow's jobs mapping.

    Parameters
    ----------
    document
        A parsed workflow document.

    Returns
    -------
    dict
        The workflow's jobs, or an empty mapping when it declares none, or
        declares one that is not a mapping.
    """
    declared = document.get("jobs")
    return declared if isinstance(declared, dict) else {}


def jobs_of(name: str, document: dict[str, object]) -> Iterator[Job]:
    """Yield the jobs a parsed workflow declares.

    Parameters
    ----------
    name
        The workflow's file name, carried on each `Job` for assertion
        messages.
    document
        A parsed workflow document.

    Yields
    ------
    Job
        Each job whose body is a mapping. A job whose body is anything else
        is skipped rather than raising, because the contracts that care about
        malformed jobs report them by name.
    """
    for job_id, body in declared_jobs_in(document).items():
        if isinstance(body, dict):
            yield Job(name, job_id, body)


def load(path: Path) -> dict[str, object]:
    """Read and parse one workflow file.

    Parameters
    ----------
    path
        Workflow file to read.

    Returns
    -------
    dict
        The parsed workflow document.

    Raises
    ------
    SourceReadError
        If the file cannot be read or is not UTF-8.
    """
    return parse_workflow(read_source(path), path.name)


def declared_jobs(path: Path) -> dict[str, object]:
    """Return one workflow file's jobs mapping.

    Parameters
    ----------
    path
        Workflow file to read.

    Returns
    -------
    dict
        The workflow's jobs, or an empty mapping when it declares none.

    Raises
    ------
    SourceReadError
        If the file cannot be read or is not UTF-8.
    """
    return declared_jobs_in(load(path))


def jobs_in(path: Path) -> Iterator[Job]:
    """Yield the jobs one workflow file declares.

    Parameters
    ----------
    path
        Workflow file to read.

    Yields
    ------
    Job
        Each job whose body is a mapping.

    Raises
    ------
    SourceReadError
        If the file cannot be read or is not UTF-8.
    """
    yield from jobs_of(path.name, load(path))


def jobs() -> Iterator[Job]:
    """Yield every job declared across the workflow estate.

    Yields
    ------
    Job
        Every job in every workflow, in workflow-name order.

    Raises
    ------
    SourceReadError
        If the workflow directory cannot be listed, or a workflow in it
        cannot be read.
    """
    for path in workflow_paths():
        yield from jobs_in(path)

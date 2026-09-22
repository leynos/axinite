"""What workflows this repository declares, read and parsed once.

The estate side of the source boundary. `_sources.py` turns a file that cannot
be read or parsed into a `SourceError` naming it; this module applies that to
the workflow directory and decides which of the results each kind of contract
is entitled to see.

Two readings, because two questions. `read_workflows` returns every file.
`read_estate` drops the dist-generated release workflow, because the suite
contracts may not judge a file that is regenerated wholesale; the placement and
cache contracts read the unfiltered one, in order to exempt that workflow by
name, which is a statement they make rather than one this module makes for
them.

`read_workflow` is the same boundary for a contract that judges one named
file, so it need not reach past it with a bare `read_text` and a parse.

`conftest.py` builds the `estate` fixture on `read_estate`, handing each test
an isolated copy. `estate_source` and `estate_jobs` are the readings for the
contracts that assert one job per test: a parameter list and its identifiers
are fixed while pytest is collecting, so those cannot take a fixture, and
reading through here is what keeps their failure a `SourceError` naming the
file. The parse behind them is cached once per process, but what they return
is a copy too, made by `isolated`: a proxy is shallow, and a contract that
appended to a cached job's `steps` would otherwise change what every later
reader judged.
"""

from __future__ import annotations

import copy
import typing as typ
from collections.abc import Mapping
from functools import cache
from types import MappingProxyType

import yaml

from _sources import SourceError, read_text
from _workflow_policy import (
    DIST_GENERATED,
    WORKFLOW_DIR,
    Job,
    jobs_of,
    parse_workflow,
    workflow_paths,
)

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from pathlib import Path

#: Every workflow this contract judges, parsed and keyed by name.
Estate = Mapping[str, Mapping[str, object]]


def read_estate(directory: Path = WORKFLOW_DIR) -> Estate:
    """Read and parse every workflow this contract judges.

    The boundary for the workflow side, matching
    `_suite_targets.read_default_features` on the manifest side. Reading here
    rather than at import is what keeps a workflow this cannot parse from
    becoming a collection error: a contract directory that fails to collect
    reports no failures at all, which reads exactly like a clean run.

    Parameters
    ----------
    directory
        The directory to scan. It defaults to the estate's own; the parameter
        exists so the failure cases can be stated against a temporary tree.

    Returns
    -------
    Mapping
        Each workflow document, keyed by file name, in a mapping the caller
        cannot alter. The dist-generated release workflow is left out: its
        contents are regenerated wholesale and nothing here may judge them.

    Raises
    ------
    SourceError
        If the directory cannot be scanned, or a workflow in it cannot be
        read or parsed as YAML.
    """
    return MappingProxyType(
        {
            name: document
            for name, document in read_workflows(directory).items()
            if name != DIST_GENERATED
        }
    )


def read_workflows(directory: Path = WORKFLOW_DIR) -> Estate:
    """Read and parse every workflow file in a directory, generated included.

    The unfiltered reading. `read_estate` drops the dist-generated release
    workflow on top of this, because the suite contracts may not judge a file
    that is regenerated wholesale; the placement and cache contracts do read
    it, in order to exempt it by name, which is a statement they make rather
    than one this reader makes for them.

    Parameters
    ----------
    directory
        The directory to scan.

    Returns
    -------
    Mapping
        Each workflow document, keyed by file name.

    Raises
    ------
    SourceError
        If the directory cannot be scanned, or a workflow in it cannot be
        read or parsed as YAML.
    """
    try:
        paths = workflow_paths(directory)
    except OSError as error:
        raise SourceError(
            directory, f"cannot be scanned ({error.strerror or error})"
        ) from error
    return MappingProxyType({path.name: read_workflow(path) for path in paths})


def read_workflow(path: Path) -> dict[str, object]:
    """Read and parse one workflow file, or raise `SourceError` naming it.

    For a contract that judges one named workflow. It is the same conversion
    `read_workflows` applies to every file in the directory, so a contract
    that wants `test.yml` alone does not reach past the boundary with a bare
    read and a parse whose failures name nothing.

    Parameters
    ----------
    path
        The workflow file to read.

    Returns
    -------
    dict
        The parsed workflow document.

    Raises
    ------
    SourceError
        If the file is missing, unreadable, not valid UTF-8, not valid YAML,
        or valid YAML whose root is not a mapping.
    """
    text = read_text(path)
    try:
        return parse_workflow(text, path.name)
    except yaml.YAMLError as error:
        raise SourceError(path, f"is not valid YAML ({error})") from error
    except AssertionError as error:
        # `parse_workflow` asserts the root is a mapping, which a file holding
        # `[]` or a bare scalar is not. That is valid YAML and an invalid
        # workflow, so it belongs here with the other readings this boundary
        # names rather than escaping as an `AssertionError` that names no file.
        raise SourceError(path, f"is not a workflow ({error})") from error


def isolated(estate: Estate) -> Estate:
    """Return a deep copy of an estate that shares nothing with the original.

    The outer mapping is a proxy, so a contract cannot add or replace a
    workflow; but a proxy is shallow, and the documents beneath it are
    ordinary dictionaries and lists. Every reading that hands out a parse
    cached for the whole run goes through this, so one contract appending to
    a job's `steps` cannot change what a later contract judges.

    Parameters
    ----------
    estate
        The parsed documents to copy.

    Returns
    -------
    Mapping
        The same documents, copied all the way down, behind a fresh proxy.
    """
    return MappingProxyType(copy.deepcopy(dict(estate)))


def estate_source(directory: Path = WORKFLOW_DIR) -> Estate:
    """Return the estate, parsed once per process and copied per call.

    The same reading the `estate` fixture is built on, for the contracts that
    must have their values while pytest is collecting. Anything that can wait
    should take the fixture instead.

    Parameters
    ----------
    directory
        The directory to scan. It defaults to the estate's own; the parameter
        exists so the failure cases can be stated against a temporary tree.

    Returns
    -------
    Mapping
        The parsed documents, keyed by file name, isolated from the cached
        parse and from every other caller's copy.

    Raises
    ------
    SourceError
        If the directory cannot be scanned, or a workflow in it cannot be
        read or parsed.
    """
    return isolated(_estate_once(directory))


def estate_jobs(directory: Path = WORKFLOW_DIR) -> tuple[Job, ...]:
    """Return every job the estate declares, in workflow order.

    For the contracts that assert one job per test. Those need their values
    while pytest is collecting, because that is when a parameter list and its
    identifiers are fixed, so they cannot take the `estate` fixture; what they
    can do, and what this is for, is read through the same boundary, so a
    workflow that cannot be read or parsed raises a `SourceError` naming the
    file rather than an `OSError` or a `YAMLError` from somewhere inside a
    module-level expression.

    A contract that merely iterates the estate should take the `estate`
    fixture instead. The job bodies here are copied from the cached parse on
    every call, so a contract that alters one alters only its own.

    Parameters
    ----------
    directory
        The directory to scan. It defaults to the estate's own; the parameter
        exists so the failure cases can be stated against a temporary tree.

    Returns
    -------
    tuple of Job
        Every job in every workflow, the dist-generated release workflow
        included. This is the unfiltered `read_workflows` reading, not
        `read_estate`, because the contracts that use it exempt that workflow
        by name and so have to be able to see it.

    Raises
    ------
    SourceError
        If the directory cannot be scanned, or a workflow in it cannot be
        read or parsed.
    """
    return tuple(
        job
        for name, document in isolated(_all_workflows_once(directory)).items()
        for job in jobs_of(name, document)
    )


@cache
def _estate_once(directory: Path) -> Estate:
    """Read the filtered estate once per process and directory.

    Private, because what it returns is the cached parse itself; the public
    readings copy it through `isolated` before anyone can touch it.

    Parameters
    ----------
    directory
        The directory to scan.

    Returns
    -------
    Mapping
        The parsed documents, the dist-generated workflow left out.
    """
    return read_estate(directory)


@cache
def _all_workflows_once(directory: Path) -> Estate:
    """Read every workflow once per process and directory, generated included.

    Separate from `_estate_once` because the two answer different questions:
    the contracts that exempt the dist-generated workflow by name have to be
    able to see it, and the suite contracts must not. Private for the same
    reason as `_estate_once`.

    Parameters
    ----------
    directory
        The directory to scan.

    Returns
    -------
    Mapping
        Every parsed workflow, keyed by file name.
    """
    return read_workflows(directory)

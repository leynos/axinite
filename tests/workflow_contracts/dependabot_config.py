"""Reading `.github/dependabot.yml` and finding one entry in it.

Concordat's DB-005 rule, `dependabot-update-shape`, is the authority on this
file's shape, and the estate runs it as a separate audit. An audit is evidence
about one revision, though: nothing a later contributor changes has to pass
before it merges. `_dependabot_shape.py` states the same questions over the
parsed document, and `dependabot_estate_test.py` applies them to the
estate, so a change that drops the catch-all or lists a directory the
repository no longer has fails the repository's own gate.

This module is the boundary and the lookups; the shape questions live next
door, and `dependabot_reader_test.py` drives them with constructed documents.
Only what a parsed document can answer is judged at all: coverage of
local action manifests under `.github/actions` belongs to the rule, which
reads a directory listing its envelope carries, and this repository has no
such directory.

`read_dependabot` is the one function here that touches the filesystem, and it
reports a file it cannot read as the same `SourceError` every other reading in
this directory raises.
"""

from __future__ import annotations

import typing as typ

from _sources import SourceError, read_text
from _strict_workflows import WorkflowReadError, parse_strict

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Mapping
    from pathlib import Path


class DependabotConfigurationError(Exception):
    """The configuration does not declare something these readings judge.

    Raised for a document that is not a configuration these readings can
    judge -- no `updates` list, an entry that is not a mapping -- and for a
    lookup that does not find exactly one entry. A shape canon refuses is not
    an error: those come back as messages, so a contract can name every one
    of them at once.
    """


def read_dependabot(path: Path) -> Mapping[str, object]:
    """Return the parsed configuration, or raise `SourceError` naming it.

    The strict loader is the workflow one, and it is used for the same reason
    here: PyYAML keeps the last value of a repeated key and says nothing, so a
    stanza carrying two `labels` lists would be judged on one of them while
    the file still said both.

    Parameters
    ----------
    path
        The configuration file, normally `.github/dependabot.yml`.

    Returns
    -------
    Mapping
        The parsed document.

    Raises
    ------
    SourceError
        If the file cannot be read, is not valid YAML, is not a mapping, or
        declares one key twice. The cause names the line.
    """
    text = read_text(path)
    try:
        return parse_strict(text, path.name)
    except WorkflowReadError as error:
        raise SourceError(
            path, f"cannot be read as a Dependabot configuration ({error})"
        ) from error


def entries_of(document: Mapping[str, object]) -> list[Mapping[str, object]]:
    """Return the `updates` entries, refusing a document that declares none.

    Parameters
    ----------
    document
        The parsed configuration.

    Returns
    -------
    list of Mapping
        One mapping per entry, in document order.

    Raises
    ------
    DependabotConfigurationError
        If the document declares no `updates` list, or an entry in it is not
        a mapping.
    """
    declared = document.get("updates")
    if not isinstance(declared, list):
        raise DependabotConfigurationError(
            "the configuration must declare an updates list"
        )
    found: list[Mapping[str, object]] = []
    for position, entry in enumerate(declared):
        if not isinstance(entry, dict):
            raise DependabotConfigurationError(
                f"updates[{position}] is not a mapping"
            )
        found.append(entry)
    return found


def declared_directories(entry: Mapping[str, object]) -> frozenset[str]:
    """Return every directory one entry covers, as absolute paths.

    Dependabot reads `directory` or `directories`, each relative to the
    repository root with or without a leading slash. A glob in `directories`
    is returned as written: this reading compares what the file declares, and
    expanding a glob against the tree is the rule's question.

    Parameters
    ----------
    entry
        One `updates` entry.

    Returns
    -------
    frozenset of str
        Each declared directory with a leading slash and without a trailing
        one, so `/tests/e2e` and `tests/e2e/` compare equal.
    """
    values: list[str] = []
    single = entry.get("directory")
    if isinstance(single, str):
        values.append(single)
    listed = entry.get("directories")
    if isinstance(listed, list):
        values.extend(value for value in listed if isinstance(value, str))
    return frozenset(absolute(value) for value in values)


def entry_for(
    document: Mapping[str, object], ecosystem: str, directory: str
) -> Mapping[str, object]:
    """Return the one entry for an ecosystem over a directory.

    Parameters
    ----------
    document
        The parsed configuration.
    ecosystem
        The `package-ecosystem` value to find.
    directory
        A directory the entry must cover.

    Returns
    -------
    Mapping
        The single matching entry.

    Raises
    ------
    DependabotConfigurationError
        If no entry, or more than one, matches. A second entry over one
        directory is itself a fault `duplicate_faults` reports; here it only
        makes the lookup ambiguous.
    """
    wanted = absolute(directory)
    matches = [
        entry
        for entry in entries_of(document)
        if entry.get("package-ecosystem") == ecosystem
        and wanted in declared_directories(entry)
    ]
    if len(matches) != 1:
        raise DependabotConfigurationError(
            f"expected one {ecosystem} entry covering {directory!r}, "
            f"found {len(matches)}"
        )
    return matches[0]


def group_names(entry: Mapping[str, object]) -> list[str]:
    """Return an entry's group names, in the order Dependabot consults them.

    Parameters
    ----------
    entry
        One `updates` entry.

    Returns
    -------
    list of str
        The declared group names. Empty when the entry declares no groups.
    """
    groups = entry.get("groups")
    return list(groups) if isinstance(groups, dict) else []


def absolute(directory: str) -> str:
    """Return a declared directory in the one spelling these readings use."""
    return "/" + directory.removeprefix("./").strip("/")


def label(entry: Mapping[str, object]) -> str:
    """Name one entry in a fault message."""
    ecosystem = entry.get("package-ecosystem", "no ecosystem")
    return f"{ecosystem} entry over {sorted(declared_directories(entry))}"

"""What a Dependabot configuration covers, as fault lists.

`_dependabot_shape.py` asks what one entry's shape must be: the catch-all
last, narrower groups before it, a daily cadence, a strategy cargo allows.
These readings ask a different question, about the configuration as a whole
and about the tree it points at. Which ecosystems have an entry of their own,
whether two entries contend for one directory, and whether the directories
that are declared actually hold the manifests their ecosystem reads.

Split from `_dependabot_shape.py` so neither module outgrows the 400-line
limit; the two questions share nothing but the entry they are handed.

`dependabot_reader_test.py` and `dependabot_coverage_test.py` drive these with
constructed documents, and `dependabot_estate_test.py` applies them to the
estate's own configuration.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import typing as typ

from _sources import is_directory, is_regular_file
from dependabot_config import declared_directories
from dependabot_config import label as _label

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Mapping
    from pathlib import Path

#: The files a listed directory has to hold for its ecosystem to find work
#: there. An ecosystem absent from the table is left unjudged: canon lets a
#: repository choose its ecosystems, and a guess at a manifest name would
#: report a fault about the guess rather than about the configuration.
#:
#: `github-actions` maps to no file because the manifests it reads are the
#: workflow files themselves, which live under one directory rather than one
#: per entry; that entry's coverage is the rule's question, not this one's.
ECOSYSTEM_MANIFESTS: typ.Final[dict[str, tuple[str, ...]]] = {
    "cargo": ("Cargo.toml",),
    "pip": ("pyproject.toml",),
    "bun": ("package.json", "bun.lock"),
    "github-actions": (),
}


def duplicate_faults(entries: list[Mapping[str, object]]) -> list[str]:
    """Return every pair of entries of one ecosystem sharing a directory.

    Canon does not judge this, and nothing else would notice it: two entries
    over one path open two update streams for the same dependencies, so each
    bump arrives twice and the second is closed by hand. The comparison is an
    overlap rather than an equality because a cargo entry's `directories` is a
    set, and two sets can share one member without being equal.

    Parameters
    ----------
    entries
        The configuration's `updates` entries, in document order.

    Returns
    -------
    list of str
        One message per overlapping pair.
    """
    faults: list[str] = []
    for position, first in enumerate(entries):
        for second in entries[position + 1 :]:
            fault = _shared_directory_fault(first, second)
            if fault is not None:
                faults.append(fault)
    return faults


def _shared_directory_fault(
    first: Mapping[str, object], second: Mapping[str, object]
) -> str | None:
    """Return why two entries of one ecosystem cover a shared directory.

    Entries of different ecosystems never contend for a dependency, so they
    return `None`.
    """
    if first.get("package-ecosystem") != second.get("package-ecosystem"):
        return None
    shared = declared_directories(first) & declared_directories(second)
    if not shared:
        return None
    return f"{_label(first)} and {_label(second)} both cover {sorted(shared)}"


def directory_faults(entry: Mapping[str, object], root: Path) -> list[str]:
    """Return every listed directory that is missing or holds no manifest.

    The reading that makes the declared paths mean something. A path a rename
    left behind, or one typed for an ecosystem whose manifest lives
    elsewhere, leaves Dependabot watching an empty directory and reporting
    nothing, which reads exactly like a repository with no updates at all.

    Parameters
    ----------
    entry
        One `updates` entry.
    root
        The repository root the declared paths are relative to.

    Returns
    -------
    list of str
        One message per directory that does not exist, or that exists but
        lacks at least one of the manifests `ECOSYSTEM_MANIFESTS` names for
        the entry's ecosystem. A `bun` directory holding `package.json` and
        no `bun.lock` is reported for the lockfile alone.

    Raises
    ------
    SourceError
        If a declared path cannot be classified. A directory that cannot be
        `stat`ed is not the same answer as one that is absent, and reporting
        it as a configuration fault would send a reader after a rename that
        never happened.
    """
    manifests = ECOSYSTEM_MANIFESTS.get(str(entry.get("package-ecosystem")))
    if not manifests:
        return []
    faults: list[str] = []
    for directory in sorted(declared_directories(entry)):
        fault = _directory_fault(entry, directory, manifests, root)
        if fault is not None:
            faults.append(fault)
    return faults


def _is_glob(directory: str) -> bool:
    """Return whether a declared directory is a pattern rather than a path."""
    return any(character in directory for character in "*?[")


def _directory_fault(
    entry: Mapping[str, object],
    directory: str,
    manifests: tuple[str, ...],
    root: Path,
) -> str | None:
    """Return why one declared directory cannot supply the entry's manifests.

    A glob is not a path this reading can resolve; canon expands one against
    the tree, so it is skipped rather than reported as a missing directory.
    """
    if _is_glob(directory):
        return None
    base = root / directory.lstrip("/")
    if not is_directory(base):
        return f"{_label(entry)} lists {directory!r}, which is not a directory"
    missing = [name for name in manifests if not is_regular_file(base / name)]
    if not missing:
        return None
    return (
        f"{_label(entry)} lists {directory!r}, which does not hold "
        f"{', '.join(missing)}"
    )


def uncovered_directories(
    entries: list[Mapping[str, object]], ecosystems: frozenset[str]
) -> list[str]:
    """Return every ecosystem in `ecosystems` with no entry of its own.

    Canon leaves ecosystem coverage to its governance rules, and this
    repository has chosen the set below. Without this reading, deleting an
    entry whole -- the pip one, say -- would pass every group and cadence
    fault, because there would be no entry left to ask.

    Parameters
    ----------
    entries
        The configuration's `updates` entries.
    ecosystems
        The `package-ecosystem` values this repository requires.

    Returns
    -------
    list of str
        One message per required ecosystem no entry declares.
    """
    declared = {str(entry.get("package-ecosystem")) for entry in entries}
    return [
        f"no {ecosystem} update entry is declared; the repository requires "
        f"one, so its dependencies would go unmanaged"
        for ecosystem in sorted(ecosystems)
        if ecosystem not in declared
    ]


__all__ = [
    "ECOSYSTEM_MANIFESTS",
    "directory_faults",
    "duplicate_faults",
    "uncovered_directories",
]

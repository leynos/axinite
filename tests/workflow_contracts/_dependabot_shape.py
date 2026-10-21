"""The shapes canon requires of a Dependabot entry, as fault lists.

Each reading answers one question about one parsed entry and returns every
way the entry breaks it, rather than raising on the first. A contributor who
drops the catch-all or widens a narrow group should see the whole fault list
in one run.

`dependabot_config.py` holds the boundary and the lookups; this module is pure
in the mapping it is handed, so `dependabot_reader_test.py` can state each
canon case against a document it wrote, and `dependabot_estate_test.py` can
apply the same readings to the estate's own configuration.
"""

from __future__ import annotations

import re
import typing as typ

from dependabot_config import absolute, declared_directories
from dependabot_config import label as _label

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Mapping
    from pathlib import Path

#: A pattern that matches every dependency. A group carrying one before the
#: catch-all is a second catch-all, and Dependabot assigns a dependency to the
#: first group that matches, so it would take the majors the catch-all leaves
#: ungrouped.
_WILDCARD_RE: typ.Final[re.Pattern[str]] = re.compile(r"^\*+$")

#: The keys a catch-all group may carry. `applies-to: version-updates` is the
#: default spelled out and is allowed; every other value changes what the
#: group takes. `security-updates` leaves version updates ungrouped, and
#: `exclude-patterns`, `dependency-type` and `group-by` each carve a class of
#: dependency out of the group, which then arrives one pull request apiece.
CATCH_ALL_KEYS: typ.Final[frozenset[str]] = frozenset(
    {"patterns", "update-types", "applies-to"}
)

#: The update types the catch-all groups, and the only ones it may.
CATCH_ALL_UPDATE_TYPES: typ.Final[frozenset[str]] = frozenset(
    {"minor", "patch"}
)

#: The versioning strategies canon allows a `cargo` entry. `auto` lets
#: Dependabot choose; `lockfile-only` updates the lock graph alone.
ALLOWED_CARGO_STRATEGIES: typ.Final[frozenset[str]] = frozenset(
    {"auto", "lockfile-only"}
)

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


def group_faults(entry: Mapping[str, object]) -> list[str]:
    """Return every way an entry's groups break the catch-all shape.

    The last group must be the catch-all and every group before it must be
    narrower, because Dependabot assigns a dependency to the first group that
    matches. A major matching no earlier group therefore arrives as its own
    pull request, where its breaking changes get their own review; one that
    matches a lockstep family moves with that family.

    Parameters
    ----------
    entry
        One `updates` entry.

    Returns
    -------
    list of str
        One message per fault. Empty when the entry has the shape canon
        requires.
    """
    groups = entry.get("groups")
    label = _label(entry)
    if not isinstance(groups, dict) or not groups:
        return [
            f"{label} has no groups; it needs a catch-all group with "
            f"patterns ['*'] and update-types [minor, patch]"
        ]
    names = list(groups)
    faults = _catch_all_faults(label, names[-1], groups[names[-1]])
    for name in names[:-1]:
        faults.extend(_width_faults(label, name, groups[name]))
    return faults


def cadence_faults(entry: Mapping[str, object]) -> list[str]:
    """Return why an entry does not run daily.

    Parameters
    ----------
    entry
        One `updates` entry.

    Returns
    -------
    list of str
        One message when `schedule.interval` is anything but `daily`,
        including when it is missing. Empty otherwise.
    """
    schedule = entry.get("schedule")
    interval = schedule.get("interval") if isinstance(schedule, dict) else None
    if interval == "daily":
        return []
    return [f"{_label(entry)} schedule.interval is {interval!r}, not daily"]


def cargo_strategy_faults(entry: Mapping[str, object]) -> list[str]:
    """Return why a cargo entry's versioning strategy canon refuses.

    Parameters
    ----------
    entry
        One `updates` entry.

    Returns
    -------
    list of str
        One message when the entry is a cargo one and sets a strategy other
        than `auto` or `lockfile-only`. Empty for every other ecosystem,
        which canon does not judge on this key.
    """
    if entry.get("package-ecosystem") != "cargo":
        return []
    strategy = entry.get("versioning-strategy")
    if strategy is None:
        return []
    if isinstance(strategy, str) and strategy in ALLOWED_CARGO_STRATEGIES:
        return []
    return [
        f"{_label(entry)} versioning-strategy is {strategy!r}; cargo allows "
        f"only auto or lockfile-only"
    ]


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
            if first.get("package-ecosystem") != second.get(
                "package-ecosystem"
            ):
                continue
            shared = declared_directories(first) & declared_directories(second)
            if shared:
                faults.append(
                    f"{_label(first)} and {_label(second)} both cover "
                    f"{sorted(shared)}"
                )
    return faults


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
        holds none of the manifests `ECOSYSTEM_MANIFESTS` names for the
        entry's ecosystem.
    """
    manifests = ECOSYSTEM_MANIFESTS.get(str(entry.get("package-ecosystem")))
    if not manifests:
        return []
    faults: list[str] = []
    for directory in sorted(declared_directories(entry)):
        # A glob is not a path this reading can resolve. Canon expands one
        # against the tree; resolving it here would report the pattern itself
        # as a missing directory.
        if any(character in directory for character in "*?["):
            continue
        base = root / directory.lstrip("/")
        if not base.is_dir():
            faults.append(
                f"{_label(entry)} lists {directory!r}, which is not a "
                f"directory"
            )
            continue
        missing = [name for name in manifests if not (base / name).is_file()]
        if missing:
            faults.append(
                f"{_label(entry)} lists {directory!r}, which holds no "
                f"{', '.join(missing)}"
            )
    return faults


def _catch_all_faults(label: str, name: str, group: object) -> list[str]:
    """Return why one group is not the catch-all the entry must end with."""
    if not isinstance(group, dict):
        return [f"{label} last group {name!r} is not a mapping"]
    faults: list[str] = []
    unexpected = sorted(set(group) - CATCH_ALL_KEYS)
    if unexpected:
        faults.append(
            f"{label} last group {name!r} carries {unexpected}; the catch-all "
            f"takes only patterns ['*'] and update-types [minor, patch]"
        )
    if group.get("applies-to", "version-updates") != "version-updates":
        faults.append(
            f"{label} last group {name!r} applies to "
            f"{group.get('applies-to')!r}; only version-updates is the "
            f"catch-all"
        )
    if group.get("patterns") != ["*"]:
        faults.append(
            f"{label} last group {name!r} has patterns "
            f"{group.get('patterns')!r}, not ['*']"
        )
    declared = group.get("update-types")
    if (
        not isinstance(declared, list)
        or len(declared) != 2
        or not all(isinstance(kind, str) for kind in declared)
        or frozenset(declared) != CATCH_ALL_UPDATE_TYPES
    ):
        faults.append(
            f"{label} last group {name!r} has update-types {declared!r}, "
            f"not [minor, patch]"
        )
    return faults


def _width_faults(label: str, name: str, group: object) -> list[str]:
    """Return why a group before the catch-all is not narrower than it."""
    if not isinstance(group, dict):
        return [f"{label} group {name!r} is not a mapping"]
    patterns = group.get("patterns")
    if not isinstance(patterns, list) or not patterns:
        return [
            f"{label} group {name!r} has no patterns; a group before the "
            f"catch-all must name what it takes"
        ]
    if not all(isinstance(pattern, str) for pattern in patterns):
        return [f"{label} group {name!r} has a pattern that is not text"]
    if any(_WILDCARD_RE.match(pattern) for pattern in patterns):
        return [
            f"{label} group {name!r} matches every dependency; a group before "
            f"the catch-all must be narrower than it"
        ]
    return []


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
        f"no {ecosystem} update entry is declared"
        for ecosystem in sorted(ecosystems)
        if ecosystem not in declared
    ]


__all__ = [
    "ALLOWED_CARGO_STRATEGIES",
    "CATCH_ALL_KEYS",
    "CATCH_ALL_UPDATE_TYPES",
    "ECOSYSTEM_MANIFESTS",
    "absolute",
    "cadence_faults",
    "cargo_strategy_faults",
    "directory_faults",
    "duplicate_faults",
    "group_faults",
    "uncovered_directories",
]

"""The shapes canon requires of a Dependabot entry, as fault lists.

Each reading answers one question about one parsed entry and returns every
way the entry breaks it, rather than raising on the first. A contributor who
drops the catch-all or widens a narrow group should see the whole fault list
in one run.

These are the questions about one entry's own shape: its groups, its cadence,
and the versioning strategy its ecosystem allows. What the configuration
covers -- which ecosystems have an entry, whether two contend for one
directory, and whether the declared directories hold their manifests -- is a
question about the configuration and the tree, and lives in
`_dependabot_coverage.py`.

`dependabot_config.py` holds the boundary and the lookups; this module is pure
in the mapping it is handed, so `dependabot_reader_test.py` can state each
canon case against a document it wrote, and `dependabot_estate_test.py` can
apply the same readings to the estate's own configuration.
"""

from __future__ import annotations

import re
import typing as typ

from dependabot_config import label as _label

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Mapping

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

#: What the catch-all's `update-types` has to look like to name exactly those
#: types once each: a list, with the length, element type and members all
#: stated independently of the values it holds. Written as a table so the
#: reading below is a lookup rather than a chain of conditions.
CATCH_ALL_UPDATE_TYPES_SHAPE: typ.Final[tuple[object, ...]] = (
    list,
    2,
    str,
    CATCH_ALL_UPDATE_TYPES,
)

#: The versioning strategies canon allows a `cargo` entry. `auto` lets
#: Dependabot choose; `lockfile-only` updates the lock graph alone.
ALLOWED_CARGO_STRATEGIES: typ.Final[frozenset[str]] = frozenset(
    {"auto", "lockfile-only"}
)


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


def _update_types_shape(declared: object) -> tuple[bool, bool, bool, bool]:
    """Return which parts of the four-part `update-types` shape hold."""
    kind, count, element, members = CATCH_ALL_UPDATE_TYPES_SHAPE
    is_list = isinstance(declared, list)
    right_length = is_list and len(declared) == count
    all_text = right_length and all(
        isinstance(entry, element) for entry in declared
    )
    right_members = all_text and frozenset(declared) == members
    return is_list, right_length, all_text, right_members


def _catch_all_shape_faults(
    label: str, name: str, group: Mapping[str, object]
) -> list[str]:
    """Return why the group's keys and values are not the catch-all's."""
    unexpected = sorted(set(group) - CATCH_ALL_KEYS)
    applies_to = group.get("applies-to", "version-updates")
    checks: tuple[tuple[bool, str], ...] = (
        (
            not unexpected,
            f"carries {unexpected}; the catch-all takes only patterns ['*'] "
            f"and update-types [minor, patch]",
        ),
        (
            applies_to == "version-updates",
            f"applies to {applies_to!r}; only version-updates is the "
            f"catch-all",
        ),
        (
            group.get("patterns") == ["*"],
            f"has patterns {group.get('patterns')!r}, not ['*']",
        ),
    )
    faults = [
        f"{label} last group {name!r} {message}"
        for holds, message in checks
        if not holds
    ]
    update_types_fault = _update_types_fault(label, name, group)
    if update_types_fault is not None:
        faults.append(update_types_fault)
    return faults


def _update_types_fault(
    label: str, name: str, group: Mapping[str, object]
) -> str | None:
    """Return why the catch-all's `update-types` is not `[minor, patch]`."""
    declared = group.get("update-types")
    _is_list, _length, _text, right_members = _update_types_shape(declared)
    if right_members:
        return None
    return (
        f"{label} last group {name!r} has update-types {declared!r}, "
        f"not [minor, patch]"
    )


def _catch_all_faults(label: str, name: str, group: object) -> list[str]:
    """Return why one group is not the catch-all the entry must end with."""
    if not isinstance(group, dict):
        return [f"{label} last group {name!r} is not a mapping"]
    return _catch_all_shape_faults(label, name, group)


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


__all__ = [
    "ALLOWED_CARGO_STRATEGIES",
    "CATCH_ALL_KEYS",
    "CATCH_ALL_UPDATE_TYPES",
    "cadence_faults",
    "cargo_strategy_faults",
    "group_faults",
]

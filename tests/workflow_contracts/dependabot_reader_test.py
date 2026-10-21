"""Drive the Dependabot readings with documents built for each fault.

`dependabot_estate_test.py` asserts the rules over the repository's own
configuration, which is correct, so it would pass with any reader's
protection deleted. Each case here states one entry that breaks a rule in one
way and drives the same reading the estate contract calls, so a reader that
stopped reporting a fault fails a named case rather than gliding along.

These are the questions about one entry's own shape: its groups, its cadence,
and its strategy. What the configuration covers -- the coverage, duplicate and
manifest readings -- is driven by `dependabot_coverage_test.py`.

The approved entry below is the positive half: it has the shape canon
requires, and every mutation is derived from it, so a reader reporting a
fault for everything cannot pass both.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import typing as typ

import pytest
from _dependabot_shape import (
    cadence_faults,
    cargo_strategy_faults,
    group_faults,
)
from _sources import SourceError
from dependabot_config import DependabotConfigurationError, entries_of
from dependabot_config import entry_for, read_dependabot

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Mapping
    from pathlib import Path

#: One approved entry. Every mutation below is this entry with one thing
#: changed, so a case that should be reported differs from a case that should
#: not by the change alone.
APPROVED_ENTRY: dict[str, object] = {
    "package-ecosystem": "cargo",
    "directory": "/",
    "schedule": {"interval": "daily"},
    "groups": {
        "lockstep": {"patterns": ["rstest-bdd*"]},
        "minor-and-patch": {
            "patterns": ["*"],
            "update-types": ["minor", "patch"],
        },
    },
}


def _catch_all(**extra: object) -> dict[str, object]:
    """Return a group with the catch-all shape, plus any keys a case adds."""
    group: dict[str, object] = {
        "patterns": ["*"],
        "update-types": ["minor", "patch"],
    }
    group.update(extra)
    return group


@pytest.mark.parametrize(
    "groups",
    [
        pytest.param(None, id="no-groups"),
        pytest.param({}, id="empty-groups"),
        pytest.param(
            {
                "minor-and-patch": _catch_all(),
                "lockstep": {"patterns": ["rstest-bdd*"]},
            },
            id="catch-all-not-last",
        ),
        pytest.param(
            {"minor-and-patch": _catch_all(**{"exclude-patterns": ["x*"]})},
            id="catch-all-excludes-a-family",
        ),
        pytest.param(
            {
                "minor-and-patch": _catch_all(
                    **{"applies-to": "security-updates"}
                )
            },
            id="catch-all-applies-to-security-updates",
        ),
        pytest.param(
            {
                "minor-and-patch": {
                    "patterns": ["tokio*"],
                    "update-types": ["minor", "patch"],
                }
            },
            id="catch-all-narrowed",
        ),
        pytest.param(
            {
                "minor-and-patch": {
                    "patterns": ["*"],
                    "update-types": ["minor"],
                }
            },
            id="catch-all-drops-patch",
        ),
        pytest.param(
            {
                "minor-and-patch": {
                    "patterns": ["*"],
                    "update-types": ["minor", "patch", "major"],
                }
            },
            id="catch-all-takes-majors",
        ),
        pytest.param(
            {
                "everything": {"patterns": ["*"]},
                "minor-and-patch": _catch_all(),
            },
            id="bare-wildcard-before-the-catch-all",
        ),
        pytest.param(
            {
                "typing": {"update-types": ["minor", "patch"]},
                "minor-and-patch": _catch_all(),
            },
            id="group-with-no-patterns",
        ),
    ],
)
def test_a_group_shape_canon_refuses_is_reported(
    groups: dict[str, object] | None,
) -> None:
    """Each mutation of the approved group shape must be reported."""
    entry = dict(APPROVED_ENTRY)
    if groups is None:
        entry.pop("groups")
    else:
        entry["groups"] = groups
    assert group_faults(entry), (
        f"{groups!r} should be reported as a shape canon refuses"
    )


def test_the_approved_group_shape_is_accepted() -> None:
    """The positive case, without which the mutations above prove nothing."""
    assert group_faults(APPROVED_ENTRY) == [], (
        "the approved entry must pass the shape reading; a reader that "
        "reported a fault for everything would satisfy every mutation above"
    )


def _without_schedule() -> dict[str, object]:
    """Return the approved entry with its schedule dropped."""
    entry = {key: value for key, value in APPROVED_ENTRY.items()}
    entry.pop("schedule")
    return entry


@pytest.mark.parametrize(
    "entry",
    [
        pytest.param(
            dict(APPROVED_ENTRY, schedule={"interval": "weekly"}),
            id="weekly",
        ),
        pytest.param(_without_schedule(), id="no-schedule"),
    ],
)
def test_an_entry_that_does_not_run_daily_is_reported(
    entry: dict[str, object],
) -> None:
    """Cadence is read from the entry, never assumed from the file.

    A weekly interval leaves a security fix unreviewed for a week, and an
    entry with no schedule at all is not the daily one a missing key might
    be read as. Both are the same fault: the cadence canon requires is not
    declared.
    """
    assert cadence_faults(entry), (
        f"{entry.get('schedule', 'no schedule')!r} must be reported; canon "
        f"requires schedule.interval to be daily"
    )


@pytest.mark.parametrize(
    "strategy",
    [
        pytest.param("increase", id="increase"),
        pytest.param("widen", id="widen"),
        pytest.param("increase-if-necessary", id="increase-if-necessary"),
    ],
)
def test_an_unlisted_cargo_versioning_strategy_is_reported(
    strategy: str,
) -> None:
    """Canon allows only auto and lockfile-only for cargo."""
    entry = dict(APPROVED_ENTRY, **{"versioning-strategy": strategy})
    assert cargo_strategy_faults(entry), (
        f"the cargo strategy {strategy!r} must be reported; canon allows "
        f"only auto or lockfile-only"
    )


@pytest.mark.parametrize(
    "strategy",
    [
        pytest.param("auto", id="auto"),
        pytest.param("lockfile-only", id="lockfile-only"),
        pytest.param(None, id="unset"),
    ],
)
def test_an_allowed_cargo_versioning_strategy_is_accepted(
    strategy: str | None,
) -> None:
    """The allowance canon states, including the key left out."""
    entry = dict(APPROVED_ENTRY, **{"versioning-strategy": strategy})
    assert cargo_strategy_faults(entry) == [], (
        f"the cargo strategy {strategy!r} is one canon allows, so it must "
        f"not be reported"
    )


def test_another_ecosystem_is_not_judged_on_its_strategy() -> None:
    """Only cargo's strategies are canon's question."""
    entry = dict(
        APPROVED_ENTRY,
        **{
            "package-ecosystem": "pip",
            "versioning-strategy": "increase",
        },
    )
    assert cargo_strategy_faults(entry) == [], (
        "a pip entry's versioning-strategy is not canon's question, so the "
        "strategy cargo refuses must not be reported for it"
    )


def test_a_repeated_key_is_reported_as_a_source_error(tmp_path: Path) -> None:
    """PyYAML would keep the last `updates` and say nothing about the first.

    The strict loader is what turns that into a failure naming the file, and
    this case states it against a file written for the purpose rather than
    against the estate, whose configuration is well formed.
    """
    path = tmp_path / "dependabot.yml"
    path.write_text("version: 2\nupdates: []\nupdates: []\n", encoding="utf-8")
    with pytest.raises(SourceError, match="dependabot.yml"):
        read_dependabot(path)


def test_a_document_that_is_not_a_mapping_is_reported(tmp_path: Path) -> None:
    """A file holding a bare list is valid YAML and not a configuration."""
    path = tmp_path / "dependabot.yml"
    path.write_text("- version: 2\n", encoding="utf-8")
    with pytest.raises(SourceError, match="dependabot.yml"):
        read_dependabot(path)


def test_a_root_key_that_is_not_a_string_is_reported(tmp_path: Path) -> None:
    """YAML keys need not be text, and every Dependabot key is.

    `1:` parses as an integer key, so a cast to the `Mapping[str, object]`
    every reading above assumes would promise something the file does not
    keep. The boundary answers for it here rather than letting a reading
    compare an integer against a name and find nothing.
    """
    path = tmp_path / "dependabot.yml"
    path.write_text("1: version\n", encoding="utf-8")
    with pytest.raises(SourceError, match="root key"):
        read_dependabot(path)


def test_a_document_with_no_updates_list_is_reported() -> None:
    """An empty configuration is an error here, though canon calls it clean.

    Canon judges the shape of a configuration, not its presence, so a
    repository with none is compliant. This reading has already been handed
    the file, and a file with no entries satisfies every per-entry question
    vacuously; the missing-entry count is what `uncovered_directories`
    reports instead.
    """
    with pytest.raises(DependabotConfigurationError, match="updates"):
        entries_of({"version": 2})


def test_an_ambiguous_lookup_is_reported() -> None:
    """A lookup answers one entry or fails, never the first of several."""
    document = {"updates": [dict(APPROVED_ENTRY), dict(APPROVED_ENTRY)]}
    with pytest.raises(DependabotConfigurationError, match="found 2"):
        entry_for(document, "cargo", "/")

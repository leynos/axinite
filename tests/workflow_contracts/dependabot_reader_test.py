"""Drive the Dependabot readings with documents built for each fault.

`dependabot_estate_test.py` asserts the rules over the repository's own
configuration, which is correct, so it would pass with any reader's
protection deleted. Each case here states one entry that breaks a rule in one
way and drives the same reading the estate contract calls, so a reader that
stopped reporting a fault fails a named case rather than gliding along.

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
    directory_faults,
    duplicate_faults,
    group_faults,
)
from _sources import SourceError
from dependabot_config import DependabotConfigurationError, entries_of
from dependabot_config import entry_for
from dependabot_config import read_dependabot

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


def _entry_listing(directories: list[str]) -> dict[str, object]:
    """Return the approved entry with `directories` instead of `directory`.

    Dependabot reads one or the other, and the approved entry carries
    `directory`, so a case that wants `directories` has to drop it. Keeping
    both would leave the reading free to see a root the case never meant to
    declare.
    """
    entry = {key: value for key, value in APPROVED_ENTRY.items()}
    entry.pop("directory")
    entry["directories"] = directories
    return entry


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
            {"minor-and-patch": _catch_all(**{"applies-to": "security-updates"})},
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
            {"minor-and-patch": {"patterns": ["*"], "update-types": ["minor"]}},
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
            {"everything": {"patterns": ["*"]}, "minor-and-patch": _catch_all()},
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
    assert group_faults(APPROVED_ENTRY) == []


def test_a_non_daily_entry_is_reported() -> None:
    """Cadence is read from the entry, not assumed from the file."""
    entry = dict(APPROVED_ENTRY, schedule={"interval": "weekly"})
    assert cadence_faults(entry)


def test_an_entry_without_a_schedule_is_reported() -> None:
    """An entry with no schedule is not silently daily."""
    entry = {key: value for key, value in APPROVED_ENTRY.items()}
    entry.pop("schedule")
    assert cadence_faults(entry)


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
    assert cargo_strategy_faults(entry)


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
    assert cargo_strategy_faults(entry) == []


def test_another_ecosystem_is_not_judged_on_its_strategy() -> None:
    """Only cargo's strategies are canon's question."""
    entry = dict(
        APPROVED_ENTRY,
        **{
            "package-ecosystem": "pip",
            "versioning-strategy": "increase",
        },
    )
    assert cargo_strategy_faults(entry) == []


def test_a_missing_directory_is_reported(tmp_path: Path) -> None:
    """A listed path that does not exist is a fault, not a quiet no-op."""
    entry = dict(APPROVED_ENTRY, directory="/absent")
    faults = directory_faults(entry, tmp_path)
    assert faults, "a listed directory that does not exist must be reported"


def test_a_directory_without_its_manifest_is_reported(tmp_path: Path) -> None:
    """A directory that exists but holds no manifest is the same no-op."""
    (tmp_path / "empty").mkdir()
    entry = dict(APPROVED_ENTRY, directory="/empty")
    faults = directory_faults(entry, tmp_path)
    assert faults, "a listed directory holding no Cargo.toml must be reported"


def test_a_directory_with_its_manifest_is_accepted(tmp_path: Path) -> None:
    """The positive case for the reading above."""
    (tmp_path / "crate").mkdir()
    (tmp_path / "crate" / "Cargo.toml").write_text("", encoding="utf-8")
    entry = dict(APPROVED_ENTRY, directory="/crate")
    assert directory_faults(entry, tmp_path) == []


def test_a_glob_is_left_to_the_rule(tmp_path: Path) -> None:
    """A pattern that names no real path is not reported as a missing one."""
    entry = dict(APPROVED_ENTRY, directory="/.github/actions/*")
    assert directory_faults(entry, tmp_path) == []


def test_two_entries_over_one_directory_are_reported() -> None:
    """Two update streams for one dependency open two pull requests a bump."""
    entries = [
        dict(APPROVED_ENTRY),
        dict(APPROVED_ENTRY, labels=["dependencies"]),
    ]
    faults = duplicate_faults(entries)
    assert faults, "two cargo entries over / must be reported"


def test_entries_over_different_directories_are_accepted() -> None:
    """The positive case: a cargo entry per directory is the normal shape."""
    entries = [
        _entry_listing(["/a"]),
        _entry_listing(["/b"]),
    ]
    assert duplicate_faults(entries) == []


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

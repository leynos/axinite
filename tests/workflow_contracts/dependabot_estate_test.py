"""Contracts holding `.github/dependabot.yml` to canon and to our own policy.

Concordat's DB-005 rule audits this file, but an audit is evidence about one
revision and nothing a later contributor edits has to pass before it merges.
These contracts state the same shape questions over the parsed file, so a
change that drops the catch-all, widens a narrow group, points an entry at a
directory the repository no longer has, or stops covering an ecosystem fails
the repository's own gate.

The readings themselves are in `_dependabot_shape.py`, for the questions
about one entry's own shape, and `_dependabot_coverage.py`, for what the
configuration covers and the tree it points at. The values this estate is held
to are in `dependabot_estate.py`. Because the estate's configuration is
correct, a reader that answered "no faults" unconditionally would pass every
test here exactly as the real one does. `dependabot_reader_test.py` and
`dependabot_coverage_test.py` drive the same readings with controlled
documents, which is what stops that reader passing.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import typing as typ

import pytest
from _dependabot_coverage import (
    directory_faults,
    duplicate_faults,
    uncovered_directories,
)
from _dependabot_shape import (
    cadence_faults,
    cargo_strategy_faults,
    group_faults,
)
from _sources import is_regular_file
from _workflow_policy import REPOSITORY_ROOT
from dependabot_config import (
    declared_directories,
    entries_of,
    entry_for,
    group_names,
    read_dependabot,
)
from dependabot_estate import (
    CONFIG_PATH,
    ENTRY_COOLDOWN,
    ENTRY_POINTS,
    ENTRY_POLICY,
    REQUIRED_ECOSYSTEMS,
)

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Mapping


def _ids() -> list[str]:
    """Name each entry point by its ecosystem and path, for the report."""
    return [
        f"{ecosystem}-{directory.strip('/').replace('/', '-') or 'root'}"
        for ecosystem, directory in ENTRY_POINTS
    ]


@pytest.fixture(scope="session")
def configuration() -> Mapping[str, object]:
    """The estate's configuration, read once and named if it cannot be."""
    return read_dependabot(CONFIG_PATH)


@pytest.fixture(scope="session")
def entries(configuration: Mapping[str, object]) -> list[Mapping[str, object]]:
    """Every declared update entry, in document order."""
    return entries_of(configuration)


@pytest.fixture
def the_entry(
    configuration: Mapping[str, object], entry_point: tuple[str, str]
) -> Mapping[str, object]:
    """The one entry a parametrized case is about."""
    ecosystem, directory = entry_point
    return entry_for(configuration, ecosystem, directory)


def test_every_entry_has_the_canonical_group_shape(
    entries: list[Mapping[str, object]],
) -> None:
    """One catch-all last, narrower groups before it, on every entry.

    Asserted over the whole list rather than one case per entry, because the
    fault messages already name the entry: a broken catch-all on the cargo
    stanza and a second catch-all before it on the bun stanza should both be
    visible in one run.
    """
    faults = [fault for entry in entries for fault in group_faults(entry)]
    assert faults == [], "\n".join(faults)


def test_every_entry_runs_daily(entries: list[Mapping[str, object]]) -> None:
    """A weekly schedule leaves a security fix unreviewed for a week.

    Canon requires the daily interval; the estate's own reason for it is the
    cooldown: seven days of delay on top of seven days of cooldown is a
    fortnight before a patch is even proposed.
    """
    faults = [fault for entry in entries for fault in cadence_faults(entry)]
    assert faults == [], "\n".join(faults)


def test_every_cargo_entry_uses_an_allowed_versioning_strategy(
    entries: list[Mapping[str, object]],
) -> None:
    """Canon refuses a cargo strategy other than auto or lockfile-only."""
    faults = [
        fault for entry in entries for fault in cargo_strategy_faults(entry)
    ]
    assert faults == [], "\n".join(faults)


def test_the_required_ecosystems_are_declared(
    entries: list[Mapping[str, object]],
) -> None:
    """Every ecosystem the repository updates has an entry of its own."""
    faults = uncovered_directories(entries, REQUIRED_ECOSYSTEMS)
    assert faults == [], "\n".join(faults)


def test_no_two_entries_cover_one_directory(
    entries: list[Mapping[str, object]],
) -> None:
    """Two entries over one path open two update streams for one dependency."""
    faults = duplicate_faults(entries)
    assert faults == [], "\n".join(faults)


def test_every_listed_directory_holds_its_manifest(
    entries: list[Mapping[str, object]],
) -> None:
    """A path a rename left behind leaves Dependabot watching nothing.

    The failure is silent: Dependabot reports a repository with no updates
    exactly as it reports an empty directory, so only a reading of the tree
    can tell them apart.
    """
    faults = [
        fault
        for entry in entries
        for fault in directory_faults(entry, REPOSITORY_ROOT)
    ]
    assert faults == [], "\n".join(faults)


def test_the_github_actions_entry_covers_the_workflows(
    configuration: Mapping[str, object],
) -> None:
    """The repository root is where the workflows Dependabot bumps live.

    Canon requires `/` whenever a `github-actions` entry exists, and this
    repository has no `.github/actions` directory for the rule's other half
    to require. Dropping `/` from the entry would leave every pinned action in
    `.github/workflows` unmanaged.
    """
    entry = entry_for(configuration, "github-actions", "/")
    covered = sorted(declared_directories(entry))
    assert "/" in declared_directories(entry), (
        "the github-actions entry must cover the repository root, where the "
        f"workflows it bumps live; it covers {covered}"
    )


@pytest.mark.parametrize("entry_point", ENTRY_POINTS, ids=_ids())
def test_each_entry_declares_its_labels_and_limit(
    the_entry: Mapping[str, object], entry_point: tuple[str, str]
) -> None:
    """Pin each entry's labels and pull-request limit.

    The pip stanza is the one this branch relabelled: the non-canonical
    `python` label sent pip bumps to a label no automation watches, and the
    canonical channel name for the ecosystem is `pip`.
    """
    expected_labels, expected_limit = ENTRY_POLICY[entry_point]
    labels = the_entry.get("labels")
    assert isinstance(labels, list), f"{the_entry} declares no labels list"
    assert set(labels) == expected_labels, (
        f"{entry_point} asks for labels {sorted(map(str, labels))}, "
        f"not {sorted(expected_labels)}"
    )
    assert the_entry.get("open-pull-requests-limit") == expected_limit, (
        f"{entry_point} keeps {the_entry.get('open-pull-requests-limit')} "
        f"pull requests open, not {expected_limit}"
    )


@pytest.mark.parametrize("entry_point", ENTRY_POINTS, ids=_ids())
def test_each_entry_applies_its_cooldown(
    the_entry: Mapping[str, object], entry_point: tuple[str, str]
) -> None:
    """Pin the cooldown each entry applies before it proposes an update."""
    cooldown = the_entry.get("cooldown")
    assert isinstance(cooldown, dict), f"{the_entry} declares no cooldown"
    assert cooldown == ENTRY_COOLDOWN[entry_point], (
        f"{entry_point} applies {cooldown!r}, not "
        f"{ENTRY_COOLDOWN[entry_point]!r}"
    )


def test_the_bun_entry_watches_the_front_end(
    configuration: Mapping[str, object],
) -> None:
    """The front-end workspace is covered by an entry of its own.

    Nothing else reached `web-src` before this branch: the cargo entry lists
    Rust manifests, and the npm-shaped alternatives would read a lockfile the
    workspace does not have. `bun.lock` is committed, so Dependabot reads it
    as a text lock.
    """
    entry = entry_for(configuration, "bun", "/web-src")
    assert is_regular_file(REPOSITORY_ROOT / "web-src" / "bun.lock"), (
        "the bun entry reads web-src/bun.lock, so that lockfile must be "
        "committed; without it the entry watches a directory whose lock "
        "Dependabot cannot find"
    )
    names = group_names(entry)
    assert names, (
        "the bun entry must declare groups, so the catch-all has a place to "
        "be last; it declares none"
    )
    assert names[-1] == "minor-and-patch", (
        f"the bun entry's last group must be the catch-all, so a major "
        f"arrives as its own pull request; its groups are {names}"
    )

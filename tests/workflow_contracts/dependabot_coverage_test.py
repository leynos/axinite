"""Drive the Dependabot coverage readings with documents and trees.

`_dependabot_coverage.py` asks what a configuration *covers*: whether every
required ecosystem has an entry, whether two entries contend for one
directory, and whether the declared directories hold the manifests their
ecosystem reads. `dependabot_estate_test.py` applies those readings to the
repository's own configuration, which is correct, so a reader that answered
"no faults" unconditionally would pass it. Each case here constructs the
fault it is about, so a reading that stopped reporting one fails a named case.

Split from `dependabot_reader_test.py` along the same seam as the readings
themselves: that module drives the per-entry shape questions, this one the
configuration's coverage.

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
from dependabot_config import (
    DependabotConfigurationError,
    declared_directories,
)

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from pathlib import Path

#: One approved entry. Every case below is this entry with one thing changed,
#: so a case that should be reported differs from a case that should not by
#: the change alone.
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


def _entry_listing(directories: list[object]) -> dict[str, object]:
    """Return the approved entry with `directories` instead of `directory`.

    Dependabot reads one or the other, and the approved entry carries
    `directory`, so a case that wants `directories` has to drop it. Keeping
    both would leave the reading free to see a root the case never meant to
    declare. The list holds `object` rather than `str` so a case can declare
    a value that is not a path at all.
    """
    entry = {key: value for key, value in APPROVED_ENTRY.items()}
    entry.pop("directory")
    entry["directories"] = directories
    return entry


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


def test_a_missing_one_of_two_manifests_is_reported_by_name(
    tmp_path: Path,
) -> None:
    """One manifest present is not enough where two are named.

    The bun entry names `package.json` and `bun.lock`, and a directory
    holding only the first is exactly the state this branch found `web-src`
    in: the package list is there, and the lockfile Dependabot reads is not.
    A reading that asked whether the directory held any manifest would call
    that covered.
    """
    (tmp_path / "front").mkdir()
    (tmp_path / "front" / "package.json").write_text("{}", encoding="utf-8")
    entry = dict(
        APPROVED_ENTRY,
        **{"package-ecosystem": "bun", "directory": "/front"},
    )
    faults = directory_faults(entry, tmp_path)
    assert len(faults) == 1, (
        f"a bun directory holding package.json and no bun.lock is one fault, "
        f"but the reading returned {faults}"
    )
    assert "bun.lock" in faults[0], (
        f"the fault must name the missing lockfile; it said {faults[0]!r}"
    )
    assert "package.json" not in faults[0], (
        f"the present manifest must not be named as missing; it said "
        f"{faults[0]!r}"
    )


def test_a_directory_with_its_manifest_is_accepted(tmp_path: Path) -> None:
    """The positive case for the reading above."""
    (tmp_path / "crate").mkdir()
    (tmp_path / "crate" / "Cargo.toml").write_text("", encoding="utf-8")
    entry = dict(APPROVED_ENTRY, directory="/crate")
    assert directory_faults(entry, tmp_path) == [], (
        "a listed directory holding its Cargo.toml must not be reported"
    )


def test_a_glob_is_left_to_the_rule(tmp_path: Path) -> None:
    """A pattern that names no real path is not reported as a missing one."""
    entry = dict(APPROVED_ENTRY, directory="/.github/actions/*")
    assert directory_faults(entry, tmp_path) == [], (
        "a glob is canon's to expand, so it must not be reported as a "
        "missing directory"
    )


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
    assert duplicate_faults(entries) == [], (
        "two cargo entries over different directories are not a duplicate"
    )


@pytest.mark.parametrize(
    "entry",
    [
        pytest.param(
            dict(APPROVED_ENTRY, directory=42), id="single-directory"
        ),
        pytest.param(
            _entry_listing(["/", 42]), id="list-holding-a-number"
        ),
        pytest.param(
            _entry_listing(["/", None]), id="list-holding-a-null"
        ),
    ],
)
def test_a_declared_directory_that_is_not_text_is_reported(
    entry: dict[str, object],
) -> None:
    """A malformed path is refused rather than left out of the set.

    Filtering a non-string value would make `directories: ["/", 42]` read as
    `["/"]`, so the entry would pass every coverage and manifest reading as
    though it declared one directory fewer -- and the directory it meant to
    declare would be watched by nothing.
    """
    with pytest.raises(DependabotConfigurationError, match="not a directory"):
        declared_directories(entry)


@pytest.mark.parametrize(
    "spelling",
    [
        pytest.param("/tests/e2e", id="leading-slash"),
        pytest.param("tests/e2e/", id="trailing-slash"),
        pytest.param("./tests/e2e", id="dot-prefix"),
    ],
)
def test_a_declared_directory_is_compared_in_one_spelling(
    spelling: str,
) -> None:
    """The three spellings Dependabot accepts name the same directory.

    Canon's rule compares what the file declares, and the file may spell one
    path any of these ways. A reading that compared the spellings would see
    two entries over one directory where the configuration declares one, and
    would report a duplicate that does not exist.
    """
    entry = dict(APPROVED_ENTRY, directory=spelling)
    assert declared_directories(entry) == frozenset({"/tests/e2e"}), (
        f"the spelling {spelling!r} must read as /tests/e2e, but the reading "
        f"returned {sorted(declared_directories(entry))}"
    )


def test_an_ecosystem_no_entry_covers_is_reported() -> None:
    """Deleting an entry whole is what the coverage reading is for.

    A configuration with no pip entry satisfies every per-entry question
    vacuously, because there is no entry left to ask. Without this reading
    the estate could stop updating Python dependencies and the suite would
    stay green.
    """
    entries = [
        dict(APPROVED_ENTRY),
        dict(APPROVED_ENTRY, **{"package-ecosystem": "bun"}),
    ]
    faults = uncovered_directories(entries, frozenset({"cargo", "pip", "bun"}))
    assert len(faults) == 1, (
        f"only pip is uncovered, so the reading must report one fault; it "
        f"returned {faults}"
    )
    assert "pip" in faults[0], (
        f"the fault must name the uncovered ecosystem; it said {faults[0]!r}"
    )


def test_every_required_ecosystem_being_covered_is_accepted() -> None:
    """The positive case for the coverage reading."""
    entries = [dict(APPROVED_ENTRY, **{"package-ecosystem": "pip"})]
    assert uncovered_directories(entries, frozenset({"pip"})) == [], (
        "an ecosystem with an entry of its own must not be reported"
    )

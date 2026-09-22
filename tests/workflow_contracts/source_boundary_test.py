"""Contracts for where these tests read the files they judge, and what fails.

Two kinds of assertion live here. The first is that the readings are pure
functions of what they are handed: `default_features_in` takes a parsed
manifest and `make_rule_in` takes Makefile text, so a manifest shape or a rule
shape can be stated outright rather than written to disk and read back.

The second is the boundary itself. `read_default_features`, `read_makefile`
and `read_estate` are the only things in this directory that touch a file, and
each converts a missing file, an undecodable one, or one that does not parse
into a `SourceError` naming the path. That conversion is the point: under the
import-time snapshots these replaced, the same faults raised while pytest was
collecting, and a contract directory that fails to collect reports no failures
at all, which reads exactly like a clean run.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import typing as typ

import pytest
from _sources import SourceError, read_text, read_toml
from _estate import read_estate
from _suite_targets import (
    GITHUB_TOOL_RECIPE,
    ROOT_MANIFEST,
    default_features_in,
    make_rule_in,
    read_default_features,
    read_makefile,
)

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from pathlib import Path

#: A Makefile whose rules exercise the shapes the reader has to tell apart.
SAMPLE_MAKEFILE = (
    "first: second third\n"
    "\techo one\n"
    "\techo two\n"
    "\n"
    "second:\n"
    "\techo alone\n"
    "\n"
    "third: fourth\n"
    "\n"
)


@pytest.mark.parametrize(
    ("manifest", "expected"),
    [
        pytest.param({"features": {"default": ["a", "b"]}}, {"a", "b"}, id="declared"),
        pytest.param({"features": {"default": []}}, set(), id="empty-list"),
        pytest.param({"features": {}}, set(), id="no-default-key"),
        pytest.param({}, set(), id="no-features-table"),
        pytest.param({"features": "not-a-table"}, set(), id="features-not-a-table"),
    ],
)
def test_default_features_reads_a_manifest_it_is_given(
    manifest: dict[str, object], expected: set[str]
) -> None:
    """The reading is a pure function of a parsed manifest.

    Splitting it from the file access is what lets these cases be stated at
    all. Each of the last three is a manifest shape that would otherwise have
    raised inside an import, taking the whole contract directory's collection
    with it.
    """
    assert default_features_in(manifest) == frozenset(expected)


@pytest.mark.parametrize(
    ("name", "prerequisites", "recipe"),
    [
        pytest.param(
            "first", ("second", "third"), ("echo one", "echo two"), id="both-halves"
        ),
        pytest.param("second", (), ("echo alone",), id="no-prerequisites"),
        pytest.param("third", ("fourth",), (), id="no-recipe"),
    ],
)
def test_a_rule_is_read_out_of_the_text_it_is_given(
    name: str, prerequisites: tuple[str, ...], recipe: tuple[str, ...]
) -> None:
    """The rule reading is a pure function of Makefile text.

    A target with prerequisites and no recipe, and one with a recipe and no
    prerequisites, are the two shapes the contracts assert about `make test`
    and `make test-github-tool`, so both are stated here against text rather
    than against whatever the repository's Makefile happens to say today.
    """
    assert make_rule_in(SAMPLE_MAKEFILE, name) == (prerequisites, recipe)


def test_a_target_the_makefile_does_not_declare_stops_the_run() -> None:
    """A missing target must fail rather than answer "no commands".

    Every mapping in `_suite_targets` rests on the target existing, so a
    reader that returned an empty recipe would let the contracts conclude
    that a lane runs nothing, which is the passing case for several of them.
    """
    with pytest.raises(AssertionError, match="no longer defines"):
        make_rule_in(SAMPLE_MAKEFILE, "absent")


def test_the_default_set_comes_from_the_manifest() -> None:
    """Read the defaults rather than restating them.

    A reader that returned an empty set would restore the old behaviour
    silently: every equality in `suite_key_test.py` would still hold for a
    command that names nothing, and the only evidence would be a duplicate
    the contract failed to report.
    """
    declared = read_toml(ROOT_MANIFEST)
    features = declared["features"]
    assert isinstance(features, dict), "the root manifest declares no features table"
    assert read_default_features() == frozenset(features["default"]), (
        "the read default list is not the manifest's; a restated set drifts "
        "from the one Cargo actually enables"
    )
    assert read_default_features(), "the root manifest declares no default features"


def test_the_repository_makefile_is_read_through_the_boundary() -> None:
    """The convenience reading and the pure one agree on the real file.

    Without this the pure reader could be correct about text nothing ever
    hands it, while the contracts read a different file or none at all.
    """
    assert make_rule_in(read_makefile(), "test-github-tool") == (
        (),
        (GITHUB_TOOL_RECIPE,),
    )


def test_the_estate_is_read_through_the_boundary() -> None:
    """The workflows these contracts judge are reachable and parse.

    Every estate contract is satisfied by an empty reading, so the reach of
    the scan is asserted before anything is concluded from it.
    """
    estate = read_estate()
    assert "test.yml" in estate, f"the estate scan found {sorted(estate)}"
    assert len(estate) > 5, "the estate scan found too few workflows to be reading one"


def test_the_estate_cannot_be_altered_by_a_contract_that_reads_it() -> None:
    """One test mutating the estate would change what a later one judges.

    The failure would then name the later test, which had done nothing
    wrong, and the reading it complained about would be gone by the time
    anyone looked.
    """
    estate = read_estate()
    with pytest.raises(TypeError):
        estate["invented.yml"] = {}  # type: ignore[index]


def test_a_missing_manifest_names_the_file(tmp_path: Path) -> None:
    """A file that is not there fails the contracts that wanted it.

    Under the import-time snapshot this replaced, the same `OSError` escaped
    during collection instead, and pytest reported no test failures at all.
    """
    absent = tmp_path / "Cargo.toml"
    with pytest.raises(SourceError, match=str(absent)):
        read_default_features(absent)


def test_a_manifest_that_is_not_toml_names_the_file(tmp_path: Path) -> None:
    """A parse failure is reported as a source failure, with the path."""
    broken = tmp_path / "Cargo.toml"
    broken.write_text("[features\ndefault = [", encoding="utf-8")
    with pytest.raises(SourceError, match="is not valid TOML"):
        read_default_features(broken)


def test_a_file_that_is_not_utf8_names_the_file(tmp_path: Path) -> None:
    """Text these contracts cannot decode is a source failure too.

    `read_text` is shared by the manifest, the Makefile and every workflow,
    so this is the one case that would otherwise surface as a
    `UnicodeDecodeError` from whichever of them was read first.
    """
    undecodable = tmp_path / "Makefile"
    undecodable.write_bytes(b"test:\n\techo \xff\xfe\n")
    with pytest.raises(SourceError, match="is not valid UTF-8"):
        read_makefile(undecodable)


def test_a_missing_makefile_names_the_file(tmp_path: Path) -> None:
    """The Makefile boundary converts the same fault as the manifest's."""
    absent = tmp_path / "Makefile"
    with pytest.raises(SourceError, match=str(absent)):
        read_makefile(absent)


def test_a_workflow_directory_that_is_not_there_names_it(tmp_path: Path) -> None:
    """A scan of a directory that does not exist is a source failure.

    It is also the fault that reads most like success: an estate of no
    workflows satisfies every contract that asks whether anything in the
    estate is wrong.
    """
    absent = tmp_path / "workflows"
    with pytest.raises(SourceError, match="cannot be scanned"):
        read_estate(absent)


def test_a_workflow_that_is_not_yaml_names_the_file(tmp_path: Path) -> None:
    """A workflow the parser rejects fails here, naming the file."""
    directory = tmp_path / "workflows"
    directory.mkdir()
    (directory / "broken.yml").write_text("on: [push]\njobs:\n  - :\n  :", "utf-8")
    with pytest.raises(SourceError, match="broken.yml"):
        read_estate(directory)


def test_reading_a_directory_as_a_file_names_it(tmp_path: Path) -> None:
    """The remaining `OSError` shape: a path that is not a regular file."""
    with pytest.raises(SourceError, match="cannot be read"):
        read_text(tmp_path)

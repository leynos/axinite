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

`is_directory` and `is_regular_file` carry the same obligation for a question
rather than a read. `Path.is_dir` and `Path.is_file` answer False for a path
they could not `stat`, so a manifest check built on them reports a missing
file it never managed to look at. These cases state all three answers each
predicate owes: yes, no, and a failure that is reported rather than guessed.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import os
import sys
import typing as typ

import pytest
from _sources import (
    SourceError,
    is_directory,
    is_regular_file,
    read_text,
    read_toml,
)
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
    found = default_features_in(manifest)
    assert found == frozenset(expected), (
        f"the manifest {manifest!r} should declare the default features "
        f"{sorted(expected)}, but the reading returned {sorted(found)}"
    )


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
    found = make_rule_in(SAMPLE_MAKEFILE, name)
    assert found == (prerequisites, recipe), (
        f"rule {name!r} should read as prerequisites {prerequisites} and "
        f"recipe {recipe}, but the reading returned {found}"
    )


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
    found = make_rule_in(read_makefile(), "test-github-tool")
    assert found == ((), (GITHUB_TOOL_RECIPE,)), (
        "the repository Makefile's `test-github-tool` rule should have no "
        f"prerequisites and the one recipe line {GITHUB_TOOL_RECIPE!r}, but "
        f"the reading returned {found}"
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
        typ.cast("dict[str, object]", estate)["invented.yml"] = {}


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


@pytest.mark.parametrize(
    ("body", "because"),
    [
        pytest.param("[]", "a sequence is not a workflow", id="a-sequence"),
        pytest.param("just a string", "nor is a scalar", id="a-scalar"),
        pytest.param("", "nor is an empty file", id="empty"),
    ],
)
def test_a_file_that_parses_but_is_not_a_workflow_names_it(
    tmp_path: Path, body: str, because: str
) -> None:
    """Valid YAML that is not a mapping is a source fault too.

    `parse_workflow` asserts the root is a mapping, and an `AssertionError`
    escaping the boundary is exactly what the boundary exists to prevent: it
    names no file, and it arrives from inside a comprehension several frames
    from the thing that was wrong.
    """
    directory = tmp_path / "workflows"
    directory.mkdir()
    (directory / "odd.yml").write_text(body, encoding="utf-8")
    with pytest.raises(SourceError, match="odd.yml") as raised:
        read_estate(directory)
    assert "odd.yml" in str(raised.value), (
        f"{because}, and the failure must name the file; it said "
        f"{raised.value}"
    )


def test_reading_a_directory_as_a_file_names_it(tmp_path: Path) -> None:
    """The remaining `OSError` shape: a path that is not a regular file."""
    with pytest.raises(SourceError, match="cannot be read"):
        read_text(tmp_path)


@pytest.mark.parametrize(
    ("predicate", "kind", "other"),
    [
        pytest.param(
            is_directory, "crate", "crate/Cargo.toml", id="directory"
        ),
        pytest.param(
            is_regular_file, "crate/Cargo.toml", "crate", id="regular-file"
        ),
    ],
)
def test_a_presence_check_answers_for_a_path_it_can_see(
    tmp_path: Path,
    predicate: typ.Callable[[Path], bool],
    kind: str,
    other: str,
) -> None:
    """The positive half of the classification boundary.

    Each predicate is asked about both paths, so a reader that answered True
    for anything it could `stat` fails here rather than passing the
    permission cases below.
    """
    (tmp_path / "crate").mkdir()
    (tmp_path / "crate" / "Cargo.toml").write_text("", encoding="utf-8")
    assert predicate(tmp_path / kind) is True, (
        f"the predicate must answer True for {kind!r}, which it can see"
    )
    assert predicate(tmp_path / other) is False, (
        f"the predicate must answer False for {other!r}, which is a path of "
        f"the other kind"
    )


@pytest.mark.parametrize(
    ("predicate", "name"),
    [
        pytest.param(is_directory, "is_directory", id="directory"),
        pytest.param(is_regular_file, "is_regular_file", id="regular-file"),
    ],
)
def test_an_absent_path_is_a_definite_answer_not_a_failure(
    tmp_path: Path, predicate: typ.Callable[[Path], bool], name: str
) -> None:
    """A path that is not there is answered False, not reported.

    Every declared directory is checked this way, so reporting an ordinary
    absence as a source failure would fail the suite on a configuration that
    is simply missing a path -- which is the configuration fault the reading
    exists to report.
    """
    assert predicate(tmp_path / "absent") is False, (
        f"{name} must answer False for an absent path, not raise"
    )
    assert predicate(tmp_path / "absent" / "deeper") is False, (
        f"{name} must answer False for a path whose parent is not a "
        f"directory, not raise"
    )


@pytest.mark.skipif(
    sys.platform == "win32",
    reason="POSIX permission bits; Windows chmod only toggles read-only",
)
@pytest.mark.parametrize(
    ("predicate", "name"),
    [
        pytest.param(is_directory, "is_directory", id="directory"),
        pytest.param(is_regular_file, "is_regular_file", id="regular-file"),
    ],
)
def test_an_unclassifiable_path_is_reported_not_answered_false(
    tmp_path: Path, predicate: typ.Callable[[Path], bool], name: str
) -> None:
    """A `stat` that fails is not the same answer as an absent file.

    This is the whole reason the boundary exists. `Path.is_dir` and
    `Path.is_file` answer `False` here, so a manifest check built on them
    would report a configuration fault about a path it never managed to look
    at.

    The parent is made unreadable rather than the entry itself: `stat` needs
    the search bit on the directory holding the name, which is the failure a
    permission mistake actually produces.
    """
    if os.geteuid() == 0:
        pytest.skip("root reads a directory whatever its mode bits say")
    unreadable = tmp_path / "unreadable"
    unreadable.mkdir()
    (unreadable / "Cargo.toml").write_text("", encoding="utf-8")
    unreadable.chmod(0o000)
    try:
        with pytest.raises(SourceError) as raised:
            predicate(unreadable / "Cargo.toml")
    finally:
        unreadable.chmod(0o700)
    assert raised.value.path == unreadable / "Cargo.toml", (
        f"{name} must name the path it could not classify; it named "
        f"{raised.value.path}"
    )
    assert isinstance(raised.value.__cause__, OSError), (
        f"{name} must chain the `stat` failure as the cause; the cause was "
        f"{raised.value.__cause__!r}"
    )

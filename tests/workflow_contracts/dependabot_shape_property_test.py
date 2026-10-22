"""Property tests holding the Dependabot shape readings to a model.

`dependabot_reader_test.py` states named cases, one mutation of the approved
entry each, and those cases are what makes a failure readable: a reader that
started rejecting the canonical catch-all fails a case called after the thing
it broke. A table cannot state the range these readings are asked about,
though. `group_faults` depends on the order the groups were inserted in,
`duplicate_faults` compares every pair, and `uncovered_directories` and
`absolute` are defined over arbitrary sets of names and spellings. Each of
those is a relation, not an example, and these properties generate the entries
and compare the reading against a model built from what was generated.

The model is deliberately simple and independent of the reader: it knows from
the way each entry was constructed which faults it should produce, rather than
re-deriving the reader's own question. A property that restated the reader
would agree with it whatever it did.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import pytest
from _dependabot_coverage import duplicate_faults, uncovered_directories
from _dependabot_shape import group_faults
from dependabot_config import absolute, declared_directories
from hypothesis import given
from hypothesis import strategies as st

#: Group names. Short and drawn from a small alphabet, so a document that
#: repeats one is cheap to reach.
NAMES = st.text(alphabet="abcdef_", min_size=1, max_size=5)

#: An ecosystem name, covering the ones canon knows and ones it does not.
ECOSYSTEMS = st.sampled_from(
    ["cargo", "pip", "bun", "github-actions", "gomod"]
)


def _catch_all() -> dict[str, object]:
    """Return the canonical catch-all group."""
    return {"patterns": ["*"], "update-types": ["minor", "patch"]}


def _narrow(pattern: str) -> dict[str, object]:
    """Return a group narrower than the catch-all."""
    return {"patterns": [pattern]}


def _entry(ecosystem: str, directory: str) -> dict[str, object]:
    """Return one entry with the canonical group shape."""
    return {
        "package-ecosystem": ecosystem,
        "directory": directory,
        "schedule": {"interval": "daily"},
        "groups": {"narrow": _narrow("dep*"), "minor-and-patch": _catch_all()},
    }


@given(names=st.lists(NAMES, min_size=1, max_size=4, unique=True))
def test_narrow_groups_before_the_catch_all_are_accepted(
    names: list[str],
) -> None:
    """Any number of narrow groups, in any order, passes the shape reading.

    The reading's rule is positional rather than about the groups' contents:
    everything before the last group must be narrower, and the last must be
    the catch-all. Nothing about how many narrow groups there are, or what
    they are called, changes that, and a reader that counted them would fail
    here on a configuration canon accepts.
    """
    groups: dict[str, object] = {
        f"{name}-{position}": _narrow("dep*")
        for position, name in enumerate(names)
    }
    groups["minor-and-patch"] = _catch_all()
    entry = dict(_entry("cargo", "/"), groups=groups)
    assert group_faults(entry) == [], (
        f"{len(names)} narrow groups before the catch-all is the shape canon "
        f"requires, but the reading reported {group_faults(entry)}"
    )


@given(
    names=st.lists(NAMES, min_size=1, max_size=3, unique=True),
    position=st.integers(min_value=0),
)
def test_a_wildcard_anywhere_before_the_catch_all_is_reported(
    names: list[str], position: int
) -> None:
    """A second catch-all is reported wherever it sits.

    A wildcard group before the last one takes every dependency Dependabot
    would otherwise leave to the catch-all, including the majors the
    catch-all deliberately does not group. The reading must find it at any
    index rather than only the first.
    """
    index = position % len(names)
    groups: dict[str, object] = {
        f"{name}-{offset}": _narrow("dep*")
        for offset, name in enumerate(names)
    }
    wildcard = f"{names[index]}-wildcard"
    groups[wildcard] = {"patterns": ["*"]}
    groups["minor-and-patch"] = _catch_all()
    entry = dict(_entry("cargo", "/"), groups=groups)
    faults = group_faults(entry)
    assert faults, (
        f"the wildcard group {wildcard!r} sits before the catch-all and must "
        f"be reported, but the reading found no fault"
    )
    assert any(wildcard in fault for fault in faults), (
        f"the fault must name the group {wildcard!r}; it reported {faults}"
    )


@given(
    ecosystems=st.lists(ECOSYSTEMS, min_size=0, max_size=4),
    required=st.frozensets(ECOSYSTEMS, max_size=4),
)
def test_coverage_is_the_difference_between_required_and_declared(
    ecosystems: list[str], required: frozenset[str]
) -> None:
    """The coverage reading reports exactly the required-and-absent set.

    Stated as a set difference rather than as an expected list, because that
    is the question: an ecosystem is reported when the repository requires it
    and no entry declares it, and for no other reason. A reader that reported
    a declared ecosystem, or missed an absent one, disagrees with the model
    here whatever order the entries were in.
    """
    entries = [_entry(ecosystem, "/") for ecosystem in ecosystems]
    expected = required - set(ecosystems)
    faults = uncovered_directories(entries, required)
    assert len(faults) == len(expected), (
        f"the entries declare {sorted(set(ecosystems))} and the repository "
        f"requires {sorted(required)}, so {sorted(expected)} are uncovered, "
        f"but the reading reported {len(faults)} faults: {faults}"
    )
    for ecosystem in expected:
        assert any(ecosystem in fault for fault in faults), (
            f"the uncovered ecosystem {ecosystem!r} must be named in a "
            f"fault; the reading reported {faults}"
        )


@given(
    first=st.lists(ECOSYSTEMS, min_size=1, max_size=3),
    second=st.lists(ECOSYSTEMS, min_size=1, max_size=3),
    directory=st.sampled_from(["/", "/web-src", "/tests/e2e"]),
)
def test_two_entries_contend_exactly_when_their_ecosystems_and_paths_meet(
    first: list[str], second: list[str], directory: str
) -> None:
    """Duplication is a shared ecosystem over a shared directory.

    Two entries contending for one dependency is what the reading exists to
    find, and the model says so directly: the fault holds when the two
    ecosystems and the two directories intersect. Entries of different
    ecosystems never contend, which is the case a reader that compared
    directories alone would get wrong.
    """
    entries = [_entry(ecosystem, directory) for ecosystem in first]
    entries += [_entry(ecosystem, directory) for ecosystem in second]
    repeated = {
        ecosystem
        for ecosystem in first + second
        if (first + second).count(ecosystem) > 1
    }
    faults = duplicate_faults(entries)
    if not repeated:
        assert faults == [], (
            f"every ecosystem over {directory!r} appears once, so nothing "
            f"contends, but the reading reported {faults}"
        )
        return
    assert faults, (
        f"the ecosystems {sorted(repeated)} appear more than once over "
        f"{directory!r}, so entries contend, but the reading reported nothing"
    )
    assert len(faults) == sum(
        count * (count - 1) // 2
        for count in (
            (first + second).count(ecosystem) for ecosystem in repeated
        )
    ), (
        f"the reading must report one fault per contending pair; it "
        f"reported {len(faults)}: {faults}"
    )


@pytest.mark.parametrize(
    ("spelling", "expected"),
    [
        pytest.param("/tests/e2e", "/tests/e2e", id="leading-slash"),
        pytest.param("tests/e2e/", "/tests/e2e", id="trailing-slash"),
        pytest.param("./tests/e2e", "/tests/e2e", id="dot-prefix"),
        pytest.param("./tests/e2e/", "/tests/e2e", id="both"),
        pytest.param("tests//e2e", "/tests/e2e", id="repeated-separator"),
        pytest.param("//tests/e2e", "/tests/e2e", id="two-leading-slashes"),
        pytest.param("//tests//e2e", "/tests/e2e", id="two-leading-and-repeated"),
        pytest.param("/", "/", id="root"),
        pytest.param("", "/", id="empty"),
    ],
)
def test_a_directory_normalizes_to_one_spelling(
    spelling: str, expected: str
) -> None:
    """Every accepted spelling of a directory reads as the same path."""
    assert absolute(spelling) == expected, (
        f"the spelling {spelling!r} must read as {expected!r}, but the "
        f"reading returned {absolute(spelling)!r}"
    )


@given(
    spellings=st.lists(
        st.sampled_from(
            [
                "/tests/e2e",
                "tests/e2e/",
                "./tests/e2e",
                "//tests/e2e",
                "tests//e2e",
            ]
        ),
        min_size=1,
        max_size=4,
    )
)
def test_the_spellings_of_one_directory_normalize_together(
    spellings: list[str],
) -> None:
    """Spellings differing only in decoration collapse to one set member.

    `duplicate_faults` compares declared directory sets, so two entries over
    one path spelled differently must not look like two directories. This is
    the property the named cases state three ways, over any mixing of them.
    """
    entry = _entry("cargo", "/")
    entry["directories"] = spellings
    entry.pop("directory")
    declared = declared_directories(entry)
    assert declared == frozenset({"/tests/e2e"}), (
        f"the spellings {spellings} name one directory, so they must read as "
        f"one member, but the reading returned {sorted(declared)}"
    )

"""Property tests holding the inheritance walk to an independent model.

`profile_inheritance_test.py` states one three-level chain and three
invalid files. The walk has to hold for any shape of inheritance: chains
of any length, several profiles sharing a parent, profiles that name
``default`` explicitly and profiles that leave it implicit. These
properties generate a forest of profiles rooted at ``default``, write it
as a nextest configuration, and compare what `profiles` reports with a
model that walks the generated parent links directly.

Each generated override selects a binary named after its profile and its
position, so the inherited list names exactly which tables were consulted
and in which order.

Run via ``make test-workflow-contracts``.
"""

import pytest
from hypothesis import given
from hypothesis import strategies as st
from nextest_config import NextestConfigurationError, profiles

#: What every generated profile table carries, so the file is one nextest
#: would load; the values are irrelevant to the walk.
PROFILE_BODY = 'slow-timeout = { period = "300s", terminate-after = 1 }\n'


def _declaration(name: str, parent: str | None, overrides: int) -> str:
    """Return one profile's table and its overrides as configuration text."""
    lines = [f"[profile.{name}]\n"]
    if parent is not None:
        lines.append(f'inherits = "{parent}"\n')
    lines.append(PROFILE_BODY)
    for index in range(overrides):
        lines.append(f"\n[[profile.{name}.overrides]]\n")
        lines.append(f"filter = 'binary({name}_{index})'\n")
        lines.append(PROFILE_BODY)
    return "".join(lines) + "\n"


@st.composite
def _forest(draw: st.DrawFn) -> tuple[dict[str, str | None], dict[str, int]]:
    """Draw profiles whose parents all precede them, so no cycle is possible.

    Returns each profile's declared parent (``None`` for an implicit
    ``default``) and its override count. ``default`` is always present and
    declares no parent.
    """
    count = draw(st.integers(min_value=1, max_value=6))
    names = [f"p{index}" for index in range(count)]
    parents: dict[str, str | None] = {"default": None}
    for index, name in enumerate(names):
        earlier = ["default", *names[:index]]
        choice = draw(st.sampled_from([None, *earlier]))
        parents[name] = choice
    overrides = {
        name: draw(st.integers(min_value=0, max_value=3)) for name in parents
    }
    return parents, overrides


def _model_ancestors(name: str, parents: dict[str, str | None]) -> list[str]:
    """Walk the generated parent links, nearest first, ending at default."""
    chain = []
    current = name
    while current != "default":
        current = parents[current] or "default"
        chain.append(current)
    return chain


def _text(parents: dict[str, str | None], overrides: dict[str, int]) -> str:
    """Write a whole forest as one configuration."""
    return "".join(
        _declaration(name, parent, overrides[name])
        for name, parent in parents.items()
    )


@given(_forest())
def test_every_profile_inherits_its_ancestors_nearest_first(
    forest: tuple[dict[str, str | None], dict[str, int]],
) -> None:
    """Each profile's inherited overrides are its ancestors', in chain order.

    The order is nextest's: the nearest ancestor's overrides first and
    ``default``'s last, each reported at the profile that declares it.
    ``default`` itself inherits nothing.
    """
    parents, overrides = forest
    parsed = profiles(_text(parents, overrides))
    for name in parents:
        expected = [
            (f"profile.{ancestor}", f"binary({ancestor}_{index})")
            for ancestor in _model_ancestors(name, parents)
            for index in range(overrides[ancestor])
        ]
        found = [
            (path, table.get("filter")) for path, table in parsed[name].inherited
        ]
        assert found == expected, (
            f"profile.{name} should inherit {expected}, nearest ancestor "
            f"first; got {found}"
        )


@given(_forest(), st.data())
def test_a_parent_the_file_does_not_declare_is_refused(
    forest: tuple[dict[str, str | None], dict[str, int]], data: st.DataObject
) -> None:
    """Pointing any profile at an undeclared parent refuses the whole file."""
    parents, overrides = forest
    victim = data.draw(st.sampled_from([name for name in parents if name != "default"]))
    parents[victim] = "absent"
    with pytest.raises(NextestConfigurationError, match="does not declare"):
        profiles(_text(parents, overrides))


@given(_forest(), st.data())
def test_a_cycle_anywhere_is_refused(
    forest: tuple[dict[str, str | None], dict[str, int]], data: st.DataObject
) -> None:
    """Pointing a profile at itself or a descendant closes a cycle.

    Every descendant of the victim, and the victim itself, is a target
    that closes one, so the cycle's length is drawn along with it.
    """
    parents, overrides = forest
    victim = data.draw(st.sampled_from([name for name in parents if name != "default"]))
    descendants = [
        name for name in parents if victim in [name, *_model_ancestors(name, parents)]
    ]
    parents[victim] = data.draw(st.sampled_from(descendants))
    with pytest.raises(NextestConfigurationError, match="cycle"):
        profiles(_text(parents, overrides))

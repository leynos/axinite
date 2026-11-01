"""Property contracts for Cargo feature selection in coverage legs.

The coverage contracts decide from a leg's flags whether it compiles PostgreSQL
and whether it has the embedded cluster its database tests run on. Cargo
accepts several spellings of one feature selection: long and short flags, a
value joined with `=` or with the flag, comma or whitespace separators, repeated
flags, and duplicate names. These properties compare both readers with a model
that works from the selection's meaning rather than from the command line, so a
reader that agrees with the model agrees with every spelling of it.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import shlex
import typing as typ
from dataclasses import dataclass

import pytest
from _cargo_features import _enables_postgres, _has_embedded_cluster
from hypothesis import given, settings
from hypothesis import strategies as st

#: The feature names the generators choose from. `libsql` stands for a feature
#: that implies neither database feature, so a selection can be
#: PostgreSQL-free.
FEATURE_NAMES: typ.Final[tuple[str, ...]] = (
    "postgres",
    "embedded-postgres",
    "libsql",
)

#: The default sets the root manifest can carry. `embedded-postgres` is absent
#: from every one: it is not a default, and a leg that names nothing must not
#: receive a database.
DEFAULT_SETS: typ.Final[tuple[frozenset[str], ...]] = (
    frozenset(),
    frozenset({"postgres"}),
    frozenset({"postgres", "libsql"}),
)

#: Cargo's spellings of one feature-list argument.
FLAG_FORMS: typ.Final[tuple[str, ...]] = (
    "long",
    "long-equals",
    "short",
    "short-equals",
    "joined",
)

#: The separators Cargo accepts inside one feature-list value, including the
#: mixed form a handwritten command often carries.
SEPARATORS: typ.Final[tuple[str, ...]] = (",", " ", ", ")


@dataclass(frozen=True)
class Selection:
    """One semantic feature selection, before it is spelled as flags."""

    explicit: tuple[str, ...]
    defaults: frozenset[str]
    no_defaults: bool
    all_features: bool


@dataclass(frozen=True)
class EquivalentForms:
    """One selection whose meaning every flag spelling must preserve.

    The names, their separator and the switches are held fixed, so only the
    spelling of the flag varies between the forms compared against the model.
    """

    features: tuple[str, ...]
    separator: str
    defaults: frozenset[str]
    no_defaults: bool
    all_features: bool

    @property
    def selection(self) -> Selection:
        """Return the selection these names and switches describe."""
        return Selection(
            self.features, self.defaults, self.no_defaults, self.all_features
        )


@dataclass(frozen=True)
class Spelling:
    """One rendering of a selection's explicit features as arguments.

    Each group is one `--features` argument: the names it carries, the
    separator between them, and the spelling of the flag. Several groups are
    the repeated flags that contribute different features. `render_empty`
    decides whether a group with no names is written out as an empty argument,
    and the two booleans move the switches and reverse the groups, so the order
    of a command is generated too.
    """

    groups: tuple[tuple[tuple[str, ...], str, str], ...]
    switches_first: bool = True
    reverse_groups: bool = False
    render_empty: bool = False


def _render_flag(form: str, value: str) -> str:
    """Render one feature-list argument in one of Cargo's spellings."""
    quoted = shlex.quote(value)
    if form == "long":
        return f"--features {quoted}"
    if form == "long-equals":
        return f"--features={quoted}"
    if form == "short":
        return f"-F {quoted}"
    if form == "short-equals":
        return f"-F={quoted}"
    return f"-F{quoted}"


def _render(selection: Selection, spelling: Spelling) -> str:
    """Render a selection as the command-line fragment a leg would carry."""
    groups = spelling.groups[::-1] if spelling.reverse_groups else spelling.groups
    feature_flags = [
        _render_flag(form, separator.join(features))
        for features, separator, form in groups
        if features or spelling.render_empty
    ]
    switches = []
    if selection.no_defaults:
        switches.append("--no-default-features")
    if selection.all_features:
        switches.append("--all-features")
    ordered = (
        [*switches, *feature_flags]
        if spelling.switches_first
        else [*feature_flags, *switches]
    )
    return " ".join(ordered)


def _model(selection: Selection) -> tuple[bool, bool]:
    """Return whether the selection compiles Postgres and the embedded cluster.

    The model states Cargo's rule over the selection's meaning, not over the
    flags a spelling produces. `--all-features` takes everything. Otherwise the
    explicit names take the default set with them unless
    `--no-default-features` turns it off. `postgres` is selected directly or
    through `embedded-postgres`, which enables it in the root manifest, while
    the cluster itself is selected only by `--all-features` or by naming it,
    because it is not a default.
    """
    if selection.all_features:
        return True, True
    effective = set(selection.explicit)
    if not selection.no_defaults:
        effective |= selection.defaults
    postgres = "postgres" in effective or "embedded-postgres" in effective
    embedded = "embedded-postgres" in selection.explicit
    return postgres, embedded


def _read(flags: str, defaults: frozenset[str]) -> tuple[bool, bool]:
    """Return what both readers say about one rendering."""
    return _enables_postgres(flags, defaults), _has_embedded_cluster(flags)


@st.composite
def _spellings(draw: st.DrawFn) -> tuple[Selection, Spelling]:
    """Draw one semantic selection and one valid spelling of it.

    Groups may be empty and names may repeat, so the generated cases include an
    empty explicit selection, duplicate names, and PostgreSQL-free selections.
    More than one non-empty group is the repeated flags that contribute
    different features.
    """
    groups = draw(
        st.lists(
            st.lists(
                st.sampled_from(FEATURE_NAMES),
                min_size=0,
                max_size=3,
            ),
            min_size=0,
            max_size=3,
        )
    )
    spelled = tuple(
        (
            tuple(group),
            draw(st.sampled_from(SEPARATORS)),
            draw(st.sampled_from(FLAG_FORMS)),
        )
        for group in groups
    )
    selection = Selection(
        explicit=tuple(name for group in groups for name in group),
        defaults=draw(st.sampled_from(DEFAULT_SETS)),
        no_defaults=draw(st.booleans()),
        all_features=draw(st.booleans()),
    )
    spelling = Spelling(
        groups=spelled,
        switches_first=draw(st.booleans()),
        reverse_groups=draw(st.booleans()),
        render_empty=draw(st.booleans()),
    )
    return selection, spelling


@st.composite
def _equivalent_forms(draw: st.DrawFn) -> EquivalentForms:
    """Draw one selection and the separator its names are written with."""
    return EquivalentForms(
        features=tuple(
            draw(st.lists(st.sampled_from(FEATURE_NAMES), min_size=0, max_size=4))
        ),
        separator=draw(st.sampled_from(SEPARATORS)),
        defaults=draw(st.sampled_from(DEFAULT_SETS)),
        no_defaults=draw(st.booleans()),
        all_features=draw(st.booleans()),
    )


@settings(max_examples=100)
@given(_equivalent_forms())
def test_equivalent_feature_flag_forms_resolve_both_helpers(
    case: EquivalentForms,
) -> None:
    """One selection renders to every flag form with the same meaning.

    Only the spelling of the flag changes between the forms compared here, so a
    reader that understands one form and not another fails even though every
    form is a valid command.
    """
    selection = case.selection
    expected = _model(selection)
    for form in FLAG_FORMS:
        spelling = Spelling(
            groups=((case.features, case.separator, form),),
            switches_first=form in {"long", "short"},
            render_empty=True,
        )
        rendered = _render(selection, spelling)
        assert _read(rendered, case.defaults) == expected, (
            f"the {form} spelling {rendered!r} with defaults "
            f"{sorted(case.defaults)!r} should read as postgres/embedded="
            f"{expected}"
        )


@settings(max_examples=150)
@given(_spellings())
def test_repeated_flags_and_duplicates_resolve_the_same_features(
    case: tuple[Selection, Spelling],
) -> None:
    """Both readers agree with the model for every generated spelling.

    The generator covers the command shapes a leg can carry: empty and repeated
    groups, duplicate names, both separators, quoted values, either switch, and
    switches before or after the feature flags.
    """
    selection, spelling = case
    rendered = _render(selection, spelling)
    expected = _model(selection)
    assert _read(rendered, selection.defaults) == expected, (
        f"{rendered!r} with defaults {sorted(selection.defaults)!r} should read "
        f"as postgres/embedded={expected}"
    )


@pytest.mark.parametrize(
    ("flags", "defaults", "expected"),
    [
        pytest.param(
            "",
            frozenset({"postgres"}),
            (True, False),
            id="defaults-only",
        ),
        pytest.param(
            "--no-default-features",
            frozenset({"postgres"}),
            (False, False),
            id="empty-selection-without-defaults",
        ),
        pytest.param(
            "--features ''",
            frozenset({"postgres"}),
            (True, False),
            id="empty-explicit-selection-keeps-defaults",
        ),
        pytest.param(
            "--no-default-features --features libsql",
            frozenset({"postgres"}),
            (False, False),
            id="postgres-free-selection",
        ),
        pytest.param(
            "--no-default-features --features 'libsql embedded-postgres'",
            frozenset({"postgres"}),
            (True, True),
            id="embedded-implies-postgres",
        ),
        pytest.param(
            "--all-features",
            frozenset(),
            (True, True),
            id="all-features",
        ),
    ],
)
def test_notable_selections_read_as_the_model_predicts(
    flags: str,
    defaults: frozenset[str],
    expected: tuple[bool, bool],
) -> None:
    """Anchor the generated cases with the selections the contracts care about."""
    assert _read(flags, defaults) == expected, (
        f"{flags!r} with defaults {sorted(defaults)!r} should read as "
        f"postgres/embedded={expected}"
    )


@given(defaults=st.sampled_from(DEFAULT_SETS))
def test_the_embedded_cluster_is_never_a_default(defaults: frozenset[str]) -> None:
    """A leg that names no feature gets the cluster only by naming it.

    `embedded-postgres` is not a member of the root manifest's default set, and
    neither is the `postgres` it implies. The reverse also holds: a default set
    that carries `postgres` makes the leg PostgreSQL-bearing with no flags at
    all.
    """
    assert _has_embedded_cluster("") is False, (
        "embedded-postgres must stay out of the default feature set"
    )
    assert _enables_postgres("", defaults) is ("postgres" in defaults)

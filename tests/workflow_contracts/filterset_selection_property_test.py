"""Property tests holding `binaries_selected` to an independent model.

`filterset_polarity_test.py` states exact spellings, one case each. The
reading, though, has to hold for any order of terms, any mix of the three
negation spellings, any joining operator and any spacing, and a table
cannot reach that range. These properties generate filtersets from their
parts and compare the reading with a model that knows, from the way each
expression was built, which binaries it selects.

The model is deliberately simple: a term is selected unless it was built
negated. That is only a faithful model for the shapes the reader promises
to read, which is why the refusals are properties of their own. A
negation over a group, a negation over a negation, and a binary both
selected and excluded are each generated inside otherwise readable
filtersets and must be refused wherever they sit.

Run via ``make test-workflow-contracts``.
"""

import typing as typ

import pytest
from hypothesis import given
from hypothesis import strategies as st
from nextest_config import NextestConfigurationError
from nextest_filtersets import binaries_selected

#: Binary names. Short and drawn from a small alphabet so collisions,
#: which the refusal property needs, are cheap to reach.
NAMES = st.text(alphabet="abcdz_", min_size=1, max_size=6)

#: Spacing around operators, including none and a line break, since the
#: reader collapses whitespace before matching.
SPACE = st.sampled_from(["", " ", "  ", "\n", " \t "])

#: The joining operators that are not themselves negations.
JOINS = st.sampled_from(["|", "&", "+", "or", "and"])

#: The prefix negation spellings. The infix ``-`` is generated separately
#: because it takes the place of the joining operator.
PREFIX_NEGATIONS = st.sampled_from(["not ", "not  ", "!", "! "])


class Term(typ.NamedTuple):
    """One ``binary(...)`` term as generated, with its polarity.

    Attributes
    ----------
    name
        The binary it names.
    negated
        Whether it was built under a negation.
    spelling
        The negation written before it: a prefix spelling, ``-`` for
        set difference, or empty when it is not negated.
    """

    name: str
    negated: bool
    spelling: str


@st.composite
def _terms(draw: st.DrawFn) -> list[Term]:
    """Draw terms over distinct names, each positive or negated.

    The first term never takes the infix ``-``, which needs a left-hand
    side.
    """
    names = draw(st.lists(NAMES, min_size=1, max_size=5, unique=True))
    terms = []
    for index, name in enumerate(names):
        negated = draw(st.booleans())
        if not negated:
            terms.append(Term(name, False, ""))
            continue
        spellings = PREFIX_NEGATIONS if index == 0 else PREFIX_NEGATIONS | st.just("-")
        terms.append(Term(name, True, draw(spellings)))
    return terms


def _render(draw: st.DrawFn, terms: list[Term]) -> str:
    """Write terms out as one filterset, with drawn operators and spacing."""
    pieces = []
    for index, term in enumerate(terms):
        inner = f"binary({draw(SPACE)}{term.name}{draw(SPACE)})"
        if term.spelling == "-":
            pieces.append(f"{draw(SPACE)} -{draw(SPACE)}{inner}")
            continue
        if index:
            pieces.append(f"{draw(SPACE)} {draw(JOINS)} {draw(SPACE)}")
        pieces.append(f"{term.spelling}{inner}")
    return "".join(pieces)


@st.composite
def _readable(draw: st.DrawFn) -> tuple[str, frozenset[str]]:
    """Draw a readable filterset and the binaries the model says it selects."""
    terms = draw(_terms())
    selected = frozenset(term.name for term in terms if not term.negated)
    return _render(draw, terms), selected


@given(_readable())
def test_the_reading_agrees_with_the_model(case: tuple[str, frozenset[str]]) -> None:
    """Whatever the order, spelling and spacing, the positive terms are selected.

    The dangerous direction is a negated binary read as selected, which
    grants it an override's allowance nextest never applies; the other
    direction drops an allowance that is in force. Equality holds both.
    """
    filterset, expected = case
    found = binaries_selected(filterset)
    assert found == expected, (
        f"{filterset!r} selects {sorted(expected)}, but the reading returned "
        f"{sorted(found)}"
    )


@st.composite
def _with_a_contradiction(draw: st.DrawFn) -> str:
    """Draw a readable filterset in which one name is selected and excluded."""
    terms = draw(_terms())
    victim = draw(st.sampled_from(terms))
    flipped = Term(
        victim.name,
        not victim.negated,
        "" if victim.negated else draw(PREFIX_NEGATIONS),
    )
    position = draw(st.integers(min_value=0, max_value=len(terms)))
    return _render(draw, [*terms[:position], flipped, *terms[position:]])


@given(_with_a_contradiction())
def test_a_binary_both_selected_and_excluded_is_refused(filterset: str) -> None:
    """Which occurrence wins depends on grouping the reader does not model."""
    with pytest.raises(NextestConfigurationError, match="both selects and excludes"):
        binaries_selected(filterset)


@st.composite
def _with_a_double_negation(draw: st.DrawFn) -> str:
    """Draw a readable filterset with one term under two negations."""
    terms = draw(_terms())
    names = {term.name for term in terms}
    name = draw(NAMES.filter(lambda candidate: candidate not in names))
    outer = draw(st.sampled_from(["-", "not ", "!"]))
    inner = draw(PREFIX_NEGATIONS)
    rendered = _render(draw, terms)
    joined = f" {outer}" if outer == "-" else f" {draw(JOINS)} {outer}"
    return f"{rendered}{joined}{draw(SPACE)}{inner}binary({name})"


@given(_with_a_double_negation())
def test_a_negation_over_a_negation_is_refused(filterset: str) -> None:
    """The pair cancels in nextest, and a term matcher cannot cancel it."""
    with pytest.raises(NextestConfigurationError, match="one negation to another"):
        binaries_selected(filterset)


@st.composite
def _with_a_negated_group(draw: st.DrawFn) -> str:
    """Draw a readable filterset with a negation applied to a group."""
    terms = draw(_terms())
    grouped = draw(_terms())
    positives = [Term(term.name, False, "") for term in grouped]
    negation = draw(PREFIX_NEGATIONS)
    group = f"{negation}({_render(draw, positives)})"
    return f"{_render(draw, terms)} {draw(JOINS)} {group}"


@given(_with_a_negated_group())
def test_a_negation_over_a_group_is_refused(filterset: str) -> None:
    """Attributing a negated group needs evaluation, not term matching."""
    with pytest.raises(NextestConfigurationError, match="negates a group"):
        binaries_selected(filterset)

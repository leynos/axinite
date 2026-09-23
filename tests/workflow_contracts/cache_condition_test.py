"""Unit tests for the cache save condition reader.

`cache_ownership_test.py` runs the reader over the estate's two save steps,
and both of them are correct. A reader parametrized over correct input
discriminates nothing: one that answered "no faults" unconditionally would
pass there exactly as the real one does. So the reader is driven here instead,
with the approved predicate once and a mutation of it per case, each mutation
being a condition that a substring reading accepts and that changes which
pushes write the key.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import pytest
from _cache_conditions import (
    MAIN_REF,
    PUSH_EVENT,
    WRITING_LEG,
    conjuncts,
    save_condition_faults,
)

#: The restore step the estate's save steps consult.
RESTORE_ID = "cargo-registry"
RESTORE_IDS: frozenset[str] = frozenset({RESTORE_ID})

#: The restore's own outcome, spelled the one way the policy accepts.
CACHE_MISS = f"steps.{RESTORE_ID}.outputs.cache-hit != 'true'"

#: The condition both save steps carry, written as the estate writes it: a
#: folded YAML scalar, so it arrives with its line breaks joined into spaces.
APPROVED = f"{WRITING_LEG} && {PUSH_EVENT} &&\n{MAIN_REF} &&\n{CACHE_MISS}"


def test_the_approved_predicate_passes() -> None:
    """The positive case, without which every mutation below proves nothing.

    A reader that reported a fault for everything would fail each mutation
    and would be useless. This is the half that says the four conjuncts the
    estate actually writes are accepted.
    """
    assert save_condition_faults(APPROVED, RESTORE_IDS) == []


@pytest.mark.parametrize(
    "condition",
    [
        pytest.param(
            f"{WRITING_LEG} && {PUSH_EVENT} && {MAIN_REF} && {CACHE_MISS}",
            id="written-on-one-line",
        ),
        pytest.param(
            f"{CACHE_MISS} && {MAIN_REF} && {PUSH_EVENT} && {WRITING_LEG}",
            id="conjuncts-reordered",
        ),
        pytest.param(
            f"  {WRITING_LEG}   &&   {PUSH_EVENT}\n&&{MAIN_REF}&& {CACHE_MISS} ",
            id="whitespace-around-the-operators",
        ),
    ],
)
def test_the_predicate_is_read_however_it_is_laid_out(condition: str) -> None:
    """Layout is not policy: the same four conjuncts are the same predicate.

    GitHub evaluates a conjunction regardless of ordering or line breaks, so
    a reader that insisted on one layout would refuse a correct condition and
    push its author towards weakening the contract instead.
    """
    assert save_condition_faults(condition, RESTORE_IDS) == []


@pytest.mark.parametrize(
    ("condition", "because"),
    [
        pytest.param(
            f"{WRITING_LEG} && {PUSH_EVENT} && {MAIN_REF} && {CACHE_MISS} "
            "|| github.event_name == 'schedule'",
            "an alternative arm admits an event the policy does not",
            id="an-extra-or-arm",
        ),
        pytest.param(
            f"{WRITING_LEG} && ({PUSH_EVENT} || {MAIN_REF}) && {CACHE_MISS}",
            "a grouped disjunction is not four plain conjuncts",
            id="a-grouped-or-arm",
        ),
        pytest.param(
            f"matrix.name == 'default' && {PUSH_EVENT} && {MAIN_REF} && {CACHE_MISS}",
            "a narrower leg publishes an archive missing most of the graph",
            id="the-wrong-writing-leg",
        ),
        pytest.param(
            f"{PUSH_EVENT} && {MAIN_REF} && {CACHE_MISS}",
            "every leg shares one key, so unnamed they race",
            id="no-writing-leg-at-all",
        ),
        pytest.param(
            f"{WRITING_LEG} && {MAIN_REF} && {CACHE_MISS}",
            "a dispatch against main satisfies a ref-only guard",
            id="no-push-event",
        ),
        pytest.param(
            f"{WRITING_LEG} && {PUSH_EVENT} && {CACHE_MISS}",
            "a push to any branch would take the key",
            id="no-main-ref",
        ),
        pytest.param(
            f"{WRITING_LEG} && {PUSH_EVENT} && {MAIN_REF}",
            "the archive is re-uploaded on every push",
            id="no-cache-miss-guard",
        ),
        pytest.param(
            f"{WRITING_LEG} && {PUSH_EVENT} && {MAIN_REF} && "
            f"steps.{RESTORE_ID}.outputs.cache-hit == 'true'",
            "the inverted comparison writes only when it need not",
            id="the-cache-hit-comparison-inverted",
        ),
        pytest.param(
            f"{WRITING_LEG} && {PUSH_EVENT} && {MAIN_REF} && "
            "steps.registry.outputs.cache-hit != 'true'",
            "an undeclared step ID resolves to the empty string, so the "
            "inequality holds and every matching push re-uploads the archive",
            id="a-restore-id-no-step-declares",
        ),
        pytest.param(
            f"{WRITING_LEG} && {PUSH_EVENT} && {MAIN_REF} && {CACHE_MISS} && "
            "github.actor != 'dependabot[bot]'",
            "a term the policy does not name changes who writes the key",
            id="an-extra-conjunct",
        ),
        pytest.param(
            f"{WRITING_LEG} && {PUSH_EVENT} && {PUSH_EVENT} && {MAIN_REF} && "
            f"{CACHE_MISS}",
            "a set comparison forgets the repeat, which usually stands where "
            "another term was meant",
            id="the-push-event-repeated",
        ),
        pytest.param(
            f"{WRITING_LEG} && {PUSH_EVENT} && {MAIN_REF} && {MAIN_REF} && "
            f"{CACHE_MISS}",
            "a set comparison forgets the repeat, which usually stands where "
            "another term was meant",
            id="the-main-ref-repeated",
        ),
        pytest.param(
            f"{WRITING_LEG} && {WRITING_LEG} && {PUSH_EVENT} && {MAIN_REF} && "
            f"{CACHE_MISS}",
            "a set comparison forgets the repeat, which usually stands where "
            "another term was meant",
            id="the-writing-leg-repeated",
        ),
        pytest.param(
            "",
            "an unguarded save runs on every leg of every event",
            id="no-condition-at-all",
        ),
    ],
)
def test_a_condition_that_is_not_the_policy_is_refused(
    condition: str, because: str
) -> None:
    """Each mutation changes which pushes write the key, and must fail.

    Every one of these satisfies a reading that searches the expression for
    the approved substrings, which is what this reader replaced: the two `or`
    arms still contain the push text, the inverted comparison still contains
    the cache-hit reference, and the extra conjunct adds to a condition in
    which every required term is present.
    """
    assert save_condition_faults(condition, RESTORE_IDS), (
        f"{condition!r} was accepted, but {because}"
    )


@pytest.mark.parametrize(
    ("condition", "expected"),
    [
        pytest.param("a && b", ("a", "b"), id="two-terms"),
        pytest.param("  a  &&\n  b  ", ("a", "b"), id="folded-and-padded"),
        pytest.param("a", ("a",), id="one-term"),
        pytest.param("", ("",), id="nothing"),
        pytest.param("a &&", ("a", ""), id="a-trailing-operator"),
        pytest.param("&& a", ("", "a"), id="a-leading-operator"),
        pytest.param("a && && b", ("a", "", "b"), id="a-doubled-operator"),
    ],
)
def test_the_split_keeps_the_terms_it_is_given(
    condition: str, expected: tuple[str, ...]
) -> None:
    """The split has to survive the layouts YAML folding produces.

    A split that dropped a term would let the reader report that term as
    missing, and a split that invented one would report it as an extra; both
    read as a policy failure where the condition is fine.

    The empty terms are kept on purpose. Dropping them is what let a stray
    operator through: `a && && b` would have yielded the same two terms as
    `a && b`, so the malformed expression compared equal to the well-formed
    one and the exact-predicate contract accepted it.
    """
    assert conjuncts(condition) == expected


@pytest.mark.parametrize(
    "stray",
    [
        pytest.param(
            f"{WRITING_LEG} && {PUSH_EVENT} && && {MAIN_REF} && {CACHE_MISS}",
            id="doubled-between-two-terms",
        ),
        pytest.param(
            f"{WRITING_LEG} && {PUSH_EVENT} && {MAIN_REF} && {CACHE_MISS} &&",
            id="trailing",
        ),
        pytest.param(
            f"&& {WRITING_LEG} && {PUSH_EVENT} && {MAIN_REF} && {CACHE_MISS}",
            id="leading",
        ),
    ],
)
def test_a_stray_operator_is_refused(stray: str) -> None:
    """A malformed conjunction is not the approved predicate either.

    Each of these carries all four approved conjuncts and nothing else, so a
    reader that discarded the empty piece would find the exact conjunct set
    it wanted and accept an expression GitHub does not evaluate.

    The fault must name the stray operator, not merely exist. The empty piece
    also reads as an extra term, so a bare "some fault" assertion passed with
    the stray-operator check deleted, while the report blamed a term nobody
    wrote.
    """
    faults = save_condition_faults(stray, RESTORE_IDS)
    assert any("stray" in fault for fault in faults), (
        f"{stray!r} should be refused for its stray operator, so it is not an "
        f"expression the approved predicate can be read out of; got {faults}"
    )

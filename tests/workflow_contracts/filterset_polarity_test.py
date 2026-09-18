"""What a filterset selects, as against what it mentions.

An override's ``filter`` decides which tests receive its budget, and a
reading that collected the names inside ``binary(...)`` terms without
regard to negation answered a different question. ``not binary(x)``
mentions ``x`` and selects the opposite of it, so such an override was
reported as granting ``x`` an allowance nextest never applies: the
binary would run under the base allowance sized for the ordinary tests
while `compile_contract_budget_test` certified it.

This repository's overrides carry no negation, so the tree cannot tell
a correct reading from the old one. These cases are controlled.

Run via ``make test-workflow-contracts``.
"""

import pytest
from nextest_config import (
    NextestConfigurationError,
    Profile,
    binaries_named,
    binaries_selected,
    profiles,
)
from timeout_budgets import (
    COMPILE_CONTRACT_ALLOWANCE_SECONDS,
    allowance_for_binary,
    binaries_short_of_allowance,
)

#: A profile whose single override grants the longer budget to
#: everything *except* the compile-contract binary, which is the shape
#: the old reading could not tell from the shape that grants it.
NEGATED_OVERRIDE = (
    "[profile.example]\n"
    'slow-timeout = { period = "300s", terminate-after = 1, '
    'grace-period = "5s" }\n'
    'global-timeout = "30m"\n'
    "\n[[profile.example.overrides]]\n"
    "filter = 'not binary(trybuild)'\n"
    'slow-timeout = { period = "900s", terminate-after = 1, '
    'grace-period = "5s" }\n'
)

#: The same profile with the negation removed, so the two differ in one
#: token and in nothing else.
POSITIVE_OVERRIDE = NEGATED_OVERRIDE.replace(
    "filter = 'not binary(trybuild)'", "filter = 'binary(trybuild)'"
)


def _example(config_text: str) -> Profile:
    """Return the ``example`` profile parsed out of a document.

    Parameters
    ----------
    config_text
        A nextest configuration's text.

    Returns
    -------
    Profile
        The profile named ``example``.
    """
    return profiles(config_text)["example"]


@pytest.mark.parametrize(
    ("filterset", "expected"),
    [
        pytest.param("binary(a) | binary(b)", {"a", "b"}, id="two-positive-terms"),
        pytest.param("not binary(a)", set(), id="a-single-negated-term"),
        pytest.param("binary(a) & not binary(b)", {"a"}, id="one-of-each"),
        pytest.param(
            "not  binary(a) | binary(b)",
            {"b"},
            id="a-negation-spaced-unusually",
        ),
        pytest.param("test(slow) & binary(a)", {"a"}, id="a-non-binary-predicate"),
        pytest.param(
            "binary(a) & not test(slow)",
            {"a"},
            id="a-negated-non-binary-predicate",
        ),
        pytest.param("!binary(a)", set(), id="a-bang-negation"),
        pytest.param("! binary(a)", set(), id="a-bang-negation-spaced"),
        pytest.param("binary(a) & !binary(b)", {"a"}, id="a-bang-in-a-conjunction"),
        pytest.param(
            "binary(a) - binary(b)",
            {"a"},
            id="a-difference-spaced",
        ),
        pytest.param(
            "binary(a)-binary(b)",
            {"a"},
            id="a-difference-unspaced",
        ),
        pytest.param(
            "nothing(a) | binary(b)",
            {"b"},
            id="a-predicate-merely-beginning-with-not",
        ),
    ],
)
def test_the_selector_honours_negation(filterset: str, expected: set[str]) -> None:
    """Selecting and mentioning are different questions.

    `binaries_named` answers which binaries a filterset mentions, which
    is what the exclusion reading wants: ``default-filter`` says
    ``not binary(trybuild)`` and the question there is whether the
    binary is named under a negation. `binaries_selected` answers which
    it selects, which is what an allowance depends on.

    A negation applied to something other than a ``binary(...)`` term
    leaves the binaries' polarity alone, so it is read rather than
    refused: ``binary(a) & not test(slow)`` still gives the tests of
    ``a`` that it matches the override's budget.
    """
    assert binaries_selected(filterset) == expected, (
        f"{filterset!r} selects {sorted(expected)}"
    )


def test_the_negation_matcher_keys_on_the_word_not_the_letters() -> None:
    """Pin the word boundary, which no realistic filterset exercises.

    Written against the matcher rather than against a filterset nextest
    would accept, and said plainly because the distinction matters: the
    input below is not valid nextest syntax. The subject is the pattern,
    and the property is that it keys on the operator `not` rather than
    on three letters that happen to end another token.

    Without the boundary and the required space, `not\\s*binary\\(` matches
    inside `cannotbinary(a)` and reports `a` as excluded. That is a
    negation the filterset never wrote. The case exists because dropping
    the boundary otherwise passes every other test in this module, and a
    guard nothing can fail is a guard nobody is holding.
    """
    assert binaries_selected("cannotbinary(a) | binary(b)") == frozenset({"a", "b"}), (
        "the letters `not` ending another token are not a negation operator"
    )


def test_a_binary_both_selected_and_excluded_is_refused() -> None:
    """Which occurrence wins depends on grouping, which is not matched.

    ``binary(a) | binary(b) - binary(b)`` is ``binary(a)`` to nextest,
    because the difference binds tighter than the union. A reader that
    strips negated terms and then collects what is left reports ``b`` as
    selected, and an override's allowance would be granted to a binary
    the filterset excludes. Refusing is the honest answer for the same
    reason a negated group is refused.
    """
    with pytest.raises(NextestConfigurationError, match=r"both selects and excludes"):
        binaries_selected("binary(a) | binary(b) - binary(b)")


@pytest.mark.parametrize(
    "filterset",
    [
        pytest.param("binary(a) - not binary(b)", id="a-difference-of-a-negation"),
        pytest.param("binary(a) - !binary(b)", id="a-difference-of-a-bang"),
        pytest.param("binary(a) - ! binary(b)", id="a-difference-of-a-spaced-bang"),
        pytest.param("not not binary(a)", id="the-word-not-twice"),
        pytest.param("!!binary(a)", id="a-bang-twice"),
    ],
)
def test_a_negation_of_a_negation_is_refused(filterset: str) -> None:
    """The pair cancels, and this reader has no operator context to cancel it with.

    nextest binds `not` tighter than `-`, so `binary(a) - not binary(b)`
    is `binary(a) and not (not binary(b))`, which is
    `binary(a) and binary(b)`. When the two names differ that selects
    nothing at all.

    A reader that matches terms sees only the inner negation: it
    excludes `b`, strips the term, and reports `a` as selected. That is
    the dangerous direction rather than the safe one. Reporting `a`
    grants it an override's allowance nextest never applies, so the
    binary runs under the base allowance while this contract certifies
    it has the longer one, which is exactly the failure
    `binaries_selected` exists to prevent.

    Every spelling of the pair is covered, because a refusal that knew
    only `- not` would let `- !` through, and the two mean the same
    thing to nextest.
    """
    with pytest.raises(NextestConfigurationError, match=r"one negation to another"):
        binaries_selected(filterset)


@pytest.mark.parametrize(
    ("filterset", "expected"),
    [
        pytest.param("binary(a) - binary(b)", {"a"}, id="a-single-difference"),
        pytest.param("binary(a) & not binary(b)", {"a"}, id="a-single-word-negation"),
        pytest.param("nothing(a) | binary(b)", {"b"}, id="a-predicate-starting-notlike"),
        pytest.param("binary(a-b) | binary(c)", {"a-b", "c"}, id="a-hyphenated-name"),
    ],
)
def test_the_double_negation_refusal_is_narrow(
    filterset: str, expected: set[str]
) -> None:
    """Assert the refusal above is narrow as well as sufficient.

    The pattern is two negations adjacent, and `-` is one of the three
    spellings, so a careless version of it would fire on a single
    difference, on a hyphen inside a binary name, or on a predicate that
    merely begins with the letters `not`. A refusal that refused
    everything would satisfy the test above and break every filterset
    this repository actually writes.
    """
    assert binaries_selected(filterset) == expected, (
        f"{filterset!r} carries no double negation and selects "
        f"{sorted(expected)}"
    )


@pytest.mark.parametrize(
    "filterset",
    [
        pytest.param("not (binary(a) | binary(b))", id="the-word-not"),
        pytest.param("!(binary(a) | binary(b))", id="a-bang"),
        pytest.param("binary(c) - (binary(a) | binary(b))", id="a-difference"),
    ],
)
def test_every_negation_spelling_refuses_a_group(filterset: str) -> None:
    """A group is unattributable however the negation is written.

    The refusal exists because attributing ``not (binary(a) |
    binary(b))`` needs the filterset evaluated rather than its terms
    matched. That is true of `!` and `-` too, and a refusal that knew
    only the word `not` would silently report the group's binaries as
    selected under the other two spellings.
    """
    with pytest.raises(NextestConfigurationError):
        binaries_selected(filterset)


def test_a_negated_group_is_refused_rather_than_guessed() -> None:
    """A filterset this reading cannot attribute must not be read.

    ``not (binary(a) | binary(b))`` needs the expression evaluated
    rather than its terms matched, and the two binaries inside it are
    selected by neither this filterset nor its negation in any way a
    term match can recover. Reporting an allowance for them would
    certify a budget nextest does not apply; reporting none would demand
    an allowance of a binary the override may already cover. Neither is
    an answer, so the reading refuses.
    """
    with pytest.raises(NextestConfigurationError):
        binaries_selected("not (binary(a) | binary(b))")


def test_a_negated_override_grants_the_binary_nothing() -> None:
    """The case the old reading could not tell from its opposite.

    These two profiles differ in the token ``not`` and in nothing else.
    Under the positive filterset the override grants ``trybuild`` the
    compile-contract allowance; under the negated one nextest applies it
    to everything but ``trybuild``, which then runs under the 300 s base
    allowance sized for the ordinary tests. A reading that collected
    names reported 900 s for both.
    """
    positive = allowance_for_binary(_example(POSITIVE_OVERRIDE), "trybuild")
    negated = allowance_for_binary(_example(NEGATED_OVERRIDE), "trybuild")
    assert positive == COMPILE_CONTRACT_ALLOWANCE_SECONDS, (
        f"the positive override grants the longer budget, got {positive}"
    )
    assert negated is None, (
        f"the negated override grants trybuild nothing, so its base allowance "
        f"governs; got {negated}"
    )


def test_the_allowance_sweep_reports_a_binary_a_negation_excludes() -> None:
    """And the sweep above it must report that binary, not skip it.

    `binaries_short_of_allowance` is what `compile_contract_budget_test`
    asserts over. With the old reading the negated override satisfied it
    silently, which is the failure this contract exists to make
    impossible: every value sat in the right order while the binary that
    drives `rustc` ran under a bound chosen for a unit test.
    """
    short = binaries_short_of_allowance(
        _example(NEGATED_OVERRIDE), frozenset({"trybuild"})
    )
    assert short == {"trybuild": None}, (
        f"a binary the override's negation excludes is short of the allowance; "
        f"got {short}"
    )
    assert not binaries_short_of_allowance(
        _example(POSITIVE_OVERRIDE), frozenset({"trybuild"})
    ), "the positive override satisfies the same sweep"


def test_mentioning_and_selecting_disagree_on_the_negated_filterset() -> None:
    """Both readings are kept, and they are not interchangeable.

    The exclusion reading wants mentions and the allowance reading wants
    selections. Pinning the disagreement here stops a later change
    collapsing the two into one function that is wrong for one caller.
    """
    assert binaries_named("not binary(trybuild)") == frozenset({"trybuild"}), (
        "the filterset mentions the binary"
    )
    assert binaries_selected("not binary(trybuild)") == frozenset(), (
        "and selects the opposite of it"
    )

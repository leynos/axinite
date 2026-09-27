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

import typing as typ

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
    excluded_from,
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


#: A profile whose two overrides both select the compile-contract
#: binary and grant different allowances. nextest applies the first and
#: stops, so the second's 900 s is never in force. Written with the
#: shorter budget first, because that is the ordering a reading taking
#: the maximum gets wrong; with the longer one first the two readings
#: agree and the case would discriminate nothing.
OVERLAPPING_OVERRIDES = (
    "[profile.example]\n"
    'slow-timeout = { period = "300s", terminate-after = 1, '
    'grace-period = "5s" }\n'
    'global-timeout = "30m"\n'
    "\n[[profile.example.overrides]]\n"
    "filter = 'binary(trybuild)'\n"
    'slow-timeout = { period = "450s", terminate-after = 1, '
    'grace-period = "5s" }\n'
    "\n[[profile.example.overrides]]\n"
    "filter = 'binary(trybuild)'\n"
    'slow-timeout = { period = "900s", terminate-after = 1, '
    'grace-period = "5s" }\n'
)

#: What the first of those two overrides grants, in seconds.
FIRST_OVERLAPPING_ALLOWANCE = 450.0


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


#: A profile whose ``default-filter`` removes the compile-contract
#: binary, once per negation cargo-nextest accepts. The word ``not`` is
#: the spelling the old text match knew; ``!`` and ``-`` are the two it
#: did not, and under those the binary read as *run* rather than
#: excluded.
EXCLUDING_FILTERS: typ.Final[dict[str, str]] = {
    "the-word-not": "not binary(trybuild)",
    "the-bang-prefix": "!binary(trybuild)",
    "set-difference": "all() - binary(trybuild)",
}


def _excluding(default_filter: str) -> Profile:
    """Return an ``example`` profile whose default filter is given.

    Parameters
    ----------
    default_filter
        The ``default-filter`` value to declare.

    Returns
    -------
    Profile
        The profile named ``example``.
    """
    return _example(
        "[profile.example]\n"
        f"default-filter = '{default_filter}'\n"
        'slow-timeout = { period = "300s", terminate-after = 1, '
        'grace-period = "5s" }\n'
        'global-timeout = "30m"\n'
    )


@pytest.mark.parametrize(
    "spelling", sorted(EXCLUDING_FILTERS), ids=sorted(EXCLUDING_FILTERS)
)
def test_every_negation_spelling_excludes_the_binary(spelling: str) -> None:
    """Exclusion must know every negation the allowance reading knows.

    `excluded_from` matched the literal text ``not binary(x)``, so a
    `default-filter` written with ``!`` or ``-`` reported the binary as
    run. `binaries_short_of_allowance` then demanded an allowance for a
    binary the profile never runs and failed a configuration that is
    valid, which is the loud half of the fault. The quiet half is that
    the two readings in this module disagreed about the same filterset.
    """
    profile = _excluding(EXCLUDING_FILTERS[spelling])
    assert excluded_from(profile, "trybuild"), (
        f"a default-filter of {EXCLUDING_FILTERS[spelling]!r} removes "
        f"binary(trybuild), so the profile never runs it"
    )
    assert binaries_short_of_allowance(profile, frozenset({"trybuild"})) == {}, (
        "a binary the profile never runs needs no allowance, so the sweep "
        "must not report it as short of one"
    )


def test_a_filter_that_selects_the_binary_does_not_exclude_it() -> None:
    """Assert the exclusion reading is narrow as well as sufficient.

    A reading that answered True for every filterset naming the binary
    would satisfy all three cases above and silence the allowance sweep
    everywhere, which is the failure that reads as success. Exclusion is
    "named but not selected", and this is the half that pins the second
    clause.
    """
    profile = _excluding("binary(trybuild)")
    assert not excluded_from(profile, "trybuild"), (
        "a default-filter that selects binary(trybuild) runs it, so the "
        "profile does not exclude it"
    )


def test_a_filter_naming_nothing_excludes_nothing() -> None:
    """And a filterset that never mentions the binary excludes nothing.

    The first clause of "named but not selected" is what this pins: a
    reading that dropped it would call every unmentioned binary excluded
    and skip the allowance assertion for the whole tree.
    """
    profile = _excluding("all()")
    assert not excluded_from(profile, "trybuild"), (
        "a default-filter that does not name binary(trybuild) says nothing "
        "about it, so it does not exclude it"
    )


def test_the_first_matching_override_governs_not_the_largest() -> None:
    """Two overrides select the binary; only the first is ever applied.

    nextest reads a profile's overrides in order and applies the first
    whose filterset selects a test, so a later override granting more is
    never in force. A reading that took the maximum would report the
    900 s here and certify a compile-contract binary under a budget
    nextest does not apply, which is the same class of silent pass as
    the negated override above: every number in the right order while
    the binary runs under something else.

    The repository's own overrides all grant 900 s, so nothing in the
    estate distinguishes the two readings. This is the case that does.
    """
    allowed = allowance_for_binary(_example(OVERLAPPING_OVERRIDES), "trybuild")
    assert allowed == FIRST_OVERLAPPING_ALLOWANCE, (
        f"the first override selecting the binary grants "
        f"{FIRST_OVERLAPPING_ALLOWANCE} s and nextest stops there; got "
        f"{allowed}"
    )
    assert allowed != COMPILE_CONTRACT_ALLOWANCE_SECONDS, (
        f"the second override's {COMPILE_CONTRACT_ALLOWANCE_SECONDS} s is "
        f"never applied, so a reading returning it is reporting a budget "
        f"nextest does not use"
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

"""Which overrides a profile inherits, and from how far up the chain.

nextest consults an ancestor's ``[[overrides]]`` for whichever profile
is selected, after that profile's own. A profile may name a parent
other than ``default`` with ``inherits``, and that parent may name a
third, so the chain has to be walked. A reading that copied
``default``'s overrides alone would miss the middle of a chain: with
``ci-extended -> ci -> default``, ``ci``'s override would be invisible,
and the ordering contract could then approve a whole-run budget below
the per-test allowance actually in force.

This repository declares no ``inherits``, so its own files cannot tell
a correct walk from the old one-step copy. Every case here is
controlled.

Run via ``make test-workflow-contracts``.
"""

import pytest
from nextest_config import NextestConfigurationError, profiles
from timeout_budgets import largest_test_allowance

#: A three-level chain. Each level declares one override with a period
#: of its own, so which levels were consulted is visible in the result.
CHAIN = (
    "[profile.default]\n"
    'slow-timeout = { period = "300s", terminate-after = 1 }\n'
    'global-timeout = "30m"\n'
    "\n[[profile.default.overrides]]\n"
    "filter = 'binary(from_default)'\n"
    'slow-timeout = { period = "400s", terminate-after = 1 }\n'
    "\n[profile.ci]\n"
    'slow-timeout = { period = "300s", terminate-after = 1 }\n'
    'global-timeout = "30m"\n'
    "\n[[profile.ci.overrides]]\n"
    "filter = 'binary(from_ci)'\n"
    'slow-timeout = { period = "900s", terminate-after = 1 }\n'
    "\n[profile.ci-extended]\n"
    'inherits = "ci"\n'
    'slow-timeout = { period = "300s", terminate-after = 1 }\n'
    'global-timeout = "30m"\n'
    "\n[[profile.ci-extended.overrides]]\n"
    "filter = 'binary(from_extended)'\n"
    'slow-timeout = { period = "500s", terminate-after = 1 }\n'
)


def test_a_chain_is_walked_to_default_rather_than_jumped() -> None:
    """The middle of the chain is the part a one-step copy loses.

    ``ci-extended`` names ``ci`` as its parent and ``ci`` inherits
    ``default``, so nextest consults all three profiles' overrides. The
    periods are distinct, so the result names which levels were read:
    500 s is the profile's own, 900 s is the parent's, 400 s is
    ``default``'s.
    """
    parsed = profiles(CHAIN)
    inherited = [path for path, _ in parsed["ci-extended"].inherited]
    assert inherited == ["profile.ci", "profile.default"], (
        f"the chain is the parent's overrides and then default's, in that "
        f"order, because nextest consults the nearest ancestor first; got "
        f"{inherited}"
    )


def test_the_middle_of_the_chain_governs_the_largest_allowance() -> None:
    """And losing it understates the allowance in force.

    The parent's 900 s override is the largest in the chain. A reading
    that copied ``default``'s overrides alone would report 500 s, the
    profile's own, and the ordering contract would then accept a
    whole-run budget that sits below what nextest actually allows one
    test.
    """
    largest = largest_test_allowance(profiles(CHAIN)["ci-extended"])
    assert largest == 900.0, (
        f"the parent's override is the largest allowance this profile runs "
        f"under; got {largest}"
    )


def test_each_inherited_override_is_reported_where_it_is_written() -> None:
    """A failure has to name the line that has to change.

    An override inherited from ``ci`` is reported against ``profile.ci``
    and not against the profile that inherits it, so a reader of the
    failure goes to the declaration rather than to the inheritor.
    """
    sources = dict.fromkeys(
        path for path, _ in profiles(CHAIN)["ci-extended"].sources()
    )
    assert list(sources) == [
        "profile.ci-extended",
        "profile.ci",
        "profile.default",
    ], f"each table is reported at its declaring profile; got {list(sources)}"


def test_the_default_profile_inherits_nothing() -> None:
    """Its own overrides are already recorded once.

    Reading ``default`` as inheriting from itself would count every one
    of its overrides twice. That changes no maximum, which is exactly
    why it needs asserting rather than observing.
    """
    parsed = profiles(CHAIN)
    assert parsed["default"].inherited == (), (
        f"default has no parent; got {parsed['default'].inherited}"
    )


@pytest.mark.parametrize(
    ("configuration", "reason"),
    [
        pytest.param(
            '[profile.default]\nslow-timeout = { period = "300s", '
            'terminate-after = 1 }\n\n[profile.ci]\ninherits = "absent"\n'
            'slow-timeout = { period = "300s", terminate-after = 1 }\n',
            "a parent the file does not declare",
            id="a-missing-parent",
        ),
        pytest.param(
            '[profile.default]\nslow-timeout = { period = "300s", '
            'terminate-after = 1 }\n\n[profile.a]\ninherits = "b"\n'
            'slow-timeout = { period = "300s", terminate-after = 1 }\n'
            '\n[profile.b]\ninherits = "a"\n'
            'slow-timeout = { period = "300s", terminate-after = 1 }\n',
            "a cycle",
            id="a-cycle",
        ),
        pytest.param(
            '[profile.default]\nslow-timeout = { period = "300s", '
            "terminate-after = 1 }\n\n[profile.ci]\ninherits = 7\n"
            'slow-timeout = { period = "300s", terminate-after = 1 }\n',
            "an inherits that is not a profile name",
            id="a-non-string-parent",
        ),
    ],
)
def test_a_chain_that_cannot_be_walked_is_refused(
    configuration: str, reason: str
) -> None:
    """nextest refuses these files, so no budget can be read from them.

    Each would otherwise be read as a profile inheriting nothing, which
    is the dangerous direction: the profile would appear to run under
    its own overrides alone and the contract would certify budgets for a
    configuration the runner will not load. The cycle is the sharpest,
    because walking it without a guard does not return at all.
    """
    with pytest.raises(NextestConfigurationError):
        profiles(configuration)

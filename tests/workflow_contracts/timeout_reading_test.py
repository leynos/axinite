"""How the timeout contract reads the files it compares.

Every assertion in ``timeout_ordering_test`` rests on turning the
nextest profiles into comparable seconds. Those readings can be wrong
while no file is wrong, and this repository's own configuration cannot
expose most of the ways they can be, so they are driven with controlled
values here.
"""

import pytest
from suite_lanes import COVERAGE_ACTION, WATCHDOG_VARIABLE, _watchdog_offences
from timeout_budgets import (
    NEXTEST_DEFAULT_GRACE_PERIOD_SECONDS,
    TERMINATION_SAFETY_MARGIN_SECONDS,
    base_slow_timeout,
    termination_allowance,
)


def test_the_base_allowance_is_read_from_the_profile_not_its_overrides() -> None:
    """The first inline table is the profile's; the rest are overrides'.

    A reader that took the largest table, or the last, would report an
    override's allowance as the base. The base is the one that governs
    every test no override names, so the substitution would leave the
    ordinary tests unbounded while the contract passed. Driven with a
    controlled profile because this repository's own base and override
    both set `terminate-after`, so a confused reader would agree with a
    correct one against the real file.
    """
    block = (
        "[profile.example]\n"
        'slow-timeout = { period = "300s", terminate-after = 1 }\n'
        "\n"
        "[[profile.example.overrides]]\n"
        "filter = 'binary(trybuild)'\n"
        'slow-timeout = { period = "900s", terminate-after = 4 }\n'
    )
    assert base_slow_timeout(block) == {"period": "300s", "terminate-after": "1"}, (
        "the base slow-timeout must come from the profile's own section"
    )


def test_a_profile_declaring_no_base_allowance_reads_as_empty() -> None:
    """An override alone is not a base allowance.

    nextest would fall back to `[profile.default]` here, which is why
    the contract states this as repository policy rather than as a
    nextest requirement. The reading still has to distinguish the two
    cases, or the policy cannot be enforced.
    """
    block = (
        "[profile.example]\n"
        "default-filter = 'all()'\n"
        "\n"
        "[[profile.example.overrides]]\n"
        "filter = 'binary(trybuild)'\n"
        'slow-timeout = { period = "900s", terminate-after = 1 }\n'
    )
    assert base_slow_timeout(block) == {}, (
        "a profile whose only slow-timeout is an override's declares no base"
    )


def test_the_termination_allowance_is_the_grace_period_plus_the_margin() -> None:
    """The two terms are added, not maximized over.

    A single floor over the grace period and the margin would absorb
    every grace period below the margin, so raising this file's five
    seconds to thirty would demand nothing more of the job ceiling above
    it. The ordering assertions cannot tell the readings apart, since
    both leave the requirement inside the ceiling, which is why the
    reading carries a test of its own.
    """
    assert termination_allowance("") == pytest.approx(
        NEXTEST_DEFAULT_GRACE_PERIOD_SECONDS + TERMINATION_SAFETY_MARGIN_SECONDS
    ), "an unnamed grace period must fall back to nextest's own default"
    configured = termination_allowance(
        'slow-timeout = { period = "300s", grace-period = "5s" }'
    )
    assert configured == pytest.approx(5.0 + TERMINATION_SAFETY_MARGIN_SECONDS), (
        "a grace period below the margin must still raise the allowance; "
        "a maximum over the two terms would have discarded it"
    )
    largest = termination_allowance(
        'slow-timeout = { grace-period = "5s" }\n'
        'slow-timeout = { grace-period = "45s" }'
    )
    assert largest == pytest.approx(45.0 + TERMINATION_SAFETY_MARGIN_SECONDS), (
        "the largest grace period in the profile governs the allowance"
    )


@pytest.mark.parametrize(
    ("step", "expected"),
    [
        pytest.param({"run": "cargo llvm-cov nextest run"}, 0, id="an-ordinary-step"),
        pytest.param({"uses": f"{COVERAGE_ACTION}@abc123"}, 1, id="adopts-the-action"),
        pytest.param(
            {"run": "make test", "env": {WATCHDOG_VARIABLE: "1800"}},
            1,
            id="names-the-variable",
        ),
        pytest.param(
            {"uses": f"{COVERAGE_ACTION}@abc123", "env": {WATCHDOG_VARIABLE: "1800"}},
            2,
            id="both-at-once",
        ),
    ],
)
def test_both_halves_of_the_absent_tier_are_detected(
    step: dict[str, object], expected: int
) -> None:
    """Adopting the action and naming the variable are separate offences.

    The tier is absent by construction here, so no workflow in the tree
    commits either offence and the assertion over the tree is satisfied
    by a reading that detects neither. Driving the reading directly is
    the only way to show it would notice.
    """
    offences = _watchdog_offences("ci.yml", "test", step)
    assert len(offences) == expected, (
        f"{step} must yield {expected} offence(s), got {offences}"
    )

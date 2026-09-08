"""How the timeout contract reads the files it compares.

Every assertion in ``timeout_ordering_test`` rests on turning the
nextest profiles into comparable seconds. Those readings can be wrong
while no file is wrong, and this repository's own configuration cannot
expose most of the ways they can be, so they are driven with controlled
values here.
"""

import pytest
from nextest_config import (
    NextestConfigurationError,
    Profile,
    UnboundedTestError,
    profiles,
    seconds,
)
from timeout_budgets import (
    NEXTEST_DEFAULT_GRACE_PERIOD_SECONDS,
    TERMINATION_SAFETY_MARGIN_SECONDS,
    base_slow_timeout,
    global_timeout,
    largest_test_allowance,
    termination_allowance,
)


def example(config_text: str) -> Profile:
    """Return the ``example`` profile parsed out of a document.

    Parameters
    ----------
    config_text
        A nextest configuration document declaring ``[profile.example]``.

    Returns
    -------
    Profile
        The parsed profile.
    """
    return profiles(config_text)["example"]


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
    assert base_slow_timeout(example(block)) == {
        "period": "300s",
        "terminate-after": "1",
    }, "the base slow-timeout must come from the profile's own section"


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
    assert base_slow_timeout(example(block)) == {}, (
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
    unset = example(
        '[profile.example]\nslow-timeout = { period = "300s", terminate-after = 1 }\n'
    )
    assert termination_allowance(unset) == pytest.approx(
        NEXTEST_DEFAULT_GRACE_PERIOD_SECONDS + TERMINATION_SAFETY_MARGIN_SECONDS
    ), "an unnamed grace period must fall back to nextest's own default"
    configured = termination_allowance(
        example(
            "[profile.example]\n"
            'slow-timeout = { period = "300s", terminate-after = 1, '
            'grace-period = "5s" }\n'
        )
    )
    assert configured == pytest.approx(5.0 + TERMINATION_SAFETY_MARGIN_SECONDS), (
        "a grace period below the margin must still raise the allowance; "
        "a maximum over the two terms would have discarded it"
    )
    largest = termination_allowance(
        example(
            "[profile.example]\n"
            'slow-timeout = { period = "300s", terminate-after = 1, '
            'grace-period = "5s" }\n'
            "\n[[profile.example.overrides]]\n"
            "filter = 'binary(trybuild)'\n"
            'slow-timeout = { period = "300s", terminate-after = 1, '
            'grace-period = "45s" }\n'
        )
    )
    assert largest == pytest.approx(45.0 + TERMINATION_SAFETY_MARGIN_SECONDS), (
        "the largest grace period in the profile governs the allowance"
    )


def test_a_commented_out_global_timeout_is_absent() -> None:
    """Tier two must read as missing when it has been switched off.

    This is the reading the contract's presence assertion rests on. A
    text match would keep reporting the budget from the comment, so the
    tier could be commented out and the four-tier contract would go on
    passing with three.
    """
    with pytest.raises(NextestConfigurationError, match=r"global-timeout"):
        global_timeout(example('[profile.example]\n# global-timeout = "30m"\n'))
    live = example('[profile.example]\nglobal-timeout = "30m"\n')
    assert global_timeout(live) == pytest.approx(1800.0)


def test_a_commented_out_slow_timeout_is_not_a_budget() -> None:
    """A comment is not configuration, and TOML is what says so.

    A reader that scraped the text would report an allowance from a line
    nextest never reads, so deleting the live entry and leaving the
    comment behind would look like a change of value rather than the
    loss of a tier.
    """
    parsed = example(
        "[profile.example]\n"
        '# slow-timeout = { period = "30m", terminate-after = 1 }\n'
        'slow-timeout = { period = "300s", terminate-after = 1 }\n'
    )
    assert largest_test_allowance(parsed) == pytest.approx(300.0)
    with pytest.raises(NextestConfigurationError, match=r"no slow-timeout"):
        largest_test_allowance(
            example(
                "[profile.example]\n"
                '# slow-timeout = { period = "300s", terminate-after = 1 }\n'
            )
        )


def test_a_commented_out_grace_period_is_not_in_force() -> None:
    """The grace period is a term of the ceiling requirement.

    A scraped comment would raise the termination allowance and with it
    the ceiling this contract demands, so the file would appear to ask
    more of the tier above it than nextest actually does.
    """
    parsed = example(
        "[profile.example]\n"
        '# slow-timeout = { period = "300s", terminate-after = 1, '
        'grace-period = "30m" }\n'
        'slow-timeout = { period = "300s", terminate-after = 1, '
        'grace-period = "5s" }\n'
    )
    assert termination_allowance(parsed) == pytest.approx(
        5.0 + TERMINATION_SAFETY_MARGIN_SECONDS
    )


def test_a_filter_naming_a_timeout_key_is_not_a_budget() -> None:
    """An override's ``filter`` is a string, not configuration.

    A binary named after one of these keys would be matched by a text
    search and read as a budget nextest never applies.
    """
    parsed = example(
        "[profile.example]\n"
        'slow-timeout = { period = "300s", terminate-after = 1 }\n'
        'global-timeout = "30m"\n'
        "\n[[profile.example.overrides]]\n"
        "filter = 'binary(global_timeout_probe) | binary(grace_period_probe)'\n"
        'slow-timeout = { period = "600s", terminate-after = 1 }\n'
    )
    assert largest_test_allowance(parsed) == pytest.approx(600.0)
    assert global_timeout(parsed) == pytest.approx(1800.0)
    assert termination_allowance(parsed) == pytest.approx(
        NEXTEST_DEFAULT_GRACE_PERIOD_SECONDS + TERMINATION_SAFETY_MARGIN_SECONDS
    )


@pytest.mark.parametrize(
    "table",
    [
        pytest.param('slow-timeout = "300s"', id="a-bare-duration"),
        pytest.param(
            'slow-timeout = { period = "300s" }',
            id="a-table-without-terminate-after",
        ),
    ],
)
def test_a_slow_timeout_that_never_terminates_is_refused(table: str) -> None:
    """``terminate-after`` is optional, and without it nothing is bounded.

    nextest marks the test slow, warns once per period, and lets it run
    on. Reading such a configuration as a period-long budget would put a
    number on the tier that is missing. Every table in
    ``.config/nextest.toml`` sets it explicitly, so nothing here relies
    on the looser reading.
    """
    with pytest.raises(UnboundedTestError, match=r"terminate-after"):
        largest_test_allowance(example(f"[profile.example]\n{table}\n"))


@pytest.mark.parametrize(
    ("duration", "expected"),
    [
        pytest.param("300s", 300.0, id="one-component"),
        pytest.param("2h 37m", 9420.0, id="two-components-spaced"),
        pytest.param("2h37m", 9420.0, id="two-components-joined"),
        pytest.param("1h 30m 15s", 5415.0, id="three-components"),
        pytest.param("15min", 900.0, id="a-long-unit-spelling"),
        pytest.param("500ms", 0.5, id="milliseconds"),
        pytest.param("1d", 86400.0, id="days"),
    ],
)
def test_the_duration_grammar_matches_the_one_nextest_reads(
    duration: str, expected: float
) -> None:
    """nextest deserializes durations with ``humantime``, not one unit.

    A reader accepting a single component rejects `2h 37m`, which
    nextest accepts, so the contract would fail on a configuration that
    is correct and the failure would name the reader's limitation as
    though it were the file's fault. Every spelling here is one
    ``humantime`` accepts.
    """
    assert seconds(duration) == pytest.approx(expected), (
        f"{duration!r} must read as {expected} seconds"
    )


@pytest.mark.parametrize(
    "duration",
    [
        pytest.param("1.5s", id="a-fractional-value"),
        pytest.param("300", id="no-unit"),
        pytest.param("300 fortnights", id="an-unknown-unit"),
        pytest.param("", id="empty"),
    ],
)
def test_a_duration_nextest_would_refuse_is_refused_here(duration: str) -> None:
    """The grammar is matched, not merely widened.

    ``humantime`` takes whole numbers with units and nothing else, so a
    reader that accepted more would put a number on a configuration
    nextest fails to load, and the contract would certify a file that
    cannot run.
    """
    with pytest.raises(NextestConfigurationError):
        seconds(duration)


def test_minutes_and_months_are_told_apart() -> None:
    """``m`` is minutes and ``M`` is months, and ``humantime`` is exact.

    Folding case here would read a thirty-minute budget as a
    two-and-a-half-year one, or the reverse, and either reading puts a
    plausible number on the wrong tier.
    """
    assert seconds("30m") == pytest.approx(1800.0)
    assert seconds("30M") == pytest.approx(30 * 2630016.0)


def test_a_custom_profile_reads_the_default_profile_s_overrides() -> None:
    """nextest consults ``[[profile.default.overrides]]`` for it too.

    An override written on the default profile governs any test the
    selected profile's own overrides do not name, so a reading confined
    to the selected profile understates the allowance in force. The
    understatement is invisible in the ordering assertions, which then
    certify a 3,600 s inherited allowance as sitting under a 1,800 s
    whole-run budget.

    This repository's default profile declares no overrides, so the real
    file cannot tell a correct reading from a confined one.
    """
    parsed = profiles(
        "[profile.default]\n"
        'slow-timeout = { period = "300s", terminate-after = 1, '
        'grace-period = "5s" }\n'
        'global-timeout = "30m"\n'
        "\n[[profile.default.overrides]]\n"
        "filter = 'binary(slow)'\n"
        'slow-timeout = { period = "1800s", terminate-after = 2, '
        'grace-period = "45s" }\n'
        "\n[profile.ci]\n"
        'slow-timeout = { period = "300s", terminate-after = 1, '
        'grace-period = "5s" }\n'
        'global-timeout = "30m"\n'
    )
    assert largest_test_allowance(parsed["ci"]) == pytest.approx(3600.0), (
        "the inherited override is the longest a test may run under ci"
    )
    assert termination_allowance(parsed["ci"]) == pytest.approx(
        45.0 + TERMINATION_SAFETY_MARGIN_SECONDS
    ), "the inherited grace period is the wait nextest takes under ci"
    assert base_slow_timeout(parsed["ci"]) == {
        "period": "300s",
        "terminate-after": "1",
        "grace-period": "5s",
    }, "an inherited override is not the profile's own base allowance"


def test_the_default_profile_does_not_inherit_from_itself() -> None:
    """Its own overrides are already read once.

    Counting them twice would be harmless for a maximum and wrong for
    anything else, and the field is the record of what a profile
    inherits rather than of what it declares.
    """
    parsed = profiles(
        "[profile.default]\n"
        'slow-timeout = { period = "300s", terminate-after = 1 }\n'
        "\n[[profile.default.overrides]]\n"
        "filter = 'binary(slow)'\n"
        'slow-timeout = { period = "600s", terminate-after = 1 }\n'
    )
    assert parsed["default"].inherited == ()
    assert len(parsed["default"].tables()) == 2

"""Reading the nextest budgets this contract compares.

Separated from ``timeout_ordering_test`` so the reading and the
assertions stay legible apart, and so neither module outgrows the
400-line limit ``AGENTS.md`` sets.

The parsing lives in ``nextest_config``; the arithmetic over what it
returns lives here.
"""

import typing as typ

from _workflow_policy import REPOSITORY_ROOT
from nextest_config import (
    NextestConfigurationError,
    Profile,
    _budget_of,
    _slow_timeout,
    seconds,
)

#: Everything the job timer covers that the whole-run budget does not:
#: the checkout, the toolchain probe, the database fixtures, the
#: instrumented build before nextest starts its clock, and the report
#: upload afterwards.
#:
#: Measured from the worst of several runs rather than one. The coverage
#: step reached 900 s on run 33966708901, of which the compile is the
#: larger part, and the work outside the step reached 207 s on the same
#: run, read across six successful runs of `coverage.yml` covering three
#: matrix legs each. Twenty minutes covers the worse of those, and none
#: of those runs was genuinely cold.
OUTSIDE_RUN_ALLOWANCE_SECONDS: typ.Final[float] = 20 * 60.0

#: How far a ceiling must sit above the sum it contains, rather than
#: merely reaching it. A ceiling equal to that sum cancels the job at
#: the moment nextest would have reported the overrun, and the report
#: is the only thing that makes an overrun actionable.
CEILING_MARGIN_SECONDS: typ.Final[float] = 15 * 60.0

#: What nextest allows a test between `SIGTERM` and `SIGKILL` when a
#: profile names no `grace-period`. Both profiles here name five seconds,
#: so this is a fallback rather than the value in force.
NEXTEST_DEFAULT_GRACE_PERIOD_SECONDS: typ.Final[float] = 10.0

#: Added to that grace period to cover the teardown and report writing
#: that follow it. A separate term rather than a floor over the two, so
#: raising a grace period raises the requirement instead of vanishing
#: into it.
TERMINATION_SAFETY_MARGIN_SECONDS: typ.Final[float] = 60.0

NEXTEST_CONFIG = REPOSITORY_ROOT / ".config" / "nextest.toml"


def largest_test_allowance(profile: Profile) -> float:
    """Return the longest a single test may run under one profile.

    nextest warns once per ``period`` and terminates after
    ``terminate-after`` of them, so the budget is their product. Reading
    the period alone would understate an override that raised the
    multiplier rather than the period.

    Read over the overrides nextest consults for this profile, which
    includes ``[[profile.default.overrides]]`` and not only the
    profile's own. A default override matching a test the selected
    profile's overrides do not name governs that test, so omitting them
    understates the effective allowance: an inherited 3,600 s override
    would sit above a 1,800 s whole-run budget and be reported as
    ordered.

    The result is an upper bound rather than the allowance any one test
    receives. Which override governs a test depends on a filterset this
    contract cannot evaluate statically, so the largest is taken; that
    errs towards demanding a whole-run budget above every allowance
    declared, which is the direction the tiers have to hold in.

    Parameters
    ----------
    profile
        The profile to read.

    Returns
    -------
    float
        The longest per-test budget, in seconds.

    Raises
    ------
    NextestConfigurationError
        If the profile declares no ``slow-timeout`` at all.
    """
    budgets = [
        _budget_of(path, value)
        for path, table in profile.sources()
        if (value := _slow_timeout(table)) is not None
    ]
    if not budgets:
        message = (
            f"[profile.{profile.name}] declares no slow-timeout, so no test is "
            f"bounded and there is no per-test tier to compare against"
        )
        raise NextestConfigurationError(message)
    return max(budgets)


def base_slow_timeout(profile: Profile) -> dict[str, str]:
    """Return one profile's own ``slow-timeout``, field by field.

    The base allowance governs every test the profile's overrides do not
    name, so it is read from the profile's own table alone. A profile
    whose only ``terminate-after`` sits in an override leaves every
    unmatched test with no bound at all while the largest allowance
    still reports a comfortable number.

    Parameters
    ----------
    profile
        The profile to read.

    Returns
    -------
    dict of str to str
        The fields of the base ``slow-timeout`` as text, empty when the
        profile declares none of its own.
    """
    table = _slow_timeout(profile.own)
    if not isinstance(table, dict):
        return {}
    return {str(key): str(value) for key, value in table.items()}


def termination_allowance(profile: Profile) -> float:
    """Return the time nextest may take to stop the run, in seconds.

    Two terms, not one. Hitting the global timeout starts nextest's
    ordinary termination procedure rather than stopping the run: on Unix
    it signals the process group and waits ``slow-timeout.grace-period``
    before killing it. That grace period is the first term, read from the
    configuration so a profile that raised it raises the requirement too;
    the second is a fixed margin for the teardown and report writing that
    follow. A single floor over the two would absorb every grace period
    below the margin, making a raised one look free until the run it
    cancelled.

    Read over the same tables as the per-test allowance, so a grace
    period inherited from ``[[profile.default.overrides]]`` raises the
    requirement here as it does the wait in force.

    Parameters
    ----------
    profile
        The profile to read.

    Returns
    -------
    float
        The grace period plus the safety margin.
    """
    periods = [
        seconds(grace)
        for table in profile.tables()
        if isinstance(entry := _slow_timeout(table), dict)
        and isinstance(grace := entry.get("grace-period"), str)
    ]
    grace = max(periods, default=NEXTEST_DEFAULT_GRACE_PERIOD_SECONDS)
    return grace + TERMINATION_SAFETY_MARGIN_SECONDS


def global_timeout(profile: Profile) -> float:
    """Return one profile's whole-run budget in seconds.

    Read from the profile's own table alone: ``global-timeout`` is a
    profile key, and an ``[[overrides]]`` entry cannot carry one.

    Parameters
    ----------
    profile
        The profile to read.

    Returns
    -------
    float
        The whole-run budget.

    Raises
    ------
    NextestConfigurationError
        If the profile declares no ``global-timeout``.
    """
    budget = profile.own.get("global-timeout")
    if not isinstance(budget, str):
        message = (
            f"[profile.{profile.name}] must set global-timeout; without it the "
            f"whole-run budget is unbounded and only the job timer ends a hung "
            f"run, by cancelling it and discarding the log"
        )
        raise NextestConfigurationError(message)
    return seconds(budget)


def required_ceiling(parsed: dict[str, Profile]) -> float:
    """Return the smallest acceptable ceiling for any suite lane.

    Four terms. The whole-run budget is what the suite may spend, the
    termination allowance is what nextest needs to stop it, the outside
    allowance is the work either side that the job timer covers, and
    the margin is added because a ceiling equal to that sum cancels the
    job at the moment nextest would have reported the overrun.

    The larger of the two profiles is taken for each of the first two,
    because a lane passing ``--profile ci`` runs under that one and
    nothing in the workflow names which it uses.

    Parameters
    ----------
    parsed
        Each nextest profile, keyed by name.

    Returns
    -------
    float
        The smallest acceptable ceiling, in seconds.
    """
    return (
        max(global_timeout(profile) for profile in parsed.values())
        + max(termination_allowance(profile) for profile in parsed.values())
        + OUTSIDE_RUN_ALLOWANCE_SECONDS
        + CEILING_MARGIN_SECONDS
    )

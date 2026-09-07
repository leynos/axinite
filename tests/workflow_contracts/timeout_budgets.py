"""Reading the nextest budgets this contract compares.

Separated from ``timeout_ordering_test`` so the reading and the
assertions stay legible apart, and so neither module outgrows the
400-line limit ``AGENTS.md`` sets.
"""

import re
import typing as typ

from _workflow_policy import REPOSITORY_ROOT

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

_DURATION: typ.Final[re.Pattern[str]] = re.compile(
    r"^\s*(?P<value>\d+(?:\.\d+)?)\s*(?P<unit>ms|s|m|h)\s*$"
)

_UNIT_SECONDS: typ.Final[dict[str, float]] = {
    "ms": 0.001,
    "s": 1.0,
    "m": 60.0,
    "h": 3600.0,
}

#: One `slow-timeout` inline table, captured whole so the period and the
#: multiplier that scales it are read together.
_SLOW_TIMEOUT: typ.Final[re.Pattern[str]] = re.compile(
    r"slow-timeout\s*=\s*\{(?P<body>[^}]*)\}"
)

_GRACE_PERIOD: typ.Final[re.Pattern[str]] = re.compile(r'grace-period\s*=\s*"([^"]+)"')

#: One ``key = value`` pair inside a ``slow-timeout`` inline table, with
#: the quotes stripped, so a period and a multiplier read alike.
_FIELD: typ.Final[re.Pattern[str]] = re.compile(r'([a-z-]+)\s*=\s*"?([^,"}]+)"?')


def seconds(duration: str) -> float:
    """Convert a nextest duration to seconds.

    Parameters
    ----------
    duration
        A duration as nextest spells it, such as ``"30m"``.

    Returns
    -------
    float
        The duration in seconds.
    """
    match = _DURATION.match(duration)
    assert match is not None, f"unrecognized nextest duration {duration!r}"
    return float(match["value"]) * _UNIT_SECONDS[match["unit"]]


def profile_blocks(config_text: str) -> dict[str, str]:
    """Return each profile's own text, keyed by profile name.

    Read textually rather than through a TOML parser, because every
    assertion below is about what one profile declares for itself. That
    is deliberately narrower than what nextest would resolve: a custom
    profile inherits ``[profile.default]``, and
    ``[[profile.default.overrides]]`` are consulted for it too. The
    contract holds each profile to stating its own budgets, which is
    repository policy rather than a nextest requirement.

    Parameters
    ----------
    config_text
        The nextest configuration file's text.

    Returns
    -------
    dict of str to str
        Profile name to the text of its section and its overrides.
    """
    blocks: dict[str, list[str]] = {}
    current: str | None = None
    for line in config_text.splitlines(keepends=True):
        header = re.match(r"^\[\[?profile\.([A-Za-z0-9_-]+)", line)
        if header is not None:
            current = header[1]
            blocks.setdefault(current, [])
        elif line.startswith("["):
            current = None
        if current is not None:
            blocks[current].append(line)
    return {name: "".join(lines) for name, lines in blocks.items()}


def largest_test_allowance(block: str) -> float:
    """Return the longest a single test may run under one profile.

    nextest warns once per ``period`` and terminates after
    ``terminate-after`` of them, so the budget is their product. Reading
    the period alone would understate an override that raised the
    multiplier rather than the period.

    Parameters
    ----------
    block
        One profile's text.

    Returns
    -------
    float
        The longest per-test budget, in seconds.
    """
    budgets: list[float] = []
    for match in _SLOW_TIMEOUT.finditer(block):
        body = match["body"]
        period = re.search(r'period\s*=\s*"([^"]+)"', body)
        assert period is not None, f"slow-timeout without a period: {body!r}"
        terminate = re.search(r"terminate-after\s*=\s*(\d+)", body)
        multiplier = 1 if terminate is None else int(terminate[1])
        budgets.append(seconds(period[1]) * multiplier)
    assert budgets, "the profile must set at least one slow-timeout"
    return max(budgets)


def base_slow_timeout(block: str) -> dict[str, str]:
    """Return one profile's own ``slow-timeout``, field by field.

    The base allowance is the one that governs every test the profile's
    overrides do not name, so it is read on its own rather than as part
    of the profile's text. The first inline table in a profile block is
    the profile's own; the tables after it belong to that profile's
    overrides.

    Parameters
    ----------
    block
        One profile's text.

    Returns
    -------
    dict of str to str
        The fields of the base ``slow-timeout``, empty when the profile
        declares none of its own.
    """
    for line in block.splitlines():
        stripped = line.strip()
        if stripped.startswith("[["):
            break
        match = _SLOW_TIMEOUT.match(stripped)
        if match is not None:
            return {key: value.strip() for key, value in _FIELD.findall(match["body"])}
    return {}


def termination_allowance(block: str) -> float:
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

    Parameters
    ----------
    block
        One profile's text.

    Returns
    -------
    float
        The grace period plus the safety margin.
    """
    periods = _GRACE_PERIOD.findall(block)
    grace = max(
        (seconds(period) for period in periods),
        default=NEXTEST_DEFAULT_GRACE_PERIOD_SECONDS,
    )
    return grace + TERMINATION_SAFETY_MARGIN_SECONDS


def global_timeout(block: str) -> float:
    """Return one profile's whole-run budget in seconds.

    Parameters
    ----------
    block
        One profile's text.

    Returns
    -------
    float
        The whole-run budget.
    """
    match = re.search(r'^global-timeout\s*=\s*"([^"]+)"', block, re.MULTILINE)
    assert match is not None, (
        "the profile must set global-timeout; without it the whole-run budget "
        "is unbounded and only the job timer ends a hung run, by cancelling "
        "it and discarding the log"
    )
    return seconds(match[1])


def required_ceiling(profiles: dict[str, str]) -> float:
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
    profiles
        Each nextest profile's text, keyed by name.

    Returns
    -------
    float
        The smallest acceptable ceiling, in seconds.
    """
    return (
        max(global_timeout(block) for block in profiles.values())
        + max(termination_allowance(block) for block in profiles.values())
        + OUTSIDE_RUN_ALLOWANCE_SECONDS
        + CEILING_MARGIN_SECONDS
    )

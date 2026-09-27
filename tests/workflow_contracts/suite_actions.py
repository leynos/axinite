"""The suite runs and the cargo watchdog the shared coverage action brings.

Two lanes run the suite through `leynos/shared-actions`' `generate-coverage`
action rather than from a `run:` line: `coverage.yml`'s `libsql-only` leg,
which writes the ratchet baseline, and `codescene-coverage.yml`'s pull-request
lane. A reading of `run:` lines alone does not see those runs, and the action
adds the third tier, a wall-clock watchdog on each `cargo` invocation that
defaults to 1,800 s. That default equals the 30 m nextest whole-run budget
and sits under the instrumented build as well, so it would kill a slow but
legal run before nextest could report it.

So this module answers three questions, all pure in the parsed workflow. Does
a step run the suite through the action? Which matrix legs does a step run
on, when its `if` names one leg? And what watchdog budget does an action step
run under, reading the input, then the variable at step, job and workflow
scope, as GitHub resolves it?
"""

from __future__ import annotations

import re
import typing as typ

from timeout_budgets import (
    OUTSIDE_RUN_ALLOWANCE_SECONDS,
    global_timeout,
    termination_allowance,
)

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from nextest_config import Profile

#: The action, as a `uses:` substring, pin excluded.
COVERAGE_ACTION: typ.Final[str] = "shared-actions/.github/actions/generate-coverage"

#: The action input that sets the watchdog, and the variable it also reads.
WATCHDOG_INPUT: typ.Final[str] = "cargo-wait-timeout"
WATCHDOG_VARIABLE: typ.Final[str] = "RUN_RUST_CARGO_WAIT_TIMEOUT"

#: A step condition naming one matrix leg, `matrix.name == 'x'` or `!= 'x'`.
#: Anything else is treated as admitting every leg, which counts more runs
#: rather than fewer and so errs towards a larger required ceiling.
_LEG_CONDITION_RE: typ.Final[re.Pattern[str]] = re.compile(
    r"^matrix\.name\s*(?P<operator>==|!=)\s*'(?P<leg>[^']*)'$"
)


def _inputs_of(step: dict[str, object]) -> dict[str, object]:
    """Return a step's `with:` mapping, or an empty one."""
    inputs = step.get("with")
    return inputs if isinstance(inputs, dict) else {}


def uses_coverage_action(step: dict[str, object]) -> bool:
    """Report whether a step calls the shared coverage action."""
    return COVERAGE_ACTION in str(step.get("uses", ""))


def runs_suite_through_action(step: dict[str, object]) -> bool:
    """Report whether a step runs the suite under nextest through the action.

    The action runs `cargo llvm-cov nextest` only when `use-cargo-nextest`
    is true; otherwise it runs `cargo llvm-cov test`, which no nextest budget
    bounds.

    Examples
    --------
    >>> runs_suite_through_action({
    ...     "uses": "leynos/shared-actions/.github/actions/generate-coverage@x",
    ...     "with": {"use-cargo-nextest": "true"},
    ... })
    True
    """
    flag = str(_inputs_of(step).get("use-cargo-nextest", "false")).strip().lower()
    return uses_coverage_action(step) and flag == "true"


def matrix_leg_names(job_body: dict[str, object]) -> tuple[str | None, ...]:
    """Return the `name` of each `include` leg, or `(None,)` without a matrix."""
    strategy = job_body.get("strategy")
    matrix = strategy.get("matrix") if isinstance(strategy, dict) else None
    include = matrix.get("include") if isinstance(matrix, dict) else None
    if not isinstance(include, list):
        return (None,)
    names = tuple(leg.get("name") for leg in include if isinstance(leg, dict))
    return tuple(name if isinstance(name, str) else None for name in names) or (None,)


def admits_leg(condition: object, leg: str | None) -> bool:
    """Report whether a step's `if` lets it run on one matrix leg.

    Examples
    --------
    >>> admits_leg("matrix.name != 'libsql-only'", "libsql-only")
    False
    >>> admits_leg("github.event_name == 'push'", "default")
    True
    """
    match = _LEG_CONDITION_RE.match(" ".join(str(condition or "").split()))
    if match is None or leg is None:
        return True
    return (match["leg"] == leg) == (match["operator"] == "==")


def _env_value(scope: object) -> object:
    """Return the watchdog variable one scope declares, or None."""
    environment = scope.get("env") if isinstance(scope, dict) else None
    return environment.get(WATCHDOG_VARIABLE) if isinstance(environment, dict) else None


def watchdog_budget(
    document: dict[str, object], job_body: dict[str, object], step: dict[str, object]
) -> float | None:
    """Return the watchdog budget, in seconds, an action step runs under.

    The input wins; otherwise the variable is taken from the most specific
    scope that declares it, as GitHub resolves an environment. None means
    nothing sets it and the action's 1,800 s default applies.

    Raises
    ------
    ValueError
        If the value set is not a number of seconds.
    """
    for value in (
        _inputs_of(step).get(WATCHDOG_INPUT),
        _env_value(step),
        _env_value(job_body),
        _env_value(document),
    ):
        if value is not None:
            return float(str(value))
    return None


def required_watchdog(parsed: dict[str, Profile]) -> float:
    """Return the smallest watchdog budget that leaves nextest to report.

    The watchdog times the whole `cargo` invocation, build included, so it
    must cover the instrumented build and the other work outside the test
    window as well as the larger profile's whole-run budget and its
    termination. Anything smaller kills a slow but legal run before nextest
    can name the test that overran.

    Parameters
    ----------
    parsed
        Each nextest profile, keyed by name.

    Returns
    -------
    float
        The smallest acceptable budget, in seconds.
    """
    return (
        max(global_timeout(profile) for profile in parsed.values())
        + max(termination_allowance(profile) for profile in parsed.values())
        + OUTSIDE_RUN_ALLOWANCE_SECONDS
    )

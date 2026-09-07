"""Contract for the timers that can end a test run.

Four independent budgets can end a coverage lane, each set somewhere
different, and they only work if each sits above the one inside it.
Three of the four apply here: a per-test ``slow-timeout`` and a whole-run
``global-timeout`` in ``.config/nextest.toml``, and the job's own
``timeout-minutes``.

The third tier, the shared coverage action's wall-clock watchdog on the
``cargo`` invocation, does not exist here: coverage runs
``cargo llvm-cov nextest`` from a ``run:`` step rather than through that
action. Its absence is asserted rather than assumed, because a lane that
adopted the action without setting ``RUN_RUST_CARGO_WAIT_TIMEOUT`` would
inherit an undocumented 1,800 s default underneath a 30 m nextest
budget, which is the inversion the canonical section exists to prevent.

Two of the four were unset until this contract was written. Nothing
bounded a single test and nothing bounded the run, so the only timer that
ended a hang was the job's own, which cancels the run and discards the
log that would have named the test.

The per-test allowance is ``period`` multiplied by ``terminate-after``,
not ``period`` alone, and the two profiles carry their own overrides
because a profile does not inherit another's.

See "Test timeouts: the tiers this repository sets" in
``docs/developers-guide.md``, and the canonical wording in
`leynos/shared-actions`' `generate-coverage` README.

Run via ``make test-workflow-contracts``.
"""

import re
import typing as typ

import pytest
from _workflow_policy import REPOSITORY_ROOT, jobs_of, load, workflow_paths

#: The environment variable the shared coverage action reads for its
#: wall-clock cap on one `cargo` invocation. Asserted absent: this
#: repository does not use that action.
WATCHDOG_VARIABLE: typ.Final[str] = "RUN_RUST_CARGO_WAIT_TIMEOUT"
COVERAGE_ACTION: typ.Final[str] = "shared-actions/.github/actions/generate-coverage"

#: The commands that run the workspace suite under nextest. A step
#: running one of these is bound by both nextest tiers. Matched as whole
#: tokens on the line, because `cargo nextest --version` is a probe and
#: not a run.
SUITE_MARKERS: typ.Final[tuple[str, ...]] = (
    "cargo llvm-cov nextest",
    "cargo nextest run",
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

#: Floor for the termination allowance, used when a profile sets no grace
#: period. Generous against nextest's ten-second default and far too
#: small to hide a real overrun.
MINIMUM_TERMINATION_ALLOWANCE_SECONDS: typ.Final[float] = 60.0

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
    assertion below must be attached to the profile it belongs to, and a
    profile's overrides are its own: ``[[profile.default.overrides]]``
    does not reach ``[profile.ci]``.

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


def termination_allowance(block: str) -> float:
    """Return the time nextest may take to stop the run, in seconds.

    Hitting the global timeout starts nextest's ordinary termination
    procedure rather than stopping the run: on Unix it signals the
    process group and waits ``slow-timeout.grace-period`` before killing
    it. Read from the configuration so a profile that raised its grace
    period raises the requirement too.

    Parameters
    ----------
    block
        One profile's text.

    Returns
    -------
    float
        The largest configured grace period, or the floor.
    """
    periods = _GRACE_PERIOD.findall(block)
    largest = max((seconds(period) for period in periods), default=0.0)
    return max(largest, MINIMUM_TERMINATION_ALLOWANCE_SECONDS)


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


class SuiteLane(typ.NamedTuple):
    """One job that runs the suite, with the ceiling enclosing it.

    Attributes
    ----------
    workflow
        The workflow file's name.
    job
        The job's identifier.
    ceiling
        The job's ``timeout-minutes`` in seconds, or None when it
        declares none and so inherits GitHub's six-hour default.
    """

    workflow: str
    job: str
    ceiling: float | None

    def __str__(self) -> str:
        """Return a location suitable for a failure message.

        Returns
        -------
        str
            ``workflow:job`` for this lane.
        """
        return f"{self.workflow}:{self.job}"


def _runs_the_suite(job_body: dict[str, object]) -> bool:
    """Return whether a job runs the workspace suite under nextest.

    Parameters
    ----------
    job_body
        The job's parsed mapping.

    Returns
    -------
    bool
        True when a step runs one of :data:`SUITE_MARKERS`.
    """
    for step in job_body.get("steps") or []:
        if not isinstance(step, dict):
            continue
        script = str(step.get("run", ""))
        if any(marker in script for marker in SUITE_MARKERS):
            return True
    return False


@pytest.fixture(scope="module")
def nextest_profiles() -> dict[str, str]:
    """Return each nextest profile's text.

    Returns
    -------
    dict of str to str
        Profile name to its section and overrides.
    """
    return profile_blocks(NEXTEST_CONFIG.read_text(encoding="utf-8"))


@pytest.fixture(scope="module")
def suite_lanes() -> tuple[SuiteLane, ...]:
    """Return every job that runs the suite, with its ceiling.

    Every such job is included, not only those declaring a ceiling, so a
    job that never had one is visible as ``None`` rather than absent. An
    absent entry would let a missing ``timeout-minutes`` pass unremarked.

    Returns
    -------
    tuple of SuiteLane
        One entry per suite-running job.
    """
    lanes: list[SuiteLane] = []
    for path in workflow_paths():
        document = load(path)
        for job in jobs_of(path.name, document):
            body = job.body
            if not isinstance(body, dict) or not _runs_the_suite(body):
                continue
            raw = body.get("timeout-minutes")
            lanes.append(
                SuiteLane(
                    workflow=job.workflow,
                    job=job.job_id,
                    ceiling=None if raw is None else float(str(raw)) * 60.0,
                )
            )
    return tuple(lanes)


def test_the_suite_runs_somewhere(suite_lanes: tuple[SuiteLane, ...]) -> None:
    """The contract needs a lane to assert against.

    A rename that stopped the detector matching would otherwise turn
    every assertion below into a vacuous pass over an empty list, and the
    loss would look exactly like success.
    """
    assert suite_lanes, (
        f"no workflow job runs one of {SUITE_MARKERS}; either the suite moved "
        f"or this contract stopped recognizing it"
    )


@pytest.mark.parametrize("profile", ["default", "ci"], ids=str)
def test_every_profile_bounds_a_single_test(
    nextest_profiles: dict[str, str], profile: str
) -> None:
    """A test that hangs must be killed, not merely reported slow.

    Both profiles need this, and a profile does not inherit another's
    settings or its overrides. The `ci` profile is the one that includes
    the trybuild binaries the default profile excludes, so it is the
    profile with the longest tests and needs its own allowance for them.
    """
    block = nextest_profiles.get(profile)
    assert block is not None, f"nextest.toml must declare [profile.{profile}]"
    assert "terminate-after" in block, (
        f"[profile.{profile}] must set slow-timeout with terminate-after, or a "
        f"hung test is reported slow for ever and only the job timer ends it"
    )


@pytest.mark.parametrize("profile", ["default", "ci"], ids=str)
def test_the_global_timeout_sits_above_the_largest_single_test(
    nextest_profiles: dict[str, str], profile: str
) -> None:
    """Tier two must not pre-empt tier one.

    A whole-run budget below the longest per-test allowance ends the run
    before the test that allowance exists for can finish, and the failure
    names the run rather than the test.
    """
    block = nextest_profiles[profile]
    whole_run = global_timeout(block)
    longest = largest_test_allowance(block)
    assert whole_run > longest, (
        f"[profile.{profile}]'s {whole_run:.0f}s global-timeout is not above "
        f"its {longest:.0f}s largest per-test allowance; the run would end "
        f"before that test could use its budget"
    )


def test_the_job_ceiling_covers_the_run_and_the_work_around_it(
    suite_lanes: tuple[SuiteLane, ...], nextest_profiles: dict[str, str]
) -> None:
    """Tier four must not pre-empt tier two.

    The two clocks do not start together. The job timer starts when the
    job starts, before the checkout, the toolchain probe, the database
    fixtures and the instrumented build; nextest's whole-run budget
    starts only once tests begin. A ceiling merely above that budget
    still cancels the job before nextest can report an overrun, and a
    cancellation discards the log that would have explained it.

    Every lane is held to the larger of the two profiles' budgets,
    because a lane that passed `--profile ci` would run under that one
    and nothing in the workflow names which it uses.
    """
    required = (
        max(global_timeout(block) for block in nextest_profiles.values())
        + max(termination_allowance(block) for block in nextest_profiles.values())
        + OUTSIDE_RUN_ALLOWANCE_SECONDS
    )
    for lane in suite_lanes:
        assert lane.ceiling is not None, (
            f"{lane} runs the suite in a job with no timeout-minutes; the "
            f"outermost tier is missing and GitHub's six-hour default applies"
        )
        assert lane.ceiling >= required, (
            f"{lane} has a ceiling of {lane.ceiling:.0f}s, below the "
            f"{required:.0f}s needed to cover the whole-run budget, nextest's "
            f"termination procedure, and {OUTSIDE_RUN_ALLOWANCE_SECONDS:.0f}s "
            f"of build and other work outside its window; an overrun would be "
            f"cancelled rather than reported"
        )


def test_the_cargo_watchdog_tier_is_absent_rather_than_defaulted() -> None:
    """The third tier does not exist here, and must not appear unnoticed.

    The canonical section has four tiers because the shared coverage
    action wraps `cargo` in a wall-clock watchdog. This repository runs
    `cargo llvm-cov nextest` from a `run:` step and does not use that
    action, so the tier is absent by construction.

    A lane that adopted the action without setting the variable would
    inherit its undocumented 1,800 s default underneath a 30 m nextest
    budget, which is the inversion the canonical section exists to
    prevent, so both halves are asserted: the action is not used, and the
    variable is not set.
    """
    offenders: list[str] = []
    for path in workflow_paths():
        document = load(path)
        for job in jobs_of(path.name, document):
            body = job.body
            if not isinstance(body, dict):
                continue
            for step in body.get("steps") or []:
                if not isinstance(step, dict):
                    continue
                if COVERAGE_ACTION in str(step.get("uses", "")):
                    offenders.append(f"{job.workflow}:{job.job_id} uses the action")
                environment = step.get("env")
                if isinstance(environment, dict) and WATCHDOG_VARIABLE in environment:
                    offenders.append(
                        f"{job.workflow}:{job.job_id} sets {WATCHDOG_VARIABLE}"
                    )
    assert not offenders, (
        f"the cargo watchdog tier is documented as absent here, so adopting it "
        f"needs the developers' guide updated in the same change: {offenders}"
    )

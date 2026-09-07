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
not ``period`` alone. Both profiles declare their own budgets, which is
repository policy rather than a nextest requirement: a custom profile
inherits ``[profile.default]``, and nextest consults
``[[profile.default.overrides]]`` for it too, so a profile declaring
nothing would still be bounded. ``ci`` is the profile that includes the
trybuild binaries the default profile excludes, and the budgets that
govern CI belong where a reader of that profile will find them.

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

#: Shapes that put a suite command on a line without running it as the
#: line's own command, or without letting its failure end the step.
#:
#: `if false; then cargo nextest run; fi` keeps the text and runs
#: nothing, so a substring search counts a lane that never runs the
#: suite and demands a ceiling of it. `cargo nextest run || true` does
#: run the suite but discards its verdict, so the lane's budgets are
#: asserted while its result is not. Neither is judged here; both are
#: reported, because a contract that cannot tell what a line does
#: should say so rather than guess.
DISGUISES: typ.Final[tuple[str, ...]] = ("|| true", "|| :", "if ", "&&", ";", "|")

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
_FIELD: typ.Final[re.Pattern[str]] = re.compile(
    r'([a-z-]+)\s*=\s*"?([^,"}]+)"?'
)


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
            return {
                key: value.strip()
                for key, value in _FIELD.findall(match["body"])
            }
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
    return any(
        _is_suite_line(line)
        for step in job_body.get("steps") or []
        if isinstance(step, dict)
        for line in str(step.get("run", "")).splitlines()
    )


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


def _names_a_suite_command(line: str) -> bool:
    """Return whether one line mentions a suite command at all.

    Mentioning is weaker than invoking, and deliberately so: the two are
    compared below, and a line that mentions one without invoking it is
    the case this contract refuses to judge.

    Parameters
    ----------
    line
        One line of a step's script.

    Returns
    -------
    bool
        True when a suite marker appears on the line.
    """
    return any(marker in line for marker in SUITE_MARKERS)


def _is_suite_line(line: str) -> bool:
    """Return whether one line runs the suite plainly.

    Plainly means the line is the command and its arguments, and
    nothing else. Reading the whole ``run`` value as one string, as an
    earlier version did, counted a lane whose only mention of the suite
    was inside `if false; then ...; fi`, and would have demanded a
    ceiling of a job that never runs it.

    Parameters
    ----------
    line
        One line of a step's script.

    Returns
    -------
    bool
        True when the line runs a suite command and nothing else.
    """
    stripped = line.strip()
    if not _names_a_suite_command(stripped):
        return False
    if any(disguise in stripped for disguise in DISGUISES):
        return False
    return any(stripped.startswith(marker) for marker in SUITE_MARKERS)


def _disguised_suite_lines(job_body: dict[str, typ.Any]) -> list[str]:
    """Return lines naming a suite command without plainly running one.

    Parameters
    ----------
    job_body
        The job's parsed mapping.

    Returns
    -------
    list[str]
        The offending lines, stripped.
    """
    return [
        stripped
        for step in job_body.get("steps") or []
        if isinstance(step, dict)
        for line in str(step.get("run", "")).splitlines()
        if (stripped := line.strip())
        and _names_a_suite_command(stripped)
        and not _is_suite_line(stripped)
    ]


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

    Each profile must declare its own base allowance. nextest would fall
    back to ``[profile.default]`` for a profile that declared none, so
    this is repository policy rather than a nextest requirement: `ci` is
    the profile that includes the trybuild binaries the default profile
    excludes, and the budgets that govern CI belong where a reader of
    that profile will find them.

    The base ``slow-timeout`` is read specifically, not the profile's
    text as a whole. An override carrying ``terminate-after`` would
    satisfy a substring check while the base allowance had none, which
    leaves every ordinary test reported slow for ever.
    """
    block = nextest_profiles.get(profile)
    assert block is not None, f"nextest.toml must declare [profile.{profile}]"
    base = base_slow_timeout(block)
    assert base, (
        f"[profile.{profile}] must declare its own slow-timeout; an override "
        f"bounds only the tests it names"
    )
    assert base.get("terminate-after") == "1", (
        f"[profile.{profile}]'s base slow-timeout must set terminate-after = 1, "
        f"got {base.get('terminate-after')}; without it a hung test is reported "
        f"slow for ever and only the job timer ends it, by cancelling the run"
    )
    assert base.get("period") == "300s", (
        f"[profile.{profile}]'s base slow-timeout must allow 300s, as the "
        f"developers' guide states; got {base.get('period')}"
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
    required = required_ceiling(nextest_profiles)
    for lane in suite_lanes:
        assert lane.ceiling is not None, (
            f"{lane} runs the suite in a job with no timeout-minutes; the "
            f"outermost tier is missing and GitHub's six-hour default applies"
        )
        assert lane.ceiling >= required, (
            f"{lane} has a ceiling of {lane.ceiling:.0f}s, below the "
            f"{required:.0f}s needed to cover the whole-run budget, nextest's "
            f"termination procedure, {OUTSIDE_RUN_ALLOWANCE_SECONDS:.0f}s "
            f"of build and other work outside its window; an overrun would be "
            f"cancelled rather than reported"
        )


def _steps_of(job_body: object) -> list[dict[str, object]]:
    """Return one job's steps, or an empty list.

    Parameters
    ----------
    job_body
        The job's parsed value, which need not be a mapping.

    Returns
    -------
    list of dict
        The step mappings, in the order the job runs them.
    """
    if not isinstance(job_body, dict):
        return []
    steps = job_body.get("steps")
    if not isinstance(steps, list):
        return []
    return [step for step in steps if isinstance(step, dict)]


def _watchdog_offences(workflow: str, job_id: str, step: dict[str, object]) -> list[str]:
    """Return what one step does that the absent tier forbids.

    Two separate things are wrong, so they are reported separately: a
    step may adopt the action without naming the variable, or name the
    variable without adopting the action, and the fix differs.

    Parameters
    ----------
    workflow
        The workflow file's name.
    job_id
        The job's identifier.
    step
        One parsed step.

    Returns
    -------
    list of str
        One entry per offence, empty when the step commits none.
    """
    offences: list[str] = []
    if COVERAGE_ACTION in str(step.get("uses", "")):
        offences.append(f"{workflow}:{job_id} uses the action")
    environment = step.get("env")
    if isinstance(environment, dict) and WATCHDOG_VARIABLE in environment:
        offences.append(f"{workflow}:{job_id} sets {WATCHDOG_VARIABLE}")
    return offences


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
    offenders = [
        offence
        for path in workflow_paths()
        for job in jobs_of(path.name, load(path))
        for step in _steps_of(job.body)
        for offence in _watchdog_offences(job.workflow, job.job_id, step)
    ]
    assert not offenders, (
        f"the cargo watchdog tier is documented as absent here, so adopting it "
        f"needs the developers' guide updated in the same change: {offenders}"
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
        '[profile.example]\n'
        'slow-timeout = { period = "300s", terminate-after = 1 }\n'
        '\n'
        '[[profile.example.overrides]]\n'
        'filter = \'binary(trybuild)\'\n'
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


def test_no_step_disguises_a_suite_command(suite_lanes: tuple[SuiteLane, ...]) -> None:
    """A suite command must be the line's command, plainly.

    Two shapes defeat a reading that searches the whole `run` value, and
    they fail in opposite directions.
    `if false; then cargo nextest run; fi` keeps the text and runs
    nothing, so the job is counted as a suite lane and held to a ceiling
    it does not need. `cargo nextest run || true` does run the suite but
    discards its verdict, so the lane's budgets are asserted while its
    result is thrown away.

    Neither is judged as an invocation. Both are reported, because a
    contract that cannot tell what a line does should say so rather than
    guess.
    """
    assert suite_lanes, "the tree must have suite lanes for this to be about"
    disguised = [
        f"{file}:{job.job_id}: {line!r}"
        for path in workflow_paths()
        for file, job in [(path.name, job) for job in jobs_of(path.name, load(path))]
        for line in _disguised_suite_lines(job.body if isinstance(job.body, dict) else {})
    ]
    assert not disguised, (
        f"these steps name a suite command without plainly running one, so "
        f"this contract cannot tell whether the lane runs the suite or "
        f"whether its failure would end the step: {disguised}"
    )


def test_the_required_ceiling_carries_all_four_terms() -> None:
    """Whole-run budget, termination, outside work, and the margin.

    Every lane here sits well above the requirement, so dropping a term
    changes nothing the assertion over the workflows can see. Driving
    the derivation with controlled profiles is what makes a missing
    term visible.
    """
    profiles = {
        "default": (
            '[profile.default]\n'
            'slow-timeout = { period = "300s", grace-period = "5s" }\n'
            'global-timeout = "30m"\n'
        ),
        "ci": (
            '[profile.ci]\n'
            'slow-timeout = { period = "300s", grace-period = "5s" }\n'
            'global-timeout = "40m"\n'
        ),
    }
    expected = (
        40 * 60.0
        + (5.0 + TERMINATION_SAFETY_MARGIN_SECONDS)
        + OUTSIDE_RUN_ALLOWANCE_SECONDS
        + CEILING_MARGIN_SECONDS
    )
    assert required_ceiling(profiles) == pytest.approx(expected), (
        "the requirement takes the larger whole-run budget of the two "
        "profiles and adds the termination allowance, the outside allowance "
        f"and the margin; expected {expected}"
    )

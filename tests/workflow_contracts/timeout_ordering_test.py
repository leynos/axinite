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

import typing as typ

import pytest
from _workflow_policy import jobs_of, load, workflow_paths
from suite_lanes import (
    SUITE_MARKERS,
    SuiteLane,
    _disguised_suite_lines,
    _steps_of,
    _watchdog_offences,
    normalized_condition,
    suite_lanes_of,
)
from timeout_budgets import (
    CEILING_MARGIN_SECONDS,
    NEXTEST_CONFIG,
    OUTSIDE_RUN_ALLOWANCE_SECONDS,
    TERMINATION_SAFETY_MARGIN_SECONDS,
    base_slow_timeout,
    global_timeout,
    largest_test_allowance,
    profile_blocks,
    required_ceiling,
)


@pytest.fixture(scope="module")
def nextest_profiles() -> dict[str, str]:
    """Return each nextest profile's block of the configuration.

    Returns
    -------
    dict[str, str]
        Profile name to the text of its block.
    """
    return profile_blocks(NEXTEST_CONFIG.read_text(encoding="utf-8"))


@pytest.fixture(scope="module")
def suite_lanes() -> tuple[SuiteLane, ...]:
    """Return every job that runs the suite, with its ceiling.

    Returns
    -------
    tuple of SuiteLane
        One entry per suite-running job.
    """
    return suite_lanes_of()


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
        for line in _disguised_suite_lines(
            job.body if isinstance(job.body, dict) else {}
        )
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
            "[profile.default]\n"
            'slow-timeout = { period = "300s", grace-period = "5s" }\n'
            'global-timeout = "30m"\n'
        ),
        "ci": (
            "[profile.ci]\n"
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


#: The condition each suite lane legitimately carries, keyed by workflow
#: and job, as the step's ``if`` and its job's, with whitespace
#: collapsed.
#:
#: A skipped step runs no suite, so none of the budgets above says
#: anything about it. `if: false` on either would leave a lane that
#: looks bounded and is not, and so would a plausible condition that
#: quietly excluded the event the lane exists for.
#:
#: `codescene-coverage.yml`'s lane legitimately runs on pull requests
#: and on manual dispatch, because `coverage.yml` covers the trunk.
REQUIRED_CONDITIONS: typ.Final[dict[tuple[str, str], tuple[object, object]]] = {
    ("codescene-coverage.yml", "coverage-check"): (
        None,
        "github.event_name == 'pull_request' || "
        "github.event_name == 'workflow_dispatch'",
    ),
    ("coverage.yml", "coverage"): (None, None),
}


def test_each_suite_lane_carries_the_condition_it_is_meant_to(
    suite_lanes: tuple[SuiteLane, ...],
) -> None:
    """A skipped step runs no suite, so no budget above bounds it.

    Every assertion above reads a lane's declared budgets and says
    nothing about whether the step runs. `if: false` on the step or on
    its job would leave a lane that looks bounded and is not, and this
    contract would certify it. So would a plausible condition that
    quietly excluded the event the lane exists for, which is why the
    conditions are pinned by value rather than checked for falsity:
    YAML parses `false` to a boolean, and enumerating falsy spellings
    would miss the plausible ones anyway.

    The coordinates are compared both ways first, so a new lane with no
    entry fails rather than passing unexamined.

    Proved by mutation: `if: false` on the suite step, the same on its
    job, the dispatch clause dropped from `coverage-check`, and a
    coordinate dropped from ``REQUIRED_CONDITIONS`` each fail this test.
    """
    found: dict[tuple[str, str], set[tuple[object, object]]] = {}
    for lane in suite_lanes:
        collapsed = {
            (normalized_condition(step), normalized_condition(job))
            for step, job in lane.conditions
        }
        found.setdefault((lane.workflow, lane.job), set()).update(collapsed)
    assert set(found) == set(REQUIRED_CONDITIONS), (
        f"the suite lanes are not the ones this contract pins: "
        f"unlisted {sorted(set(found) - set(REQUIRED_CONDITIONS))}, missing "
        f"{sorted(set(REQUIRED_CONDITIONS) - set(found))}; a lane with no "
        f"entry here is a lane whose condition nobody has judged"
    )
    wrong = {
        coordinate: (expected, found[coordinate])
        for coordinate, expected in REQUIRED_CONDITIONS.items()
        if found[coordinate] != {expected}
    }
    assert not wrong, (
        f"these suite lanes do not carry the conditions the developers' "
        f"guide records, as expected versus found: {wrong}; a lane that is "
        f"skipped runs no suite, so none of the budgets above bounds it"
    )

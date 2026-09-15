"""Contract for the reading that finds suite lanes in the workflows.

Split from ``timeout_ordering_test`` so neither module outgrows the
400-line limit ``AGENTS.md`` sets. What the budgets must be is asserted
there; what a *lane* is, and how many runs it makes, is asserted here.

Every assertion in the ordering contract is over the lanes this reading
returns, so a reading that invents a lane, misses one, or under-counts
its runs weakens every one of them without failing any. Driving the
reading with supplied documents is what makes that visible.

Run via ``make test-workflow-contracts``.
"""

import typing as typ

import pytest
from _workflow_policy import parse_workflow
from nextest_config import Profile, profiles
from suite_lanes import SuiteLane, suite_lanes_in, suite_lanes_of
from timeout_budgets import (
    CEILING_MARGIN_SECONDS,
    OUTSIDE_RUN_ALLOWANCE_SECONDS,
    TERMINATION_SAFETY_MARGIN_SECONDS,
    required_ceiling,
)

#: A profile pair with budgets chosen to be unmistakable in arithmetic,
#: so a term that vanished from the requirement is visible rather than
#: absorbed. The `ci` whole-run budget is the larger of the two.
CONTROLLED_PROFILES: typ.Final[str] = (
    "[profile.default]\n"
    'slow-timeout = { period = "300s", terminate-after = 1, '
    'grace-period = "5s" }\n'
    'global-timeout = "30m"\n'
    "\n[profile.ci]\n"
    'slow-timeout = { period = "300s", terminate-after = 1, '
    'grace-period = "5s" }\n'
    'global-timeout = "40m"\n'
)


def _lanes_of(jobs: str) -> tuple[SuiteLane, ...]:
    """Return the lanes a controlled set of jobs produces.

    Parameters
    ----------
    jobs
        The body of a workflow's ``jobs:`` mapping, already indented.

    Returns
    -------
    tuple of SuiteLane
        The lanes the reading finds in it.
    """
    document = parse_workflow(
        f"name: controlled\non: push\njobs:\n{jobs}", "controlled.yml"
    )
    return suite_lanes_in([("controlled.yml", document)])


def _controlled_profiles() -> dict[str, Profile]:
    """Return the parsed controlled profiles.

    Returns
    -------
    dict[str, Profile]
        Profile name to its table and overrides.
    """
    return profiles(CONTROLLED_PROFILES)


def test_a_supplied_workflow_is_read_without_touching_the_tree() -> None:
    """The query answers about documents, not about this repository.

    Reading the workflow files inside the query left no way to ask what
    it makes of a lane that does not exist here: a job that runs the
    suite under no ceiling, beside one that runs nothing. Driven with
    supplied documents, both readings are visible, and the missing
    ceiling reads as ``None`` rather than as an absent lane.
    """
    lanes = _lanes_of(
        "  bounded:\n"
        "    runs-on: ubuntu-latest\n"
        "    timeout-minutes: 30\n"
        "    steps:\n"
        "      - run: cargo nextest run --workspace\n"
        "  unbounded:\n"
        "    runs-on: ubuntu-latest\n"
        "    steps:\n"
        "      - run: cargo nextest run --workspace\n"
        "  unrelated:\n"
        "    runs-on: ubuntu-latest\n"
        "    steps:\n"
        "      - run: markdownlint docs\n"
    )
    assert [(lane.job, lane.ceiling) for lane in lanes] == [
        ("bounded", 1800.0),
        ("unbounded", None),
    ], lanes


@pytest.mark.parametrize(
    "command",
    [
        pytest.param("cargo nextest run --help", id="the-help-probe"),
        pytest.param("cargo nextest run --version", id="the-version-probe"),
        pytest.param("cargo nextest run -h", id="the-short-help-probe"),
        pytest.param("cargo nextest run -V", id="the-short-version-probe"),
        pytest.param("cargo nextest runbook --workspace", id="a-longer-token"),
        pytest.param("cargo nextest runner --workspace", id="another-longer-token"),
    ],
)
def test_a_line_that_does_not_run_the_suite_makes_no_lane(command: str) -> None:
    """A probe and a near-miss are not invocations.

    Matching the marker as a text prefix accepted `cargo nextest
    runbook`, which begins with the same characters and runs no test,
    and accepted `cargo nextest run --help`, which prints and exits. A
    job whose only suite line was one of these was reported as a lane,
    so the assertion that the suite runs somewhere could pass with no
    suite running anywhere, and a ceiling was demanded of a job that
    needs none. The marker is matched as whole shell words now, and a
    probe argument refuses the line whatever else is on it.
    """
    assert not _lanes_of(
        "  probing:\n"
        "    runs-on: ubuntu-latest\n"
        "    timeout-minutes: 30\n"
        "    steps:\n"
        f"      - run: {command}\n"
    ), f"{command!r} runs no test, so it is not a lane"


@pytest.mark.parametrize(
    ("script", "expected"),
    [
        pytest.param(["cargo nextest run --workspace"], 1, id="one-run"),
        pytest.param(
            ["cargo nextest run --workspace", "cargo nextest run --profile ci"],
            2,
            id="two-runs-in-one-step",
        ),
        pytest.param(
            ["cargo llvm-cov nextest --workspace", "cargo nextest run --profile ci"],
            2,
            id="two-runs-under-different-commands",
        ),
    ],
)
def test_every_suite_command_in_a_job_is_counted(
    script: list[str], expected: int
) -> None:
    """A lane's runs are counted, not detected.

    The reading returned one lane per job whatever the job ran, so two
    suite commands were budgeted one whole-run window. nextest starts
    that clock afresh for the second run, which can therefore spend a
    second full window under the same job timer and be cancelled part
    way through with its log discarded.
    """
    body = "".join(f"          {line}\n" for line in script)
    lanes = _lanes_of(
        "  suite:\n"
        "    runs-on: ubuntu-latest\n"
        "    timeout-minutes: 300\n"
        "    steps:\n"
        "      - run: |\n" + body
    )
    assert [lane.invocations for lane in lanes] == [expected], lanes


def test_two_runs_in_separate_steps_are_two_runs() -> None:
    """Counting is per command, not per step.

    A job splitting its two runs across two steps spends exactly what a
    job running both from one script spends, so the two must read the
    same. Counting steps rather than commands would report one for the
    single-step spelling and two for this one.
    """
    lanes = _lanes_of(
        "  suite:\n"
        "    runs-on: ubuntu-latest\n"
        "    timeout-minutes: 300\n"
        "    steps:\n"
        "      - run: cargo nextest run --workspace\n"
        "      - run: cargo nextest run --profile ci\n"
    )
    assert [lane.invocations for lane in lanes] == [2], lanes


def test_the_requirement_grows_with_every_run_the_lane_makes() -> None:
    """A second run needs a second whole-run budget, not a second job.

    The whole-run budget and the termination allowance are spent once
    per run; the checkout, the build and the report happen once per job.
    So the requirement is the per-run terms multiplied by the count plus
    the per-job terms added once, and the difference between one run and
    two is exactly one per-run term.

    Every lane in this repository makes one run, so this is unobservable
    through the workflows: driving the derivation with controlled values
    is what makes a dropped multiplier visible.
    """
    parsed = _controlled_profiles()
    per_run = 40 * 60.0 + (5.0 + TERMINATION_SAFETY_MARGIN_SECONDS)
    per_job = OUTSIDE_RUN_ALLOWANCE_SECONDS + CEILING_MARGIN_SECONDS
    assert required_ceiling(parsed, 1) == pytest.approx(per_run + per_job)
    assert required_ceiling(parsed, 2) == pytest.approx(2 * per_run + per_job)
    assert required_ceiling(parsed, 2) - required_ceiling(parsed, 1) == pytest.approx(
        per_run
    ), "the second run costs a whole-run budget and a termination, and nothing else"


def test_a_lane_that_makes_no_run_is_refused_rather_than_budgeted() -> None:
    """Zero runs is not a lane, and must not shrink the requirement.

    Saturating or defaulting a count of zero would return a requirement
    below what a single run needs, and the ceiling contract would then
    certify a job that cannot finish one.
    """
    with pytest.raises(ValueError, match="not a"):
        required_ceiling(_controlled_profiles(), 0)


#: How many suite runs each lane in this repository makes, by workflow
#: and job.
#:
#: Pinned by coordinate because the requirement above is derived from
#: the count: deleting a suite command lowers what the ceiling must be,
#: and every timing assertion would still pass against the smaller
#: requirement. A count that changes is a change to what the lane
#: spends, and belongs in the developers' guide in the same commit.
REQUIRED_INVOCATIONS: typ.Final[dict[tuple[str, str], int]] = {
    ("codescene-coverage.yml", "coverage-check"): 1,
    ("coverage.yml", "coverage"): 1,
}


def test_each_lane_makes_the_number_of_runs_it_is_pinned_to() -> None:
    """The counts the requirement is derived from are the pinned ones.

    Compared both ways, so a new lane with no entry fails rather than
    passing unexamined, exactly as the conditions are.
    """
    found = {(lane.workflow, lane.job): lane.invocations for lane in suite_lanes_of()}
    assert found == REQUIRED_INVOCATIONS, (
        f"the suite lanes are not the ones this contract pins, as found "
        f"versus pinned: {found} against {REQUIRED_INVOCATIONS}; a count "
        f"nobody has judged is a job ceiling nobody has sized"
    )

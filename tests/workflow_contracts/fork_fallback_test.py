"""Contracts for the fork fallback on pull-request lanes.

A pull request from a fork cannot obtain an Ubicloud runner: the runner pool
belongs to this repository, and the fork's workflow run does not reach it. A
lane that asks for one anyway does not fail loudly. It queues, waits, and is
eventually reported as a stuck or cancelled check on somebody else's
contribution, which reads as this repository being broken rather than as a
placement error.

So every lane that can run on a `pull_request` names a GitHub-hosted runner
for the fork case and keeps its paid shape for a branch pull request, which is
the one a maintainer waits on. A lane that also serves a cron composes both
conditions in one expression, because `runs-on` is the only place either
distinction can live.

The rule is easy to break in a way that still reads correctly. Swapping the
fork field for a sibling of the same object leaves an expression of the same
shape, the same length and the same two labels, and it sends every branch
pull request to a free runner while forks still queue for a paid one. These
contracts therefore assert the field, not the shape.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import pytest
from _estate import Estate, estate_source, jobs_across
from _fork_lanes import (
    FORKABLE_EVENT,
    HOSTED_LABEL,
    is_a_bare_matrix_reference,
    opaque_runs_on_values,
    paid_arms_before_the_fork,
    runs_on_scalar,
)
from _workflow_policy import (
    FORK_CONDITION,
    UBICLOUD_LABEL_PREFIX,
    Job,
    conditional_runs_on_arms,
    jobs_of,
    runs_on_event,
    triggers,
)

def _forkable_ubicloud_jobs(estate: Estate) -> tuple[Job, ...]:
    """Return every Ubicloud job a pull request can dispatch."""
    return tuple(
        job
        for name, document in estate.items()
        if FORKABLE_EVENT in triggers(document)
        for job in jobs_of(name, document)
        if job.uses_ubicloud and runs_on_event(job, FORKABLE_EVENT)
    )


def _forkable_lanes() -> tuple[Job, ...]:
    """Return the forkable Ubicloud lanes, read through the source boundary."""
    return _forkable_ubicloud_jobs(estate_source())


#: The lanes each fork assertion runs against, one test per lane. Named rather
#: than called: `pytest_generate_tests` in `conftest.py` calls it during
#: collection and parametrizes each test's `job` argument, turning a
#: `SourceError` into a failure of that test naming the file.
JOB_SELECTOR = _forkable_lanes


def test_the_selector_finds_the_lanes(estate: Estate) -> None:
    """Guard against a selector that silently matches nothing.

    Every assertion below is satisfied by finding no lanes, so the reach of
    the scan is asserted first.
    """
    found = _forkable_ubicloud_jobs(estate)
    assert len(found) >= 8, (
        "expected the pull-request lanes that ask for an Ubicloud runner; "
        f"found {sorted(str(job) for job in found)}"
    )
    assert {str(job) for job in found} == {str(job) for job in _forkable_lanes()}, (
        "the lanes this file parametrizes over are read at collection, and "
        "the lanes the estate declares are read here; they must agree, or "
        "the per-lane assertions are running against a stale reading"
    )


def _opaque_expression_jobs(estate: Estate) -> tuple[tuple[Job, str], ...]:
    """Return every job with a `runs-on` expression the reader cannot resolve."""
    return tuple(
        (job, declared)
        for job in jobs_across(estate)
        for declared in opaque_runs_on_values(job)
    )


def test_no_lane_hides_behind_an_expression_the_reader_cannot_split(
    estate: Estate,
) -> None:
    """Close the selector's blind spot, rather than trusting the selector.

    `runner_labels` reports an expression it cannot parse as one opaque
    label. That label does not carry the Ubicloud prefix, so `uses_ubicloud`
    answers False and the selector above drops the job before a single fork
    assertion runs. The lane is then exempt from this file, and from the
    placement, timeout and sccache contracts as well, while still asking for
    a paid runner a fork cannot obtain.

    Failing towards opaque is right for the reader: guessing at a condition
    nobody taught it would answer confidently and wrongly. It is wrong for
    the estate, so the unreadable shape is refused here instead. The reader
    stays cautious; the workflows stay readable.
    """
    hidden = [
        (job, declared)
        for job, declared in _opaque_expression_jobs(estate)
        if not is_a_bare_matrix_reference(declared)
    ]
    assert not hidden, (
        "these lanes compute `runs-on` in a form the placement reader cannot "
        "split, so every contract that selects on the runner silently skips "
        f"them: {[(str(job), declared) for job, declared in hidden]}"
    )


def test_the_blind_spot_is_real() -> None:
    """Prove the assertion above guards something, not nothing.

    An expression naming an Ubicloud label in a condition the reader does not
    recognize is classified as GitHub-hosted, which is exactly why the
    contract above cannot be left to `uses_ubicloud`. If this ever stops
    holding, the contract above has become redundant rather than merely
    quiet, and should be reconsidered rather than kept as decoration.
    """
    declared = (
        "${{ github.actor == 'dependabot[bot]' && 'ubuntu-latest' "
        f"|| '{UBICLOUD_LABEL_PREFIX}standard-2' }}}}"
    )
    job = Job("fixture.yml", "fixture", {"runs-on": declared})
    assert conditional_runs_on_arms(declared) is None, (
        "the reader now understands this condition, so the fixture no longer "
        "demonstrates the blind spot"
    )
    assert not job.uses_ubicloud, (
        "an unreadable expression naming an Ubicloud label is no longer "
        "classified as GitHub-hosted; the selector's blind spot has closed"
    )


def test_every_pull_request_lane_falls_back_for_a_fork(job: Job) -> None:
    """Name the fork field itself, not an expression that looks like it.

    `github.event.pull_request.head.repo.private` produces an expression of
    the same shape with the same labels, sends every branch pull request to a
    free runner, and leaves a fork's queuing for a runner it cannot have.
    Nothing about the workflow's appearance would show it.
    """
    declared = runs_on_scalar(job)
    assert FORK_CONDITION in declared, (
        f"{job} can run on a fork's pull request and asks for "
        f"{job.runner_summary} without a fork fallback. Add "
        f"`{FORK_CONDITION} && '{HOSTED_LABEL}'` ahead of the paid label: a "
        "fork cannot obtain an Ubicloud runner and its check will hang."
    )


def test_the_fork_arm_selects_a_hosted_runner(job: Job) -> None:
    """The fallback must be free, and it must be the fork arm's own label."""
    declared = runs_on_scalar(job)
    _, found, arm = declared.partition(FORK_CONDITION)
    assert found, f"{job} names no fork condition, so there is no fallback arm to read"
    assert arm.lstrip().startswith(f"&& '{HOSTED_LABEL}'"), (
        f"{job} tests the fork field but does not hand a fork "
        f"{HOSTED_LABEL!r}; the arm reads {arm.strip()!r}"
    )


def test_no_earlier_arm_hands_a_fork_a_paid_runner(job: Job) -> None:
    """The fork arm must be reached before any paid pull-request arm.

    The chain is read in order and the first arm whose condition holds wins.
    An arm ahead of the fork arm selecting Ubicloud on a pull request takes a
    fork's run as surely as a branch's, while both assertions above still
    find the fork field and its hosted label further along.
    """
    shadowing = paid_arms_before_the_fork(runs_on_scalar(job))
    assert not shadowing, (
        f"{job} selects a paid runner for a pull request before its fork arm "
        f"is reached: {list(shadowing)}. Put `{FORK_CONDITION} && "
        f"'{HOSTED_LABEL}'` ahead of every arm a pull request can take."
    )


def test_a_branch_pull_request_keeps_the_paid_shape(job: Job) -> None:
    """The fallback must not quietly move every pull request off Ubicloud.

    This is the half that keeps the rule narrow. A lane rewritten to send
    every pull request to `ubuntu-latest` would satisfy both assertions above
    and lose the runner the estate is paying for.
    """
    selected = job.labels_for_event(FORKABLE_EVENT)
    assert any(label.startswith(UBICLOUD_LABEL_PREFIX) for label in selected), (
        f"{job} selects {', '.join(selected)} for a branch pull request, "
        "which is the run a maintainer waits on and the reason the shape was "
        "measured"
    )


def test_a_scheduled_lane_still_lands_hosted(job: Job) -> None:
    """Composing the two conditions must not lose the cron arm.

    The fork arm is inserted into the same expression that keeps cron work off
    paid runners, and an arm added in the wrong place shadows the one before
    it.
    """
    if "schedule" not in runs_on_scalar(job):
        pytest.skip(f"{job} does not serve a schedule")
    assert job.labels_for_event("schedule") == (HOSTED_LABEL,), (
        f"{job} selects {job.labels_for_event('schedule')} on a schedule; "
        "cron work has nobody waiting on it and belongs on a free runner"
    )

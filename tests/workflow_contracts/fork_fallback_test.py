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
from _workflow_policy import (
    DIST_GENERATED,
    FORK_CONDITION,
    UBICLOUD_LABEL_PREFIX,
    Job,
    jobs_of,
    load,
    runs_on_event,
    triggers,
    workflow_paths,
)

#: The trigger this rule is about. A `push`, a `schedule` and a
#: `workflow_dispatch` all run in this repository's own context, where an
#: Ubicloud runner is available.
FORKABLE_EVENT = "pull_request"

#: What a fork's pull request must be given instead.
HOSTED_LABEL = "ubuntu-latest"


def _runs_on(job: Job) -> str:
    """Return a job's `runs-on` scalar with its folded line breaks joined."""
    declared = job.body.get("runs-on")
    if isinstance(declared, dict):
        declared = declared.get("labels")
    return " ".join(str(declared).split()) if isinstance(declared, str) else ""


def _forkable_ubicloud_jobs() -> tuple[Job, ...]:
    """Return every Ubicloud job a pull request can dispatch.

    A workflow that does not declare the trigger contributes nothing, and
    neither does a job whose own guard excludes it: neither can be reached by
    a fork's pull request, so neither can strand one.
    """
    found: list[Job] = []
    for path in workflow_paths():
        if path.name == DIST_GENERATED:
            continue
        document = load(path)
        if FORKABLE_EVENT not in triggers(document):
            continue
        found.extend(
            job
            for job in jobs_of(path.name, document)
            if job.uses_ubicloud and runs_on_event(job, FORKABLE_EVENT)
        )
    return tuple(found)


def _ids(candidates: tuple[Job, ...]) -> list[str]:
    """Return readable parameter identifiers for a job sequence."""
    return [str(job) for job in candidates]


FORKABLE = _forkable_ubicloud_jobs()


def test_the_selector_finds_the_lanes() -> None:
    """Guard against a selector that silently matches nothing.

    Every assertion below is satisfied by finding no lanes, so the reach of
    the scan is asserted first.
    """
    assert len(FORKABLE) >= 8, (
        "expected the pull-request lanes that ask for an Ubicloud runner; "
        f"found {sorted(str(job) for job in FORKABLE)}"
    )


@pytest.mark.parametrize("job", FORKABLE, ids=_ids(FORKABLE))
def test_every_pull_request_lane_falls_back_for_a_fork(job: Job) -> None:
    """Name the fork field itself, not an expression that looks like it.

    `github.event.pull_request.head.repo.private` produces an expression of
    the same shape with the same labels, sends every branch pull request to a
    free runner, and leaves a fork's queuing for a runner it cannot have.
    Nothing about the workflow's appearance would show it.
    """
    declared = _runs_on(job)
    assert FORK_CONDITION in declared, (
        f"{job} can run on a fork's pull request and asks for "
        f"{job.runner_summary} without a fork fallback. Add "
        f"`{FORK_CONDITION} && '{HOSTED_LABEL}'` ahead of the paid label: a "
        "fork cannot obtain an Ubicloud runner and its check will hang."
    )


@pytest.mark.parametrize("job", FORKABLE, ids=_ids(FORKABLE))
def test_the_fork_arm_selects_a_hosted_runner(job: Job) -> None:
    """The fallback must be free, and it must be the fork arm's own label."""
    declared = _runs_on(job)
    _, found, arm = declared.partition(FORK_CONDITION)
    assert found, f"{job} names no fork condition, so there is no fallback arm to read"
    assert arm.lstrip().startswith(f"&& '{HOSTED_LABEL}'"), (
        f"{job} tests the fork field but does not hand a fork "
        f"{HOSTED_LABEL!r}; the arm reads {arm.strip()!r}"
    )


@pytest.mark.parametrize("job", FORKABLE, ids=_ids(FORKABLE))
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


@pytest.mark.parametrize("job", FORKABLE, ids=_ids(FORKABLE))
def test_a_scheduled_lane_still_lands_hosted(job: Job) -> None:
    """Composing the two conditions must not lose the cron arm.

    The fork arm is inserted into the same expression that keeps cron work off
    paid runners, and an arm added in the wrong place shadows the one before
    it.
    """
    if "schedule" not in _runs_on(job):
        pytest.skip(f"{job} does not serve a schedule")
    assert job.labels_for_event("schedule") == (HOSTED_LABEL,), (
        f"{job} selects {job.labels_for_event('schedule')} on a schedule; "
        "cron work has nobody waiting on it and belongs on a free runner"
    )


class TestChainReading:
    """The reader has to see every arm, or the contracts above are vacuous."""

    @staticmethod
    def _job(declared: str) -> Job:
        """Return a job declaring one `runs-on` value."""
        return Job("fixture.yml", "fixture", {"runs-on": declared})

    _COMPOSED = (
        "${{ github.event_name == 'schedule' && 'ubuntu-latest' "
        "|| github.event.pull_request.head.repo.fork && 'ubuntu-latest' "
        "|| 'ubicloud-standard-2' }}"
    )

    def test_a_three_armed_chain_reports_every_label(self) -> None:
        """A label the parser cannot see is exempt from every contract."""
        job = self._job(self._COMPOSED)
        assert job.runner_labels == ("ubuntu-latest", "ubicloud-standard-2")
        assert job.uses_ubicloud

    @pytest.mark.parametrize(
        ("event", "expected"),
        [
            ("schedule", ("ubuntu-latest",)),
            ("pull_request", ("ubicloud-standard-2",)),
            ("push", ("ubicloud-standard-2",)),
        ],
        ids=["cron", "branch-pull-request", "push"],
    )
    def test_each_event_selects_its_own_arm(
        self, event: str, expected: tuple[str, ...]
    ) -> None:
        """A fork condition answers false, so a branch pull request pays.

        The contracts ask what a lane costs this repository. A fork's run
        costs nothing here, so reading the fork arm as the pull-request answer
        would report every one of these lanes as free and hide the shape they
        actually buy.
        """
        assert self._job(self._COMPOSED).labels_for_event(event) == expected

    @pytest.mark.parametrize(
        "declared",
        [
            "${{ github.event.pull_request.head.repo.fork && 'ubuntu-latest' }}",
            "${{ 'ubuntu-latest' || 'ubicloud-standard-2' }}",
            "${{ github.repository_owner == 'leynos' && 'a' || 'b' }}",
        ],
        ids=["no-fallback", "unguarded-first-arm", "unrecognized-condition"],
    )
    def test_an_unreadable_chain_stays_one_opaque_label(self, declared: str) -> None:
        """Fail towards opaque rather than towards a confident wrong answer.

        A chain with no fallback has no label to fall through to, an unguarded
        first arm makes every later arm unreachable, and a condition the
        helpers cannot evaluate would make `labels_for_event` invent an
        answer. Each stays one label, and the job is then visibly unplaced
        rather than silently misread.
        """
        job = self._job(declared)
        assert job.runner_labels == (declared,)

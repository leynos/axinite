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

from collections.abc import Iterator

import pytest
from _workflow_policy import (
    DIST_GENERATED,
    FORK_CONDITION,
    UBICLOUD_LABEL_PREFIX,
    Job,
    conditional_runs_on_arms,
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

#: The one expression form allowed to stay opaque: a matrix reference, whose
#: labels are in the matrix rather than in the expression. Naming it rather
#: than exempting every unreadable form means a new one has to be added here
#: on purpose.
MATRIX_REFERENCE = "matrix."


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


def _is_opaque(declared: str) -> bool:
    """Report whether a scalar is an expression the reader cannot split."""
    return declared.startswith("${{") and conditional_runs_on_arms(declared) is None


def _authored_jobs() -> Iterator[Job]:
    """Yield every job anyone here wrote.

    The generated release workflow is excluded, as it is everywhere: nobody
    edits it, and its matrix reference is not a placement decision anyone
    made here.
    """
    for path in workflow_paths():
        if path.name == DIST_GENERATED:
            continue
        yield from jobs_of(path.name, load(path))


def _opaque_expression_jobs() -> tuple[tuple[Job, str], ...]:
    """Return every job whose `runs-on` expression the reader cannot split."""
    return tuple(
        (job, declared)
        for job, declared in ((job, _runs_on(job)) for job in _authored_jobs())
        if _is_opaque(declared)
    )


def test_no_lane_hides_behind_an_expression_the_reader_cannot_split() -> None:
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
        for job, declared in _opaque_expression_jobs()
        if MATRIX_REFERENCE not in declared
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
        assert job.runner_labels == ("ubuntu-latest", "ubicloud-standard-2"), (
            f"a three-armed chain reports {job.runner_labels!r}; a label the "
            "reader drops is exempt from every placement contract"
        )
        assert job.uses_ubicloud, (
            "the chain names an Ubicloud label, so the job must answer that "
            "it uses one; otherwise the sizing contracts skip it"
        )

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
        assert self._job(self._COMPOSED).labels_for_event(event) == expected, (
            f"{event!r} should select {expected!r}; reading the fork arm as the "
            "pull-request answer reports a paid lane as free"
        )

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
        assert job.runner_labels == (declared,), (
            f"{declared!r} is not a shape the reader parses, so it must be "
            "reported whole rather than split into arms nobody checked"
        )

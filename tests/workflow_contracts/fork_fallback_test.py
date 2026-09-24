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

import re

import pytest
from _estate import Estate, estate_source, jobs_across
from _workflow_policy import (
    FORK_CONDITION,
    UBICLOUD_LABEL_PREFIX,
    Job,
    conditional_runs_on_arms,
    jobs_of,
    runs_on_event,
    triggers,
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
#: A `runs-on` that is nothing but one matrix key: `${{ matrix.runner }}`.
#: Exempt from the assertion below because the label it resolves to is in the
#: matrix rather than the expression, and the sizing contract reads it there.
#:
#: The match is anchored on purpose. Testing whether the text merely contains
#: `matrix.` exempted every expression that mentioned a leg anywhere, which
#: includes a real selection the reader cannot split:
#: `${{ matrix.name == 'x' && 'ubuntu-latest' || 'ubicloud-standard-2' }}`
#: asks for a paid runner on every leg but one, and a fork's pull request
#: reaching such a leg is stranded exactly as this file exists to prevent.
BARE_MATRIX_REFERENCE_RE: re.Pattern[str] = re.compile(
    r"^\$\{\{\s*matrix\.[A-Za-z0-9_-]+\s*\}\}$"
)


def _is_a_bare_matrix_reference(declared: str) -> bool:
    """Report whether a `runs-on` is one matrix key and nothing else."""
    return BARE_MATRIX_REFERENCE_RE.fullmatch(declared) is not None


def _runs_on(job: Job) -> str:
    """Return a job's `runs-on` scalar with its folded line breaks joined."""
    declared = job.body.get("runs-on")
    if isinstance(declared, dict):
        declared = declared.get("labels")
    return " ".join(str(declared).split()) if isinstance(declared, str) else ""


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


def _is_opaque(declared: str) -> bool:
    """Report whether a scalar is an expression the reader cannot split."""
    return declared.startswith("${{") and conditional_runs_on_arms(declared) is None


def _opaque_expression_jobs(estate: Estate) -> tuple[tuple[Job, str], ...]:
    """Return every job whose `runs-on` expression the reader cannot split."""
    return tuple(
        (job, declared)
        for job, declared in ((job, _runs_on(job)) for job in jobs_across(estate))
        if _is_opaque(declared)
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
        if not _is_a_bare_matrix_reference(declared)
    ]
    assert not hidden, (
        "these lanes compute `runs-on` in a form the placement reader cannot "
        "split, so every contract that selects on the runner silently skips "
        f"them: {[(str(job), declared) for job, declared in hidden]}"
    )


@pytest.mark.parametrize(
    ("declared", "exempt"),
    [
        pytest.param("${{ matrix.runner }}", True, id="a-bare-reference"),
        pytest.param("${{ matrix.os }}", True, id="another-bare-reference"),
        pytest.param("${{  matrix.runner  }}", True, id="padded"),
        pytest.param(
            "${{ matrix.name == 'x' && 'ubuntu-latest' "
            f"|| '{UBICLOUD_LABEL_PREFIX}standard-2' }}}}",
            False,
            id="a-selection-wearing-a-matrix-key",
        ),
        pytest.param(
            f"${{{{ matrix.runner || '{UBICLOUD_LABEL_PREFIX}standard-2' }}}}",
            False,
            id="a-reference-with-a-fallback",
        ),
        pytest.param(
            "${{ github.actor == 'x' && 'ubuntu-latest' || 'matrix.runner' }}",
            False,
            id="the-key-named-inside-a-literal",
        ),
    ],
)
def test_only_a_bare_matrix_reference_is_exempt(declared: str, exempt: bool) -> None:
    """The exemption must not cover a selection that merely mentions a leg.

    A `runs-on` that is one matrix key resolves to whatever the leg names,
    and the sizing contract reads the label there, so the expression itself
    carries no placement decision and is rightly exempt.

    Testing for the substring `matrix.` exempted far more than that. The
    `a-selection-wearing-a-matrix-key` case is the one that cost something:
    it asks for a paid runner on every leg but one, the reader cannot split
    it, and a fork's pull request reaching such a leg is stranded exactly as
    this file exists to prevent.
    """
    assert _is_a_bare_matrix_reference(declared) is exempt, (
        f"{declared!r} should read as exempt={exempt}"
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
    declared = _runs_on(job)
    assert FORK_CONDITION in declared, (
        f"{job} can run on a fork's pull request and asks for "
        f"{job.runner_summary} without a fork fallback. Add "
        f"`{FORK_CONDITION} && '{HOSTED_LABEL}'` ahead of the paid label: a "
        "fork cannot obtain an Ubicloud runner and its check will hang."
    )


def test_the_fork_arm_selects_a_hosted_runner(job: Job) -> None:
    """The fallback must be free, and it must be the fork arm's own label."""
    declared = _runs_on(job)
    _, found, arm = declared.partition(FORK_CONDITION)
    assert found, f"{job} names no fork condition, so there is no fallback arm to read"
    assert arm.lstrip().startswith(f"&& '{HOSTED_LABEL}'"), (
        f"{job} tests the fork field but does not hand a fork "
        f"{HOSTED_LABEL!r}; the arm reads {arm.strip()!r}"
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

"""Contracts for the right-sized Ubicloud shapes and the evidence behind them.

Ubicloud bills by the minute against a shape chosen for a workload, so a job
sitting on a larger shape than it needs is a standing overcharge. Axinite ran
every Linux job on `ubicloud-standard-8` regardless of what the job did: the
formatting gate, which compiles nothing, cost the same per minute as the test
matrix.

Choosing a smaller shape is only defensible with measurement, and the two
numbers that decide it, the peak memory and the low-water disk mark, appear
nowhere in a job's ordinary output. So the rule enforced here has two halves:
each job declares the shape its work justifies, and each job that runs on a
paid shape carries the sampler that proves the choice. A resize with no
sampler is a guess that nobody can check afterwards.

Peaks belong to the shape, not to the workload. Cargo scales parallelism with
the processor count, so a peak measured on one shape does not transfer to
another; the sampler has to run on the shape actually in use.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import pytest
from _workflow_policy import Job, jobs, step_text

ALL_JOBS: tuple[Job, ...] = tuple(jobs())

#: The shapes this repository buys, smallest first, with the reason each
#: exists. Adding a shape is a cost decision and belongs in the pull request
#: that first uses it, alongside the measurement that justifies it.
APPROVED_SHAPES: dict[str, str] = {
    "ubicloud-standard-2": "jobs that compile little or nothing",
    "ubicloud-standard-4": "jobs that compile the workspace",
    "ubicloud-standard-8": "no job chooses it; e2e.yml still holds it "
    "because that workflow is right-sized separately",
}

SAMPLER = "./scripts/ci-resource-sampler.sh"
START_STEP = "Start resource sampler"
REPORT_STEP = "Report resource peaks"


def _ubicloud_jobs() -> tuple[Job, ...]:
    """Return every job that requests an Ubicloud runner on any event."""
    return tuple(job for job in ALL_JOBS if job.uses_ubicloud)


def _ids(candidates: tuple[Job, ...]) -> list[str]:
    """Return readable parameter identifiers for a job sequence."""
    return [str(job) for job in candidates]


def test_some_job_uses_ubicloud() -> None:
    """Guard against a selector that silently matches nothing."""
    assert _ubicloud_jobs(), "expected at least one Ubicloud job"


@pytest.mark.parametrize("job", _ubicloud_jobs(), ids=_ids(_ubicloud_jobs()))
def test_every_ubicloud_job_uses_an_approved_shape(job: Job) -> None:
    """Confine the estate to shapes someone has costed."""
    for label in job.ubicloud_labels:
        assert label in APPROVED_SHAPES, (
            f"{job} requests {label!r}, which is not one of "
            f"{', '.join(sorted(APPROVED_SHAPES))}. A new shape is a cost "
            "decision and needs the measurement that justifies it."
        )


@pytest.mark.parametrize("job", _ubicloud_jobs(), ids=_ids(_ubicloud_jobs()))
def test_every_ubicloud_job_samples_its_own_resources(job: Job) -> None:
    """Require the evidence that makes the next resize decidable.

    Without this, a job can be moved to a smaller shape on a hunch and the
    only signal that it was too small is the job dying, which reads as a
    flake rather than as a sizing error.
    """
    names = [str(step.get("name", "")) for step in job.steps]
    assert START_STEP in names, (
        f"{job} runs on {job.runner_summary} and does not start the resource "
        f"sampler. Add a {START_STEP!r} step; the shape it occupies cannot be "
        "reviewed without the peak it reaches."
    )
    assert REPORT_STEP in names, (
        f"{job} starts the sampler and never reports it, so the measurement "
        "is taken and thrown away"
    )
    assert names.index(START_STEP) < names.index(REPORT_STEP), (
        f"{job} reports the sampler before starting it"
    )


@pytest.mark.parametrize("job", _ubicloud_jobs(), ids=_ids(_ubicloud_jobs()))
def test_the_sampler_reports_even_when_the_job_fails(job: Job) -> None:
    """A job killed by its shape is exactly the job whose peak matters."""
    report = next(step for step in job.steps if step.get("name") == REPORT_STEP)
    assert report.get("if") == "always()", (
        f"{job} reports resource peaks only on success, which loses the "
        "measurement in the one case that would explain the failure"
    )
    assert f"{SAMPLER} report" in step_text(report), (
        f"{job} must report through {SAMPLER}"
    )


@pytest.mark.parametrize("job", _ubicloud_jobs(), ids=_ids(_ubicloud_jobs()))
def test_the_sampler_starts_after_the_checkout(job: Job) -> None:
    """The script lives in the repository, so it cannot run before checkout."""
    steps = job.steps
    start_at = next(
        index for index, step in enumerate(steps) if step.get("name") == START_STEP
    )
    checkout_at = next(
        (
            index
            for index, step in enumerate(steps)
            if str(step.get("uses", "")).startswith("actions/checkout@")
        ),
        None,
    )
    assert checkout_at is not None, f"{job} runs the sampler without checkout"
    assert checkout_at < start_at, (
        f"{job} starts the sampler at step {start_at}, before the checkout at "
        f"step {checkout_at} has put the script on disk"
    )


#: The reviewed shape for every Ubicloud job, with the reason. Wall times are
#: warm medians measured on `ubicloud-standard-8` on `main` at 75f186d8, before
#: the resize, so a later reader can see what the choice was made against.
#:
#: A structural predicate cannot make this decision. `cargo fmt` needs the
#: toolchain but compiles nothing, and `telegram-tests` compiles a small
#: out-of-workspace crate in 33 seconds; the first would be sized up and the
#: second sized up wrongly by any rule keyed on "does it invoke the compiler".
#: So the assignment is explicit, and changing one is a reviewable line in the
#: pull request that changes it.
REVIEWED_SHAPES: dict[tuple[str, str], tuple[str, str]] = {
    ("code_style.yml", "format"): (
        "ubicloud-standard-4",
        "compiles nothing, but `make nixie` renders Mermaid through a headless "
        "browser and the sampler measured 6,741 MiB of the smaller shape's "
        "7,940 MiB, which leaves no room for a cold run",
    ),
    ("code_style.yml", "clippy"): (
        "ubicloud-standard-4",
        "compiles the workspace under three feature shapes, 315 s on the widest leg",
    ),
    ("codescene-coverage.yml", "coverage-check"): (
        "ubicloud-standard-4",
        "off the critical path, so it takes the cheaper shape: 683 s at half "
        "the rate beats 455 s at full, and it peaked at 7,311 MiB of 15,991, "
        "or 46 %",
    ),
    ("coverage.yml", "coverage"): (
        "ubicloud-standard-4",
        "instrumented workspace build across three feature shapes",
    ),
    ("coverage.yml", "e2e-coverage"): (
        "ubicloud-standard-4",
        "instrumented workspace build plus a browser suite",
    ),
    ("test.yml", "tests"): (
        "ubicloud-standard-4",
        "the workspace test matrix, 484 to 551 s",
    ),
    ("test.yml", "telegram-tests"): (
        "ubicloud-standard-2",
        "one small out-of-workspace crate, 33 s",
    ),
    ("test.yml", "wasm-wit-compat"): (
        "ubicloud-standard-4",
        "compiles the workspace to wasm32-wasip2, 277 s",
    ),
    ("test.yml", "docker-build"): (
        "ubicloud-standard-4",
        "off the critical path, so it takes the cheaper shape: 605 s at half "
        "the rate beats 378 s at full, and it peaked at 5,527 MiB of 15,991, "
        "or 35 %",
    ),
    ("e2e.yml", "build"): (
        "ubicloud-standard-8",
        "not yet resized; its label is being changed by the scheduled-work "
        "pull request and moves in the follow-up",
    ),
    ("e2e.yml", "test"): (
        "ubicloud-standard-8",
        "not yet resized, as above",
    ),
}


def test_every_ubicloud_job_has_a_reviewed_shape() -> None:
    """Keep the table and the workflows in step, in both directions.

    A job that appears on a paid runner without an entry here has had its cost
    decided by whoever wrote the label. An entry with no job left it behind.
    """
    declared = {(job.workflow, job.job_id) for job in _ubicloud_jobs()}
    assert declared == set(REVIEWED_SHAPES), (
        "Ubicloud jobs and reviewed shapes disagree; "
        f"only in workflows: {sorted(declared - set(REVIEWED_SHAPES))}; "
        f"only in the table: {sorted(set(REVIEWED_SHAPES) - declared)}"
    )


@pytest.mark.parametrize("job", _ubicloud_jobs(), ids=_ids(_ubicloud_jobs()))
def test_each_ubicloud_job_holds_the_shape_it_was_given(job: Job) -> None:
    """Resizing a job is a cost decision, so it must be a reviewed line."""
    expected, reason = REVIEWED_SHAPES[job.workflow, job.job_id]
    assert job.ubicloud_labels == (expected,), (
        f"{job} requests {job.runner_summary} but is recorded as {expected} "
        f"because it {reason}. Change the table in the same pull request, "
        "with the measurement behind the new shape."
    )

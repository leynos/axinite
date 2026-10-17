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
paid shape carries the telemetry that proves the choice; that half is held by
`ci_telemetry_test.py`. A resize with no measurement is a guess that nobody
can check afterwards.

Peaks belong to the shape, not to the workload. Cargo scales parallelism with
the processor count, so a peak measured on one shape does not transfer to
another; the telemetry has to run on the shape actually in use.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import pytest
from _telemetry import INSTRUMENTED_WORKFLOWS
from _utility_jobs import SMALLEST_ARM, is_utility_job
from _workflow_files import jobs
from _workflow_policy import Job

ALL_JOBS: tuple[Job, ...] = tuple(jobs())

#: The shapes this repository buys, smallest first, with the reason each
#: exists. Adding a shape is a cost decision and belongs in the pull request
#: that first uses it, alongside the measurement that justifies it.
APPROVED_SHAPES: dict[str, str] = {
    "ubicloud-standard-2": "jobs that compile little or nothing",
    "ubicloud-standard-4": "jobs that compile the workspace",
    SMALLEST_ARM: "short utility jobs moved off the hosted pool because it made "
    "them wait; architecture-independent (user ruling, 25 September 2026)",
}

def _ubicloud_jobs() -> tuple[Job, ...]:
    """Return every job that requests an Ubicloud runner on any event."""
    return tuple(job for job in ALL_JOBS if job.uses_ubicloud)


def _sized_jobs() -> tuple[Job, ...]:
    """Return the Ubicloud jobs whose shape is a sizing decision.

    The named utility jobs are left out: each is pinned to the smallest shape
    by `runner_placement_test.py`, so there is no resize for the sampler to
    inform and no reviewed shape to choose.
    """
    return tuple(
        job for job in _ubicloud_jobs() if not is_utility_job(job.workflow, job.job_id)
    )


def _ids(candidates: tuple[Job, ...]) -> list[str]:
    """Return readable parameter identifiers for a job sequence."""
    return [str(job) for job in candidates]


@pytest.mark.parametrize("job", _sized_jobs(), ids=_ids(_sized_jobs()))
def test_every_sized_job_is_in_a_workflow_that_carries_telemetry(job: Job) -> None:
    """Link the sizing decision to the measurement that justifies it.

    `ci_telemetry_test.py` checks the jobs of `INSTRUMENTED_WORKFLOWS`. A paid
    shape in any other workflow would escape it, so the workflow must be listed.
    """
    assert job.workflow in INSTRUMENTED_WORKFLOWS, (
        f"{job} sits on a paid shape in {job.workflow}, which is not in "
        "INSTRUMENTED_WORKFLOWS, so no contract requires its telemetry. Add the "
        "workflow to the list in _telemetry.py and to the collector's list."
    )


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
    ("test.yml", "github-tool-tests"): (
        "ubicloud-standard-2",
        "one small out-of-workspace crate, on the same terms as "
        "telegram-tests; the shape is provisional until the first run on it "
        "reports a peak",
    ),
    ("test.yml", "libsql-transport-tests"): (
        "ubicloud-standard-2",
        "the TLS transport all-features matrix without the application or WASM "
        "runtime; provisional until its first resource peak is recorded",
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
        "ubicloud-standard-4",
        "compiles the workspace under --no-default-features --features libsql "
        "and peaked at 6,522 and 6,797 MiB on two cold runs, which leaves no "
        "headroom in the 7,940 MiB a -2 presents; the first run on the new "
        "shape peaked at 5,746 MiB of 15,991, or 36 %, in 582 s against a "
        "12 % Rust cache hit rate, where the -8 history is 330 s warm "
        "(dispatch 34991982816 on 3301e095e)",
    ),
    ("e2e.yml", "test"): (
        "ubicloud-standard-2",
        "compiles nothing: it downloads the binary `build` produced and "
        "drives it from Playwright, peaking at 1,273 to 1,432 MiB across "
        "nine legs; the first run on the new shape peaked at 916 to 997 MiB "
        "of 7,940, or 12 %, in 198 to 264 s against 120 to 216 s on the -8 "
        "(dispatch 34991982816 on 3301e095e)",
    ),
}


def test_every_ubicloud_job_has_a_reviewed_shape() -> None:
    """Keep the table and the workflows in step, in both directions.

    A job that appears on a paid runner without an entry here has had its cost
    decided by whoever wrote the label. An entry with no job left it behind.
    """
    declared = {(job.workflow, job.job_id) for job in _sized_jobs()}
    assert declared == set(REVIEWED_SHAPES), (
        "Ubicloud jobs and reviewed shapes disagree; "
        f"only in workflows: {sorted(declared - set(REVIEWED_SHAPES))}; "
        f"only in the table: {sorted(set(REVIEWED_SHAPES) - declared)}"
    )


@pytest.mark.parametrize("job", _sized_jobs(), ids=_ids(_sized_jobs()))
def test_each_ubicloud_job_holds_the_shape_it_was_given(job: Job) -> None:
    """Resizing a job is a cost decision, so it must be a reviewed line."""
    expected, reason = REVIEWED_SHAPES[job.workflow, job.job_id]
    assert job.ubicloud_labels == (expected,), (
        f"{job} requests {job.runner_summary} but is recorded as {expected} "
        f"because it {reason}. Change the table in the same pull request, "
        "with the measurement behind the new shape."
    )

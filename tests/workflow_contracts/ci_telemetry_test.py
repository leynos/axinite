"""Contracts for the CI telemetry every job carries.

Right-sizing a runner needs the busy cores, the peak memory and the low-water
disk mark of the job on the shape it actually uses, and none of them appears in
a job's own output. Each job in the five instrumented workflows therefore
starts `ci-telemetry-start` early and ends with `ci-telemetry-finish` under
`if: always()`, so the central collector can read one record per job, including
the job that died of its shape. The only jobs without the pair are the pure
roll-ups below: they run one step over `needs` and measure nothing.

A job added later without the pair is not excused: the collector reads it as a
missing record, which is the signal the coverage numbers count. The exemption
table here mirrors the collector's own list in leynos/grafana, so a roll-up is
excused in both places or in neither.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import re

import pytest
from _workflow_files import jobs
from _workflow_policy import Job

#: The workflows whose runs the collector reads for telemetry.
INSTRUMENTED_WORKFLOWS: tuple[str, ...] = (
    "code_style.yml",
    "codescene-coverage.yml",
    "coverage.yml",
    "e2e.yml",
    "test.yml",
)

#: The shared-actions commit every telemetry call pins. It carries
#: `ci-telemetry-start`, `ci-telemetry-finish` (with its sccache fields) and
#: `sccache-report`.
PIN = "81374b6f183e59f0570ad36217ded90d3240fda6"
ACTIONS = "leynos/shared-actions/.github/actions"
START = f"{ACTIONS}/ci-telemetry-start@{PIN}"
FINISH = f"{ACTIONS}/ci-telemetry-finish@{PIN}"
REPORT = f"{ACTIONS}/sccache-report@{PIN}"

#: Roll-up jobs that run one step over `needs`, with the reason.
ROLL_UPS: dict[tuple[str, str], str] = {
    ("code_style.yml", "code-style"): "branch-protection roll-up over three jobs",
    ("coverage.yml", "coverage-gate"): "branch-protection roll-up over three jobs",
    ("e2e.yml", "e2e"): "branch-protection roll-up over the matrix",
    ("test.yml", "run-tests"): "branch-protection roll-up over eight jobs",
}

_PINNED = re.compile(r"^[\w./-]+@[0-9a-f]{40}$")


def _instrumented() -> tuple[Job, ...]:
    return tuple(j for j in jobs() if j.workflow in INSTRUMENTED_WORKFLOWS)


def _carrying() -> tuple[Job, ...]:
    return tuple(
        j for j in _instrumented() if (j.workflow, j.job_id) not in ROLL_UPS
    )


def _ids(candidates: tuple[Job, ...]) -> list[str]:
    return [str(j) for j in candidates]


def _steps(job: Job) -> list[dict[str, object]]:
    steps = job.body.get("steps")
    assert isinstance(steps, list), f"{job} has no steps"
    return steps


def _uses(step: dict[str, object]) -> str:
    return str(step.get("uses", ""))


def test_the_selectors_match_something() -> None:
    """Guard against a selector that silently matches nothing."""
    assert len(_carrying()) >= 15
    assert {j.workflow for j in _instrumented()} == set(INSTRUMENTED_WORKFLOWS)


@pytest.mark.parametrize("job", _carrying(), ids=_ids(_carrying()))
def test_every_job_starts_and_reports_telemetry(job: Job) -> None:
    """Start early, report last: the pair brackets the job's work."""
    steps = _steps(job)
    uses = [_uses(s) for s in steps]
    starts = [i for i, u in enumerate(uses) if u == START]
    finishes = [i for i, u in enumerate(uses) if u == FINISH]
    assert len(starts) == 1, f"{job} must start telemetry exactly once"
    assert len(finishes) == 1, f"{job} must report telemetry exactly once"
    assert starts[0] < finishes[0], f"{job} reports telemetry before starting it"
    after = uses[finishes[0] + 1 :]
    assert all(u.startswith("actions/cache/save@") for u in after), (
        f"{job} must report telemetry after its work; only a cache save may "
        f"follow, but {after} do"
    )


@pytest.mark.parametrize("job", _carrying(), ids=_ids(_carrying()))
def test_the_start_comes_before_any_work(job: Job) -> None:
    """Only freeing the disk and the checkout may precede the start."""
    steps = _steps(job)
    start = next(i for i, s in enumerate(steps) if _uses(s) == START)
    before = steps[:start]
    assert all(
        _uses(s).startswith("actions/checkout@") or s.get("name") == "Free disk space"
        for s in before
    ), f"{job} does work before it starts telemetry: {before}"


@pytest.mark.parametrize("job", _carrying(), ids=_ids(_carrying()))
def test_the_report_runs_even_when_the_job_fails(job: Job) -> None:
    """A job killed by its shape is the one whose record matters."""
    finish = next(s for s in _steps(job) if _uses(s) == FINISH)
    assert finish.get("if") == "always()", (
        f"{job} reports telemetry only on success, which loses the record in "
        "the one case that would explain the failure"
    )


@pytest.mark.parametrize("job", _carrying(), ids=_ids(_carrying()))
def test_an_sccache_job_passes_setup_rusts_status_through(job: Job) -> None:
    """A job with `setup-rust` reports sccache through the action, not by hand."""
    steps = _steps(job)
    if not any(s.get("id") == "setup-rust" for s in steps):
        pytest.skip("the job does not use setup-rust")
    status = "${{ steps.setup-rust.outputs.sccache-status }}"
    finish = next(s for s in steps if _uses(s) == FINISH)
    assert (finish.get("with") or {}).get("sccache-status") == status
    report = [s for s in steps if _uses(s) == REPORT]
    assert len(report) == 1, f"{job} must report sccache through sccache-report"
    assert report[0].get("if") == "always()"
    assert (report[0].get("with") or {}).get("status") == status
    assert report[0].get("id") == "sccache"
    text = repr(steps)
    assert "--show-stats" not in text, f"{job} still reads sccache by hand"


@pytest.mark.parametrize("job", _carrying(), ids=_ids(_carrying()))
def test_the_old_sampler_is_gone(job: Job) -> None:
    """No job still calls the inline sampler that the actions replace."""
    assert "ci-resource-sampler" not in repr(job.body)


def test_every_telemetry_call_is_pinned_to_the_one_commit() -> None:
    """All three actions use the same full SHA, in every instrumented job."""
    seen: set[str] = set()
    for job in _instrumented():
        for step in job.body.get("steps", []):  # type: ignore[union-attr]
            action = _uses(step)
            if "/ci-telemetry-" in action or "/sccache-report@" in action:
                assert _PINNED.match(action), f"{job} uses an unpinned {action}"
                seen.add(action.rsplit("@", 1)[1])
    assert seen == {PIN}


@pytest.mark.parametrize(
    ("key", "reason"), sorted(ROLL_UPS.items()), ids=lambda v: str(v)
)
def test_a_roll_up_exemption_names_a_real_roll_up(
    key: tuple[str, str], reason: str
) -> None:
    """An exemption is a `needs` roll-up that runs one step and measures nothing."""
    job = next(j for j in jobs() if (j.workflow, j.job_id) == key)
    steps = _steps(job)
    assert reason.strip()
    assert job.body.get("needs"), f"{job} is exempt but has no needs"
    assert len(steps) == 1, f"{job} is exempt but runs {len(steps)} steps"
    assert START not in repr(steps)
    assert "ci-resource-sampler" not in repr(steps)


def test_the_resource_sampler_script_is_gone() -> None:
    """The script the actions replace is deleted, so nothing can call it."""
    from pathlib import Path

    assert not (Path(__file__).parents[2] / "scripts" / "ci-resource-sampler.sh").exists()

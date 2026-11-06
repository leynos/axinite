"""Keep dedicated prover jobs pinned, bounded, and outside the Rust build lane.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import re

import pytest
from _build_standard_workflows import Step, steps_of
from _estate import read_workflow
from _strict_workflows import triggers
from _workflow_policy import WORKFLOW_DIR

WORKFLOW = "formal-verification.yml"
PINNED_ACTION = re.compile(r"^[^@]+@[0-9a-f]{40}$")


def _mapping(value: object, label: str) -> dict[str, object]:
    """Return a declared mapping with a useful failure if its shape drifts."""
    assert isinstance(value, dict), f"{label} must be a mapping, got {value!r}"
    return value


def _job(workflow: dict[str, object], name: str) -> dict[str, object]:
    """Return a required proof job."""
    jobs = _mapping(workflow.get("jobs"), "workflow jobs")
    return _mapping(jobs.get(name), f"job {name}")


def _step(job: dict[str, object], name: str) -> Step:
    """Return one required step, naming an omission."""
    found = [step for step in steps_of(job) if step.name == name]
    assert len(found) == 1, f"expected one step named {name!r}, found {len(found)}"
    return found[0]


@pytest.fixture(scope="module")
def workflow() -> dict[str, object]:
    """Read the dedicated prover workflow through the shared boundary."""
    return read_workflow(WORKFLOW_DIR / WORKFLOW)


def test_formal_jobs_run_for_reviews_main_and_weekly_dispatch(
    workflow: dict[str, object],
) -> None:
    """Both proof suites run on reviewed changes and periodic main builds."""
    events = triggers(workflow)
    assert set(events) == {"pull_request", "push", "schedule", "workflow_dispatch"}
    for event in ("pull_request", "push"):
        trigger = _mapping(events[event], f"{event} trigger")
        assert trigger.get("branches") == ["main"]
    schedule = events["schedule"]
    assert isinstance(schedule, list) and len(schedule) == 1
    assert isinstance(schedule[0], dict)
    assert schedule[0].get("cron") == "17 4 * * 0"


def test_formal_jobs_are_read_only_and_separate(workflow: dict[str, object]) -> None:
    """The workflow has only the two bounded proof jobs and read permission."""
    assert workflow.get("permissions") == {"contents": "read"}
    jobs = _mapping(workflow.get("jobs"), "workflow jobs")
    assert set(jobs) == {"kani-smoke", "verus-proofs"}
    for name in jobs:
        job = _job(workflow, name)
        assert job.get("runs-on") == "ubuntu-latest"
        assert job.get("timeout-minutes") == 30
        assert (
            _mapping(job.get("env"), f"{name} environment").get("CARGO_INCREMENTAL")
            == "0"
        )


@pytest.mark.parametrize("job_id", ["kani-smoke", "verus-proofs"])
def test_formal_jobs_pin_checkout_and_disable_uv_cache(
    workflow: dict[str, object], job_id: str
) -> None:
    """Each prover job checks out without credentials and uses pinned uv."""
    job = _job(workflow, job_id)
    checkout = _step(job, "Checkout repository")
    assert checkout.uses.startswith("actions/checkout@")
    assert PINNED_ACTION.fullmatch(checkout.uses)
    assert checkout.inputs.get("persist-credentials") is False
    setup_uv = _step(job, "Setup uv")
    assert setup_uv.uses.startswith("astral-sh/setup-uv@")
    assert PINNED_ACTION.fullmatch(setup_uv.uses)
    assert setup_uv.inputs.get("python-version") == "3.14"
    assert setup_uv.inputs.get("enable-cache") is False
    assert {step.uses for step in steps_of(job) if step.uses} == {
        checkout.uses,
        setup_uv.uses,
    }, "the proof jobs must not add a build or Cargo-target cache action"


@pytest.mark.parametrize("job_id", ["kani-smoke", "verus-proofs"])
def test_formal_jobs_cannot_be_skipped_or_soft_failed(
    workflow: dict[str, object], job_id: str
) -> None:
    """Keep both proof jobs and their steps unconditional and fail-closed."""
    job = _job(workflow, job_id)
    assert "if" not in job, f"{job_id} must not be skippable"
    assert "continue-on-error" not in job, f"{job_id} must fail the workflow"
    for step in steps_of(job):
        assert not step.guarded, (
            f"{job_id} step {step.name!r} may be skipped or fail softly"
        )


def test_kani_job_installs_probes_and_checks_the_pinned_harness(
    workflow: dict[str, object],
) -> None:
    """Kani CI uses the repository installer, version check, proof, and test."""
    job = _job(workflow, "kani-smoke")
    assert _step(job, "Install Kani").run == "make install-kani"
    assert _step(job, "Probe Kani").run == "make kani-check"
    assert _step(job, "Run Kani proof").run == "make kani"
    assert _step(job, "Test Kani integration").run == "make test-kani-tools"


def test_verus_job_installs_both_binaries_and_checks_proofs(
    workflow: dict[str, object],
) -> None:
    """Verus CI installs its pinned solver and exercises positive and negative proofs."""
    job = _job(workflow, "verus-proofs")
    assert _step(job, "Install Verus").run == "make install-verus"
    assert _step(job, "Install Z3").run == "make install-z3"
    assert _step(job, "Run Verus proofs").run == "make verus"
    assert _step(job, "Test Verus integration").run == "make test-verus-tools"

"""Contracts for the libSQL coverage report hand-off and roll-up.

The publisher saves its LCOV report for the separate Codecov job. These
contracts keep the artifact, OIDC upload and successful-job roll-up connected.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import typing as typ

from _workflow_files import load
from _workflow_policy import WORKFLOW_DIR

WORKFLOW: typ.Final[str] = "coverage.yml"
ARTIFACT_NAME: typ.Final[str] = "libsql-only-lcov"
PUBLISHER_JOB: typ.Final[str] = "coverage-libsql"
CODECOV_JOB: typ.Final[str] = "codecov-libsql"
GATE_JOB: typ.Final[str] = "coverage-gate"
GATED_JOBS: typ.Final[frozenset[str]] = frozenset(
    {"coverage", PUBLISHER_JOB, CODECOV_JOB, "e2e-coverage", "compile-contracts"}
)


def _job(name: str) -> dict[str, object]:
    """Return one required job from the coverage workflow."""
    jobs = load(WORKFLOW_DIR / WORKFLOW).get("jobs")
    assert isinstance(jobs, dict), f"{WORKFLOW} must declare a jobs mapping"
    job = jobs.get(name)
    assert isinstance(job, dict), f"{WORKFLOW} must declare a {name!r} job"
    return job


def _steps(job: dict[str, object]) -> list[dict[str, object]]:
    """Return a job's step mappings."""
    steps = job.get("steps")
    assert isinstance(steps, list), "the job must declare steps"
    return [step for step in steps if isinstance(step, dict)]


def _step(job: dict[str, object], name: str) -> dict[str, object]:
    """Return a named workflow step, failing loudly if it was renamed."""
    for step in _steps(job):
        if step.get("name") == name:
            return step
    raise AssertionError(f"the job must declare the {name!r} step")


def _with(step: dict[str, object]) -> dict[str, object]:
    """Return a step's `with` mapping."""
    inputs = step.get("with")
    assert isinstance(inputs, dict), f"{step.get('name')!r} must declare inputs"
    return inputs


def _needs(job: dict[str, object]) -> set[str]:
    """Return a job's declared prerequisites."""
    needs = job.get("needs")
    assert isinstance(needs, list), "the job must declare a needs list"
    return {str(job_name) for job_name in needs}


def test_libsql_publisher_saves_a_short_lived_lcov_artifact() -> None:
    """Fail the publisher if it cannot hand off its report to Codecov."""
    upload = _step(
        _job(PUBLISHER_JOB), "Save the libsql-only report for the Codecov upload"
    )

    assert upload.get("uses") == "actions/upload-artifact@v7"
    inputs = _with(upload)
    assert inputs.get("name") == ARTIFACT_NAME
    assert inputs.get("path") == "lcov.info"
    assert inputs.get("if-no-files-found") == "error"
    assert inputs.get("retention-days") == 1


def test_codecov_job_downloads_and_uploads_the_publisher_artifact() -> None:
    """Keep the OIDC job tied to the report produced by the publisher."""
    job = _job(CODECOV_JOB)
    assert _needs(job) == {PUBLISHER_JOB}

    permissions = job.get("permissions")
    assert isinstance(permissions, dict), "the Codecov job must declare permissions"
    assert permissions == {"contents": "read", "id-token": "write"}

    download = _step(job, "Fetch the libsql-only report")
    assert download.get("uses") == "actions/download-artifact@v8"
    assert _with(download).get("name") == ARTIFACT_NAME

    upload = _step(job, "Upload to Codecov")
    assert upload.get("uses") == "codecov/codecov-action@v5"
    inputs = _with(upload)
    assert inputs.get("files") == "lcov.info"
    assert inputs.get("flags") == "libsql-only"
    assert inputs.get("disable_search") is True
    assert inputs.get("use_oidc") is True
    assert inputs.get("fail_ci_if_error") is True


def test_coverage_gate_checks_every_required_job_result() -> None:
    """The successful roll-up must depend on and check all coverage jobs."""
    gate = _job(GATE_JOB)
    assert _needs(gate) == GATED_JOBS
    run_steps = [step for step in _steps(gate) if isinstance(step.get("run"), str)]
    assert len(run_steps) == 1, "the coverage gate must have one result-check step"
    script = str(run_steps[0]["run"])

    for job_name in GATED_JOBS:
        assert f"needs.{job_name}.result" in script, (
            f"the coverage gate must fail when {job_name} does not succeed"
        )
    assert '!= "success"' in script

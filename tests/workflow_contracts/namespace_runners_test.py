"""Contract-test Axinite's compatible Namespace release runner slice."""

from __future__ import annotations

from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[2]
PROFILE = "namespace-profile-default"
RELEASE_JOBS = (
    "plan",
    "build-global-artifacts",
    "build-wasm-extensions",
    "host",
    "update-registry-checksums",
    "announce",
)


def _jobs(workflow_name: str) -> dict[str, object]:
    """Load the jobs mapping from one repository workflow."""
    workflow_path = ROOT / ".github" / "workflows" / workflow_name
    workflow = yaml.safe_load(workflow_path.read_text(encoding="utf-8"))
    assert isinstance(workflow, dict), f"{workflow_name} must parse to a mapping"
    jobs = workflow.get("jobs")
    assert isinstance(jobs, dict), f"{workflow_name} must declare jobs"
    return jobs


def test_ubuntu_22_release_jobs_use_the_shared_namespace_profile() -> None:
    """Keep the compatible generated release jobs on the reviewed profile."""
    jobs = _jobs("release.yml")
    for job_name in RELEASE_JOBS:
        job = jobs.get(job_name)
        assert isinstance(job, dict), f"release.yml must define {job_name}"
        assert job.get("runs-on") == PROFILE


def test_eight_core_and_tool_controlled_runners_remain_unchanged() -> None:
    """Preserve capacity-sensitive and cargo-dist-controlled runner selection."""
    test_jobs = _jobs("test.yml")
    assert test_jobs["tests"].get("runs-on") == "ubicloud-standard-8"

    release_jobs = _jobs("release.yml")
    assert release_jobs["build-local-artifacts"].get("runs-on") == (
        "${{ matrix.runner }}"
    )

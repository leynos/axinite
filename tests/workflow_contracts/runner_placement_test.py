"""Contracts for where each job runs.

Ubicloud bills by the minute for a runner shape this repository chose for
compiling Rust. A labelling job, a gate, a report, or a roll-up that only
compares upstream results consumes that shape for API calls, so placement is
a cost contract, not a preference. These tests pin the rule: a job may use an
Ubicloud runner only when it appears in the allow-list, Windows lanes stay on
GitHub-hosted runners because Ubicloud offers Linux only, and every Ubicloud
job bounds its own runtime.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import pytest
import yaml
from _workflow_policy import (
    REPOSITORY_ROOT,
    UBICLOUD_ALLOW_LIST,
    UBICLOUD_LABEL,
    Job,
    jobs,
)

ACTIONLINT_CONFIG = REPOSITORY_ROOT / ".github" / "actionlint.yaml"

ALL_JOBS: tuple[Job, ...] = tuple(jobs())


def _ids(candidates: tuple[Job, ...]) -> list[str]:
    """Return readable parameter identifiers for a job sequence."""
    return [str(job) for job in candidates]


def _ubicloud_jobs() -> tuple[Job, ...]:
    """Return every job that currently requests an Ubicloud runner."""
    return tuple(job for job in ALL_JOBS if job.runs_on == UBICLOUD_LABEL)


def test_workflow_estate_is_parsed() -> None:
    """Guard against a glob that silently matches nothing."""
    assert len(ALL_JOBS) > 20, "the workflow estate should declare many jobs"


@pytest.mark.parametrize("job", _ubicloud_jobs(), ids=_ids(_ubicloud_jobs()))
def test_only_allow_listed_jobs_use_ubicloud(job: Job) -> None:
    """Restrict the paid runner shape to build and test work."""
    assert (job.workflow, job.job_id) in UBICLOUD_ALLOW_LIST, (
        f"{job} runs on {UBICLOUD_LABEL} but is not in the build/test "
        "allow-list. Move it to ubuntu-latest, or add it to "
        "UBICLOUD_ALLOW_LIST with the reason it must compile or execute "
        "the product."
    )


@pytest.mark.parametrize("job", _ubicloud_jobs(), ids=_ids(_ubicloud_jobs()))
def test_ubicloud_jobs_bound_their_runtime(job: Job) -> None:
    """Cap paid runner time so a hung job cannot bill indefinitely."""
    timeout = job.body.get("timeout-minutes")
    assert isinstance(timeout, int), (
        f"{job} runs on {UBICLOUD_LABEL} and must declare timeout-minutes"
    )
    assert 0 < timeout <= 60, f"{job} declares an implausible timeout: {timeout}"


def test_allow_list_has_no_stale_entries() -> None:
    """Keep the allow-list honest when a job moves or is deleted."""
    declared = {(job.workflow, job.job_id) for job in ALL_JOBS}
    stale = UBICLOUD_ALLOW_LIST - declared
    assert not stale, (
        f"the Ubicloud allow-list names jobs that no longer exist: {stale}"
    )


def test_windows_jobs_stay_github_hosted() -> None:
    """Keep Windows lanes on GitHub: Ubicloud provides Linux images only."""
    windows_jobs = [job for job in ALL_JOBS if "windows" in job.job_id]
    assert windows_jobs, "the estate must still declare its Windows lanes"
    for job in windows_jobs:
        assert (job.runs_on or "").startswith("windows-"), (
            f"{job} is a Windows lane but requests {job.runs_on!r}; Ubicloud "
            "offers Linux runners only"
        )


def test_roll_up_and_administrative_jobs_are_github_hosted() -> None:
    """Name the classes that must never return to the paid runner."""
    # These are the jobs the Tier 2 preparation moved off Ubicloud. Listing
    # them explicitly means a revert fails here rather than on the invoice.
    github_hosted = {
        ("audit.yml", "audit"),
        ("claude-review.yml", "review"),
        ("code_style.yml", "code-style"),
        ("coverage.yml", "coverage-gate"),
        ("e2e.yml", "e2e"),
        ("pr-label-classify.yml", "classify"),
        ("pr-label-scope.yml", "scope"),
        ("regression-test-check.yml", "regression-test"),
        ("release-plz.yml", "release-plz-pr"),
        ("release-plz.yml", "release-plz-release"),
        ("staging-ci.yml", "check-changes"),
        ("staging-ci.yml", "create-promotion-pr"),
        ("staging-ci.yml", "gate"),
        ("staging-ci.yml", "report"),
        ("staging-ci.yml", "update-tag"),
        ("test.yml", "audit"),
        ("test.yml", "run-tests"),
        ("test.yml", "version-check"),
    }
    by_identity = {(job.workflow, job.job_id): job for job in ALL_JOBS}
    for identity in sorted(github_hosted):
        job = by_identity.get(identity)
        assert job is not None, f"{identity} no longer exists"
        assert job.runs_on == "ubuntu-latest", (
            f"{job} is API-bound, scheduled, or a roll-up and must stay on "
            f"ubuntu-latest, but requests {job.runs_on!r}"
        )


def test_actionlint_registers_every_referenced_ubicloud_label() -> None:
    """Keep the actionlint label list exactly in step with the workflows."""
    config = yaml.safe_load(ACTIONLINT_CONFIG.read_text(encoding="utf-8"))
    registered = set(config["self-hosted-runner"]["labels"])
    referenced = {
        job.runs_on
        for job in ALL_JOBS
        if job.runs_on is not None and job.runs_on.startswith("ubicloud-")
    }
    assert registered == referenced, (
        "every Ubicloud label a workflow references must be registered with "
        "actionlint, and no unused label may linger to mask a typo"
    )

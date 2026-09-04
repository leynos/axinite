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
    Job,
    builds_or_tests,
    jobs,
)

ACTIONLINT_CONFIG = REPOSITORY_ROOT / ".github" / "actionlint.yaml"

ALL_JOBS: tuple[Job, ...] = tuple(jobs())


def _ids(candidates: tuple[Job, ...]) -> list[str]:
    """Return readable parameter identifiers for a job sequence."""
    return [str(job) for job in candidates]


def _ubicloud_jobs() -> tuple[Job, ...]:
    """Return every job that requests an Ubicloud runner.

    Matched on the label prefix rather than on the one label in use today, so
    the migration wave's `ubicloud-standard-2` is covered by the placement and
    timeout contracts from the moment it appears.
    """
    return tuple(job for job in ALL_JOBS if job.uses_ubicloud)


def test_workflow_estate_is_parsed() -> None:
    """Guard against a glob that silently matches nothing."""
    assert len(ALL_JOBS) > 20, "the workflow estate should declare many jobs"


@pytest.mark.parametrize("job", _ubicloud_jobs(), ids=_ids(_ubicloud_jobs()))
def test_only_build_and_test_jobs_use_ubicloud(job: Job) -> None:
    """Restrict the paid runner shape to work that compiles or executes.

    The classification comes from the job's own steps, not from a list of job
    names, so a job that stops building stops qualifying at the same moment.
    """
    assert builds_or_tests(job), (
        f"{job} runs on {job.runner_summary} but no step compiles or executes "
        "the product. Move it to ubuntu-latest. If it genuinely does build or "
        "test, add the command it runs to BUILD_OR_TEST_PATTERNS."
    )


@pytest.mark.parametrize("job", _ubicloud_jobs(), ids=_ids(_ubicloud_jobs()))
def test_ubicloud_jobs_bound_their_runtime(job: Job) -> None:
    """Cap paid runner time so a hung job cannot bill indefinitely."""
    timeout = job.body.get("timeout-minutes")
    assert isinstance(timeout, int), (
        f"{job} runs on {job.runner_summary} and must declare timeout-minutes"
    )
    assert 0 < timeout <= 60, f"{job} declares an implausible timeout: {timeout}"


@pytest.mark.parametrize(
    ("declared", "expected"),
    [
        ("ubicloud-standard-8", ("ubicloud-standard-8",)),
        (
            ["self-hosted", "ubicloud-standard-8"],
            ("self-hosted", "ubicloud-standard-8"),
        ),
        (
            {"group": "linux", "labels": ["ubicloud-standard-8"]},
            ("ubicloud-standard-8",),
        ),
        # The mapping's `labels` key takes a bare string as readily as a list.
        ({"group": "linux", "labels": "ubicloud-standard-8"}, ("ubicloud-standard-8",)),
        ("${{ matrix.runner }}", ("${{ matrix.runner }}",)),
        (None, ()),
    ],
    ids=["scalar", "list", "mapping-list", "mapping-scalar", "expression", "absent"],
)
def test_every_runs_on_form_is_read(
    declared: object, expected: tuple[str, ...]
) -> None:
    """Read all three shapes `runs-on` accepts.

    A job whose label the parser cannot see is excluded from the placement,
    timeout, actionlint, and sccache contracts at once, and every test still
    passes. Silent exclusion is the failure mode worth pinning.
    """
    body: dict[str, object] = {} if declared is None else {"runs-on": declared}
    job = Job("fixture.yml", "fixture", body)
    assert job.runner_labels == expected
    assert job.uses_ubicloud == any(label.startswith("ubicloud-") for label in expected)


def test_the_build_classification_discriminates() -> None:
    """Guard against a predicate so loose that it accepts everything.

    A pattern list that matched any shell at all would make the placement
    contract vacuous. The roll-ups are the control group: their entire body
    compares `needs.*.result`, so none of them may classify as a build.
    """
    by_identity = {(job.workflow, job.job_id): job for job in ALL_JOBS}
    roll_ups = (
        ("code_style.yml", "code-style"),
        ("coverage.yml", "coverage-gate"),
        ("e2e.yml", "e2e"),
        ("test.yml", "run-tests"),
    )
    for identity in roll_ups:
        job = by_identity[identity]
        assert not builds_or_tests(job), (
            f"{job} only compares upstream results but classifies as a build; "
            "BUILD_OR_TEST_PATTERNS is too permissive"
        )
    builders = [job for job in ALL_JOBS if builds_or_tests(job)]
    assert len(builders) > 5, "the estate should still contain build jobs"


def test_windows_jobs_stay_github_hosted() -> None:
    """Keep Windows lanes on GitHub: Ubicloud provides Linux images only."""
    windows_jobs = [job for job in ALL_JOBS if "windows" in job.job_id]
    assert windows_jobs, "the estate must still declare its Windows lanes"
    for job in windows_jobs:
        assert (
            all(label.startswith("windows-") for label in job.runner_labels)
            and job.runner_labels
        ), (
            f"{job} is a Windows lane but requests {job.runner_summary}; "
            "Ubicloud offers Linux runners only"
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
    referenced = {label for job in ALL_JOBS for label in job.ubicloud_labels}
    assert registered == referenced, (
        "every Ubicloud label a workflow references must be registered with "
        "actionlint, and no unused label may linger to mask a typo"
    )

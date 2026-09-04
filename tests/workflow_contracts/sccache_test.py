"""Contracts for the sccache wiring on Ubicloud Rust jobs.

Installing sccache does nothing on its own. Cargo only routes compilation
through it when `RUSTC_WRAPPER` names it, and its GitHub Actions backend only
reaches Ubicloud's store when the runner's cache endpoint is re-exported into
the step environment. Either omission is silent: the build succeeds, the job
just recompiles everything. Since no cache step archives a `target` tree any
more, a silent sccache is a straight regression, so these contracts pin all
three halves of the wiring together.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import re

import pytest
from _workflow_policy import Job, jobs

ALL_JOBS: tuple[Job, ...] = tuple(jobs())

#: Step that re-exports the runner's Actions cache endpoint. sccache's GHA
#: backend reads these from the runner environment, which `run:` steps do not
#: inherit.
EXPORT_STEP = "Export the Actions cache endpoint for sccache"
INSTALL_STEP = "Install sccache"
ZERO_STEP = "Start sccache statistics"
REPORT_STEP = "Report sccache statistics"


#: Commands that actually invoke rustc, and therefore benefit from a compiler
#: cache. `cargo fmt` is absent on purpose: the formatter gate needs the
#: toolchain but compiles nothing, so wrapping it would add an install for no
#: cache traffic. `docker build` is absent because compilation happens inside
#: the image, where sccache on the host cannot see it. The trailing boundary
#: matters: `make test-workflow-contracts` is a PyYAML parse, not a build.
COMPILING_COMMANDS: tuple[re.Pattern[str], ...] = tuple(
    re.compile(pattern)
    for pattern in (
        r"\bcargo\s+(?:build|check|clippy|test|nextest|llvm-cov)\b",
        r"\bmake\s+(?:test|lint-whitaker)\b(?!-)",
        r"\./scripts/build-wasm-extensions\.sh",
    )
)


def _compiles(job: Job) -> bool:
    """Report whether any of a job's steps invokes the Rust compiler."""
    return any(
        pattern.search(str(step.get("run", "")))
        for step in job.steps
        for pattern in COMPILING_COMMANDS
    )


def _compiling_ubicloud_jobs() -> tuple[Job, ...]:
    """Return the Ubicloud jobs that invoke the Rust compiler."""
    return tuple(job for job in ALL_JOBS if job.uses_ubicloud and _compiles(job))


def _ids(candidates: tuple[Job, ...]) -> list[str]:
    """Return readable parameter identifiers for a job sequence."""
    return [str(job) for job in candidates]


WRAPPED = _compiling_ubicloud_jobs()


def test_the_wrapped_set_is_not_empty() -> None:
    """Guard against a selector that quietly matches nothing."""
    assert len(WRAPPED) >= 8, (
        "every Ubicloud job that invokes the Rust compiler should be wrapped"
    )


@pytest.mark.parametrize("job", WRAPPED, ids=_ids(WRAPPED))
def test_the_compiler_is_actually_wrapped(job: Job) -> None:
    """Export `RUSTC_WRAPPER`; installing sccache alone changes nothing."""
    env = job.body.get("env")
    assert isinstance(env, dict), f"{job} must declare a job-level env block"
    assert env.get("RUSTC_WRAPPER") == "sccache", (
        f"{job} installs sccache but does not set RUSTC_WRAPPER, so every "
        "build compiles as though sccache were absent"
    )
    assert env.get("SCCACHE_GHA_ENABLED") == "true", (
        f"{job} must enable sccache's GitHub Actions backend"
    )
    # sccache cannot cache incremental compilation, and Cargo enables it by
    # default for dev profiles.
    assert env.get("CARGO_INCREMENTAL") == "0", (
        f"{job} must disable incremental compilation for sccache"
    )


@pytest.mark.parametrize("job", WRAPPED, ids=_ids(WRAPPED))
def test_the_cache_endpoint_is_exported_before_any_build(job: Job) -> None:
    """Order the endpoint export, the install, and the reset before the build."""
    names = [str(step.get("name", step.get("uses", ""))) for step in job.steps]
    for required in (EXPORT_STEP, INSTALL_STEP, ZERO_STEP, REPORT_STEP):
        assert required in names, f"{job} is missing the {required!r} step"
    assert names.index(EXPORT_STEP) < names.index(INSTALL_STEP), (
        f"{job} installs sccache before exporting the cache endpoint"
    )
    assert names.index(ZERO_STEP) < names.index(REPORT_STEP), (
        f"{job} reports statistics before resetting them"
    )
    first_build = next(
        (
            index
            for index, step in enumerate(job.steps)
            if "cargo " in str(step.get("run", ""))
            or "make " in str(step.get("run", ""))
        ),
        None,
    )
    if first_build is None:
        return
    assert names.index(ZERO_STEP) < first_build, (
        f"{job} runs a build before sccache is installed and reset, so that "
        "build bypasses the compiler cache"
    )


@pytest.mark.parametrize("job", WRAPPED, ids=_ids(WRAPPED))
def test_statistics_are_reported_even_when_the_build_fails(job: Job) -> None:
    """Report the hit rate unconditionally, or a broken cache stays invisible."""
    report = next(step for step in job.steps if step.get("name") == REPORT_STEP)
    assert str(report.get("if", "")).strip() == "always()", (
        f"{job} must report sccache statistics with `if: always()`; a wrapper "
        "doing nothing is most visible on the run that fails"
    )
    body = str(report.get("run", ""))
    assert "--show-stats" in body, f"{job} must run sccache --show-stats"
    assert "GITHUB_STEP_SUMMARY" in body, (
        f"{job} must write the statistics to the job summary"
    )


def test_github_hosted_jobs_are_left_alone() -> None:
    """Keep the wiring off GitHub-hosted runners.

    The export step re-points sccache at Ubicloud's local proxy, which does
    not exist on a GitHub-hosted runner. The Windows lanes keep whatever they
    already had.
    """
    for job in ALL_JOBS:
        if job.uses_ubicloud:
            continue
        names = [str(step.get("name", "")) for step in job.steps]
        assert EXPORT_STEP not in names, (
            f"{job} is not on Ubicloud but exports Ubicloud's cache endpoint"
        )

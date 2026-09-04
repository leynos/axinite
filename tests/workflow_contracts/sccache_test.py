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
        # The optional +toolchain segment matters: `cargo +nightly test`
        # invokes rustc just as surely, and a selector that missed it would
        # exempt that job from every assertion below.
        r"\bcargo\s+(?:\+\S+\s+)?(?:build|check|clippy|test|nextest|llvm-cov)\b",
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
    assert names.index(INSTALL_STEP) < names.index(ZERO_STEP), (
        f"{job} resets sccache statistics before sccache is installed; the "
        "step names alone would still look correct"
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
    # The job summary is not readable through the REST API, so the statistics
    # must also reach the log, where anyone can confirm the hit rate or a read
    # or write error after the run.
    assert "printf '%s\\n' \"$stats\"" in body, (
        f"{job} must print the statistics to the log as well as the summary"
    )


@pytest.mark.parametrize("job", WRAPPED, ids=_ids(WRAPPED))
def test_a_missing_cache_endpoint_is_reported(job: Job) -> None:
    """Warn when the proxy address is absent instead of failing silently.

    With `SCCACHE_GHA_ENABLED` set and no endpoint, sccache misses every
    compilation and the wrapper becomes pure overhead. That looks exactly like
    a cold cache, so it has to announce itself.
    """
    export = next(step for step in job.steps if step.get("name") == EXPORT_STEP)
    script = str((export.get("with") or {}).get("script", ""))
    assert "CUSTOM_ACTIONS_CACHE_URL" in script, (
        f"{job} must fall back to Ubicloud's CUSTOM_ACTIONS_CACHE_URL"
    )
    assert "core.warning" in script, (
        f"{job} must warn when no cache endpoint is available"
    )
    assert "process.env.ACTIONS_RUNTIME_TOKEN" in script
    # The token must never be printed, only whether one was found. A prefix
    # check is not enough: `token present: ${process.env.ACTIONS_RUNTIME_TOKEN}`
    # would satisfy it while printing the secret into the log.
    assert "sccache runtime token present: ${Boolean(runtimeToken)}" in script, (
        f"{job} must report the token as a boolean, never as its value"
    )
    for call in re.findall(r"core\.(?:info|warning|error|notice)\([^;]*", script):
        assert "ACTIONS_RUNTIME_TOKEN" not in call, (
            f"{job} interpolates the raw runtime token into a log call: {call[:80]!r}"
        )
        # Remove the one permitted expression, then reject every remaining
        # mention. Matching `runtimeToken}` alone would pass
        # `core.info(runtimeToken)` and `core.warning(String(runtimeToken))`,
        # both of which print the secret.
        residue = call.replace("Boolean(runtimeToken)", "")
        assert "runtimeToken" not in residue, (
            f"{job} passes the runtime token to a log call other than as "
            f"Boolean(runtimeToken): {call[:80]!r}"
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

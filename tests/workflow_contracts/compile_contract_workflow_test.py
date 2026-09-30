"""Keep required compile-contract executions beside both coverage paths."""

from __future__ import annotations

import typing as typ

import pytest
from _strict_workflows import Workflows, jobs, read_workflows, steps
from _workflow_policy import REPOSITORY_ROOT, WORKFLOW_DIR
from contract_sources import read_source
from timeout_budgets import compile_contract_binaries

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Mapping

BASELINE_FLAGS = {
    "all-features": "--all-features",
    "default": "--features test-helpers",
    "libsql-only": "--no-default-features --features libsql,test-helpers",
}


@pytest.fixture(scope="module")
def workflows() -> Workflows:
    """Read each workflow through the strict YAML boundary."""
    return read_workflows(WORKFLOW_DIR)


def _job(workflows: Workflows, workflow: str, job_id: str) -> dict[object, object]:
    """Return a required job, failing with its workflow location."""
    found = jobs(workflows[workflow]).get(job_id)
    assert found is not None, f"{workflow} must declare job {job_id!r}"
    return found


def _step(job: Mapping[object, object], name: str) -> dict[object, object]:
    """Return one named step from a job."""
    matches = [step for step in steps(job) if step.get("name") == name]
    assert len(matches) == 1, f"expected one {name!r} step; found {len(matches)}"
    return matches[0]


def _needs(job: Mapping[object, object]) -> set[str]:
    """Normalize the GitHub Actions scalar or sequence form of `needs`."""
    declared = job.get("needs", [])
    if isinstance(declared, str):
        return {declared}
    if isinstance(declared, list):
        return {str(item) for item in declared}
    return set()


def _assert_binary_selection(command: str) -> None:
    """Require every discovered compile-contract binary in the nextest filter."""
    binaries = compile_contract_binaries(WORKFLOW_DIR.parent.parent / "tests")
    assert binaries, "source discovery found no compile-contract binaries"
    assert "cargo nextest run" in command and "--profile ci" in command, (
        "compile contracts must run uninstrumented through nextest's full ci "
        f"profile; found {command!r}"
    )
    missing = {binary for binary in binaries if f"binary({binary})" not in command}
    assert not missing, (
        f"the nextest filter omits discovered compile-contract binaries "
        f"{sorted(missing)}: {command!r}"
    )


def test_baseline_runs_compile_contracts_for_every_coverage_configuration(
    workflows: Workflows,
) -> None:
    """The main baseline cannot publish after dropping any feature run."""
    job = _job(workflows, "coverage.yml", "compile-contracts")
    strategy = job.get("strategy")
    matrix = strategy.get("matrix") if isinstance(strategy, dict) else None
    include = matrix.get("include") if isinstance(matrix, dict) else None
    assert isinstance(include, list), "baseline compile contracts need a matrix"
    configurations = {
        str(leg.get("name")): str(leg.get("flags"))
        for leg in include
        if isinstance(leg, dict)
    }
    assert configurations == BASELINE_FLAGS, (
        "baseline compile contracts must cover default, libSQL-only, and "
        f"all-features selections; found {configurations}"
    )
    command = str(_step(job, "Run compile-contract tests").get("run", ""))
    assert "${{ matrix.flags }}" in command, (
        "the baseline command must pass the matrix feature flags to Cargo"
    )
    _assert_binary_selection(command)

    gate = _job(workflows, "coverage.yml", "coverage-gate")
    assert "compile-contracts" in _needs(gate), (
        "the main coverage roll-up must fail when compile-contracts are missing"
    )


def test_pull_request_ratchet_runs_compile_contracts_before_coverage(
    workflows: Workflows,
) -> None:
    """The PR ratchet and baseline retain the same libSQL contract selection."""
    job = _job(workflows, "codescene-coverage.yml", "compile-contracts")
    command = str(_step(job, "Run compile-contract tests").get("run", ""))
    _assert_binary_selection(command)
    for required in ("--no-default-features", "--features libsql,test-helpers"):
        assert required in command, (
            f"PR compile contracts must use the ratchet's libSQL selection; "
            f"missing {required!r} in {command!r}"
        )
    coverage = _job(workflows, "codescene-coverage.yml", "coverage-check")
    assert "compile-contracts" in _needs(coverage), (
        "the pull-request coverage lane must wait for compile-contract tests"
    )


def test_windows_still_runs_the_non_unix_startup_fixture(workflows: Workflows) -> None:
    """The platform-gated fixture stays covered with useful diagnostics."""
    job = _job(workflows, "test.yml", "windows-build")
    step = _step(job, "Run non-Unix startup compile contract")
    assert step.get("if") == "matrix.name == 'default'", (
        "the startup fixture must run on the default Windows feature leg"
    )
    assert step.get("shell") == "pwsh" and step.get("run") == (
        "./scripts/ci-startup-compile-contract.ps1"
    ), "the Windows startup fixture must use the process-sampling runner"
    script = read_source(REPOSITORY_ROOT / "scripts/ci-startup-compile-contract.ps1")
    for required in (
        "'--features', 'test-helpers'",
        "'--test', 'trybuild'",
        "'startup_compile_contracts'",
        "'--', '--nocapture'",
        "Get-CimInstance Win32_Process",
        "TRYBUILD_LOCK",
        "NESTED_CARGO_OUTPUT",
    ):
        assert required in script, (
            f"the startup runner omits diagnostic or fixture argument {required!r}"
        )


@pytest.mark.parametrize(
    ("workflow", "job_id", "step_name", "action"),
    [
        ("coverage.yml", "coverage", "Generate coverage", False),
        (
            "coverage.yml",
            "coverage",
            "Generate coverage and the ratchet baseline",
            True,
        ),
        ("codescene-coverage.yml", "coverage-check", "Generate coverage", True),
    ],
)
def test_instrumented_coverage_uses_the_filtered_profile(
    workflows: Workflows,
    workflow: str,
    job_id: str,
    step_name: str,
    action: bool,
) -> None:
    """Both sides of the ratchet leave fixture compilation to the ci job."""
    step = _step(_job(workflows, workflow, job_id), step_name)
    if action:
        environment = step.get("env")
        assert isinstance(environment, dict)
        assert environment.get("NEXTEST_PROFILE") == "coverage", (
            f"{workflow}:{step_name} must use the filtered coverage profile"
        )
    else:
        command = str(step.get("run", ""))
        assert "--profile coverage" in command, (
            f"{workflow}:{step_name} must use the filtered coverage profile"
        )

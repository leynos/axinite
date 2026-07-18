"""Contract tests for the isolated CodeScene pull-request coverage workflow.

Axinite's main coverage workflow retains its PostgreSQL matrix, E2E coverage,
Codecov uploads, and aggregate gate. The pull-request workflow deliberately
isolates the proven libsql-only report path so those unrelated main-only legs
cannot block CodeScene's changed-line coverage check.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import re
from pathlib import Path

import yaml

SHA_RE = re.compile(r"^[0-9a-f]{40}$")

WORKFLOW_PATH = (
    Path(__file__).resolve().parents[2]
    / ".github"
    / "workflows"
    / "codescene-coverage.yml"
)


def _load() -> dict[str, object]:
    """Parse the CodeScene coverage workflow."""
    workflow = yaml.safe_load(WORKFLOW_PATH.read_text(encoding="utf-8"))
    assert isinstance(workflow, dict), "the workflow must parse as a mapping"
    return workflow


def _job(workflow: dict[str, object]) -> dict[str, object]:
    """Return the isolated coverage job."""
    jobs = workflow.get("jobs")
    assert isinstance(jobs, dict), "the workflow must declare a jobs mapping"
    job = jobs.get("coverage-check")
    assert isinstance(job, dict), "the workflow must declare coverage-check"
    return job


def _steps(job: dict[str, object]) -> list[dict[str, object]]:
    """Return the coverage job's ordered step mappings."""
    steps = job.get("steps")
    assert isinstance(steps, list), "coverage-check must declare steps"
    assert all(isinstance(step, dict) for step in steps), (
        "every coverage-check step must be a mapping"
    )
    return [step for step in steps if isinstance(step, dict)]


def _find_step(job: dict[str, object], name: str) -> dict[str, object]:
    """Return a named coverage step."""
    matches = [step for step in _steps(job) if step.get("name") == name]
    assert len(matches) == 1, f"expected exactly one {name!r} step"
    return matches[0]


def test_trigger_permissions_and_job_are_pr_only_and_isolated() -> None:
    """The workflow runs one least-privilege job only for PRs to main."""
    workflow = _load()
    assert workflow.get("on") == {"pull_request": {"branches": ["main"]}}, (
        "the CodeScene workflow must trigger only for pull requests to main"
    )
    assert workflow.get("permissions") == {"contents": "read"}, (
        "the CodeScene workflow must grant only read access to contents"
    )

    jobs = workflow.get("jobs")
    assert isinstance(jobs, dict), "the workflow must declare a jobs mapping"
    assert list(jobs) == ["coverage-check"], (
        "the workflow must contain only the isolated coverage-check job"
    )

    job = _job(workflow)
    assert job.get("if") == "github.event_name == 'pull_request'", (
        "coverage-check must retain an explicit pull-request guard"
    )
    assert job.get("runs-on") == "ubuntu-latest", (
        "coverage-check must use the proven Ubuntu runner"
    )
    assert {"strategy", "services", "needs"}.isdisjoint(job), (
        "coverage-check must not inherit the main matrix, PostgreSQL, or gate"
    )
    assert all(
        not str(step.get("uses", "")).startswith("codecov/") for step in _steps(job)
    ), "the isolated CodeScene job must not request Codecov OIDC uploads"


def test_setup_and_generator_match_proven_libsql_coverage() -> None:
    """The isolated job copies the proven libsql-only setup and generator."""
    job = _job(_load())
    steps = _steps(job)
    identities = [step.get("name", step.get("uses")) for step in steps]
    assert identities == [
        "Free disk space",
        "actions/checkout@v6",
        "dtolnay/rust-toolchain@stable",
        "Install clang",
        "Install mold",
        "Swatinem/rust-cache@v2",
        "Install cargo-llvm-cov",
        "Install cargo-nextest",
        "Install cargo-component",
        "Build GitHub WASM tool (for metadata/schema tests)",
        "Build WASM channels (for integration tests)",
        "Generate coverage",
        "Check coverage against CodeScene gates",
        "Trim build artefacts before cache save",
    ], "coverage-check setup, report, check, and cleanup steps must stay ordered"

    checkout = next(step for step in steps if step.get("uses") == "actions/checkout@v6")
    assert checkout.get("with") == {"fetch-depth": 0}, (
        "CodeScene requires a full-history checkout"
    )

    rust = next(
        step for step in steps if step.get("uses") == "dtolnay/rust-toolchain@stable"
    )
    assert rust.get("with") == {
        "components": "llvm-tools-preview",
        "targets": "wasm32-wasip2",
    }, "coverage-check must install the proven Rust components and WASM target"

    cache = next(step for step in steps if step.get("uses") == "Swatinem/rust-cache@v2")
    assert cache.get("with") == {"key": "coverage-libsql-only"}, (
        "the cache key must remain scoped to libsql-only coverage"
    )
    assert _find_step(job, "Install cargo-llvm-cov").get("uses") == (
        "taiki-e/install-action@cargo-llvm-cov"
    ), "coverage-check must reuse the proven cargo-llvm-cov installer"
    assert _find_step(job, "Install cargo-nextest").get("uses") == (
        "taiki-e/install-action@cargo-nextest"
    ), "coverage-check must reuse the proven cargo-nextest installer"
    assert _find_step(job, "Install cargo-component").get("with") == {
        "tool": "cargo-component"
    }, "coverage-check must install cargo-component for the WASM fixtures"
    assert (
        _find_step(job, "Build GitHub WASM tool (for metadata/schema tests)").get("run")
        == "make build-github-tool-wasm"
    ), "coverage-check must build the GitHub WASM fixture"
    assert (
        _find_step(job, "Build WASM channels (for integration tests)").get("run")
        == "./scripts/build-wasm-extensions.sh --channels"
    ), "coverage-check must build the WASM channel fixtures"

    generator = _find_step(job, "Generate coverage").get("run")
    assert isinstance(generator, str), "Generate coverage must declare a command"
    assert " ".join(generator.split()) == (
        "cargo llvm-cov nextest --no-default-features --features libsql "
        "--features test-helpers --workspace --lcov --output-path lcov.info"
    ), "coverage-check must preserve the proven libsql-only LCOV generator"


def test_codescene_check_uses_canonical_guard_and_inputs() -> None:
    """The report is submitted once through the canonical guarded check step."""
    job = _job(_load())
    steps = _steps(job)
    codescene_steps = [
        step
        for step in steps
        if str(step.get("uses", "")).startswith(
            "leynos/shared-actions/.github/actions/upload-codescene-coverage@"
        )
    ]
    assert len(codescene_steps) == 1, (
        "coverage-check must contain exactly one CodeScene submission step"
    )
    check = codescene_steps[0]
    generator_index = steps.index(_find_step(job, "Generate coverage"))
    assert steps.index(check) == generator_index + 1, (
        "the CodeScene check must immediately follow report generation"
    )
    codescene_ref = str(check.get("uses", "")).split("@")[-1]
    assert SHA_RE.match(codescene_ref), (
        "coverage-check must pin the CodeScene action to a full commit SHA, "
        f"got {codescene_ref!r}"
    )
    assert check.get("env") == {"CS_ACCESS_TOKEN": "${{ secrets.CS_ACCESS_TOKEN }}"}, (
        "the CodeScene token must remain scoped to the check step"
    )
    assert check.get("if") == (
        "github.event_name == 'pull_request' && env.CS_ACCESS_TOKEN != ''"
    ), "the CodeScene step must guard its pull-request secret"
    assert check.get("with") == {
        "format": "lcov",
        "mode": "check",
        "project-url": "https://api.codescene.io/v2/projects/77987",
        "access-token": "${{ env.CS_ACCESS_TOKEN }}",
        "installer-checksum": "${{ vars.CODESCENE_CLI_SHA256 }}",
    }, "the CodeScene step must use the canonical project and check-mode inputs"

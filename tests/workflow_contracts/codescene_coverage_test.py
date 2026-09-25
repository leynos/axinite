"""Contract tests for the isolated pull-request coverage ratchet workflow.

Axinite's main coverage workflow retains its PostgreSQL matrix, E2E coverage,
Codecov uploads, and aggregate gate, and it is the one CodeScene publisher.
The pull-request workflow deliberately isolates the libsql-only report path,
and ratchets it against the baseline that workflow writes, so the unrelated
main-only legs cannot block it. It never contacts CodeScene; that half of the
rule is `coverage_publication_test.py`'s.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import re
from pathlib import Path

import yaml

SHA_RE: re.Pattern[str] = re.compile(r"[0-9a-f]{40}")

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


def _collapse(value: object) -> str:
    """Collapse a folded YAML condition to a single spaced line."""
    return " ".join(str(value).split())


def _find_step(job: dict[str, object], name: str) -> dict[str, object]:
    """Return a named coverage step."""
    matches = [step for step in _steps(job) if step.get("name") == name]
    assert len(matches) == 1, f"expected exactly one {name!r} step"
    return matches[0]


def test_trigger_permissions_and_job_are_pr_only_and_isolated() -> None:
    """The workflow runs one least-privilege job for PRs to main or a dispatch."""
    workflow = _load()
    # `workflow_dispatch` carries no inputs on purpose: the Actions UI and
    # `gh workflow run --ref` already choose the ref, and a branch input would
    # be a second, unvalidated way to say the same thing.
    assert workflow.get("on") == {
        "pull_request": {"branches": ["main"]},
        "workflow_dispatch": None,
    }, (
        "the CodeScene workflow must trigger for pull requests to main and "
        "for a manual warm-cache dispatch, and for nothing else"
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
    assert _collapse(job.get("if")) == (
        "github.event_name == 'pull_request' || "
        "github.event_name == 'workflow_dispatch'"
    ), "coverage-check must run only for a pull request or a manual dispatch"
    assert job.get("name") == "Coverage Ratchet", (
        "the job's check name must say what it does now: it ratchets, and it "
        "no longer checks anything with CodeScene"
    )
    assert job.get("runs-on") == "ubicloud-standard-4", (
        "coverage-check is off the critical path, so it takes the cheaper "
        "shape: 683 s at half the rate beats 455 s at full"
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
        "Start resource sampler",
        "dtolnay/rust-toolchain@stable",
        "Install clang",
        "Install mold",
        "Export the Actions cache endpoint for sccache",
        "Install sccache",
        "Start sccache statistics",
        "Restore Cargo registry and index",
        "Install cargo-binstall",
        "Install cargo-component",
        "Probe Cargo tooling",
        "Build GitHub WASM tool (for metadata/schema tests)",
        "Build WASM channels (for integration tests)",
        "Generate coverage",
        "Report sccache statistics",
        "Report resource peaks",
    ], "coverage-check setup and report steps must stay ordered"

    rust = next(
        step for step in steps if step.get("uses") == "dtolnay/rust-toolchain@stable"
    )
    assert rust.get("with") == {
        "components": "llvm-tools-preview",
        "targets": "wasm32-wasip2",
    }, "coverage-check must install the proven Rust components and WASM target"

    # The registry cache carries no compiler output. sccache owns that from
    # the migration wave onwards; a target archive here would be a second
    # owner of the same state. The generic cache-ownership and tool-install
    # contracts in workflow_policy_test.py cover the key and pin shapes.
    cache = _find_step(job, "Restore Cargo registry and index")
    cache_with = cache.get("with")
    assert isinstance(cache_with, dict), "the cache step must declare inputs"
    assert "target" not in str(cache_with.get("path", "")), (
        "coverage-check must not archive a target tree"
    )
    # cargo-llvm-cov and cargo-nextest come from generate-coverage, which
    # installs its own pinned, checksum-verified releases.
    for tool in ("cargo-binstall",):
        step = _find_step(job, f"Install {tool}")
        step_with = step.get("with")
        assert isinstance(step_with, dict), f"Install {tool} must declare inputs"
        assert str(step_with.get("tool", "")).startswith(f"{tool}@"), (
            f"coverage-check must pin the {tool} version"
        )
        assert step_with.get("fallback") == "none", (
            f"the {tool} installer must fail closed rather than build from source"
        )
    # cargo-component has no install-action manifest, so it comes from
    # cargo-binstall with fail-closed strategies and a pinned version.
    component_step = _find_step(job, "Install cargo-component")
    component = str(component_step.get("run", ""))
    assert "--strategies crate-meta-data,quick-install" in component, (
        "cargo-component must be installed with fail-closed binstall strategies"
    )
    assert "cargo-component@$CARGO_COMPONENT_PIN" in component, (
        "cargo-component must carry a version pin; strategies alone stop a "
        "source build but not version drift"
    )
    assert "${{" not in component, (
        "the installer must read the pin from the step environment; a "
        "${{ }} expression is substituted into the script before the shell "
        "sees it, which is the shape that makes a run: body injectable"
    )
    pin = (component_step.get("env") or {}).get("CARGO_COMPONENT_PIN")
    assert pin == "${{ env.CARGO_COMPONENT_VERSION }}", (
        "the pin must come from the job's CARGO_COMPONENT_VERSION, so one "
        "edit moves every installer in the workflow"
    )
    # The alias only means something if the job actually defines the version.
    job_env = job.get("env")
    assert isinstance(job_env, dict), "coverage-check must declare a job env"
    declared_version = job_env.get("CARGO_COMPONENT_VERSION")
    assert isinstance(declared_version, str) and declared_version.strip(), (
        "coverage-check must define a non-empty CARGO_COMPONENT_VERSION; "
        "without it the step-local alias resolves to nothing and the "
        "installer silently loses its version pin"
    )
    assert (
        _find_step(job, "Build GitHub WASM tool (for metadata/schema tests)").get("run")
        == "make build-github-tool-wasm"
    ), "coverage-check must build the GitHub WASM fixture"
    assert (
        _find_step(job, "Build WASM channels (for integration tests)").get("run")
        == "./scripts/build-wasm-extensions.sh --channels"
    ), "coverage-check must build the WASM channel fixtures"

    generator = _find_step(job, "Generate coverage")
    assert generator.get("with") == {
        "features": "libsql,test-helpers",
        "with-default-features": "false",
        "use-cargo-nextest": "true",
        "cargo-wait-timeout": "3600",
        "format": "lcov",
        "output-path": "lcov.info",
        "with-ratchet": "true",
        "publish-artefact": "false",
        "cache-provider": "external",
    }, (
        "coverage-check must measure the libsql-only selection through "
        "generate-coverage, ratchet it, publish nothing, leave the registry "
        "and compiler caches to their existing owners, and set the cargo "
        "watchdog that timeout_ordering_test.py orders"
    )

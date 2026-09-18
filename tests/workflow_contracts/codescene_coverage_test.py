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
        "Install cargo-llvm-cov",
        "Install cargo-nextest",
        "Install cargo-binstall",
        "Install cargo-component",
        "Probe Cargo tooling",
        "Build GitHub WASM tool (for metadata/schema tests)",
        "Build WASM channels (for integration tests)",
        "Generate coverage",
        "Check coverage against CodeScene gates",
        "Report sccache statistics",
        "Report resource peaks",
    ], "coverage-check setup, report, and check steps must stay ordered"

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
    for tool in ("cargo-llvm-cov", "cargo-nextest", "cargo-binstall"):
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
    assert SHA_RE.fullmatch(codescene_ref), (
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
    }, "the CodeScene step must use the canonical project and check-mode inputs"


#: The input the uploader no longer accepts, and the estate no longer passes.
WITHDRAWN_INPUT = "installer-checksum"

#: The prefix every uploader step's `uses` shares.
UPLOADER = "leynos/shared-actions/.github/actions/upload-codescene-coverage@"


#: The extensions GitHub accepts for a workflow file.
WORKFLOW_SUFFIXES = frozenset({".yml", ".yaml"})

#: One uploader step, located: its workflow file, its job's key, and itself.
UploaderStep = tuple[str, str, dict[str, object]]


def _step_mappings(job: object) -> list[dict[str, object]]:
    """Return a job's steps, dropping anything that is not a mapping.

    Parameters
    ----------
    job
        A parsed job body, which a malformed workflow may make anything at all.

    Returns
    -------
    list of dict
        The step mappings, in order. Empty when the job declares no usable
        steps, so a malformed job is skipped rather than half-read.
    """
    steps = job.get("steps") if isinstance(job, dict) else None
    if not isinstance(steps, list):
        return []
    return [step for step in steps if isinstance(step, dict)]


def _jobs_in(document: object) -> dict[str, object]:
    """Return a parsed workflow's jobs mapping, empty when it declares none.

    Parameters
    ----------
    document
        A parsed workflow file.

    Returns
    -------
    dict
        The jobs, keyed by job ID.
    """
    jobs = document.get("jobs") if isinstance(document, dict) else None
    return jobs if isinstance(jobs, dict) else {}


def _uploader_steps_in(workflow: str, document: object) -> list[UploaderStep]:
    """Return every uploader step one parsed workflow declares.

    Parameters
    ----------
    workflow
        The file name, carried through so a failure names the file.
    document
        The parsed workflow.

    Returns
    -------
    list of UploaderStep
        One entry per step whose `uses` names the CodeScene uploader.
    """
    return [
        (workflow, str(job_id), step)
        for job_id, job in _jobs_in(document).items()
        for step in _step_mappings(job)
        if str(step.get("uses", "")).startswith(UPLOADER)
    ]


def _uploader_steps() -> list[UploaderStep]:
    """Return every step in the estate that runs the CodeScene uploader.

    Returns
    -------
    list of UploaderStep
        The workflow file name, the job's key, and the step mapping, for each
        uploader step found anywhere in `.github/workflows`.
    """
    return [
        found
        for path in sorted(WORKFLOW_PATH.parent.iterdir())
        if path.suffix in WORKFLOW_SUFFIXES
        for found in _uploader_steps_in(
            path.name, yaml.safe_load(path.read_text(encoding="utf-8"))
        )
    ]


def test_no_uploader_step_passes_the_withdrawn_installer_checksum() -> None:
    """Every uploader step must stop naming an input the action will reject.

    The next version of the uploader refuses `installer-checksum` outright, so
    a step still passing it fails the whole job rather than the input. The
    variable it read is not the replacement either: `CODESCENE_CLI_SHA256`
    holds the installer script's digest, and the input that replaces this one
    wants the archive's, so renaming it would pass a digest of the wrong
    artefact and fail verification instead of argument parsing.

    The estate is scanned rather than one file, because the input appeared in
    two workflows and a contract on either alone would have passed while the
    other broke. The count is asserted first: every assertion below is
    satisfied by finding no uploader steps at all.
    """
    steps = _uploader_steps()
    assert len(steps) >= 2, (
        f"the scan found {len(steps)} uploader steps; it is meant to find the "
        "check step and the upload step, and a scan that finds none passes "
        "this contract with the input restored"
    )
    for workflow, job_id, step in steps:
        inputs = step.get("with")
        declared = set(inputs) if isinstance(inputs, dict) else set()
        assert WITHDRAWN_INPUT not in declared, (
            f"{workflow}:{job_id} passes {WITHDRAWN_INPUT!r} to the CodeScene "
            "uploader. The action no longer accepts it, so the step fails on "
            "the argument; and it must be removed rather than renamed, "
            "because the value it reads is the installer script's digest, not "
            "the archive's"
        )

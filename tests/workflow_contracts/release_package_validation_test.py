"""Keep native package validation complete and unable to publish a release.

The generated release workflow is tag-only. This separate PR path executes
its cargo-dist local-to-global hand-off on the candidate SHA and retains the
manifests, packages and checksums without release-write permission.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import shlex
import subprocess

import pytest

from _build_standard_workflows import Step, steps_of
from _estate import read_workflow
from _sources import read_toml
from _strict_workflows import triggers
from _workflow_policy import REPOSITORY_ROOT, WORKFLOW_DIR

WORKFLOW = "release-package-validation.yml"
CANDIDATE_REF = "${{ github.event.pull_request.head.sha || github.sha }}"
GENERATED_MATRIX = (
    "${{ fromJson(needs.plan.outputs.manifest).ci.github.artifacts_matrix }}"
)
EXPECTED_TARGETS = {
    "aarch64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
}
APPLE_RUNNER_MAPPINGS = {
    "aarch64-apple-darwin": "macos-15",
    "x86_64-apple-darwin": "macos-15-intel",
}
# Images GitHub has retired or scheduled to retire. Keeping this list explicit
# means a future migration has to delete an entry rather than silently reuse
# the label.
RETIRED_RUNNER_LABELS = frozenset({"macos-14"})


def _mapping(value: object, label: str) -> dict[str, object]:
    """Return a declared mapping with a useful failure if its shape changes."""
    assert isinstance(value, dict), f"{label} must be a mapping, got {value!r}"
    return value


def _job(workflow: dict[str, object], name: str) -> dict[str, object]:
    """Return one required validation job."""
    jobs = _mapping(workflow.get("jobs"), "workflow jobs")
    return _mapping(jobs.get(name), f"job {name}")


def _step(job: dict[str, object], name: str) -> Step:
    """Return one required validation step, naming an omission."""
    found = [step for step in steps_of(job) if step.name == name]
    assert len(found) == 1, f"expected one step named {name!r}, found {len(found)}"
    return found[0]


def _declared_step(job: dict[str, object], name: str) -> dict[str, object]:
    """Return a raw step when the contract needs its exact condition."""
    declared = job.get("steps")
    assert isinstance(declared, list), "validation steps must be a list"
    found = [
        step for step in declared if isinstance(step, dict) and step.get("name") == name
    ]
    assert len(found) == 1, f"expected one declared step {name!r}"
    return found[0]


def _deployment_target_awk(script: str) -> str:
    """Return the awk program the evidence step applies to ``otool -l`` output."""
    marker = "awk '"
    start = script.index(marker) + len(marker)
    end = script.index("'", start)
    program = script[start:end]
    assert "LC_VERSION_MIN_MACOSX" in program, (
        "the deployment-target parser must understand the legacy load command"
    )
    return program


def _read_deployment_target(program: str, otool_output: str) -> list[str]:
    """Run the workflow's parser over recorded ``otool -l`` output."""
    finished = subprocess.run(  # noqa: S603 S607 - fixed awk, workflow-owned text
        ["awk", program],
        input=otool_output,
        check=True,
        capture_output=True,
        text=True,
    )
    return finished.stdout.splitlines()


def _architecture_case(script: str) -> str:
    """Return the shell ``case`` block that maps a target to its architecture."""
    start = script.index('case "$target" in')
    end = script.index("esac", start) + len("esac")
    return script[start:end]


def _expected_architecture(
    case_block: str, target: str
) -> subprocess.CompletedProcess[str]:
    """Run the workflow's target mapping for one release target."""
    program = (
        f"target={shlex.quote(target)}\n"
        f"{case_block}\n"
        "printf '%s' \"$expected_arch\""
    )
    return subprocess.run(  # noqa: S603 S607 - fixed bash, workflow-owned text
        ["bash", "-c", program],
        check=False,
        capture_output=True,
        text=True,
    )


@pytest.fixture(scope="module")
def workflow() -> dict[str, object]:
    """Read the validation workflow through the shared fallible boundary."""
    return read_workflow(WORKFLOW_DIR / WORKFLOW)


@pytest.fixture(scope="module")
def dist_configuration() -> dict[str, object]:
    """Read the configured release targets and cargo-dist version."""
    manifest = read_toml(REPOSITORY_ROOT / "Cargo.toml")
    workspace = _mapping(manifest.get("workspace"), "Cargo workspace")
    metadata = _mapping(workspace.get("metadata"), "workspace metadata")
    return _mapping(metadata.get("dist"), "cargo-dist configuration")


def test_package_validation_runs_for_release_input_changes(
    workflow: dict[str, object],
) -> None:
    """A release-input PR can gather package evidence before any tag exists."""
    events = triggers(workflow)
    assert set(events) == {"pull_request", "workflow_dispatch"}, (
        "package validation must run on PRs and remain manually available"
    )
    pull_request = _mapping(events["pull_request"], "pull-request trigger")
    assert {f".github/workflows/{WORKFLOW}", "rust-toolchain.toml"} <= set(
        pull_request.get("paths", [])
    ), "the workflow and toolchain must trigger their own package validation"


def test_package_validation_cannot_publish(workflow: dict[str, object]) -> None:
    """Only read access and artefact actions are available to the build jobs."""
    assert workflow.get("permissions") == {"contents": "read"}, (
        "package validation must not receive release-write permission"
    )
    jobs = _mapping(workflow.get("jobs"), "workflow jobs")
    assert set(jobs) == {"plan", "build-local-artefacts", "build-global-artefacts"}, (
        "the validation path must contain only planning and package-build jobs"
    )
    for name in jobs:
        job = _job(workflow, name)
        assert "permissions" not in job, f"{name} must inherit read-only permissions"
        for step in steps_of(job):
            assert not step.uses or step.uses.startswith(
                (
                    "actions/checkout@",
                    "actions/upload-artifact@",
                    "actions/download-artifact@",
                )
            ), f"{name} uses an unexpected action {step.uses!r}"
            assert not any(
                command in step.run for command in ("dist host", "gh release", "--tag")
            ), f"{name}:{step.name} attempts a release operation"


def test_local_packages_follow_the_complete_release_plan(
    workflow: dict[str, object], dist_configuration: dict[str, object]
) -> None:
    """The native matrix comes from dist rather than a second target list."""
    assert set(dist_configuration.get("targets", [])) == EXPECTED_TARGETS, (
        "the release target promise must include all five native targets"
    )
    plan = _job(workflow, "plan")
    assert "dist plan --output-format=json" in _step(plan, "Plan package builds").run, (
        "the native matrix must be derived by cargo-dist on the candidate"
    )
    local = _job(workflow, "build-local-artefacts")
    assert (
        _mapping(local.get("strategy"), "local strategy").get("matrix")
        == GENERATED_MATRIX
    ), "the validation workflow must not maintain a second release matrix"
    assert local.get("runs-on") == "${{ matrix.runner }}", (
        "each target needs its native runner"
    )
    assert _step(local, "Install dist").run == "${{ matrix.install_dist.run }}", (
        "each native host must use its release-plan installer"
    )
    assert (
        _step(local, "Install dependencies").run == "${{ matrix.packages_install }}"
    ), "native dependencies must follow the release plan"
    assert "${{ matrix.dist_args }}" in _step(local, "Build local artefacts").run, (
        "the local build must use its target-specific release arguments"
    )


def test_apple_release_runners_avoid_retired_images(
    dist_configuration: dict[str, object],
) -> None:
    """Each Apple target keeps a supported image and its native architecture."""
    runners = _mapping(
        dist_configuration.get("github-custom-runners"),
        "github custom runners",
    )
    assert {
        target: runners.get(target) for target in APPLE_RUNNER_MAPPINGS
    } == APPLE_RUNNER_MAPPINGS, (
        "Apple Silicon must use the arm64 macos-15 image and Intel must keep "
        "macos-15-intel"
    )
    retired = RETIRED_RUNNER_LABELS.intersection(
        value for value in runners.values() if isinstance(value, str)
    )
    assert not retired, f"release runners still use retired labels: {sorted(retired)}"


def test_macos_packages_record_toolchain_and_binary_evidence(
    workflow: dict[str, object],
) -> None:
    """Apple builds report the host, SDK and binary architecture they produced."""
    local = _job(workflow, "build-local-artefacts")
    step_name = "Record macOS toolchain and binary evidence"
    declared = _declared_step(local, step_name)
    assert declared.get("if") == "always() && runner.os == 'macOS'", (
        "the evidence step must run for both Apple legs, even after a failure"
    )

    evidence = _step(local, step_name)
    assert "set -euo pipefail" in evidence.run, (
        "the evidence step must fail when extraction or a command fails"
    )
    for command in (
        "sw_vers",
        "uname -m",
        "rustc -vV",
        "xcodebuild -version",
        "xcode-select -p",
        "xcrun --show-sdk-version",
        "xcrun --show-sdk-path",
        "lipo -archs",
        "LC_BUILD_VERSION",
        "LC_VERSION_MIN_MACOSX",
        "minos: ",
        "sdk: ",
        "validation-context.txt",
        "arm64",
        "x86_64",
    ):
        assert command in evidence.run, (
            f"the macOS evidence step must record {command!r}"
        )

    declared_steps = local.get("steps")
    assert isinstance(declared_steps, list), "local build steps must be a list"
    step_names = [
        step.get("name") for step in declared_steps if isinstance(step, dict)
    ]
    assert step_names.index(step_name) > step_names.index("Verify local checksums"), (
        "binary evidence must follow the checksum check it supplements"
    )


@pytest.mark.parametrize(
    ("load_command", "expected"),
    [
        (
            "LC_BUILD_VERSION\n  cmdsize 32\n platform 1\n    minos 11.0\n"
            "      sdk 15.5",
            ["LC_BUILD_VERSION minos: 11.0", "LC_BUILD_VERSION sdk: 15.5"],
        ),
        (
            "LC_VERSION_MIN_MACOSX\n  cmdsize 16\n  version 10.12\n"
            "      sdk 15.5",
            [
                "LC_VERSION_MIN_MACOSX minos: 10.12",
                "LC_VERSION_MIN_MACOSX sdk: 15.5",
            ],
        ),
    ],
)
def test_macos_deployment_target_parser_reads_otool_output(
    workflow: dict[str, object], load_command: str, expected: list[str]
) -> None:
    """The parser extracts a minimum version and SDK from either load command."""
    local = _job(workflow, "build-local-artefacts")
    evidence = _step(local, "Record macOS toolchain and binary evidence")
    program = _deployment_target_awk(evidence.run)
    otool_output = f"Load command 12\n      cmd {load_command}\n"
    assert _read_deployment_target(program, otool_output) == expected


@pytest.mark.parametrize(
    ("target", "expected_arch"),
    [
        ("aarch64-apple-darwin", "arm64"),
        ("x86_64-apple-darwin", "x86_64"),
    ],
)
def test_macos_evidence_step_maps_each_apple_target_to_its_architecture(
    workflow: dict[str, object], target: str, expected_arch: str
) -> None:
    """The step expects the architecture its Apple target actually produces."""
    local = _job(workflow, "build-local-artefacts")
    evidence = _step(local, "Record macOS toolchain and binary evidence")
    finished = _expected_architecture(_architecture_case(evidence.run), target)
    assert finished.returncode == 0, finished.stderr
    assert finished.stdout == expected_arch


def test_macos_evidence_step_rejects_an_unplanned_apple_target(
    workflow: dict[str, object],
) -> None:
    """A macOS leg outside the two planned targets cannot claim evidence."""
    local = _job(workflow, "build-local-artefacts")
    evidence = _step(local, "Record macOS toolchain and binary evidence")
    case_block = _architecture_case(evidence.run)
    finished = _expected_architecture(case_block, "aarch64-apple-ios")
    assert finished.returncode == 1
    assert "unexpected macOS release target" in finished.stderr


@pytest.mark.parametrize(
    "job_id", ["plan", "build-local-artefacts", "build-global-artefacts"]
)
def test_package_jobs_use_the_candidate_pin_and_release_flags(
    workflow: dict[str, object], dist_configuration: dict[str, object], job_id: str
) -> None:
    """Every job checks out the same head and excludes development RUSTFLAGS."""
    job = _job(workflow, job_id)
    assert _step(job, "Checkout candidate").inputs.get("ref") == CANDIDATE_REF, (
        f"{job_id} must package the PR head rather than a different ref"
    )
    assert (
        _step(job, "Checkout candidate").inputs.get("persist-credentials") is False
    ), f"{job_id} must leave no checkout credential in candidate Git config"
    assert (
        _step(job, "Install Rust").run == "rustup toolchain install --profile minimal"
    ), f"{job_id} must follow rust-toolchain.toml"
    assert _mapping(job.get("env"), f"{job_id} environment").get("RUSTFLAGS") == "", (
        f"{job_id} must exclude the development frontend and linker flags"
    )
    assert _mapping(workflow.get("env"), "workflow environment").get(
        "CARGO_DIST_VERSION"
    ) == (dist_configuration.get("cargo-dist-version")), (
        "the validation installer must match the configured cargo-dist pin"
    )
    if job_id != "build-local-artefacts":
        assert "v${CARGO_DIST_VERSION}" in _step(job, "Install dist").run, (
            f"{job_id} must use the declared cargo-dist installer pin"
        )


def test_global_packages_require_every_local_artefact(
    workflow: dict[str, object],
) -> None:
    """Global installers and checksums consume all successful native packages."""
    local = _job(workflow, "build-local-artefacts")
    global_job = _job(workflow, "build-global-artefacts")
    assert set(global_job.get("needs", [])) == {"plan", "build-local-artefacts"}, (
        "global packaging must wait for the complete native matrix"
    )
    upload = _step(local, "Upload local artefacts")
    download = _step(global_job, "Fetch every local artefact")
    assert (
        upload.inputs.get("name")
        == "release-validation-local-${{ join(matrix.targets, '_') }}"
    ), "each native package needs a distinct workflow artefact"
    assert download.inputs.get("pattern") == "release-validation-local-*", (
        "the global job must download every local package"
    )
    assert download.inputs.get("merge-multiple") is True, (
        "the local packages must share cargo-dist's expected output directory"
    )
    assert "${{ env.BUILD_MANIFEST_NAME }}" in str(upload.inputs.get("path")), (
        "local package uploads must include their target-specific manifest"
    )
    handoff = _step(global_job, "Verify complete local hand-off").run
    assert (
        ".ci.github.artifacts_matrix.include[]" in handoff
        and ".upload_files[]" in handoff
    ), "the hand-off must verify every planned manifest and its declared files"
    assert (
        "dist build --output-format=json --artifacts=global"
        in _step(global_job, "Build global artefacts").run
    ), "the final job must execute cargo-dist's global packaging path"
    assert (
        "sha256sum -c sha256.sum" in _step(global_job, "Verify combined checksums").run
    ), "the combined package set must pass the generated unified checksum file"


@pytest.mark.parametrize(
    ("job_id", "step_name"),
    [
        ("build-local-artefacts", "Upload local evidence"),
        ("build-global-artefacts", "Upload global evidence"),
    ],
)
def test_package_evidence_survives_a_build_failure(
    workflow: dict[str, object], job_id: str, step_name: str
) -> None:
    """Retain the manifest, compiler log, tool identity and checksum results."""
    job = _job(workflow, job_id)
    evidence = _step(job, step_name)
    assert _declared_step(job, step_name).get("if") == "always()", (
        "evidence upload must run after a failed package command"
    )
    assert {
        "dist-manifest.json",
        "dist-build.log",
        "validation-context.txt",
        "checksum-results.txt",
    } <= set(str(evidence.inputs.get("path")).splitlines()), (
        "package evidence must retain the manifest, log, identity and checksums"
    )

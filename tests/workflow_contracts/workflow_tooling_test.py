"""Contracts for how CI installs the tools it runs.

A tool that compiles from source rebuilds a published binary on every run,
and an installer without a pinned version reaches for whatever was released
this morning. These tests pin both, and check that an installer precedes the
first use of what it installs rather than merely appearing somewhere in the
job. Cache ownership is a separate concern and lives in
`cache_ownership_test.py`.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import re
import typing as typ

import pytest
from _workflow_files import declared_jobs, jobs, workflow_paths
from _workflow_policy import SHA_RE, SOURCE_BUILD_PATTERNS, Job, step_text

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Iterator

ALL_JOBS: tuple[Job, ...] = tuple(jobs())

#: Commands whose first use must be preceded by their installer step. The
#: value is the step-name fragment that installs the command.
INSTALLER_BEFORE_USE: dict[str, str] = {
    "cargo nextest": "Install cargo-nextest",
    "cargo component": "Install cargo-component",
    "cargo llvm-cov": "Install cargo-llvm-cov",
    "cargo audit": "Install cargo-audit",
    "nixie": "Install Nixie and Merman CLI",
    "merman-cli": "Install Nixie and Merman CLI",
    "whitaker": "Install Whitaker",
}

#: Tools installed by a shared action rather than by `taiki-e/install-action`,
#: because they ship as checksum-verified release archives rather than as
#: crates. The value is the action path fragment, the version inputs it must
#: pin, and the probe step that proves the installed binary runs.
#:
#: These carry their own assertions because the generic installer contract
#: reads `taiki-e/install-action` inputs only. Without them, deleting the
#: Whitaker installer or either probe would leave the whole suite green.
SHARED_INSTALLERS: tuple[tuple[str, str, tuple[str, ...], str], ...] = (
    (
        "Install Nixie and Merman CLI",
        "leynos/shared-actions/.github/actions/install-nixie@",
        ("nixie-version", "merman-version"),
        "Probe Mermaid toolchain",
    ),
    (
        "Install Whitaker",
        "leynos/shared-actions/.github/actions/install-whitaker@",
        ("installer-version",),
        "Probe Whitaker",
    ),
)

#: What each probe must actually execute. `command -v` alone proves a name is
#: on PATH, which a broken shim also satisfies; each probe must run the binary.
#: Nixie has no `--version`, so `--help` is what proves its shim resolves to a
#: working interpreter and entry point.
PROBE_COMMANDS: dict[str, tuple[str, ...]] = {
    "Probe Mermaid toolchain": (
        "command -v nixie",
        "command -v merman-cli",
        "nixie --help",
        "merman-cli --version",
    ),
    "Probe Whitaker": ("command -v whitaker", "whitaker --version"),
}


def _ids(candidates: tuple[Job, ...]) -> list[str]:
    """Return readable parameter identifiers for a job sequence."""
    return [str(job) for job in candidates]


def _assert_no_source_build(subject: str, body: str) -> None:
    """Fail when a shell body matches any known source-build form."""
    for pattern, reason in SOURCE_BUILD_PATTERNS:
        assert not pattern.search(body), (
            f"{subject} {reason}. Use a pinned, checksum-verified release "
            "archive instead."
        )


@pytest.mark.parametrize("job", ALL_JOBS, ids=_ids(ALL_JOBS))
def test_no_job_builds_a_tool_from_source(job: Job) -> None:
    """Reject any step that compiles a CI tool instead of downloading it."""
    for step in job.steps:
        subject = f"{job} step {step.get('name', '<unnamed>')!r}"
        _assert_no_source_build(subject, step_text(step))


def _setup_commands(body: object) -> str | None:
    """Return the shell a reusable-workflow caller hands to its callee."""
    inputs = body.get("with") if isinstance(body, dict) else None
    if not isinstance(inputs, dict):
        return None
    commands = inputs.get("setup-commands")
    return commands if isinstance(commands, str) else None


def _reusable_setup_commands() -> Iterator[tuple[str, str]]:
    """Yield every reusable-workflow caller's setup shell, with its subject."""
    for path in workflow_paths():
        for job_id, body in declared_jobs(path).items():
            commands = _setup_commands(body)
            if commands is not None:
                yield f"{path.name}:{job_id} setup-commands", commands


def test_setup_commands_passed_to_reusable_workflows_avoid_source_builds() -> None:
    """Apply the same rule to shell handed to a reusable workflow."""
    for subject, commands in _reusable_setup_commands():
        _assert_no_source_build(subject, commands)


@pytest.mark.parametrize("job", ALL_JOBS, ids=_ids(ALL_JOBS))
def test_tool_installers_pin_a_version_and_fail_closed(job: Job) -> None:
    """Pin every installed tool and forbid a silent source-build fallback."""
    for step in job.steps:
        uses = step.get("uses")
        if not isinstance(uses, str) or not uses.startswith("taiki-e/install-action@"):
            continue
        ref = uses.split("@", 1)[1]
        assert SHA_RE.match(ref), (
            f"{job} pins the installer to {ref!r}; use a full commit SHA so "
            "the installer itself cannot change under the repository"
        )
        inputs = step.get("with")
        assert isinstance(inputs, dict), f"{job} installer step must declare inputs"
        tool = inputs.get("tool")
        assert isinstance(tool, str) and "@" in tool, (
            f"{job} installs {tool!r} without a version pin"
        )
        assert inputs.get("fallback") == "none", (
            f"{job} installs {tool!r} without `fallback: none`; the default "
            "fallback can compile the tool from source"
        )


@pytest.mark.parametrize("job", ALL_JOBS, ids=_ids(ALL_JOBS))
def test_binstall_invocations_pin_every_package(job: Job) -> None:
    """Pin the version of anything installed through cargo-binstall.

    `taiki-e/install-action` carries no manifest for `cargo-component`, so
    that tool comes from cargo-binstall instead. The fail-closed strategies
    keep it from compiling, and the version pin keeps the installed binary
    reproducible.
    """
    for step in job.steps:
        body = step_text(step)
        if "cargo binstall" not in body:
            continue
        packages = re.findall(r"\bcargo-[a-z0-9-]+(?:@[^\s\"']+)?", body)
        installed = [name for name in packages if name != "cargo-binstall"]
        assert installed, f"{job} runs cargo binstall without naming a package"
        for name in installed:
            assert "@" in name, f"{job} installs {name!r} without a version pin"


@pytest.mark.parametrize("job", ALL_JOBS, ids=_ids(ALL_JOBS))
def test_installers_precede_first_use(job: Job) -> None:
    """Order each installer before the first step that runs its command."""
    names = [str(step.get("name", "")) for step in job.steps]
    for command, installer in INSTALLER_BEFORE_USE.items():
        install_at = next(
            (index for index, name in enumerate(names) if name == installer), None
        )
        use_at = next(
            (
                index
                for index, step in enumerate(job.steps)
                if re.search(rf"\b{re.escape(command)}\b", step_text(step))
            ),
            None,
        )
        if use_at is None:
            continue
        assert install_at is not None, f"{job} runs {command!r} but never installs it"
        assert install_at < use_at, (
            f"{job} runs {command!r} at step {use_at} before {installer!r} at "
            f"step {install_at}"
        )


@pytest.mark.parametrize(
    ("install_step", "action", "version_inputs", "probe_step"),
    SHARED_INSTALLERS,
    ids=[entry[0] for entry in SHARED_INSTALLERS],
)
def test_each_shared_installer_is_pinned_and_probed(
    install_step: str,
    action: str,
    version_inputs: tuple[str, ...],
    probe_step: str,
) -> None:
    """Pin the shared installers and prove the binary they leave behind runs.

    These replaced `cargo install` source builds, so the installer's presence
    is the whole point of the change and nothing else asserts it. The probe
    matters because a warm cache can restore an unusable binary: the build
    would then fail somewhere far from the cause.
    """
    matches = [
        (job, step)
        for job in ALL_JOBS
        for step in job.steps
        if step.get("name") == install_step
    ]
    assert len(matches) == 1, (
        f"expected exactly one {install_step!r} step in the estate, "
        f"found {len(matches)}"
    )
    job, step = matches[0]
    uses = str(step.get("uses", ""))
    assert uses.startswith(action), f"{job} must install through {action}, not {uses!r}"
    assert SHA_RE.fullmatch(uses.split("@")[-1]), (
        f"{job} must pin {install_step!r} to a full commit SHA, got {uses!r}"
    )
    inputs = step.get("with")
    assert isinstance(inputs, dict), f"{job} {install_step!r} must declare inputs"
    for name in version_inputs:
        declared = inputs.get(name)
        assert isinstance(declared, str) and declared.strip(), (
            f"{job} {install_step!r} must pin {name}; without it the action "
            "installs whatever its default resolves to today"
        )

    names = [str(candidate.get("name", "")) for candidate in job.steps]
    assert probe_step in names, (
        f"{job} installs through {install_step!r} but never probes the result"
    )
    assert names.index(install_step) < names.index(probe_step), (
        f"{job} probes {probe_step!r} before {install_step!r} installs anything"
    )
    probe = job.steps[names.index(probe_step)]
    body = step_text(probe)
    for command in PROBE_COMMANDS[probe_step]:
        assert command in body, (
            f"{job} {probe_step!r} must run {command!r}; a probe that only "
            "checks PATH passes for a restored but unusable binary"
        )


def test_shared_action_references_are_pinned_to_a_commit() -> None:
    """Pin every leynos/shared-actions reference to a full commit SHA."""
    references = 0
    for path in workflow_paths():
        for match in re.finditer(
            r"leynos/shared-actions/[^@\s]+@(\S+)", path.read_text(encoding="utf-8")
        ):
            references += 1
            ref = match.group(1)
            assert SHA_RE.match(ref), (
                f"{path.name} references shared-actions at {ref!r}; use a full "
                "commit SHA, never a branch or tag"
            )
    assert references > 0, "the estate should still consume shared actions"

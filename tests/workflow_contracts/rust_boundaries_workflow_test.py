"""Contracts for the Rust boundary verification workflow (ADR 0015).

The workflow runs the unsafe-boundary contract and Miri. It is a proof lane,
so it stays on GitHub-hosted runners (ADR 013), bounds its own runtime, never
lets the checkout token reach the code it builds, and installs the `miri`
component in the one job that needs it. Each rule is stated against the real
workflow and against a mutation of it, so that the rule would fail if the
workflow stopped following it.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import copy
import re
import typing as typ
from pathlib import Path

import pytest

from _workflow_files import load
from _workflow_policy import WORKFLOW_DIR

WORKFLOW = WORKFLOW_DIR / "rust-boundaries.yml"
PROOF_JOBS = ("contract", "miri")

#: The command each proof job must run, and the Make target it names.
PROOF_COMMANDS = {"contract": "make boundary-contract", "miri": "make miri"}
BUILD_STANDARD_PATHS = {
    "rust-toolchain.toml",
    ".cargo/**",
    "tools/mold/**",
    "scripts/install-build-tools.sh",
}

Workflow = dict[str, typ.Any]


def _workflow() -> Workflow:
    """Read the real workflow."""
    return typ.cast("Workflow", load(WORKFLOW))


def _job(document: Workflow, name: str) -> dict[str, typ.Any]:
    """Return one job's mapping."""
    return typ.cast("dict[str, typ.Any]", document["jobs"][name])


def _steps(document: Workflow, name: str) -> list[dict[str, typ.Any]]:
    """Return one job's steps."""
    return typ.cast("list[dict[str, typ.Any]]", _job(document, name)["steps"])


def _run_text(document: Workflow, name: str) -> str:
    """Return every shell line one job runs, joined."""
    return "\n".join(str(step.get("run", "")) for step in _steps(document, name))


def hosted_violations(document: Workflow) -> list[str]:
    """Report proof jobs that leave the GitHub-hosted pool or run unbounded.

    Parameters
    ----------
    document : Workflow
        Parsed GitHub Actions workflow document.

    Returns
    -------
    list[str]
        Policy violations, or an empty list when both proof jobs meet the
        runner and timeout requirements.
    """
    problems = []
    for name in PROOF_JOBS:
        job = _job(document, name)
        if job.get("runs-on") != "ubuntu-latest":
            problems.append(f"{name}: proof lanes stay on ubuntu-latest")
        timeout = job.get("timeout-minutes")
        if not isinstance(timeout, int) or not 0 < timeout <= 120:
            problems.append(f"{name}: needs a bounded timeout-minutes")
    return problems


def credential_violations(document: Workflow) -> list[str]:
    """Report checkouts that persist the token where built code can read it.

    Parameters
    ----------
    document : Workflow
        Parsed GitHub Actions workflow document.

    Returns
    -------
    list[str]
        Policy violations, or an empty list when each proof checkout disables
        credential persistence.
    """
    problems = []
    for name in PROOF_JOBS:
        checkouts = [
            step
            for step in _steps(document, name)
            if str(step.get("uses", "")).startswith("actions/checkout@")
        ]
        if not checkouts:
            problems.append(f"{name}: no checkout step")
        problems += [
            f"{name}: checkout must set persist-credentials: false"
            for step in checkouts
            if step.get("with", {}).get("persist-credentials") is not False
        ]
    return problems


def _installs_miri(text: str) -> bool:
    """Report whether shell text adds the `miri` component to a toolchain."""
    joined = text.replace("\\\n", " ")
    return re.search(r"\bcomponent\b[^\n]*\bmiri\b", joined) is not None


def component_violations(document: Workflow) -> list[str]:
    """Report incorrect installation of the `miri` component.

    Parameters
    ----------
    document : Workflow
        Parsed GitHub Actions workflow document.

    Returns
    -------
    list[str]
        Policy violations, or an empty list when only the Miri job installs
        the component and that job installs it.
    """
    problems = []
    for name in PROOF_JOBS:
        installs_miri = _installs_miri(_run_text(document, name))
        if name == "miri" and not installs_miri:
            problems.append("miri: must install the miri component")
        if name != "miri" and installs_miri:
            problems.append(f"{name}: must not install the miri component")
    return problems


def command_violations(document: Workflow) -> list[str]:
    """Report proof jobs that no longer run their Make target.

    Parameters
    ----------
    document : Workflow
        Parsed GitHub Actions workflow document.

    Returns
    -------
    list[str]
        Missing Make-target requirements, or an empty list when every proof
        job runs its required target.
    """
    return [
        f"{name}: must run `{command}`"
        for name, command in PROOF_COMMANDS.items()
        if command not in _run_text(document, name)
    ]


def trigger_violations(document: Workflow) -> list[str]:
    """Report missing triggers, excess permissions, or unsafe cancellation.

    Parameters
    ----------
    document : Workflow
        Parsed GitHub Actions workflow document.

    Returns
    -------
    list[str]
        Trigger, permission, or concurrency policy violations, or an empty
        list when the workflow meets all three requirements.
    """
    triggers = document.get(True, document.get("on", {}))
    problems = [
        f"missing trigger: {trigger}"
        for trigger in ("pull_request", "schedule", "workflow_dispatch")
        if trigger not in triggers
    ]
    if document.get("permissions") != {"contents": "read"}:
        problems.append("permissions must be exactly contents: read")
    cancel = document.get("concurrency", {}).get("cancel-in-progress")
    if cancel != "${{ github.event_name == 'pull_request' }}":
        problems.append("cancellation must be conditioned on the pull_request event")
    return problems


def build_standard_path_violations(document: Workflow) -> list[str]:
    """Report build-standard inputs missing from the pull-request path filter.

    Parameters
    ----------
    document : Workflow
        Parsed GitHub Actions workflow document.

    Returns
    -------
    list[str]
        Missing pull-request path entries, or an empty list when all build
        standard inputs trigger the proof workflow.
    """
    triggers = document.get(True, document.get("on", {}))
    paths = set(triggers.get("pull_request", {}).get("paths", []))
    return [
        f"pull_request path filter must include {path}"
        for path in sorted(BUILD_STANDARD_PATHS - paths)
    ]


JUDGES = {
    "hosted": hosted_violations,
    "credentials": credential_violations,
    "component": component_violations,
    "commands": command_violations,
    "triggers": trigger_violations,
    "build-standard-paths": build_standard_path_violations,
}


@pytest.mark.parametrize("judge", JUDGES.values(), ids=JUDGES.keys())
def test_the_workflow_follows_each_rule(
    judge: typ.Callable[[Workflow], list[str]],
) -> None:
    """The real workflow satisfies every rule."""
    problems = judge(_workflow())
    assert problems == [], f"{judge.__name__}: workflow violates its rule: {problems}"


def _drop_timeout(document: Workflow) -> None:
    """Remove the Miri job's timeout bound."""
    del _job(document, "miri")["timeout-minutes"]


def _move_to_paid_runner(document: Workflow) -> None:
    """Move the contract job onto a paid runner."""
    _job(document, "contract")["runs-on"] = "ubicloud-standard-4"


def _persist_credentials(document: Workflow) -> None:
    """Let the contract job's checkout persist its credentials."""
    _steps(document, "contract")[0]["with"]["persist-credentials"] = True


def _install_miri_everywhere(document: Workflow) -> None:
    """Install the Miri component in the contract job as well."""
    _steps(document, "contract").append({"run": "rustup component add miri"})


def _stop_installing_miri(document: Workflow) -> None:
    """Stop the Miri job installing the component it needs."""
    for step in _steps(document, "miri"):
        step["run"] = str(step.get("run", "")).replace("--component miri", "")


def _skip_the_contract(document: Workflow) -> None:
    """Point the contract job at a target that checks nothing."""
    for step in _steps(document, "contract"):
        step["run"] = str(step.get("run", "")).replace("boundary-contract", "help")


def _unconditional_cancellation(document: Workflow) -> None:
    """Cancel in-progress runs for every trigger."""
    document["concurrency"]["cancel-in-progress"] = True


def _widen_permissions(document: Workflow) -> None:
    """Grant the workflow write access to repository contents."""
    document["permissions"] = {"contents": "write"}


def _drop_build_standard_path(document: Workflow) -> None:
    """Drop the toolchain pin from the pull-request trigger paths."""
    triggers = document.get(True, document.get("on", {}))
    triggers["pull_request"]["paths"].remove("rust-toolchain.toml")


MUTATIONS: tuple[tuple[str, str, typ.Callable[[Workflow], None]], ...] = (
    ("hosted", "miri-loses-its-timeout", _drop_timeout),
    ("hosted", "contract-moves-to-a-paid-runner", _move_to_paid_runner),
    ("credentials", "checkout-persists-credentials", _persist_credentials),
    ("component", "contract-job-installs-miri", _install_miri_everywhere),
    ("component", "miri-job-stops-installing-miri", _stop_installing_miri),
    ("commands", "contract-job-skips-the-target", _skip_the_contract),
    ("triggers", "cancellation-becomes-unconditional", _unconditional_cancellation),
    ("triggers", "permissions-widen", _widen_permissions),
    (
        "build-standard-paths",
        "toolchain-pin-no-longer-triggers-proof-jobs",
        _drop_build_standard_path,
    ),
)


@pytest.mark.parametrize(
    ("rule", "mutation"),
    [pytest.param(rule, fn, id=name) for rule, name, fn in MUTATIONS],
)
def test_each_rule_catches_its_violation(
    rule: str, mutation: typ.Callable[[Workflow], None]
) -> None:
    """A mutated copy of the workflow breaks the rule that guards it."""
    document = copy.deepcopy(_workflow())
    mutation(document)
    assert JUDGES[rule](document), f"{rule} did not notice the mutation"


def test_the_workflow_is_a_regular_file() -> None:
    """Guard against the path silently matching nothing."""
    assert isinstance(WORKFLOW, Path) and WORKFLOW.is_file(), (
        f"Rust boundary workflow is missing or not a regular file: {WORKFLOW}"
    )


def test_proof_targets_check_build_tools_before_running() -> None:
    """Developer runs receive the same pinned linker/toolchain guard as CI."""
    makefile = WORKFLOW_DIR.parent.parent / "Makefile"
    text = makefile.read_text(encoding="utf-8")
    for target in ("boundary-contract", "miri"):
        match = re.search(rf"^{re.escape(target)}:([^\n]*)$", text, re.MULTILINE)
        assert match is not None, f"Makefile is missing {target}"
        assert "check-build-tools" in match.group(1), (
            f"Makefile target {target} must check the pinned build tools"
        )

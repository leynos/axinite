"""Contracts for the Rust boundary verification workflow (ADR 015).

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
    """Report proof jobs that leave the GitHub-hosted pool or run unbounded."""
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
    """Report checkouts that persist the token where built code can read it."""
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
    """Report the `miri` component outside the one job that needs it."""
    problems = []
    for name in PROOF_JOBS:
        installs_miri = _installs_miri(_run_text(document, name))
        if name == "miri" and not installs_miri:
            problems.append("miri: must install the miri component")
        if name != "miri" and installs_miri:
            problems.append(f"{name}: must not install the miri component")
    return problems


def command_violations(document: Workflow) -> list[str]:
    """Report proof jobs that no longer run their Make target."""
    return [
        f"{name}: must run `{command}`"
        for name, command in PROOF_COMMANDS.items()
        if command not in _run_text(document, name)
    ]


def trigger_violations(document: Workflow) -> list[str]:
    """Report a workflow that cannot run on a schedule or by hand, or leaks writes."""
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


JUDGES = {
    "hosted": hosted_violations,
    "credentials": credential_violations,
    "component": component_violations,
    "commands": command_violations,
    "triggers": trigger_violations,
}


@pytest.mark.parametrize("judge", JUDGES.values(), ids=JUDGES.keys())
def test_the_workflow_follows_each_rule(
    judge: typ.Callable[[Workflow], list[str]],
) -> None:
    """The real workflow satisfies every rule."""
    assert judge(_workflow()) == []


def _drop_timeout(document: Workflow) -> None:
    del _job(document, "miri")["timeout-minutes"]


def _move_to_paid_runner(document: Workflow) -> None:
    _job(document, "contract")["runs-on"] = "ubicloud-standard-4"


def _persist_credentials(document: Workflow) -> None:
    _steps(document, "contract")[0]["with"]["persist-credentials"] = True


def _install_miri_everywhere(document: Workflow) -> None:
    _steps(document, "contract").append({"run": "rustup component add miri"})


def _stop_installing_miri(document: Workflow) -> None:
    for step in _steps(document, "miri"):
        step["run"] = str(step.get("run", "")).replace("--component miri", "")


def _skip_the_contract(document: Workflow) -> None:
    for step in _steps(document, "contract"):
        step["run"] = str(step.get("run", "")).replace("boundary-contract", "help")


def _unconditional_cancellation(document: Workflow) -> None:
    document["concurrency"]["cancel-in-progress"] = True


def _widen_permissions(document: Workflow) -> None:
    document["permissions"] = {"contents": "write"}


MUTATIONS: tuple[tuple[str, str, typ.Callable[[Workflow], None]], ...] = (
    ("hosted", "miri-loses-its-timeout", _drop_timeout),
    ("hosted", "contract-moves-to-a-paid-runner", _move_to_paid_runner),
    ("credentials", "checkout-persists-credentials", _persist_credentials),
    ("component", "contract-job-installs-miri", _install_miri_everywhere),
    ("component", "miri-job-stops-installing-miri", _stop_installing_miri),
    ("commands", "contract-job-skips-the-target", _skip_the_contract),
    ("triggers", "cancellation-becomes-unconditional", _unconditional_cancellation),
    ("triggers", "permissions-widen", _widen_permissions),
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
    assert isinstance(WORKFLOW, Path) and WORKFLOW.is_file()

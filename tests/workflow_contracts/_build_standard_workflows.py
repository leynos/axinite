"""Judging how the workflows provision, apply and exclude the build standard.

Three questions, each answered from what a job runs rather than from its name:

* Does every Linux job that compiles install the pinned build tools first,
  unconditionally? A job compiles when a step runs Cargo, runs a Make target
  whose evaluated recipe reaches Cargo, runs the WASM builder, or calls the
  coverage action; the Make half is derived by evaluating the Makefile, so a
  recipe that gains or loses a Cargo line moves the answer.
* Do coverage and release jobs exclude the standard? That half is judged in
  `_build_standard_exclusions`.
* Does the Whitaker job install through the shared action and gate afterwards?
  Also `_build_standard_exclusions`.

Every judge is pure: it takes parsed workflow documents and returns messages.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import re
import typing as typ

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Iterable, Mapping

#: The unconditional provisioning command and the Make target it names.
INSTALL_TARGET = "install-build-tools"
INSTALL_COMMAND = re.compile(rf"^\s*make\s+{INSTALL_TARGET}\s*$")

#: A Cargo command that compiles. ``cargo component --version`` and
#: ``cargo binstall`` do not, so verbs are named.
CARGO_COMPILE = re.compile(
    r"\bcargo\s+(?:\+\S+\s+)?(?:build|check|clippy|test|nextest|llvm-cov)\b"
)
MAKE_TARGET = re.compile(r"\bmake\s+(?:-\S+\s+)*([A-Za-z0-9_.-]+)")
WASM_BUILDER = re.compile(r"\./scripts/build-wasm-extensions\.sh")
GENERATE_COVERAGE = "leynos/shared-actions/.github/actions/generate-coverage@"
INSTALL_WHITAKER = "leynos/shared-actions/.github/actions/install-whitaker@"
LINUX_LABEL = re.compile(r"ubuntu|ubicloud|linux", re.IGNORECASE)
CONFIG_REMOVAL = re.compile(r"\brm\b[^\n]*\.cargo/config\.toml")


class Step(typ.NamedTuple):
    """A step, or a line of a reusable workflow's ``setup-commands``.

    Attributes
    ----------
    name
        Display name, for messages.
    run
        The shell text it executes, empty for an action.
    uses
        The action it calls, empty for a shell step.
    guarded
        Whether the step can be skipped or can fail without failing the job.
    env
        The step's own ``env`` mapping.
    inputs
        The step's ``with`` mapping.
    """

    name: str
    run: str
    uses: str
    guarded: bool
    env: Mapping[str, object]
    inputs: Mapping[str, object]


def _truthy(value: object) -> bool:
    """Report whether a ``continue-on-error`` value can ever be true."""
    return str(value).strip().lower() not in ("false", "")


def steps_of(job: Mapping[str, object]) -> list[Step]:
    """Return a job's steps, with a reusable caller's setup commands as steps.

    A job that calls a reusable workflow declares no steps, but its
    ``setup-commands`` input runs as shell before the suite does, so each of
    its lines is read as a step. Reading only ``steps`` would let the mutation
    caller skip every contract here.
    """
    result: list[Step] = []
    declared = job.get("steps")
    for raw in declared if isinstance(declared, list) else []:
        if not isinstance(raw, dict):
            continue
        guarded = "if" in raw or _truthy(raw.get("continue-on-error", False))
        env = raw.get("env")
        inputs = raw.get("with")
        result.append(
            Step(
                str(raw.get("name", raw.get("uses", "unnamed"))),
                str(raw.get("run", "")),
                str(raw.get("uses", "")),
                guarded,
                env if isinstance(env, dict) else {},
                inputs if isinstance(inputs, dict) else {},
            )
        )
    inputs = job.get("with")
    commands = inputs.get("setup-commands") if isinstance(inputs, dict) else None
    if isinstance(commands, str):
        for line in commands.splitlines():
            stripped = line.strip().rstrip("\\")
            if stripped:
                tail = stripped.endswith(("|| true", "|| :"))
                result.append(Step(stripped, stripped, "", tail, {}, {}))
    return result


def runs_on_linux(job: Mapping[str, object]) -> bool:
    """Report whether a job can land on a Linux runner.

    A reusable-workflow caller declares no runner, and the shared workflow it
    calls runs on Linux, so it counts. Every label of a chain counts, so a job
    that reaches a Linux runner in any context is judged.
    """
    declared = job.get("runs-on")
    if declared is None:
        return "uses" in job
    if isinstance(declared, dict):
        declared = declared.get("labels")
    if isinstance(declared, list):
        return any(LINUX_LABEL.search(str(item)) for item in declared)
    return bool(LINUX_LABEL.search(str(declared)))


def compiles(step: Step, compiling_targets: Iterable[str]) -> bool:
    """Report whether a step compiles Rust, directly or through Make."""
    if step.uses.startswith(GENERATE_COVERAGE):
        return True
    for line in step.run.splitlines():
        if CARGO_COMPILE.search(line) or WASM_BUILDER.search(line):
            return True
        match = MAKE_TARGET.search(line)
        if match and match.group(1) in set(compiling_targets):
            return True
    return False


def make_targets_in(documents: Mapping[str, Mapping[str, object]]) -> set[str]:
    """Return every Make target any workflow step names."""
    named: set[str] = set()
    for document in documents.values():
        jobs = document.get("jobs")
        for job in jobs.values() if isinstance(jobs, dict) else []:
            if not isinstance(job, dict):
                continue
            for step in steps_of(job):
                named.update(
                    match.group(1)
                    for line in step.run.splitlines()
                    for match in [MAKE_TARGET.search(line)]
                    if match
                )
    return named


def install_violations(
    label: str, job: Mapping[str, object], compiling_targets: Iterable[str]
) -> list[str]:
    """Judge that a Linux compiling job installs the build tools first.

    Parameters
    ----------
    label
        ``workflow:job`` for messages.
    job
        The parsed job.
    compiling_targets
        Make targets whose evaluated recipe reaches Cargo.

    Returns
    -------
    list of str
        One message per violation. Empty for a job that need not install, which
        is one that is not on Linux, compiles nothing, or removes the
        configuration before it compiles.
    """
    if not runs_on_linux(job):
        return []
    sequence = steps_of(job)
    targets = set(compiling_targets)
    first = next(
        (i for i, step in enumerate(sequence) if compiles(step, targets)), None
    )
    if first is None:
        return []
    removal = next(
        (i for i, s in enumerate(sequence) if CONFIG_REMOVAL.search(s.run)), None
    )
    if removal is not None and removal < first:
        return []
    installs = [
        i for i, step in enumerate(sequence) if INSTALL_COMMAND.search(step.run)
    ]
    if not installs:
        return [
            f"{label}: compiles at step {sequence[first].name!r} but never runs make {INSTALL_TARGET}"
        ]
    problems: list[str] = []
    early = [i for i in installs if i < first]
    if not early:
        problems.append(
            f"{label}: make {INSTALL_TARGET} runs after the first compile, {sequence[first].name!r}"
        )
    for index in early:
        if sequence[index].guarded:
            problems.append(
                f"{label}: {sequence[index].name!r} is conditional or soft-failing"
            )
    if _truthy(job.get("continue-on-error", False)):
        problems.append(
            f"{label}: the job continues on error, so a failed install does not fail it"
        )
    return problems

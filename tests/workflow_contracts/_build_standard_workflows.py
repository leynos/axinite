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

import collections.abc as cabc
import re
import typing as typ

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Iterable, Iterator, Mapping

#: The unconditional provisioning command and the Make target it names.
INSTALL_TARGET = "install-build-tools"
INSTALL_COMMAND = re.compile(rf"^\s*make\s+{INSTALL_TARGET}\s*$")

#: A Cargo command that compiles. ``cargo component --version`` and
#: ``cargo binstall`` do not, so verbs are named.
CARGO_COMPILE = re.compile(
    r"\bcargo\s+(?:\+\S+\s+)?"
    r"(?:(?:-q|--quiet|-v|--verbose|--locked|--offline|--frozen)\s+)*"
    r"(?:build|check|clippy|test|nextest|llvm-cov)\b"
)
MAKE_TARGET = re.compile(
    r"\bmake\s+(?:-\S+\s+)*(?:[A-Za-z_][A-Za-z0-9_]*[:+?!]?=\S*\s+)*"
    r"([A-Za-z0-9_.-]+)"
)
WASM_BUILDER = re.compile(r"\./scripts/build-wasm-extensions\.sh")
GENERATE_COVERAGE = "leynos/shared-actions/.github/actions/generate-coverage@"
INSTALL_WHITAKER = "leynos/shared-actions/.github/actions/install-whitaker@"
LINUX_LABEL = re.compile(r"ubuntu|ubicloud|linux", re.IGNORECASE)
MATRIX_RUNNER = re.compile(r"\$\{\{\s*matrix\.([A-Za-z_][A-Za-z0-9_-]*)\s*\}\}")
UNRESOLVED_EXPRESSION = re.compile(r"\$\{\{.*?\}\}")
CONFIG_REMOVAL = re.compile(
    r"rm\s+(?:(?:-f|--force)\s+)?\.cargo/config\.toml"
    r"(?:\s+rust-toolchain\.toml)?"
)


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


def _job_matrix(job: Mapping[str, object]) -> Mapping[str, object] | None:
    """Return a job's declared matrix, if it is a mapping."""
    strategy = job.get("strategy")
    matrix = strategy.get("matrix") if isinstance(strategy, dict) else None
    return matrix if isinstance(matrix, dict) else None


def _declared_axis_values(matrix: Mapping[str, object], name: str) -> list[object]:
    """Return values declared directly for one matrix axis."""
    if name not in matrix:
        return []
    declared = matrix[name]
    return list(declared) if isinstance(declared, list) else [declared]


def _included_axis_values(matrix: Mapping[str, object], name: str) -> list[object]:
    """Return values supplied for one axis by matrix include rows."""
    included = matrix.get("include")
    if not isinstance(included, list):
        return []
    return [row[name] for row in included if isinstance(row, dict) and name in row]


def _matrix_values(job: Mapping[str, object], name: str) -> list[object] | None:
    """Return declared values for a runner matrix axis, if they are resolvable."""
    matrix = _job_matrix(job)
    if matrix is None:
        return None
    values = _declared_axis_values(matrix, name) + _included_axis_values(matrix, name)
    if name not in matrix and not values:
        return None
    if any(
        isinstance(value, str) and UNRESOLVED_EXPRESSION.search(value)
        for value in values
    ):
        return None
    return values


def _matrix_axis_may_be_linux(job: Mapping[str, object], name: str) -> bool:
    """Report whether an unresolved matrix axis can select a Linux runner."""
    values = _matrix_values(job, name)
    return values is None or any(LINUX_LABEL.search(str(value)) for value in values)


def _matrix_runner_may_be_linux(job: Mapping[str, object], runner: str) -> bool:
    """Report whether a runner expression can resolve through Linux matrix values."""
    return any(
        _matrix_axis_may_be_linux(job, name) for name in MATRIX_RUNNER.findall(runner)
    )


def _truthy(value: object) -> bool:
    """Report whether a ``continue-on-error`` value can ever be true."""
    return str(value).strip().lower() not in ("false", "")


def _step_of(raw: Mapping[str, object]) -> Step:
    """Read one workflow step mapping as a `Step`."""
    env = raw.get("env")
    inputs = raw.get("with")
    return Step(
        str(raw.get("name", raw.get("uses", "unnamed"))),
        str(raw.get("run", "")),
        str(raw.get("uses", "")),
        "if" in raw or _truthy(raw.get("continue-on-error", False)),
        env if isinstance(env, dict) else {},
        inputs if isinstance(inputs, dict) else {},
    )


def _setup_command_steps(job: Mapping[str, object]) -> list[Step]:
    """Read a reusable caller's `setup-commands` lines as steps."""
    inputs = job.get("with")
    commands = inputs.get("setup-commands") if isinstance(inputs, dict) else None
    if not isinstance(commands, str):
        return []
    lines = (line.strip().rstrip("\\") for line in commands.splitlines())
    return [
        Step(line, line, "", line.endswith(("|| true", "|| :")), {}, {})
        for line in lines
        if line
    ]


def steps_of(job: Mapping[str, object]) -> list[Step]:
    """Return a job's steps, with a reusable caller's setup commands as steps.

    A job that calls a reusable workflow declares no steps, but its
    ``setup-commands`` input runs as shell before the suite does, so each of
    its lines is read as a step. Reading only ``steps`` would let the mutation
    caller skip every contract here.
    """
    declared = job.get("steps")
    raw_steps = declared if isinstance(declared, list) else []
    steps = [_step_of(raw) for raw in raw_steps if isinstance(raw, dict)]
    return steps + _setup_command_steps(job)


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
    runner = " ".join(map(str, declared)) if isinstance(declared, list) else str(declared)
    if LINUX_LABEL.search(MATRIX_RUNNER.sub("", runner)):
        return True

    return _matrix_runner_may_be_linux(job, runner)


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


def iter_jobs(
    documents: Mapping[str, Mapping[str, object]],
) -> Iterator[tuple[str, str, Mapping[str, object]]]:
    """Yield ``(workflow, job id, job)`` for every job mapping in the documents."""
    for workflow, document in documents.items():
        jobs = document.get("jobs")
        declared = jobs.items() if isinstance(jobs, dict) else []
        yield from (
            (workflow, job_id, job) for job_id, job in declared if isinstance(job, dict)
        )


def _targets_named(step: Step) -> set[str]:
    """Return the Make targets one step's shell lines name."""
    matches = (MAKE_TARGET.search(line) for line in step.run.splitlines())
    return {match.group(1) for match in matches if match}


def make_targets_in(documents: Mapping[str, Mapping[str, object]]) -> set[str]:
    """Return every Make target any workflow step names."""
    named: set[str] = set()
    for _, _, job in iter_jobs(documents):
        for step in steps_of(job):
            named |= _targets_named(step)
    return named


def _first_index(sequence: list[Step], test: cabc.Callable[[Step], bool]) -> int | None:
    """Return the first index whose workflow step satisfies ``test``."""
    return next((i for i, step in enumerate(sequence) if test(step)), None)


def _removes_configuration(step: Step) -> bool:
    """Recognize a standalone, hard-failing removal of the build config."""
    return not step.guarded and CONFIG_REMOVAL.fullmatch(step.run.strip()) is not None


def _early_install_violations(
    label: str, sequence: list[Step], installs: list[int], first: int
) -> list[str]:
    """Judge the installers that precede the first compile at `first`."""
    early = [i for i in installs if i < first]
    problems = [
        f"{label}: {sequence[i].name!r} is conditional or soft-failing"
        for i in early
        if sequence[i].guarded
    ]
    if not early:
        problems.append(
            f"{label}: make {INSTALL_TARGET} runs after the first compile, "
            f"{sequence[first].name!r}"
        )
    return problems


def _ordering_violations(
    label: str, job: Mapping[str, object], sequence: list[Step], first: int
) -> list[str]:
    """Judge the installer steps of a job whose first compile is at `first`."""
    installs = [i for i, s in enumerate(sequence) if INSTALL_COMMAND.search(s.run)]
    if not installs:
        return [
            f"{label}: compiles at step {sequence[first].name!r} but never runs "
            f"make {INSTALL_TARGET}"
        ]
    problems = _early_install_violations(label, sequence, installs, first)
    if _truthy(job.get("continue-on-error", False)):
        problems.append(
            f"{label}: the job continues on error, so a failed install does not fail it"
        )
    return problems


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
        configuration unconditionally before it compiles.
    """
    if not runs_on_linux(job):
        return []
    sequence = steps_of(job)
    targets = set(compiling_targets)
    first = _first_index(sequence, lambda step: compiles(step, targets))
    if first is None:
        return []
    removal = _first_index(sequence, _removes_configuration)
    if removal is not None and removal < first:
        return []
    return _ordering_violations(label, job, sequence, first)

"""What the CI telemetry contract asks of a job, as pure functions over steps.

The telemetry contract (`ci_telemetry_test.py`) and the sizing contract
(`runner_sizing_test.py`) share the set of workflows the collector reads and
the pinned actions, so both import them from here. The checks are pure
functions over a job's step mappings, so a test can hand them a deliberately
wrong arrangement and see the contract refuse it, independently of the
workflows as they stand today.
"""

from __future__ import annotations

import re
import typing as typ

#: The workflows whose runs the collector reads for telemetry.
INSTRUMENTED_WORKFLOWS: tuple[str, ...] = (
    "code_style.yml",
    "codescene-coverage.yml",
    "coverage.yml",
    "e2e.yml",
    "test.yml",
)

#: The shared-actions commit every telemetry call pins. It carries
#: `ci-telemetry-start`, `ci-telemetry-finish` (with its sccache fields) and
#: `sccache-report`.
PIN: typ.Final[str] = "81374b6f183e59f0570ad36217ded90d3240fda6"
ACTIONS: typ.Final[str] = "leynos/shared-actions/.github/actions"
START: typ.Final[str] = f"{ACTIONS}/ci-telemetry-start@{PIN}"
FINISH: typ.Final[str] = f"{ACTIONS}/ci-telemetry-finish@{PIN}"
REPORT: typ.Final[str] = f"{ACTIONS}/sccache-report@{PIN}"
SETUP_RUST_PREFIX: typ.Final[str] = f"{ACTIONS}/setup-rust@"
STATUS: typ.Final[str] = "${{ steps.setup-rust.outputs.sccache-status }}"

#: Roll-up jobs that run one step over `needs`, with the reason.
ROLL_UPS: dict[tuple[str, str], str] = {
    ("code_style.yml", "code-style"): "branch-protection roll-up over three jobs",
    ("coverage.yml", "coverage-gate"): "branch-protection roll-up over three jobs",
    ("e2e.yml", "e2e"): "branch-protection roll-up over the matrix",
    ("test.yml", "run-tests"): "branch-protection roll-up over eight jobs",
}

#: Steps that may precede the start: the checkout, and freeing the disk.
BEFORE_START_NAMES: frozenset[str] = frozenset({"Free disk space"})
PINNED: typ.Final[re.Pattern[str]] = re.compile(r"^[\w./-]+@[0-9a-f]{40}$")

Step = dict[str, object]


def uses(step: Step) -> str:
    """Return the step's `uses` reference, or an empty string."""
    return str(step.get("uses", ""))


def is_telemetry_action(step: Step) -> bool:
    """Report whether a step calls any of the three telemetry actions."""
    reference = uses(step)
    return "/ci-telemetry-" in reference or "/sccache-report@" in reference


def _positions(steps: list[Step], reference: str) -> list[int]:
    """Return the indexes of the steps that call exactly `reference`."""
    return [i for i, step in enumerate(steps) if uses(step) == reference]


def _may_precede_start(step: Step) -> bool:
    """Report whether a step may come before the telemetry start."""
    return (
        uses(step).startswith("actions/checkout@")
        or step.get("name") in BEFORE_START_NAMES
    )


def _before_start_problems(steps: list[Step], start: int) -> list[str]:
    """Return the steps that do work before the start."""
    return [
        f"does work before the start: {step.get('name')!r}"
        for step in steps[:start]
        if not _may_precede_start(step)
    ]


def _after_report_problems(steps: list[Step], finish: int) -> list[str]:
    """Return the steps that do work after the report, bar a cache save."""
    return [
        f"does work after the report: {step.get('name')!r}"
        for step in steps[finish + 1 :]
        if not uses(step).startswith("actions/cache/save@")
    ]


def telemetry_problems(steps: list[Step]) -> list[str]:
    """Return what is wrong with a carrying job's telemetry steps; empty if sound."""
    starts = _positions(steps, START)
    finishes = _positions(steps, FINISH)
    if len(starts) != 1 or len(finishes) != 1:
        return [f"needs one start and one finish, has {len(starts)} and {len(finishes)}"]
    start, finish = starts[0], finishes[0]
    problems = [
        *_before_start_problems(steps, start),
        *_after_report_problems(steps, finish),
    ]
    if start > finish:
        problems.append("reports telemetry before starting it")
    if steps[finish].get("if") != "always()":
        problems.append("reports only on success")
    return problems


def _setup_problems(steps: list[Step]) -> list[str]:
    """Return what is wrong with how `setup-rust` is called."""
    setups = [s for s in steps if uses(s).startswith(SETUP_RUST_PREFIX)]
    if len(setups) == 1 and setups[0].get("id") == "setup-rust":
        return []
    return ["setup-rust must be called once, with the id `setup-rust`"]


def _status_problems(finish: Step) -> list[str]:
    """Return what is wrong with the status the finish step is given."""
    inputs = finish.get("with")
    status = inputs.get("sccache-status") if isinstance(inputs, dict) else None
    return [] if status == STATUS else ["finish does not take setup-rust's status"]


def _report_problems(report: Step) -> list[str]:
    """Return what is wrong with the `sccache-report` step itself."""
    sound = report.get("if") == "always()" and report.get("id") == "sccache"
    return [] if sound else ["sccache-report must run under always(), id `sccache`"]


def _placement_problems(steps: list[Step], report: int, finish: int) -> list[str]:
    """Return what is wrong with where the report sits."""
    setup_at = next(
        (i for i, s in enumerate(steps) if uses(s).startswith(SETUP_RUST_PREFIX)), 0
    )
    problems = []
    if report + 1 != finish:
        problems.append("sccache-report must run directly before the telemetry finish")
    if report < setup_at:
        problems.append("sccache-report runs before setup-rust")
    return problems


def sccache_problems(steps: list[Step]) -> list[str]:
    """Return what is wrong with a `setup-rust` job's sccache reporting."""
    if not any(uses(s).startswith(SETUP_RUST_PREFIX) for s in steps):
        return []
    finishes = _positions(steps, FINISH)
    reports = _positions(steps, REPORT)
    if len(finishes) != 1 or len(reports) != 1:
        return [*_setup_problems(steps), "needs one sccache-report and one finish"]
    report, finish = reports[0], finishes[0]
    problems = [
        *_setup_problems(steps),
        *_status_problems(steps[finish]),
        *_report_problems(steps[report]),
        *_placement_problems(steps, report, finish),
    ]
    if "--show-stats" in repr(steps):
        problems.append("still reads sccache statistics by hand")
    return problems


def _step_problems(step: object) -> list[str]:
    """Return what is wrong with a roll-up's one step."""
    if not isinstance(step, dict) or "uses" in step:
        return ["its one step must be a `run`, not an action"]
    problems = []
    if is_telemetry_action(step) or "ci-resource-sampler" in repr(step):
        problems.append("carries telemetry")
    if "needs." not in str(step.get("run", "")):
        problems.append("its step does not aggregate `needs`")
    return problems


def roll_up_problems(body: dict[str, object]) -> list[str]:
    """Return what is wrong with a roll-up exemption; empty if it is a roll-up."""
    problems = [] if body.get("needs") else ["has no needs"]
    steps = body.get("steps")
    if not isinstance(steps, list) or len(steps) != 1:
        return [*problems, "must run exactly one step"]
    return [*problems, *_step_problems(steps[0])]

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


def telemetry_problems(steps: list[Step]) -> list[str]:
    """Return what is wrong with a carrying job's telemetry steps; empty if sound."""
    refs = [uses(s) for s in steps]
    starts = [i for i, u in enumerate(refs) if u == START]
    finishes = [i for i, u in enumerate(refs) if u == FINISH]
    if len(starts) != 1 or len(finishes) != 1:
        return [f"needs one start and one finish, has {len(starts)} and {len(finishes)}"]
    start, finish = starts[0], finishes[0]
    problems: list[str] = []
    if start > finish:
        problems.append("reports telemetry before starting it")
    for step in steps[:start]:
        if not (
            uses(step).startswith("actions/checkout@")
            or step.get("name") in BEFORE_START_NAMES
        ):
            problems.append(f"does work before the start: {step.get('name')!r}")
    if steps[finish].get("if") != "always()":
        problems.append("reports only on success")
    for step in steps[finish + 1 :]:
        if not uses(step).startswith("actions/cache/save@"):
            problems.append(f"does work after the report: {step.get('name')!r}")
    return problems


def sccache_problems(steps: list[Step]) -> list[str]:
    """Return what is wrong with a `setup-rust` job's sccache reporting."""
    setups = [s for s in steps if uses(s).startswith(SETUP_RUST_PREFIX)]
    if not setups:
        return []
    problems: list[str] = []
    if len(setups) != 1 or setups[0].get("id") != "setup-rust":
        problems.append("setup-rust must be called once, with the id `setup-rust`")
    finishes = [i for i, s in enumerate(steps) if uses(s) == FINISH]
    reports = [i for i, s in enumerate(steps) if uses(s) == REPORT]
    if len(finishes) != 1 or len(reports) != 1:
        return [*problems, "needs one sccache-report and one telemetry finish"]
    inputs = steps[finishes[0]].get("with")
    status = inputs.get("sccache-status") if isinstance(inputs, dict) else None
    if status != STATUS:
        problems.append("finish does not take setup-rust's sccache-status")
    report = steps[reports[0]]
    if report.get("if") != "always()" or report.get("id") != "sccache":
        problems.append("sccache-report must run under always() with the id `sccache`")
    if reports[0] + 1 != finishes[0]:
        problems.append("sccache-report must run directly before the telemetry finish")
    setup_at = next(i for i, s in enumerate(steps) if uses(s).startswith(SETUP_RUST_PREFIX))
    if reports[0] < setup_at:
        problems.append("sccache-report runs before setup-rust")
    if "--show-stats" in repr(steps):
        problems.append("still reads sccache statistics by hand")
    return problems


def roll_up_problems(body: dict[str, object]) -> list[str]:
    """Return what is wrong with a roll-up exemption; empty if it is a roll-up."""
    steps = body.get("steps")
    problems: list[str] = []
    if not body.get("needs"):
        problems.append("has no needs")
    if not isinstance(steps, list) or len(steps) != 1:
        return [*problems, "must run exactly one step"]
    step = steps[0]
    if not isinstance(step, dict) or "uses" in step:
        return [*problems, "its one step must be a `run`, not an action"]
    if is_telemetry_action(step) or "ci-resource-sampler" in repr(step):
        problems.append("carries telemetry")
    if "needs." not in str(step.get("run", "")):
        problems.append("its step does not aggregate `needs`")
    return problems

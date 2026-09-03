"""Shared parsing helpers for the workflow-policy contract tests.

The name has no ``_test`` suffix, so pytest imports it as a helper rather
than collecting it. It exists so the placement, cache-ownership, and
tool-install contracts read one parsed view of ``.github/workflows``.
"""

from __future__ import annotations

import re
import typing as typ
from dataclasses import dataclass
from pathlib import Path

import yaml

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Iterator

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_DIR = REPOSITORY_ROOT / ".github" / "workflows"

#: A full 40-character commit SHA. Dependabot owns the value; the contracts
#: assert the shape so a bump never fails a test (see the developers' guide,
#: "Workflow pins and Dependabot").
SHA_RE: re.Pattern[str] = re.compile(r"^[0-9a-f]{40}$")

#: The Ubicloud label this repository currently uses. The migration wave will
#: right-size these jobs; update this constant and .github/actionlint.yaml
#: together when it does.
UBICLOUD_LABEL = "ubicloud-standard-8"

#: Commands that compile or execute the product. A job is a build or test job
#: when one of its steps runs one of these; nothing else about the job matters.
#: Deriving the classification from what a job runs, rather than from a list of
#: job names, means a job that stops building also stops qualifying.
#:
#: `cargo audit` and `cargo binstall` are deliberately absent: they read
#: metadata and download archives. `cargo fmt` is present because a formatter
#: gate needs the Rust toolchain and runs on the same feedback path.
BUILD_OR_TEST_PATTERNS: tuple[re.Pattern[str], ...] = tuple(
    re.compile(pattern, re.MULTILINE)
    for pattern in (
        r"\bcargo\s+(?:\+\S+\s+)?"
        r"(?:build|check|clippy|fmt|test|nextest|llvm-cov|component|run)\b",
        r"\bdocker\s+build\b",
        r"\bpytest\b",
        r"\bmake\s+(?:all|test|test-matrix|lint|typecheck|check-fmt"
        r"|build-github-tool-wasm)\b",
        r"\./scripts/build-wasm-extensions\.sh",
    )
)

#: Forms that build a CI tool from source. `cargo install` compiles by
#: definition; a bare `cargo binstall` silently falls back to it.
SOURCE_BUILD_PATTERNS: tuple[tuple[re.Pattern[str], str], ...] = (
    (
        re.compile(r"\bcargo\s+(\+\S+\s+)?install\b"),
        "`cargo install` compiles the tool from source",
    ),
    (
        re.compile(r"\bcargo\s+binstall\b(?![^\n]*--strategies)"),
        "`cargo binstall` without --strategies falls back to `cargo install`",
    ),
)

#: The reviewed pin for the Actions cache, v6.1.0. Ubicloud's transparent
#: cache proxy is confirmed to intercept this version's traffic.
CACHE_ACTION_SHA = "55cc8345863c7cc4c66a329aec7e433d2d1c52a9"
CACHE_ACTION_PREFIXES = (
    "actions/cache@",
    "actions/cache/restore@",
    "actions/cache/save@",
)


@dataclass(frozen=True)
class Job:
    """One job, with the file it came from and its parsed body."""

    workflow: str
    job_id: str
    body: dict[str, object]

    @property
    def runs_on(self) -> str | None:
        """Return the job's runner label when it declares one directly."""
        label = self.body.get("runs-on")
        return label if isinstance(label, str) else None

    @property
    def steps(self) -> list[dict[str, object]]:
        """Return the job's step mappings, or an empty list for a caller."""
        steps = self.body.get("steps")
        if not isinstance(steps, list):
            return []
        return [step for step in steps if isinstance(step, dict)]

    def __str__(self) -> str:
        """Identify the job as ``workflow.yml:job-id`` in assertion output."""
        return f"{self.workflow}:{self.job_id}"


def workflow_paths() -> list[Path]:
    """Return every workflow file, sorted for deterministic test ordering."""
    return sorted(WORKFLOW_DIR.glob("*.yml"))


def load(path: Path) -> dict[str, object]:
    """Parse one workflow file into a mapping."""
    document = yaml.safe_load(path.read_text(encoding="utf-8"))
    if not isinstance(document, dict):
        message = f"{path.name} must parse as a mapping"
        raise AssertionError(message)
    return document


def declared_jobs(path: Path) -> dict[str, object]:
    """Return one workflow's jobs mapping, or an empty mapping."""
    declared = load(path).get("jobs")
    return declared if isinstance(declared, dict) else {}


def jobs_in(path: Path) -> Iterator[Job]:
    """Yield the jobs one workflow file declares."""
    for job_id, body in declared_jobs(path).items():
        if isinstance(body, dict):
            yield Job(path.name, job_id, body)


def jobs() -> Iterator[Job]:
    """Yield every job declared across the workflow estate."""
    for path in workflow_paths():
        yield from jobs_in(path)


def step_text(step: dict[str, object]) -> str:
    """Return a step's shell body, or an empty string for an action step."""
    run = step.get("run")
    return run if isinstance(run, str) else ""


def cache_paths(step: dict[str, object]) -> list[str]:
    """Return the paths a cache step declares, one per line."""
    inputs = step.get("with")
    if not isinstance(inputs, dict):
        return []
    declared = inputs.get("path")
    if not isinstance(declared, str):
        return []
    return [line.strip() for line in declared.splitlines() if line.strip()]


def is_cache_step(step: dict[str, object]) -> bool:
    """Report whether a step invokes the Actions cache in any of its forms."""
    uses = step.get("uses")
    return isinstance(uses, str) and uses.startswith(CACHE_ACTION_PREFIXES)


def builds_or_tests(job: Job) -> bool:
    """Report whether a job compiles or executes the product.

    Read from the job's own steps rather than its name, so a job that loses
    its build step loses its claim on a paid runner at the same moment.
    """
    return any(
        pattern.search(step_text(step))
        for step in job.steps
        for pattern in BUILD_OR_TEST_PATTERNS
    )

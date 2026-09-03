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

#: Jobs permitted to run on an Ubicloud runner. Everything else must be
#: GitHub-hosted. Membership means "this job compiles or executes the
#: product", not "this job is important": roll-ups, gates, labelling,
#: release orchestration, reports, and review jobs are API-bound and belong
#: on GitHub-hosted runners regardless of how developer-visible they are.
UBICLOUD_ALLOW_LIST: frozenset[tuple[str, str]] = frozenset(
    {
        ("code_style.yml", "format"),
        ("code_style.yml", "clippy"),
        ("codescene-coverage.yml", "coverage-check"),
        ("coverage.yml", "coverage"),
        ("coverage.yml", "e2e-coverage"),
        ("e2e.yml", "build"),
        ("e2e.yml", "test"),
        ("test.yml", "tests"),
        ("test.yml", "telegram-tests"),
        ("test.yml", "wasm-wit-compat"),
        # Privileged and daemon-dependent. It keeps its current label until a
        # Docker daemon and privilege preflight passes; it is not a candidate
        # for the first migration slice.
        ("test.yml", "docker-build"),
    }
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

"""Shared parsing helpers for the workflow-policy contract tests.

The name has no ``_test`` suffix, so pytest imports it as a helper rather
than collecting it. It exists so the placement, cache-ownership, and
tool-install contracts read one parsed view of ``.github/workflows``.

The module is split in two. Everything from `parse_workflow` downwards is
pure: it takes workflow text or an already-parsed mapping and answers
questions about it, so `_workflow_policy_test.py` can exercise every runner
shape and command form without writing a file. The handful of functions that
name a `Path` are the file-reading edge, and they do nothing but read and
delegate.
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

#: A `runs-on` that picks its label from the event, as
#: `${{ github.event_name == 'schedule' && 'ubuntu-latest'
#: || 'ubicloud-standard-8' }}`. A workflow that is both a developer gate and a
#: cron needs the Ubicloud runner on one path and not the other, and the label
#: is the only place that distinction can live. Reading such a value as one
#: opaque label would hide the Ubicloud request from every placement contract,
#: so the forms are parsed rather than passed through.
CONDITIONAL_RUNNER_RE: re.Pattern[str] = re.compile(
    r"^\$\{\{\s*github\.event_name\s*==\s*'(?P<event>[a-z_]+)'\s*&&\s*"
    r"'(?P<when>[^']+)'\s*\|\|\s*'(?P<otherwise>[^']+)'\s*\}\}$"
)

#: Prefix shared by every Ubicloud runner label. Match on the prefix, not on
#: one exact label: the migration wave introduces `ubicloud-standard-2`, and a
#: contract keyed to the current label would wave the new one through.
UBICLOUD_LABEL_PREFIX = "ubicloud-"

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
#: definition; a bare `cargo binstall` silently falls back to it, and naming
#: the `compile` strategy asks for the same build outright.
SOURCE_BUILD_PATTERNS: tuple[tuple[re.Pattern[str], str], ...] = (
    (
        re.compile(r"\bcargo\s+(\+\S+\s+)?install\b"),
        "`cargo install` compiles the tool from source",
    ),
    (
        re.compile(r"\bcargo\s+binstall\b(?![^\n]*--strategies)"),
        "`cargo binstall` without --strategies falls back to `cargo install`",
    ),
    (
        # A strategy list is only fail-closed while `compile` is absent from
        # it. Without this the previous pattern waves through the one spelling
        # that asks for a source build in as many words.
        re.compile(r"\bcargo\s+binstall\b[^\n]*--strategies[^\n]*\bcompile\b"),
        "the `compile` binstall strategy builds the tool from source",
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


def _conditional_runner(declared: str) -> tuple[str, str, str] | None:
    """Split an event-conditional `runs-on` into its event and two labels.

    Parameters
    ----------
    declared
        The raw `runs-on` scalar. A folded YAML scalar arrives with its line
        breaks already joined into single spaces.

    Returns
    -------
    tuple of str, or None
        The event name, the label chosen for that event, and the label used
        otherwise. ``None`` when the value is not the conditional form, which
        includes a matrix expression and every plain label.
    """
    match = CONDITIONAL_RUNNER_RE.match(" ".join(declared.split()))
    if match is None:
        return None
    return match["event"], match["when"], match["otherwise"]


@dataclass(frozen=True)
class Job:
    """One job, carrying the file it came from alongside its parsed body.

    Attributes
    ----------
    workflow
        File name of the workflow that declares the job, such as
        ``test.yml``.
    job_id
        The job's key under the workflow's `jobs` mapping.
    body
        The job's parsed mapping, exactly as PyYAML produced it.
    """

    workflow: str
    job_id: str
    body: dict[str, object]

    @property
    def runner_labels(self) -> tuple[str, ...]:
        """Return every runner label the job requests.

        `runs-on` accepts a single label, a list of labels, or a mapping with
        a `labels` key. Reading only the scalar form would let a job written
        in either of the other two forms escape every placement contract.

        Returns
        -------
        tuple of str
            The declared labels, empty when the job is a reusable-workflow
            caller or computes its label from a matrix expression.
        """
        declared = self.body.get("runs-on")
        # Unwrap the mapping form first. Its `labels` key takes either a list
        # or a single string, so checking for a scalar before unwrapping would
        # miss `runs-on: {group: ..., labels: ubicloud-standard-8}` entirely.
        if isinstance(declared, dict):
            declared = declared.get("labels")
        if isinstance(declared, str):
            conditional = _conditional_runner(declared)
            if conditional is not None:
                # Both arms are reported, so a job that reaches Ubicloud on any
                # event still answers `uses_ubicloud` and stays inside the
                # timeout and sccache contracts.
                return conditional[1], conditional[2]
            return (declared,)
        if isinstance(declared, list):
            return tuple(label for label in declared if isinstance(label, str))
        return ()

    @property
    def runs_on(self) -> str | None:
        """Return the job's runner label when it declares exactly one.

        Returns
        -------
        str or None
            The single literal label, or ``None`` when the job declares none,
            declares several, or computes one from a matrix.
        """
        labels = self.runner_labels
        return labels[0] if len(labels) == 1 else None

    @property
    def runner_summary(self) -> str:
        """Return the job's labels for an assertion message.

        Returns
        -------
        str
            The labels joined by commas, or ``<none>`` when the job declares
            none directly.
        """
        return ", ".join(self.runner_labels) or "<none>"

    @property
    def uses_ubicloud(self) -> bool:
        """Report whether the job requests any Ubicloud runner.

        Returns
        -------
        bool
            True when any declared label carries the Ubicloud prefix, not only
            the one label this repository uses today.
        """
        return any(
            label.startswith(UBICLOUD_LABEL_PREFIX) for label in self.runner_labels
        )

    @property
    def ubicloud_labels(self) -> tuple[str, ...]:
        """Return the job's Ubicloud labels.

        Returns
        -------
        tuple of str
            Every declared label carrying the Ubicloud prefix.
        """
        return tuple(
            label
            for label in self.runner_labels
            if label.startswith(UBICLOUD_LABEL_PREFIX)
        )

    def labels_for_event(self, event: str) -> tuple[str, ...]:
        """Return the labels this job requests when triggered by an event.

        Parameters
        ----------
        event
            A `github.event_name` value, such as ``schedule``.

        Returns
        -------
        tuple of str
            The single label the conditional form selects for this event, or
            every declared label when the job's `runs-on` does not depend on
            the event.
        """
        declared = self.body.get("runs-on")
        if isinstance(declared, str):
            conditional = _conditional_runner(declared)
            if conditional is not None:
                chosen, alternative = conditional[1], conditional[2]
                return (chosen if conditional[0] == event else alternative,)
        return self.runner_labels

    @property
    def steps(self) -> list[dict[str, object]]:
        """Return the job's step mappings.

        Returns
        -------
        list of dict
            The job's steps, or an empty list when the job calls a reusable
            workflow and therefore declares none.
        """
        steps = self.body.get("steps")
        if not isinstance(steps, list):
            return []
        return [step for step in steps if isinstance(step, dict)]

    def __str__(self) -> str:
        """Identify the job in assertion output.

        Returns
        -------
        str
            The job as ``workflow.yml:job-id``.
        """
        return f"{self.workflow}:{self.job_id}"


#: Extensions GitHub accepts for a workflow file. Scanning only `.yml` would
#: silently exempt a `.yaml` workflow from every contract in this directory,
#: which is the same vacuous pass an unread `runs-on` produces.
WORKFLOW_SUFFIXES: tuple[str, ...] = (".yml", ".yaml")


def workflow_paths(directory: Path = WORKFLOW_DIR) -> list[Path]:
    """Return every workflow file in a directory.

    Parameters
    ----------
    directory
        Directory to scan. It defaults to the repository's workflow
        directory; the parameter exists so a test can point the same scan at
        a temporary tree instead of the estate.

    Returns
    -------
    list of Path
        Workflow paths sorted by name, so parameterized tests report in a
        stable order. Both extensions GitHub accepts are included.
    """
    return sorted(
        path
        for path in directory.iterdir()
        if path.is_file() and path.suffix in WORKFLOW_SUFFIXES
    )


def parse_workflow(text: str, name: str) -> dict[str, object]:
    """Parse workflow text into a mapping.

    Parameters
    ----------
    text
        The workflow document's YAML source.
    name
        File name to quote in the failure message. It identifies the
        document and is not used to read anything.

    Returns
    -------
    dict
        The parsed workflow document.

    Raises
    ------
    AssertionError
        If the text does not parse as a mapping, which means it is not a
        workflow at all.
    """
    document = yaml.safe_load(text)
    if not isinstance(document, dict):
        message = f"{name} must parse as a mapping"
        raise AssertionError(message)
    return document


def declared_jobs_in(document: dict[str, object]) -> dict[str, object]:
    """Return a parsed workflow's jobs mapping.

    Parameters
    ----------
    document
        A parsed workflow document.

    Returns
    -------
    dict
        The workflow's jobs, or an empty mapping when it declares none, or
        declares one that is not a mapping.
    """
    declared = document.get("jobs")
    return declared if isinstance(declared, dict) else {}


def jobs_of(name: str, document: dict[str, object]) -> Iterator[Job]:
    """Yield the jobs a parsed workflow declares.

    Parameters
    ----------
    name
        The workflow's file name, carried on each `Job` for assertion
        messages.
    document
        A parsed workflow document.

    Yields
    ------
    Job
        Each job whose body is a mapping. A job whose body is anything else
        is skipped rather than raising, because the contracts that care about
        malformed jobs report them by name.
    """
    for job_id, body in declared_jobs_in(document).items():
        if isinstance(body, dict):
            yield Job(name, job_id, body)


def load(path: Path) -> dict[str, object]:
    """Read and parse one workflow file.

    Parameters
    ----------
    path
        Workflow file to read.

    Returns
    -------
    dict
        The parsed workflow document.
    """
    return parse_workflow(path.read_text(encoding="utf-8"), path.name)


def declared_jobs(path: Path) -> dict[str, object]:
    """Return one workflow file's jobs mapping.

    Parameters
    ----------
    path
        Workflow file to read.

    Returns
    -------
    dict
        The workflow's jobs, or an empty mapping when it declares none.
    """
    return declared_jobs_in(load(path))


def jobs_in(path: Path) -> Iterator[Job]:
    """Yield the jobs one workflow file declares.

    Parameters
    ----------
    path
        Workflow file to read.

    Yields
    ------
    Job
        Each job whose body is a mapping.
    """
    yield from jobs_of(path.name, load(path))


def jobs() -> Iterator[Job]:
    """Yield every job declared across the workflow estate.

    Yields
    ------
    Job
        Every job in every workflow, in workflow-name order.
    """
    for path in workflow_paths():
        yield from jobs_in(path)


def step_text(step: dict[str, object]) -> str:
    """Return a step's shell body.

    Parameters
    ----------
    step
        One step mapping.

    Returns
    -------
    str
        The step's `run` script, or an empty string when the step invokes an
        action instead.
    """
    run = step.get("run")
    return run if isinstance(run, str) else ""


def cache_paths(step: dict[str, object]) -> list[str]:
    """Return the paths a cache step declares.

    Parameters
    ----------
    step
        One step mapping, normally an `actions/cache` invocation.

    Returns
    -------
    list of str
        One entry per non-empty line of the step's `path` input, stripped of
        surrounding whitespace. Empty when the step declares no paths.
    """
    inputs = step.get("with")
    if not isinstance(inputs, dict):
        return []
    declared = inputs.get("path")
    if not isinstance(declared, str):
        return []
    return [line.strip() for line in declared.splitlines() if line.strip()]


def is_cache_step(step: dict[str, object]) -> bool:
    """Report whether a step invokes the Actions cache.

    Parameters
    ----------
    step
        One step mapping.

    Returns
    -------
    bool
        True for the combined action and for its `restore` and `save`
        sub-actions alike.
    """
    uses = step.get("uses")
    return isinstance(uses, str) and uses.startswith(CACHE_ACTION_PREFIXES)


def builds_or_tests(job: Job) -> bool:
    """Report whether a job compiles or executes the product.

    The answer comes from the job's own steps rather than its name, so a job
    that loses its build step loses its claim on a paid runner at the same
    moment.

    Parameters
    ----------
    job
        The job to classify.

    Returns
    -------
    bool
        True when any step runs a command in `BUILD_OR_TEST_PATTERNS`.
    """
    return any(
        pattern.search(step_text(step))
        for step in job.steps
        for pattern in BUILD_OR_TEST_PATTERNS
    )

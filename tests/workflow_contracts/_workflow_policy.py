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

import json
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

#: One arm of a `runs-on` that chooses its label from the context, as
#: `<condition> && 'ubuntu-latest'`. A workflow that is both a developer gate
#: and a cron, or that must hand a fork a runner it can actually get, has
#: nowhere but the label to put the distinction. Reading such a value as one
#: opaque label would hide the Ubicloud request from every placement contract,
#: so the arms are parsed.
RUNNER_ARM_RE: re.Pattern[str] = re.compile(
    r"^(?P<condition>.+?)\s*&&\s*'(?P<label>[^']+)'$"
)

#: A bare label, which can only be the last arm: the value everything else
#: falls through to.
RUNNER_FALLBACK_RE: re.Pattern[str] = re.compile(r"^'(?P<label>[^']+)'$")

#: The conditions an arm may test. Anything else leaves the whole expression
#: unparsed and therefore opaque, which is the safe direction: a shape the
#: helpers cannot read is reported as one label rather than silently split
#: into arms nobody checked.
EVENT_CONDITION_RE: re.Pattern[str] = re.compile(
    r"^github\.event_name\s*==\s*'(?P<event>[a-z_]+)'$"
)

#: A field of the pull request's head repository, `fork` above all. A pull
#: request from a fork cannot obtain an Ubicloud runner, so those lanes fall
#: back to a GitHub-hosted one. The pattern admits any field of that object so
#: that swapping `fork` for a sibling leaves the expression parseable and the
#: fork contract, not the parser, is what reports the swap.
HEAD_REPO_CONDITION_RE: re.Pattern[str] = re.compile(
    r"^github\.event\.pull_request\.head\.repo\.(?P<field>[a-z_]+)$"
)

#: The field that decides whether a pull request came from a fork.
FORK_CONDITION = "github.event.pull_request.head.repo.fork"

#: Prefix shared by every Ubicloud runner label. Match on the prefix, not on
#: one exact label: the migration wave introduces `ubicloud-standard-2`, and a
#: contract keyed to the current label would wave the new one through.
UBICLOUD_LABEL_PREFIX = "ubicloud-"

#: One Ubicloud label the estate uses, for the helper tests to quote as a
#: sample. It is deliberately not "the" label: the jobs are right-sized per
#: job, so the set in use lives in `runner_sizing_test.APPROVED_SHAPES` and in
#: .github/actionlint.yaml, and no single constant can stand for it.
UBICLOUD_LABEL = "ubicloud-standard-4"

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
        r"\bmake\s+(?:all|test|test-workspace|test-github-tool|test-matrix"
        r"|lint|typecheck|check-fmt|build-github-tool-wasm)\b",
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


def _expression_body(declared: str) -> str | None:
    """Return the inside of a `${{ ... }}` scalar, or None if it is not one.

    Parameters
    ----------
    declared
        The raw scalar. A folded YAML scalar arrives with its line breaks
        already joined into single spaces.

    Returns
    -------
    str or None
        The expression's body, stripped, or ``None`` for a plain label.
    """
    joined = " ".join(declared.split())
    if not (joined.startswith("${{") and joined.endswith("}}")):
        return None
    return joined[3:-2].strip()


def _parse_arm(part: str, *, last: bool) -> tuple[str | None, str] | None:
    """Return one arm of a `runs-on` chain as a condition and a label.

    Parameters
    ----------
    part
        One `||`-separated piece of the expression.
    last
        Whether this is the final piece, which is the only place a bare label
        may appear: an unguarded arm earlier in the chain would make every
        arm after it unreachable.

    Returns
    -------
    tuple, or None
        ``(condition, label)``, with ``None`` as the condition of the final
        fallback. ``None`` when the piece is not an arm these helpers read.
    """
    fallback = RUNNER_FALLBACK_RE.match(part)
    if fallback is not None:
        return (None, fallback["label"]) if last else None
    if last:
        return None
    arm = RUNNER_ARM_RE.match(part)
    if arm is None or not _recognized_condition(arm["condition"]):
        return None
    return arm["condition"], arm["label"]


def _runs_on_chain(declared: str) -> tuple[tuple[str | None, str], ...] | None:
    """Split a context-dependent `runs-on` into its arms.

    The value is a chain of guarded labels ending in an unguarded one, as
    `${{ a && 'x' || b && 'y' || 'z' }}`. GitHub binds `&&` tighter than `||`
    and yields the first truthy operand, so the arms are tried in order and
    the bare label is what everything falls through to.

    Parameters
    ----------
    declared
        The raw `runs-on` scalar.

    Returns
    -------
    tuple of tuple, or None
        One `(condition, label)` pair per arm, with ``None`` as the condition
        of the final fallback. ``None`` when the value is not this form, which
        includes a plain label, a matrix expression, a chain with no fallback,
        and any chain naming a condition these helpers do not recognize.
    """
    body = _expression_body(declared)
    if body is None:
        return None
    parts = [part.strip() for part in body.split("||")]
    if len(parts) < 2:
        return None
    arms = [
        _parse_arm(part, last=index == len(parts) - 1)
        for index, part in enumerate(parts)
    ]
    if any(arm is None for arm in arms):
        return None
    return typ.cast("tuple[tuple[str | None, str], ...]", tuple(arms))


def conditional_runs_on_arms(
    declared: str,
) -> tuple[tuple[str | None, str], ...] | None:
    """Split a context-dependent `runs-on` into its arms, or report it opaque.

    The public face of the chain reader. A contract that has to refuse a
    shape the reader cannot split needs to ask that question directly:
    `runner_labels` answers it by returning the raw text as one label, which
    is indistinguishable from a job that genuinely names a runner nobody
    recognizes.

    Parameters
    ----------
    declared
        The raw `runs-on` scalar, with any folded line breaks already joined.

    Returns
    -------
    tuple of tuple, or None
        One `(condition, label)` pair per arm, with ``None`` as the condition
        of the final fallback. ``None`` when the value is not a chain this
        reader splits, a plain label included.
    """
    return _runs_on_chain(declared)


def selected_value(declared: str, event: str) -> str | None:
    """Resolve a guarded `${{ a && 'x' || 'y' }}` scalar for one event.

    `runs-on` is not the only value a workflow keys on the event. An `env`
    entry that says what a gate must see from an upstream job has the same
    shape, and reading it as opaque text would let the gate's own expectation
    drift from the job it describes without any contract noticing.

    Parameters
    ----------
    declared
        The raw scalar. A folded YAML scalar arrives with its line breaks
        already joined into single spaces.
    event
        A `github.event_name` value, such as ``push``.

    Returns
    -------
    str or None
        The value this event selects, or ``None`` when the scalar is not a
        guarded chain these helpers read. A plain literal is not a chain, so
        it answers ``None`` too: a caller asking what an event selects wants
        to know that nothing was selected by the event at all.
    """
    chain = _runs_on_chain(declared)
    if chain is None:
        return None
    return next(
        (value for condition, value in chain if _arm_selected_by(condition, event)),
        None,
    )


def _recognized_condition(condition: str) -> bool:
    """Report whether an arm's condition is one these helpers can answer.

    An unrecognized condition leaves the whole expression opaque. That is
    deliberate: splitting on a condition nobody has taught the helpers to
    evaluate would let `labels_for_event` answer confidently and wrongly.
    """
    return (
        EVENT_CONDITION_RE.match(condition) is not None
        or HEAD_REPO_CONDITION_RE.match(condition) is not None
    )


def _arm_selected_by(condition: str | None, event: str) -> bool:
    """Report whether an arm's condition holds for an event.

    A head-repository condition is answered ``False``. The contracts ask what
    a lane costs, and a fork's pull request runs GitHub-hosted at no cost to
    this repository; answering ``True`` would report every such lane as free
    and hide the shape it actually buys for a branch pull request.
    """
    if condition is None:
        return True
    match = EVENT_CONDITION_RE.match(condition)
    if match is not None:
        return match["event"] == event
    return False


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
            chain = _runs_on_chain(declared)
            if chain is not None:
                # Every arm is reported, so a job that reaches Ubicloud in any
                # context still answers `uses_ubicloud` and stays inside the
                # timeout and sccache contracts. Repeats are dropped: two arms
                # naming `ubuntu-latest` are one runner, and reporting it twice
                # would read as a job asking for two labels.
                return tuple(dict.fromkeys(label for _, label in chain))
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
        # The mapping form has to be unwrapped first, exactly as
        # `runner_labels` does. Reading `{labels: <conditional>}` without
        # unwrapping would fall through to both arms and report the Ubicloud
        # fallback as the label a schedule selects.
        if isinstance(declared, dict):
            declared = declared.get("labels")
        if isinstance(declared, str):
            chain = _runs_on_chain(declared)
            if chain is not None:
                selected = next(
                    (
                        label
                        for condition, label in chain
                        if _arm_selected_by(condition, event)
                    ),
                    chain[-1][1],
                )
                return (selected,)
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


#: `github.event_name == 'x'` inside a job's `if`. The events a condition
#: names are what decides whether the job can run on a given trigger, and a
#: reader that missed them would judge every guarded job runnable everywhere.
EVENT_EQUALITY_RE: re.Pattern[str] = re.compile(
    r"github\.event_name\s*==\s*'(?P<event>[a-z_]+)'"
)

#: `github.event_name != 'x'`, the other half of the same question. An
#: inequality excludes exactly one event and admits every other, including the
#: ones nobody has added yet.
EVENT_INEQUALITY_RE: re.Pattern[str] = re.compile(
    r"github\.event_name\s*!=\s*'(?P<event>[a-z_]+)'"
)

#: `release.yml` is generated by dist and regenerated wholesale on a version
#: bump, so a hand-added key there does not survive. It runs only on a tag
#: push, never on Ubicloud, and computes its matrix from a previous job's
#: output, so it sits outside the runner-cost and suite contracts alike.
DIST_GENERATED = "release.yml"

#: A matrix leg list chosen by the event, as
#: `${{ github.event_name == 'pull_request' && fromJSON('[...]')
#: || fromJSON('[...]') }}`. It is the `runs-on` story one level down: one
#: workflow serves several triggers, the leg list differs between them, and
#: `exclude` cannot express it because GitHub processes `include` afterwards.
#: Reading the scalar as opaque would leave the contracts unable to say what
#: any leg runs, so the arms are parsed.
CONDITIONAL_INCLUDE_RE: re.Pattern[str] = re.compile(
    r"^\$\{\{\s*github\.event_name\s*==\s*'(?P<event>[a-z_]+)'\s*&&\s*"
    r"fromJSON\('(?P<when>.*?)'\)\s*\|\|\s*"
    r"fromJSON\('(?P<otherwise>.*)'\)\s*\}\}$"
)


def triggers(document: dict[str, object]) -> dict[str, object]:
    """Return a parsed workflow's `on:` mapping.

    PyYAML resolves an unquoted `on:` key to the boolean ``True``, so a
    workflow that omits the quotes would otherwise read as having no triggers
    and pass every trigger-keyed contract vacuously.

    Parameters
    ----------
    document
        A parsed workflow document.

    Returns
    -------
    dict
        The workflow's triggers, or an empty mapping when it declares none.
    """
    declared = document.get("on", document.get(True))
    return declared if isinstance(declared, dict) else {}


def runs_on_event(job: Job, event: str) -> bool:
    """Report whether a job's own guard admits an event.

    The reading is deliberately narrow and errs towards "it runs". A condition
    that never mentions `github.event_name` cannot exclude an event. An
    inequality naming the event excludes it and admits every other, including
    the triggers nobody has added yet. A set of equalities admits exactly the
    events it names. Anything else is treated as runnable, so an expression
    this cannot follow reports work rather than hiding it.

    Parameters
    ----------
    job
        The job whose `if` condition is read.
    event
        A `github.event_name` value, such as ``push``.

    Returns
    -------
    bool
        True when the job can run on that event.
    """
    condition = " ".join(str(job.body.get("if", "")).split())
    if "github.event_name" not in condition:
        return True
    excluded = {match["event"] for match in EVENT_INEQUALITY_RE.finditer(condition)}
    if event in excluded:
        return False
    admitted = {match["event"] for match in EVENT_EQUALITY_RE.finditer(condition)}
    if admitted:
        return event in admitted
    return True


def _declared_matrix(job: Job) -> dict[str, object] | None:
    """Return a job's `strategy.matrix` mapping, or None when it has none.

    Raises
    ------
    AssertionError
        If the matrix is a form these helpers cannot read. Returning "no legs"
        for an unreadable matrix would exempt the job from every contract
        keyed on what its legs run, which is the silent pass they exist to
        prevent.
    """
    strategy = job.body.get("strategy")
    if not isinstance(strategy, dict) or "matrix" not in strategy:
        return None
    matrix = strategy["matrix"]
    if not isinstance(matrix, dict) or set(matrix) != {"include"}:
        message = (
            f"{job} declares a matrix this helper cannot read: {matrix!r}. "
            "Every matrix in this estate is an `include` list, literal or "
            "chosen by the event; teach the helper before writing another."
        )
        raise AssertionError(message)
    return matrix


def _resolve_include(job: Job, declared: object, event: str) -> list[object]:
    """Return a matrix `include` as a list, resolving an event-chosen one.

    Raises
    ------
    AssertionError
        If the value is neither a list nor a conditional expression these
        helpers can read.
    """
    if isinstance(declared, str):
        match = CONDITIONAL_INCLUDE_RE.match(" ".join(declared.split()))
        if match is None:
            message = f"{job} computes its legs in a form this cannot read"
            raise AssertionError(message)
        arm = match["when"] if match["event"] == event else match["otherwise"]
        declared = json.loads(arm)
    if not isinstance(declared, list):
        message = f"{job} declares a matrix include that is not a list"
        raise AssertionError(message)
    return declared


def matrix_legs(job: Job, event: str) -> tuple[dict[str, str], ...]:
    """Return the matrix legs a job expands to on an event.

    Parameters
    ----------
    job
        The job whose `strategy.matrix` is read.
    event
        The `github.event_name` an event-conditional leg list is resolved
        against.

    Returns
    -------
    tuple of dict
        One mapping per leg. A job with no matrix expands to a single empty
        leg, so a caller can treat every job the same way.
    """
    matrix = _declared_matrix(job)
    if matrix is None:
        return ({},)
    return tuple(
        {str(key): str(value) for key, value in leg.items()}
        for leg in _resolve_include(job, matrix["include"], event)
        if isinstance(leg, dict)
    )


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

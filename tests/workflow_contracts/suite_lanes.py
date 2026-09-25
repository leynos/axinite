"""Reads every suite-running lane out of the workflow files.

The matching is deliberately line-by-line. A contract that finds a
suite command anywhere inside a multiline ``run`` passes when the step
wraps it in ``if false; then ...; fi`` or appends ``|| true``, so each
line is judged as a plain invocation and a disguised one is reported
rather than counted.
"""

import collections.abc as cabc
import pathlib
import shlex
import typing as typ

from _workflow_policy import WORKFLOW_DIR, jobs_of, load, workflow_paths
from suite_actions import admits_leg, matrix_leg_names, runs_suite_through_action

#: The commands that run the workspace suite under nextest. A step
#: running one of these is bound by both nextest tiers. Matched as whole
#: tokens on the line, because `cargo nextest --version` is a probe and
#: not a run.
SUITE_MARKERS: typ.Final[tuple[str, ...]] = (
    "cargo llvm-cov nextest",
    "cargo nextest run",
)

#: Shapes that put a suite command on a line without running it as the
#: line's own command, or without letting its failure end the step.
#:
#: `if false; then cargo nextest run; fi` keeps the text and runs
#: nothing, so a substring search counts a lane that never runs the
#: suite and demands a ceiling of it. `cargo nextest run || true` does
#: run the suite but discards its verdict, so the lane's budgets are
#: asserted while its result is not. Neither is judged here; both are
#: reported, because a contract that cannot tell what a line does
#: should say so rather than guess.
DISGUISES: typ.Final[tuple[str, ...]] = ("|| true", "|| :", "if ", "&&", ";", "|")

#: Arguments that turn a suite command into a probe. `cargo nextest run
#: --help` and `--version` print and exit without running a test, so a
#: job whose only suite line is one of these runs no suite and must not
#: be held to a ceiling, nor counted as the lane the tree needs.
PROBE_ARGUMENTS: typ.Final[frozenset[str]] = frozenset(
    {"--help", "-h", "--version", "-V"}
)


def _tokens_of(line: str) -> list[str] | None:
    """Return the shell words of one line, or None if it is unreadable.

    Parameters
    ----------
    line
        One line of a step's script.

    Returns
    -------
    list of str or None
        The words, or None when the quoting does not close.
    """
    try:
        return shlex.split(line)
    except ValueError:
        return None


def _collapsed(line: str) -> str:
    """Return one line with every run of whitespace reduced to a space.

    The markers below are written with single spaces and the shell does
    not care: ``cargo   nextest run`` and a tab-separated spelling both
    run the suite. Comparing against the raw line missed them in the
    dangerous direction, because a lane whose suite command was written
    that way was counted as running no suite, held to no ceiling, and
    not reported as a line the reading could not judge either.

    Parameters
    ----------
    line
        One line of a step's script.

    Returns
    -------
    str
        The same line with its whitespace runs collapsed.
    """
    return " ".join(line.split())


def _names_a_suite_command(line: str) -> bool:
    """Return whether one line mentions a suite command at all.

    Mentioning is weaker than invoking, and deliberately so: the two are
    compared below, and a line that mentions one without invoking it is
    the case this contract refuses to judge.

    Parameters
    ----------
    line
        One line of a step's script.

    Returns
    -------
    bool
        True when a suite marker appears on the line.
    """
    collapsed = _collapsed(line)
    return any(marker in collapsed for marker in SUITE_MARKERS)


def _is_suite_line(line: str) -> bool:
    """Return whether one line runs the suite plainly.

    Plainly means the line is the command and its arguments, and
    nothing else. Reading the whole ``run`` value as one string, as an
    earlier version did, counted a lane whose only mention of the suite
    was inside `if false; then ...; fi`, and would have demanded a
    ceiling of a job that never runs it.

    The marker is matched as whole shell words rather than as a text
    prefix, because `cargo nextest runbook` begins with the same
    characters as `cargo nextest run` and runs no test. A probe is
    rejected for the same reason: `cargo nextest run --help` prints and
    exits, so a job whose only suite line is a probe would satisfy the
    assertion that the suite runs somewhere while running no test.
    Neither is silently dropped; both still name a suite command, so
    both are reported as lines this reading cannot judge.

    Parameters
    ----------
    line
        One line of a step's script.

    Returns
    -------
    bool
        True when the line runs a suite command and nothing else.
    """
    stripped = line.strip()
    if not _names_a_suite_command(stripped):
        return False
    if any(disguise in stripped for disguise in DISGUISES):
        return False
    tokens = _tokens_of(stripped)
    if tokens is None:
        return False
    if PROBE_ARGUMENTS.intersection(tokens):
        return False
    return any(
        tokens[: len(marker_tokens)] == marker_tokens
        for marker_tokens in (marker.split() for marker in SUITE_MARKERS)
    )


def _disguised_suite_lines(job_body: dict[str, typ.Any]) -> list[str]:
    """Return lines naming a suite command without plainly running one.

    Parameters
    ----------
    job_body
        The job's parsed mapping.

    Returns
    -------
    list[str]
        The offending lines, stripped.
    """
    return [
        stripped
        for step in job_body.get("steps") or []
        if isinstance(step, dict)
        for line in str(step.get("run", "")).splitlines()
        if (stripped := line.strip())
        and _names_a_suite_command(stripped)
        and not _is_suite_line(stripped)
    ]


class SuiteLane(typ.NamedTuple):
    """One job that runs the suite, with the ceiling enclosing it.

    Attributes
    ----------
    workflow
        The workflow file's name.
    job
        The job's identifier.
    ceiling
        The job's ``timeout-minutes`` in seconds, or None when it
        declares none and so inherits GitHub's six-hour default.
    invocations
        How many times the job runs the suite. One ceiling encloses all
        of them, so the requirement below it scales with this count; a
        lane reported as one run when it makes two is budgeted a single
        whole-run window and cancelled part way through the second.
    conditions
        The ``if`` on each suite step and on its job, in step order. A
        skipped step runs no suite, so every budget here says nothing
        about it; the condition is part of what identifies a lane
        rather than incidental to it.
    """

    workflow: str
    job: str
    ceiling: float | None
    invocations: int = 1
    conditions: tuple[tuple[object, object], ...] = ()

    def __str__(self) -> str:
        """Return a location suitable for a failure message.

        Returns
        -------
        str
            ``workflow:job`` for this lane.
        """
        return f"{self.workflow}:{self.job}"


def _step_runs(step: dict[str, object]) -> int:
    """Return how many suite runs one step makes.

    A step calling the shared coverage action with nextest makes one; a
    `run:` step makes one per plain suite line.
    """
    if runs_suite_through_action(step):
        return 1
    return sum(_is_suite_line(line) for line in str(step.get("run", "")).splitlines())


def _suite_invocations(job_body: dict[str, object]) -> int:
    """Return how many times one leg of a job runs the suite, at most.

    Every plain suite line counts, wherever it sits: two commands in one
    step's script are two runs exactly as two steps are. Each carries
    its own whole-run budget, because nextest starts that clock when
    tests begin and starts it again for the next invocation, while the
    job timer above them runs once.

    The count is per matrix leg, and the largest leg's count is the
    lane's, because each leg is its own job with its own timer. A step
    whose `if` names one leg (`matrix.name == 'x'` or `!= 'x'`) counts
    only on the legs it admits; `coverage.yml` runs the suite from a
    `run:` step on two legs and through the action on the third, and
    that is one run per leg, not two.

    Parameters
    ----------
    job_body
        The job's parsed mapping.

    Returns
    -------
    int
        The number of suite invocations on the busiest leg.
    """
    steps = [step for step in job_body.get("steps") or [] if isinstance(step, dict)]
    return max(
        sum(_step_runs(step) for step in steps if admits_leg(step.get("if"), leg))
        for leg in matrix_leg_names(job_body)
    )


def _suite_steps(job_body: dict[str, object]) -> list[dict[str, object]]:
    """Return the steps in one job that run the suite, in job order."""
    return [
        step
        for step in job_body.get("steps") or []
        if isinstance(step, dict) and _step_runs(step)
    ]


def normalized_condition(condition: object) -> object:
    """Return a condition with its whitespace collapsed.

    GitHub's folded scalars leave a trailing newline and wrap long
    expressions, so the same condition can be written several ways
    without changing what it means. Comparing the collapsed form keeps
    the pin about the expression rather than about how it was folded,
    while a non-string value such as ``False`` is returned unchanged so
    ``if: false`` is still distinguishable from an absent condition.

    Parameters
    ----------
    condition
        The value the YAML parser produced for an ``if``.

    Returns
    -------
    object
        The collapsed string, or the value unchanged.

    Examples
    --------
    >>> normalized_condition("a\n  || b\n")
    'a || b'
    """
    if isinstance(condition, str):
        return " ".join(condition.split())
    return condition


def suite_lanes_in(
    documents: cabc.Iterable[tuple[str, dict[str, object]]],
) -> tuple[SuiteLane, ...]:
    """Return every job in those documents that runs the suite.

    The query is separate from the acquisition so it can be driven with
    controlled workflows. Reading the repository's own files inside the
    query left no way to ask what this reading makes of a lane that does
    not exist here, and a contract that can only be exercised against
    the tree it guards is one whose own behaviour goes unasserted.

    Every suite-running job is included, not only those declaring a
    ceiling, so a job that never had one is visible as ``None`` rather
    than absent.

    Parameters
    ----------
    documents
        Pairs of workflow file name and parsed document.

    Returns
    -------
    tuple of SuiteLane
        One entry per suite-running job.
    """
    lanes: list[SuiteLane] = []
    for name, document in documents:
        for job in jobs_of(name, document):
            body = job.body
            invocations = _suite_invocations(body)
            if not invocations:
                continue
            raw = body.get("timeout-minutes")
            lanes.append(
                SuiteLane(
                    workflow=job.workflow,
                    job=job.job_id,
                    ceiling=None if raw is None else float(str(raw)) * 60.0,
                    invocations=invocations,
                    conditions=tuple(
                        (step.get("if"), body.get("if")) for step in _suite_steps(body)
                    ),
                )
            )
    return tuple(lanes)


def suite_lanes_of(directory: pathlib.Path = WORKFLOW_DIR) -> tuple[SuiteLane, ...]:
    """Return every job in the workflows that runs the suite.

    This is the acquisition half: it reads the workflow files and hands
    the parsed documents to :func:`suite_lanes_in`, which is the pure
    query and takes parsed documents. Callers that already have
    documents should use that one; this exists for the callers that do
    not. The directory is a parameter for the same reason it is one on
    ``workflow_paths``, so the same reading can be pointed at a
    temporary tree.

    Parameters
    ----------
    directory
        Directory of workflow files. Defaults to the repository's own.

    Returns
    -------
    tuple of SuiteLane
        One entry per suite-running job.

    Raises
    ------
    SourceReadError
        If the directory cannot be listed, or a workflow in it cannot be
        read. Reported rather than skipped: a workflow the reading never
        saw could hold the very lane this contract exists to bound, and
        an empty result would read as a tree with no suite lanes and
        satisfy every assertion over it.
    """
    return suite_lanes_in((path.name, load(path)) for path in workflow_paths(directory))

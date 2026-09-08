"""Reads every suite-running lane out of the workflow files.

The matching is deliberately line-by-line. A contract that finds a
suite command anywhere inside a multiline ``run`` passes when the step
wraps it in ``if false; then ...; fi`` or appends ``|| true``, so each
line is judged as a plain invocation and a disguised one is reported
rather than counted.
"""

import typing as typ

from _workflow_policy import jobs_of, load, workflow_paths

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
    return any(marker in line for marker in SUITE_MARKERS)


def _is_suite_line(line: str) -> bool:
    """Return whether one line runs the suite plainly.

    Plainly means the line is the command and its arguments, and
    nothing else. Reading the whole ``run`` value as one string, as an
    earlier version did, counted a lane whose only mention of the suite
    was inside `if false; then ...; fi`, and would have demanded a
    ceiling of a job that never runs it.

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
    return any(stripped.startswith(marker) for marker in SUITE_MARKERS)


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
    conditions
        The ``if`` on each suite step and on its job, in step order. A
        skipped step runs no suite, so every budget here says nothing
        about it; the condition is part of what identifies a lane
        rather than incidental to it.
    """

    workflow: str
    job: str
    ceiling: float | None
    conditions: tuple[tuple[object, object], ...] = ()

    def __str__(self) -> str:
        """Return a location suitable for a failure message.

        Returns
        -------
        str
            ``workflow:job`` for this lane.
        """
        return f"{self.workflow}:{self.job}"


def _runs_the_suite(job_body: dict[str, object]) -> bool:
    """Return whether a job runs the workspace suite under nextest.

    Parameters
    ----------
    job_body
        The job's parsed mapping.

    Returns
    -------
    bool
        True when a step runs one of :data:`SUITE_MARKERS`.
    """
    return any(
        _is_suite_line(line)
        for step in job_body.get("steps") or []
        if isinstance(step, dict)
        for line in str(step.get("run", "")).splitlines()
    )


def _suite_steps(job_body: dict[str, object]) -> list[dict[str, object]]:
    """Return the steps in one job that run the suite.

    Parameters
    ----------
    job_body
        The job's parsed mapping.

    Returns
    -------
    list of dict
        The suite-running steps, in the order the job runs them.
    """
    return [
        step
        for step in job_body.get("steps") or []
        if isinstance(step, dict)
        and any(_is_suite_line(line) for line in str(step.get("run", "")).splitlines())
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


def suite_lanes_of() -> tuple[SuiteLane, ...]:
    """Return every job that runs the suite, with its ceiling.

    Every such job is included, not only those declaring a ceiling, so a
    job that never had one is visible as ``None`` rather than absent.

    Returns
    -------
    tuple of SuiteLane
        One entry per suite-running job.
    """
    lanes: list[SuiteLane] = []
    for path in workflow_paths():
        document = load(path)
        for job in jobs_of(path.name, document):
            body = job.body
            if not isinstance(body, dict) or not _runs_the_suite(body):
                continue
            raw = body.get("timeout-minutes")
            lanes.append(
                SuiteLane(
                    workflow=job.workflow,
                    job=job.job_id,
                    ceiling=None if raw is None else float(str(raw)) * 60.0,
                    conditions=tuple(
                        (step.get("if"), body.get("if")) for step in _suite_steps(body)
                    ),
                )
            )
    return tuple(lanes)

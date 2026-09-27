"""Which events a workflow and its jobs admit, and what matrix legs they run.

Split from `_workflow_policy.py`, which reads jobs and runners. The questions
here are all keyed on the event: which triggers a workflow declares, whether a
job's own guard admits an event, and which matrix legs a job expands to when
its leg list is chosen by the event. Every function is pure in the parsed job
or document it is handed.

A matrix this cannot read raises `MatrixReadError` naming the workflow and
job, rather than answering "no legs": an unreadable matrix read as empty would
exempt the job from every contract keyed on what its legs run.
"""

from __future__ import annotations

import json
import re
import typing as typ

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from _workflow_policy import Job


class MatrixReadError(ValueError):
    """A job's matrix is in a form these helpers cannot expand.

    The message names the job as `workflow.yml:job-id`, so the failure says
    which workflow to fix.
    """


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
    MatrixReadError
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
        raise MatrixReadError(message)
    return matrix


def _resolve_include(job: Job, declared: object, event: str) -> list[object]:
    """Return a matrix `include` as a list, resolving an event-chosen one.

    Raises
    ------
    MatrixReadError
        If the value is neither a list nor a conditional expression these
        helpers can read, or the arm it selects is not valid JSON.
    """
    if isinstance(declared, str):
        declared = _selected_include(job, declared, event)
    if not isinstance(declared, list):
        message = f"{job} declares a matrix include that is not a list"
        raise MatrixReadError(message)
    return declared


def _selected_include(job: Job, declared: str, event: str) -> object:
    """Return the `fromJSON` arm an event-chosen `include` selects, parsed.

    Raises
    ------
    MatrixReadError
        If the expression is not the conditional form, or the selected arm is
        not valid JSON.
    """
    match = CONDITIONAL_INCLUDE_RE.match(" ".join(declared.split()))
    if match is None:
        message = f"{job} computes its legs in a form this cannot read"
        raise MatrixReadError(message)
    arm = match["when"] if match["event"] == event else match["otherwise"]
    try:
        return json.loads(arm)
    except json.JSONDecodeError as error:
        message = (
            f"{job} selects a leg list for {event!r} that is not valid JSON "
            f"({error})"
        )
        raise MatrixReadError(message) from error


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

    Raises
    ------
    MatrixReadError
        If the matrix, or the leg list the event selects, cannot be read.
    """
    matrix = _declared_matrix(job)
    if matrix is None:
        return ({},)
    return tuple(
        {str(key): str(value) for key, value in leg.items()}
        for leg in _resolve_include(job, matrix["include"], event)
        if isinstance(leg, dict)
    )

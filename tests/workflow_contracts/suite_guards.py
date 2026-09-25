"""What a suite lane must not do, read out of the workflow files.

Separated from ``suite_lanes`` so that identifying a lane and judging
one stay legible apart, and so neither module outgrows the 400-line
limit ``AGENTS.md`` sets.

Two things are judged here, and neither is a nextest budget. The cargo
watchdog tier exists only where a step calls the shared coverage action,
and there it must be set explicitly rather than left at the action's
1,800 s default; a watchdog variable written where no step reads it is
reported too, in any of the three scopes a step inherits its environment
from. And a lane that runs the suite while tolerating its failure
satisfies every budget this contract asserts while discarding the
verdict those budgets exist to protect.
"""

import typing as typ

from _workflow_policy import Job, jobs_of
from suite_actions import (
    WATCHDOG_VARIABLE,
    uses_coverage_action,
    watchdog_budget,
)
from suite_lanes import _suite_steps

def _steps_of(job_body: object) -> list[dict[str, object]]:
    """Return one job's steps, or an empty list.

    Parameters
    ----------
    job_body
        The job's parsed value, which need not be a mapping.

    Returns
    -------
    list of dict
        The step mappings, in the order the job runs them.
    """
    if not isinstance(job_body, dict):
        return []
    steps = job_body.get("steps")
    if not isinstance(steps, list):
        return []
    return [step for step in steps if isinstance(step, dict)]


def _watchdog_offences(
    workflow: str, job_id: str, step: dict[str, object], budget: float | None
) -> list[str]:
    """Return what one step does with the watchdog that the tiers forbid.

    Two separate things are wrong, so they are reported separately: a
    step may call the action with nothing setting its watchdog, so the
    1,800 s default applies, or name the variable on a step that does not
    call the action, where nothing reads it.

    Parameters
    ----------
    workflow
        The workflow file's name.
    job_id
        The job's identifier.
    step
        One parsed step.
    budget
        The watchdog budget the step runs under, None when nothing sets
        it.

    Returns
    -------
    list of str
        One entry per offence, empty when the step commits none.
    """
    offences: list[str] = []
    uses_action = uses_coverage_action(step)
    if uses_action and budget is None:
        offences.append(
            f"{workflow}:{job_id} uses the action with its watchdog at the "
            f"1,800 s default"
        )
    if WATCHDOG_VARIABLE in _env_of(step) and not uses_action:
        offences.append(
            f"{workflow}:{job_id} sets {WATCHDOG_VARIABLE} at step level on a "
            f"step that does not use the action"
        )
    return offences


def _env_of(scope: object) -> dict[str, object]:
    """Return one scope's ``env`` mapping, or an empty one.

    Parameters
    ----------
    scope
        A parsed workflow document, job body, or step.

    Returns
    -------
    dict of str to object
        The mapping, or an empty one when the scope declares no ``env``
        or declares it as something other than a mapping.
    """
    match scope:
        case {"env": dict() as environment}:
            return environment
        case _:
            return {}


def _uses_action(job_body: object) -> bool:
    """Report whether any step of a job calls the coverage action."""
    return any(uses_coverage_action(step) for step in _steps_of(job_body))


def watchdog_offences_of(workflow: str, document: dict[str, object]) -> list[str]:
    """Return what one workflow does with the watchdog that the tiers forbid.

    All three scopes are read. GitHub resolves a name declared at more
    than one of workflow, job and step scope to the most specific
    declaration rather than merging them, so a
    ``RUN_RUST_CARGO_WAIT_TIMEOUT`` written at workflow or job level sets
    the watchdog of every action step beneath it. Written where no step
    beneath it calls the action, it sets nothing, and is reported at the
    scope that declares it, because that is the line that has to change.

    Parameters
    ----------
    workflow
        The workflow file's name.
    document
        The parsed workflow document.

    Returns
    -------
    list of str
        One entry per offence, empty when the workflow commits none.
    """
    jobs = list(jobs_of(workflow, document))
    offences = _unread_variable_offences(workflow, document, jobs)
    for job in jobs:
        for step in _steps_of(job.body):
            budget = watchdog_budget(document, job.body, step)
            offences.extend(_watchdog_offences(workflow, job.job_id, step, budget))
    return offences


def _unread_variable_offences(
    workflow: str, document: dict[str, object], jobs: list[Job]
) -> list[str]:
    """Report the watchdog variable at workflow or job scope with no reader."""
    offences: list[str] = []
    if WATCHDOG_VARIABLE in _env_of(document) and not any(
        _uses_action(job.body) for job in jobs
    ):
        offences.append(f"{workflow} sets {WATCHDOG_VARIABLE} at workflow level")
    offences.extend(
        f"{workflow}:{job.job_id} sets {WATCHDOG_VARIABLE} at job level"
        for job in jobs
        if WATCHDOG_VARIABLE in _env_of(job.body) and not _uses_action(job.body)
    )
    return offences


def _ceiling_of(job_body: dict[str, object]) -> float | None:
    """Return a job's `timeout-minutes` in seconds, None when it has none."""
    raw = job_body.get("timeout-minutes")
    return None if raw is None else float(str(raw)) * 60.0


def watchdog_windows_of(
    workflow: str, document: dict[str, object]
) -> list[tuple[str, float, float | None]]:
    """Return each explicitly set watchdog with the job ceiling above it.

    Parameters
    ----------
    workflow
        The workflow file's name.
    document
        The parsed workflow document.

    Returns
    -------
    list of tuple
        `(location, budget, ceiling)` per action step whose watchdog is
        set, in seconds; the ceiling is None when the job declares no
        `timeout-minutes`. A step left at the default is an offence
        above, not a window.
    """
    return [
        (f"{workflow}:{job.job_id}", budget, _ceiling_of(job.body))
        for job in jobs_of(workflow, document)
        for step in _steps_of(job.body)
        if uses_coverage_action(step)
        and (budget := watchdog_budget(document, job.body, step)) is not None
    ]


def _tolerates_failure(declared: object) -> bool:
    """Return whether a ``continue-on-error`` value tolerates a failure.

    Only an explicit false is a refusal. An absent key is also a
    refusal, and is not passed here. Everything else is treated as
    tolerance, including an expression: a contract cannot evaluate
    ``${{ ... }}`` and must not certify a lane whose failure handling it
    cannot read.

    Parameters
    ----------
    declared
        The value the YAML parser produced for ``continue-on-error``.

    Returns
    -------
    bool
        True when the value is anything but an explicit false.

    Examples
    --------
    >>> _tolerates_failure(False)
    False
    >>> _tolerates_failure("false")
    False
    >>> _tolerates_failure(True)
    True
    """
    match declared:
        case bool():
            return declared
        case str():
            return declared.strip().lower() != "false"
        case _:
            return True


def failure_tolerances(
    workflow: str, job_id: str, job_body: dict[str, object]
) -> list[str]:
    """Return where a suite lane tolerates the suite failing.

    ``continue-on-error`` on the step or on the job makes a failing
    suite a passing lane. Every budget this contract asserts is about
    when the suite is stopped; none of them says anything about a lane
    that runs it and discards the verdict, so such a lane is reported
    rather than certified.

    Both scopes are read because they differ in effect and in fix: on
    the step the job goes green with the step failed, on the job the
    workflow goes green with the job failed.

    A job that runs no suite step is not judged at all. The consumer
    walks every job in every workflow, so reading the job-level key
    unconditionally would report a documentation or lint lane that
    tolerates its own failure as a lane discarding the suite's verdict,
    which is an offence it has not committed and a fix that would change
    the wrong line.

    Parameters
    ----------
    workflow
        The workflow file's name.
    job_id
        The job's identifier.
    job_body
        The job's parsed mapping.

    Returns
    -------
    list of str
        One entry per offence, empty when the lane commits none.
    """
    offences: list[str] = []
    suite_steps = _suite_steps(job_body)
    if not suite_steps:
        return offences
    declared = job_body.get("continue-on-error")
    if declared is not None and _tolerates_failure(declared):
        offences.append(
            f"{workflow}:{job_id} sets continue-on-error: {declared!r} on the job"
        )
    for step in suite_steps:
        on_step = step.get("continue-on-error")
        if on_step is not None and _tolerates_failure(on_step):
            offences.append(
                f"{workflow}:{job_id} sets continue-on-error: {on_step!r} on the "
                f"suite step {step.get('name', step.get('run'))!r}"
            )
    return offences

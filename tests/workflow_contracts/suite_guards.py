"""What a suite lane must not do, read out of the workflow files.

Separated from ``suite_lanes`` so that identifying a lane and judging
one stay legible apart, and so neither module outgrows the 400-line
limit ``AGENTS.md`` sets.

Two things are judged here, and neither is a budget. The cargo watchdog
tier does not exist in this repository and must not appear unnoticed, in
any of the three scopes a step inherits its environment from. And a lane
that runs the suite while tolerating its failure satisfies every budget
this contract asserts while discarding the verdict those budgets exist
to protect.
"""

import typing as typ

from _workflow_policy import jobs_of
from suite_lanes import _suite_steps

#: The environment variable the shared coverage action reads for its
#: wall-clock cap on one `cargo` invocation. Asserted absent: this
#: repository does not use that action.
WATCHDOG_VARIABLE: typ.Final[str] = "RUN_RUST_CARGO_WAIT_TIMEOUT"
COVERAGE_ACTION: typ.Final[str] = "shared-actions/.github/actions/generate-coverage"


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
    workflow: str, job_id: str, step: dict[str, object]
) -> list[str]:
    """Return what one step does that the absent tier forbids.

    Two separate things are wrong, so they are reported separately: a
    step may adopt the action without naming the variable, or name the
    variable without adopting the action, and the fix differs.

    Parameters
    ----------
    workflow
        The workflow file's name.
    job_id
        The job's identifier.
    step
        One parsed step.

    Returns
    -------
    list of str
        One entry per offence, empty when the step commits none.
    """
    offences: list[str] = []
    if COVERAGE_ACTION in str(step.get("uses", "")):
        offences.append(f"{workflow}:{job_id} uses the action")
    if WATCHDOG_VARIABLE in _env_of(step):
        offences.append(f"{workflow}:{job_id} sets {WATCHDOG_VARIABLE} at step level")
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


def watchdog_offences_of(workflow: str, document: dict[str, object]) -> list[str]:
    """Return what one workflow does that the absent tier forbids.

    All three scopes are read. GitHub gives a step the union of the
    workflow's ``env``, its job's and its own, so a
    ``RUN_RUST_CARGO_WAIT_TIMEOUT`` written at workflow or job level
    reaches the suite step exactly as one written on the step does. A
    check reading the step alone therefore certifies the tier as absent
    while the watchdog is in force, which is the inversion this contract
    exists to catch.

    Each scope is reported once, at the scope that declares it, because
    that is the line that has to change.

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
    offences: list[str] = []
    if WATCHDOG_VARIABLE in _env_of(document):
        offences.append(f"{workflow} sets {WATCHDOG_VARIABLE} at workflow level")
    for job in jobs_of(workflow, document):
        if WATCHDOG_VARIABLE in _env_of(job.body):
            offences.append(
                f"{workflow}:{job.job_id} sets {WATCHDOG_VARIABLE} at job level"
            )
        for step in _steps_of(job.body):
            offences.extend(_watchdog_offences(workflow, job.job_id, step))
    return offences


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
    declared = job_body.get("continue-on-error")
    if declared is not None and _tolerates_failure(declared):
        offences.append(
            f"{workflow}:{job_id} sets continue-on-error: {declared!r} on the job"
        )
    for step in _suite_steps(job_body):
        on_step = step.get("continue-on-error")
        if on_step is not None and _tolerates_failure(on_step):
            offences.append(
                f"{workflow}:{job_id} sets continue-on-error: {on_step!r} on the "
                f"suite step {step.get('name', step.get('run'))!r}"
            )
    return offences

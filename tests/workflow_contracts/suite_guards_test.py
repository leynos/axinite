"""How the timeout contract reads what a suite lane must not do.

The two guards in ``suite_guards`` are both assertions of absence: the
cargo watchdog tier does not exist in this repository, and no lane
tolerates the suite failing. Every workflow here satisfies both, so a
reading that detected neither would pass over the tree exactly as a
correct one does. They are therefore driven with controlled workflows.
"""

import pytest
from _workflow_policy import parse_workflow
from suite_guards import (
    COVERAGE_ACTION,
    WATCHDOG_VARIABLE,
    _watchdog_offences,
    failure_tolerances,
    watchdog_offences_of,
)


def workflow(text: str) -> dict[str, object]:
    """Return a controlled workflow document, parsed.

    Parameters
    ----------
    text
        A workflow file's text.

    Returns
    -------
    dict of str to object
        The parsed document.
    """
    return parse_workflow(text, "controlled.yml")


def suite_job(text: str) -> dict[str, object]:
    """Return the ``test`` job of a controlled workflow.

    Parameters
    ----------
    text
        A workflow file's text declaring a ``test`` job.

    Returns
    -------
    dict of str to object
        The job's parsed mapping.
    """
    jobs = workflow(text)["jobs"]
    assert isinstance(jobs, dict)
    body = jobs["test"]
    assert isinstance(body, dict)
    return body


@pytest.mark.parametrize(
    ("step", "expected"),
    [
        pytest.param({"run": "cargo llvm-cov nextest run"}, 0, id="an-ordinary-step"),
        pytest.param({"uses": f"{COVERAGE_ACTION}@abc123"}, 1, id="adopts-the-action"),
        pytest.param(
            {"run": "make test", "env": {WATCHDOG_VARIABLE: "1800"}},
            1,
            id="names-the-variable",
        ),
        pytest.param(
            {"uses": f"{COVERAGE_ACTION}@abc123", "env": {WATCHDOG_VARIABLE: "1800"}},
            2,
            id="both-at-once",
        ),
    ],
)
def test_both_halves_of_the_absent_tier_are_detected(
    step: dict[str, object], expected: int
) -> None:
    """Adopting the action and naming the variable are separate offences.

    The tier is absent by construction here, so no workflow in the tree
    commits either offence and the assertion over the tree is satisfied
    by a reading that detects neither. Driving the reading directly is
    the only way to show it would notice.
    """
    offences = _watchdog_offences("ci.yml", "test", step)
    assert len(offences) == expected, (
        f"{step} must yield {expected} offence(s), got {offences}"
    )


#: A controlled workflow whose watchdog variable is written at the scope
#: named by the format field, and nowhere else.
_WATCHDOG_AT = """\
name: controlled
on: push
{workflow_env}jobs:
  test:
    runs-on: ubuntu-latest
{job_env}    steps:
{step_env}      - run: cargo nextest run --workspace
"""


@pytest.mark.parametrize(
    ("scope", "fields"),
    [
        pytest.param(
            "workflow",
            {"workflow_env": f"env:\n  {WATCHDOG_VARIABLE}: '1800'\n"},
            id="workflow-level",
        ),
        pytest.param(
            "job",
            {"job_env": f"    env:\n      {WATCHDOG_VARIABLE}: '1800'\n"},
            id="job-level",
        ),
        pytest.param(
            "step",
            {
                "step_env": (
                    f"      - run: make prepare\n"
                    f"        env:\n          {WATCHDOG_VARIABLE}: '1800'\n"
                )
            },
            id="step-level",
        ),
    ],
)
def test_the_watchdog_is_found_in_every_scope_a_step_inherits(
    scope: str, fields: dict[str, str]
) -> None:
    """GitHub hands a step the union of three ``env`` mappings.

    A variable written at workflow or job level reaches the suite step
    exactly as one written on the step does, so a reading confined to
    the step reports the tier as absent while the watchdog is in force.
    That is the inversion the tier's asserted absence exists to catch,
    and it is the one shape this repository's own workflows cannot show,
    because none of them sets the variable anywhere.
    """
    blanks = {"workflow_env": "", "job_env": "", "step_env": ""}
    offences = watchdog_offences_of(
        "controlled.yml", workflow(_WATCHDOG_AT.format(**(blanks | fields)))
    )
    assert len(offences) == 1, (
        f"a watchdog variable at {scope} level must be reported once, got {offences}"
    )
    assert scope in offences[0], (
        f"the offence must name the scope that declares it, got {offences[0]!r}"
    )


def test_a_workflow_setting_the_watchdog_nowhere_is_clean() -> None:
    """The reading must not report an offence the workflow never commits.

    Without this the scope sweep above would be satisfied by a reading
    that reported every workflow, which would make the assertion over
    the tree fail permanently rather than pass.
    """
    assert (
        watchdog_offences_of(
            "controlled.yml",
            workflow(_WATCHDOG_AT.format(workflow_env="", job_env="", step_env="")),
        )
        == []
    )


#: A controlled suite lane whose failure tolerance is written at the
#: scope named by the format field, and nowhere else.
_TOLERANCE_AT = """\
name: controlled
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    timeout-minutes: 60
{job_tolerance}    steps:
      - run: cargo nextest run --workspace
{step_tolerance}"""


@pytest.mark.parametrize(
    ("fields", "expected"),
    [
        pytest.param({}, 0, id="tolerating-nothing"),
        pytest.param(
            {"job_tolerance": "    continue-on-error: false\n"},
            0,
            id="an-explicit-refusal-on-the-job",
        ),
        pytest.param(
            {"step_tolerance": "        continue-on-error: false\n"},
            0,
            id="an-explicit-refusal-on-the-step",
        ),
        pytest.param(
            {"job_tolerance": "    continue-on-error: true\n"},
            1,
            id="tolerated-on-the-job",
        ),
        pytest.param(
            {"step_tolerance": "        continue-on-error: true\n"},
            1,
            id="tolerated-on-the-step",
        ),
        pytest.param(
            {
                "job_tolerance": "    continue-on-error: true\n",
                "step_tolerance": "        continue-on-error: true\n",
            },
            2,
            id="tolerated-at-both-scopes",
        ),
        pytest.param(
            {
                "step_tolerance": (
                    "        continue-on-error: ${{ github.event_name == 'push' }}\n"
                )
            },
            1,
            id="an-expression-nobody-can-evaluate",
        ),
    ],
)
def test_a_tolerated_suite_failure_is_reported_at_either_scope(
    fields: dict[str, str], expected: int
) -> None:
    """`continue-on-error` makes a failing suite a passing lane.

    Every budget the timeout contract asserts is about when the suite is
    stopped, and none of them says anything about the verdict, so a lane
    carrying this passes each of them while discarding the result. The
    two scopes differ in effect and in fix, so they are reported
    separately, and an expression is reported because a contract that
    cannot evaluate it must not certify the lane.
    """
    blanks = {"job_tolerance": "", "step_tolerance": ""}
    body = suite_job(_TOLERANCE_AT.format(**(blanks | fields)))
    offences = failure_tolerances("controlled.yml", "test", body)
    assert len(offences) == expected, (
        f"{fields} must yield {expected} offence(s), got {offences}"
    )


def test_tolerance_on_a_step_that_runs_no_suite_is_not_reported() -> None:
    """The guard is about the suite, not about every step in the lane.

    A preparatory step allowed to fail says nothing about whether the
    suite's verdict survives, so reporting it would make the guard
    unusable in any job that has one.
    """
    body = suite_job(
        "name: controlled\n"
        "on: push\n"
        "jobs:\n"
        "  test:\n"
        "    runs-on: ubuntu-latest\n"
        "    steps:\n"
        "      - run: make prepare\n"
        "        continue-on-error: true\n"
        "      - run: cargo nextest run --workspace\n"
    )
    assert failure_tolerances("controlled.yml", "test", body) == []

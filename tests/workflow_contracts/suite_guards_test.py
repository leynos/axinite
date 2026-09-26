"""How the timeout contract reads what a suite lane must not do.

The two guards in ``suite_guards`` both refuse a shape the tree does not
contain: a cargo watchdog left at the action's default or written where
nothing reads it, and a lane tolerating the suite failing. Every workflow
here satisfies both, so a reading that detected neither would pass over
the tree exactly as a correct one does. They are therefore driven with
controlled workflows.
"""

import pytest
from _workflow_files import parse_workflow
from suite_actions import COVERAGE_ACTION, WATCHDOG_VARIABLE
from suite_guards import (
    _watchdog_offences,
    failure_tolerances,
    watchdog_offences_of,
    watchdog_windows_of,
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
    assert isinstance(jobs, dict), (
        f"the controlled document's jobs must parse to a mapping, got "
        f"{type(jobs).__name__}; the fixture is malformed rather than the "
        f"reading being wrong"
    )
    body = jobs["test"]
    assert isinstance(body, dict), (
        f"the controlled document's `test` job must parse to a mapping, got "
        f"{type(body).__name__}; the fixture is malformed rather than the "
        f"reading being wrong"
    )
    return body


@pytest.mark.parametrize(
    ("step", "budget", "expected"),
    [
        pytest.param({"run": "cargo llvm-cov nextest run"}, None, 0, id="an-ordinary-step"),
        pytest.param(
            {"uses": f"{COVERAGE_ACTION}@abc123"}, None, 1, id="the-action-at-its-default"
        ),
        pytest.param(
            {"uses": f"{COVERAGE_ACTION}@abc123"}, 3600.0, 0, id="the-action-set"
        ),
        pytest.param(
            {"run": "make test", "env": {WATCHDOG_VARIABLE: "1800"}},
            None,
            1,
            id="the-variable-where-nothing-reads-it",
        ),
    ],
)
def test_each_watchdog_offence_is_detected(
    step: dict[str, object], budget: float | None, expected: int
) -> None:
    """An action at its default and a variable nothing reads are offences.

    The tree sets the watchdog on both action steps and nowhere else, so
    the assertion over the tree is satisfied by a reading that detects
    neither offence. Driving the reading directly is the only way to show
    it would notice.
    """
    offences = _watchdog_offences("ci.yml", "test", step, budget)
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
    """GitHub resolves ``env`` to the most specific scope that declares it.

    It does not merge same-name declarations, so each of the three
    scopes has to be scanned: a variable written at workflow or job
    level and nowhere else reaches the suite step, and one written on
    the step overrides it. Either way the watchdog is in force, so a
    reading confined to the step reports the tier as absent while it
    runs.
    Written where no step calls the action, it sets nothing, and this
    repository's own workflows cannot show that shape.
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
    offences = watchdog_offences_of(
        "controlled.yml",
        workflow(_WATCHDOG_AT.format(workflow_env="", job_env="", step_env="")),
    )
    assert offences == [], (
        f"a workflow that sets the watchdog at no scope commits no offence, "
        f"so the reading must report none; it reported {offences}"
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
    offences = failure_tolerances("controlled.yml", "test", body)
    assert offences == [], (
        f"a preparatory step allowed to fail discards no suite verdict, so "
        f"the reading must report none; it reported {offences}"
    )


def test_a_job_that_runs_no_suite_is_not_judged_on_its_own_tolerance() -> None:
    """The guard reports lanes that discard the suite's verdict.

    The consumer walks every job in every workflow, so a documentation
    or lint job allowed to fail reaches this reading too. It runs no
    suite, so its tolerance discards no verdict, and reporting it would
    name a line whose change would fix nothing.
    """
    body = suite_job(
        "name: controlled\n"
        "on: push\n"
        "jobs:\n"
        "  test:\n"
        "    runs-on: ubuntu-latest\n"
        "    continue-on-error: true\n"
        "    steps:\n"
        "      - run: markdownlint docs\n"
    )
    offences = failure_tolerances("controlled.yml", "test", body)
    assert offences == [], (
        f"a job that runs no suite discards no suite verdict however "
        f"tolerant it is, so the reading must report none; it reported "
        f"{offences}"
    )


def test_a_job_that_runs_the_suite_is_judged_on_its_own_tolerance() -> None:
    """Scoping the reading to suite jobs must not disarm it.

    This is the case the previous test's scoping could have taken with
    it: the same job-level key, on a job that does run the suite, still
    makes a failing suite a passing workflow and is still reported.
    """
    body = suite_job(
        "name: controlled\n"
        "on: push\n"
        "jobs:\n"
        "  test:\n"
        "    runs-on: ubuntu-latest\n"
        "    continue-on-error: true\n"
        "    steps:\n"
        "      - run: cargo nextest run --workspace\n"
    )
    offences = failure_tolerances("controlled.yml", "test", body)
    assert len(offences) == 1, (
        f"a suite job tolerating its own failure is one offence, not several "
        f"and not none; the reading returned {offences}"
    )
    assert "on the job" in offences[0], (
        f"the offence must name the scope a reader has to edit, which is the "
        f"job rather than a step; it said {offences[0]!r}"
    )


#: A controlled workflow whose one action step reads its watchdog from the
#: scope named by the format field.
_ACTION_WATCHDOG_AT = """\
name: controlled
on: push
{workflow_env}jobs:
  test:
    runs-on: ubuntu-latest
    timeout-minutes: 90
{job_env}    steps:
      - uses: leynos/shared-actions/.github/actions/generate-coverage@abc
{step_setting}
"""


@pytest.mark.parametrize(
    "fields",
    [
        pytest.param(
            {"step_setting": "        with:\n          cargo-wait-timeout: '3600'"},
            id="the-input",
        ),
        pytest.param(
            {"step_setting": f"        env:\n          {WATCHDOG_VARIABLE}: '3600'"},
            id="step-level",
        ),
        pytest.param(
            {"job_env": f"    env:\n      {WATCHDOG_VARIABLE}: '3600'\n"},
            id="job-level",
        ),
        pytest.param(
            {"workflow_env": f"env:\n  {WATCHDOG_VARIABLE}: '3600'\n"},
            id="workflow-level",
        ),
    ],
)
def test_an_action_step_reads_its_watchdog_from_any_scope(fields: dict[str, str]) -> None:
    """Every place GitHub would resolve the budget from is read, and none offends.

    A reading confined to the input would report a job-level budget as
    the default, and one confined to the step would miss both outer
    scopes.
    """
    blanks = {"workflow_env": "", "job_env": "", "step_setting": ""}
    document = workflow(_ACTION_WATCHDOG_AT.format(**(blanks | fields)))
    offences = watchdog_offences_of("controlled.yml", document)
    assert offences == [], f"a set watchdog is no offence; got {offences}"
    windows = watchdog_windows_of("controlled.yml", document)
    assert windows == [("controlled.yml:test", 3600.0, 5400.0)], (
        f"the window should read the budget and the ceiling; got {windows}"
    )


def test_an_action_step_left_at_the_default_is_no_window() -> None:
    """The default is an offence, not a window the ordering check could pass."""
    document = workflow(
        _ACTION_WATCHDOG_AT.format(workflow_env="", job_env="", step_setting="")
    )
    assert watchdog_windows_of("controlled.yml", document) == []
    offences = watchdog_offences_of("controlled.yml", document)
    assert len(offences) == 1 and "default" in offences[0], (
        f"the action at its default must be reported once; got {offences}"
    )

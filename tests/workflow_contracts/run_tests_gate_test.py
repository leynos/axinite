"""Contracts for the aggregate gate that decides whether `test.yml` passed.

`run-tests` is the job branch protection watches. It runs `if: always()` and
reads the result of every other job in the workflow, so it is the only place
where a lane that vanished is told apart from a lane that passed. Two things
this pull request changed make that reading easy to get silently wrong.

The `tests` job now stands down on a push to `main`, because `coverage.yml`
executes the same suite there with instrumentation. A gate that still demanded
`success` would fail every push; one that tolerated `skipped` everywhere would
stop noticing a suite that disappeared on a pull request. The gate therefore
compares against `EXPECTED_TESTS_RESULT`, which is `skipped` on a push and
`success` on every other trigger, and these contracts assert both arms.

The GitHub tool crate now has a lane of its own, `github-tool-tests`, because
`tools-src/github` is outside the workspace and no `--workspace` run reaches
it. A lane nothing reads is decoration: it would go red while the gate went
green. The equalities below are therefore in both directions, between the
jobs the gate declares as dependencies, the jobs its loop walks, and the jobs
its `case` can actually answer for. A `case` with no arm for a name leaves
`result` holding the previous iteration's value, so a missing arm does not
fail loudly; it reports the wrong job's outcome.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import re

import pytest
from _workflow_policy import (
    REPOSITORY_ROOT,
    Job,
    jobs_of,
    load,
    selected_value,
    step_text,
    triggers,
)

#: The workflow whose gate this file reads.
WORKFLOW = "test.yml"

#: The gate job itself.
GATE_JOB = "run-tests"

#: The job whose result the gate compares against `EXPECTED_TESTS_RESULT`
#: rather than against the pass-or-skip rule the other jobs get.
SUITE_JOB = "tests"

#: The lane for the crate that is excluded from the workspace. Named here
#: because it is the one this pull request added: no `--workspace` run reaches
#: `tools-src/github`, so if the gate does not read this lane, nothing does.
GITHUB_TOOL_JOB = "github-tool-tests"

#: The variable the gate resolves from the event.
EXPECTATION_VARIABLE = "EXPECTED_TESTS_RESULT"

#: What `tests` must report on a push to `main`, where `coverage.yml` runs the
#: suite instead.
SKIPPED = "skipped"

#: What `tests` must report on every other trigger.
SUCCESS = "success"

#: A `needs.<job>.result` reference in the gate's shell body.
RESULT_REFERENCE_RE: re.Pattern[str] = re.compile(
    r"\$\{\{\s*needs\.(?P<job>[a-z0-9-]+)\.result\s*\}\}"
)

#: The `for job in ... ; do` list the gate walks.
LOOP_LIST_RE: re.Pattern[str] = re.compile(
    r"for\s+job\s+in\s+(?P<names>.+?);\s*do", re.DOTALL
)

#: One arm of the gate's `case`, which binds a name to the result it reads.
CASE_ARM_RE: re.Pattern[str] = re.compile(
    r"^\s*(?P<job>[a-z0-9-]+)\)\s*result=", re.MULTILINE
)


def _gate() -> Job:
    """Return the gate job, failing the run if the workflow has lost it."""
    document = load(REPOSITORY_ROOT / ".github" / "workflows" / WORKFLOW)
    for job in jobs_of(WORKFLOW, document):
        if job.job_id == GATE_JOB:
            return job
    pytest.fail(f"{WORKFLOW} declares no {GATE_JOB} job")


def _gate_body() -> str:
    """Return the gate's single shell body.

    Joining several steps would let an assertion pass on text from a step that
    never runs, so the gate is required to be one step.
    """
    steps = _gate().body.get("steps")
    assert isinstance(steps, list) and len(steps) == 1, (
        f"{GATE_JOB} is expected to be one shell step; the readings below "
        "assume the whole gate is in that one body"
    )
    return step_text(steps[0])


def _declared_needs() -> tuple[str, ...]:
    """Return the jobs the gate declares as dependencies."""
    declared = _gate().body.get("needs")
    assert isinstance(declared, list), f"{GATE_JOB} declares no needs list"
    return tuple(str(name) for name in declared)


def _looped_jobs() -> tuple[str, ...]:
    """Return the job names the gate's loop walks."""
    body = _gate_body()
    match = LOOP_LIST_RE.search(body)
    assert match is not None, (
        f"{GATE_JOB}'s body has no `for job in ...; do` list; the loop is how "
        "every gated job is checked, and its absence is not a passing state"
    )
    # The list is written across continuation lines. The backslashes are what
    # the shell consumes; splitting on whitespace afterwards is what leaves
    # the names.
    return tuple(match["names"].replace("\\", " ").split())


def _case_arms() -> tuple[str, ...]:
    """Return the job names the gate's `case` binds a result to."""
    return tuple(match["job"] for match in CASE_ARM_RE.finditer(_gate_body()))


def _supported_events() -> tuple[str, ...]:
    """Return the triggers the workflow declares, less the reusable caller.

    `workflow_call` is not a `github.event_name`: a called workflow reports
    the caller's event, so asking what the expectation resolves to for it
    would be asking a question GitHub never poses.
    """
    document = load(REPOSITORY_ROOT / ".github" / "workflows" / WORKFLOW)
    return tuple(
        event for event in triggers(document) if event != "workflow_call"
    )


def test_the_gate_reads_something_at_all() -> None:
    """Guard against readings that match nothing.

    Every equality below is satisfied by three empty sets, so the gate's own
    reach is asserted first. The floor is the suite lane plus the tool lane
    plus the loop having found names at all.
    """
    body = _gate_body()
    referenced = {match["job"] for match in RESULT_REFERENCE_RE.finditer(body)}
    assert SUITE_JOB in referenced, (
        f"{GATE_JOB} never reads {SUITE_JOB}'s result; the contracts below "
        "would pass with the gate's body emptied"
    )
    assert _looped_jobs(), f"{GATE_JOB}'s loop walks no jobs"
    assert _case_arms(), f"{GATE_JOB}'s case binds no results"


def test_the_gate_runs_whatever_happened() -> None:
    """A gate that can itself be skipped decides nothing.

    On a push the suite job stands down. Without `if: always()` the gate would
    stand down with it, branch protection would see a skipped check, and the
    expectation this file exists to enforce would never be evaluated.
    """
    assert _gate().body.get("if") == "always()", (
        f"{GATE_JOB} must run with `if: always()`; without it the gate is "
        "skipped exactly when an upstream job is, which is when it matters"
    )


def test_every_gated_job_is_a_dependency_and_is_read() -> None:
    """Hold the dependency list, the loop and the case arms equal.

    Each of the three can drift from the others silently. A job missing from
    `needs` reports `skipped` forever. A job missing from the loop is never
    looked at. A job missing from the `case` leaves `result` holding the
    previous iteration's value, so the gate reports another job's outcome
    under this one's name.
    """
    needs = set(_declared_needs())
    looped = set(_looped_jobs())
    arms = set(_case_arms())

    # The suite job is checked by the expectation rather than by the loop, so
    # it is the one dependency the loop is allowed not to walk.
    assert SUITE_JOB in needs, f"{GATE_JOB} does not depend on {SUITE_JOB}"
    assert needs - {SUITE_JOB} == looped, (
        "the gate's dependencies and the jobs its loop walks disagree: "
        f"only in needs {sorted(needs - {SUITE_JOB} - looped)}, "
        f"only in the loop {sorted(looped - (needs - {SUITE_JOB}))}"
    )
    assert looped == arms, (
        "the gate's loop and its case arms disagree: "
        f"only in the loop {sorted(looped - arms)}, "
        f"only in the case {sorted(arms - looped)}"
    )


def test_the_github_tool_lane_is_gated() -> None:
    """The lane for the crate outside the workspace must reach the gate.

    `tools-src/github` is excluded from the workspace, so neither the coverage
    runs nor the `tests` legs reach it; this lane is the only execution of
    that crate's suite. A lane nothing reads goes red on its own while the
    gate branch protection watches goes green.
    """
    assert GITHUB_TOOL_JOB in _declared_needs(), (
        f"{GATE_JOB} does not depend on {GITHUB_TOOL_JOB}"
    )
    assert GITHUB_TOOL_JOB in _looped_jobs(), (
        f"{GATE_JOB}'s loop does not walk {GITHUB_TOOL_JOB}"
    )
    assert GITHUB_TOOL_JOB in _case_arms(), (
        f"{GATE_JOB}'s case binds no result for {GITHUB_TOOL_JOB}"
    )


def test_the_expected_suite_result_follows_the_event() -> None:
    """A push expects a skip; every other trigger expects a pass.

    Both arms are asserted. Reading only the push arm would pass with the
    expectation pinned to `skipped`, which is the mutation that stops the gate
    noticing a suite that vanished from a pull request.
    """
    declared = _gate().body.get("env", {})
    assert isinstance(declared, dict), f"{GATE_JOB} declares no env mapping"
    expression = declared.get(EXPECTATION_VARIABLE)
    assert isinstance(expression, str), (
        f"{GATE_JOB} does not declare {EXPECTATION_VARIABLE}"
    )

    assert selected_value(expression, "push") == SKIPPED, (
        f"{EXPECTATION_VARIABLE} must resolve to {SKIPPED!r} on a push, "
        f"where coverage.yml runs the suite instead: {expression}"
    )
    for event in _supported_events():
        if event == "push":
            continue
        assert selected_value(expression, event) == SUCCESS, (
            f"{EXPECTATION_VARIABLE} must resolve to {SUCCESS!r} on {event}, "
            f"which is the only run of the suite there: {expression}"
        )


def test_the_gate_fails_when_the_suite_result_is_not_the_expected_one() -> None:
    """Assert the comparison, not the variable's presence.

    A body that merely mentions `EXPECTED_TESTS_RESULT` proves nothing: the
    check is that the suite's result is compared against it and that a
    mismatch exits non-zero. Asserting the name alone stays green with the
    comparison deleted.
    """
    body = " ".join(_gate_body().split())
    comparison = (
        f'if [[ "${{{{ needs.{SUITE_JOB}.result }}}}" '
        f'!= "${EXPECTATION_VARIABLE}" ]]; then'
    )
    assert comparison in body, (
        f"{GATE_JOB} must compare {SUITE_JOB}'s result against "
        f"${EXPECTATION_VARIABLE}; found: {body}"
    )
    tail = body.split(comparison, 1)[1]
    assert tail.lstrip().startswith("echo") and "exit 1" in tail.split("fi", 1)[0], (
        "a mismatch must exit non-zero; the comparison currently reports "
        f"without failing: {tail.split('fi', 1)[0]}"
    )

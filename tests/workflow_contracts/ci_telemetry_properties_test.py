"""Properties of the telemetry contract checks over generated step sequences.

`_telemetry.py` holds the checks as pure functions over a job's steps. These
tests generate bounded step lists around the sound arrangement and assert that
every sound one is accepted and every generated fault is refused, so the
contract does not depend on the particular jobs the workflows have today.
"""

from __future__ import annotations

import copy
import typing as typ

from _telemetry import (
    FINISH,
    PIN,
    REPORT,
    START,
    STATUS,
    roll_up_problems,
    sccache_problems,
    telemetry_problems,
)
from hypothesis import given, settings
from hypothesis import strategies as st

Step = dict[str, object]
SETUP = f"leynos/shared-actions/.github/actions/setup-rust@{PIN}"
CHECKOUT: Step = {"uses": "actions/checkout@v6"}
DISK: Step = {"name": "Free disk space", "run": "df -h"}
WORK_STEP = st.builds(
    lambda i, text: {"name": f"work {i}", "run": text},
    st.integers(0, 99),
    st.sampled_from(["make", "cargo build", "pytest", "echo hi"]),
)
WORK = st.lists(WORK_STEP, max_size=6)
PRELUDE = st.lists(st.sampled_from([CHECKOUT, DISK]), max_size=2, unique_by=repr)
SAVES = st.lists(st.just({"uses": "actions/cache/save@v6"}), max_size=2)


def finish_step(*, with_status: bool) -> Step:
    """Return the finish step, optionally carrying setup-rust's status."""
    step: Step = {"name": "Report", "if": "always()", "uses": FINISH}
    if with_status:
        step["with"] = {"sccache-status": STATUS}
    return step


@st.composite
def sound_jobs(draw: st.DrawFn) -> list[Step]:
    """Draw a sound carrying job, with or without setup-rust and sccache."""
    with_cache = draw(st.booleans())
    steps: list[Step] = [*draw(PRELUDE), {"name": "Start", "uses": START}]
    if with_cache:
        steps.append({"name": "Setup", "id": "setup-rust", "uses": SETUP})
    steps += draw(WORK)
    if with_cache:
        steps.append(
            {"name": "sccache", "id": "sccache", "if": "always()", "uses": REPORT}
        )
    steps.append(finish_step(with_status=with_cache))
    return copy.deepcopy([*steps, *draw(SAVES)])


def start_at(steps: list[Step]) -> int:
    """Return the index of the start step."""
    return next(i for i, s in enumerate(steps) if s.get("uses") == START)


def finish_at(steps: list[Step]) -> int:
    """Return the index of the finish step."""
    return next(i for i, s in enumerate(steps) if s.get("uses") == FINISH)


@settings(max_examples=200, deadline=None)
@given(sound_jobs())
def test_every_sound_arrangement_is_accepted(steps: list[Step]) -> None:
    """No problems are reported for any generated sound job."""
    assert telemetry_problems(steps) == [], "telemetry"
    assert sccache_problems(steps) == [], "sccache"


FAULTS: dict[str, typ.Callable[[list[Step]], None]] = {
    "duplicate-start": lambda s: s.insert(start_at(s), {"uses": START}),
    "duplicate-finish": lambda s: s.insert(finish_at(s), {"uses": FINISH}),
    "finish-removed": lambda s: s.pop(finish_at(s)),
    "start-removed": lambda s: s.pop(start_at(s)),
    "finish-not-always": lambda s: s[finish_at(s)].pop("if"),
    "work-before-start": lambda s: s.insert(start_at(s), {"name": "w", "run": "make"}),
    "work-after-finish": lambda s: s.insert(
        finish_at(s) + 1, {"name": "w", "run": "x"}
    ),
}


@settings(max_examples=200, deadline=None)
@given(sound_jobs(), st.sampled_from(sorted(FAULTS)))
def test_every_generated_telemetry_fault_is_refused(
    steps: list[Step], fault: str
) -> None:
    """Whatever the surrounding job, each fault is reported."""
    FAULTS[fault](steps)
    assert telemetry_problems(steps) != [], fault


def has_cache(steps: list[Step]) -> bool:
    """Report whether the generated job calls setup-rust."""
    return any(s.get("id") == "setup-rust" for s in steps)


@settings(max_examples=200, deadline=None)
@given(
    sound_jobs().filter(has_cache),
    st.sampled_from(
        [
            "no-status",
            "wrong-status",
            "report-not-always",
            "wrong-id",
            "by-hand",
            "moved",
        ]
    ),
)
def test_every_generated_sccache_fault_is_refused(
    steps: list[Step], fault: str
) -> None:
    """Bad wiring of the report or the status is refused in any surrounding job."""
    report = next(i for i, s in enumerate(steps) if s.get("uses") == REPORT)
    finish = finish_at(steps)
    if fault == "no-status":
        steps[finish].pop("with")
    elif fault == "wrong-status":
        steps[finish]["with"] = {"sccache-status": "fallback"}
    elif fault == "report-not-always":
        steps[report].pop("if")
    elif fault == "wrong-id":
        steps[report]["id"] = "other"
    elif fault == "by-hand":
        steps.insert(report, {"run": "sccache --show-stats"})
    else:
        steps.insert(report, {"name": "w", "run": "make"})
        steps.insert(finish_at(steps), steps.pop(report + 1))
    assert sccache_problems(steps) != [], fault


@settings(max_examples=100, deadline=None)
@given(st.lists(st.sampled_from("abcd"), min_size=1, max_size=4, unique=True))
def test_a_roll_up_is_one_run_step_over_its_needs(needs: list[str]) -> None:
    """One run step aggregating `needs` is accepted; any extra step is refused."""
    body: dict[str, object] = {
        "needs": needs,
        "steps": [{"run": "echo ${{ needs.a.result }}"}],
    }
    assert roll_up_problems(body) == [], "a roll-up"
    body["steps"] = [*typ.cast("list[Step]", body["steps"]), {"run": "echo"}]
    assert roll_up_problems(body) != [], "two steps"


@settings(max_examples=100, deadline=None)
@given(st.sampled_from([START, FINISH, REPORT]))
def test_a_roll_up_carrying_any_telemetry_action_is_refused(action: str) -> None:
    """A roll-up with a telemetry action is not a roll-up."""
    body: dict[str, object] = {"needs": ["a"], "steps": [{"uses": action}]}
    assert roll_up_problems(body) != [], action

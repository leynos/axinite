"""Contracts for the CI telemetry every job carries.

Right-sizing a runner needs the busy cores, the peak memory and the low-water
disk mark of the job on the shape it actually uses, and none of them appears in
a job's own output. Each job in the five instrumented workflows therefore
starts `ci-telemetry-start` early and ends with `ci-telemetry-finish` under
`if: always()`, so the central collector can read one record per job, including
the job that died of its shape. The only jobs without the pair are the pure
roll-ups below: they run one step over `needs` and measure nothing.

A job added later without the pair is not excused: the collector reads it as a
missing record, which is the signal the coverage numbers count. The exemption
table in `_telemetry.py` mirrors the collector's own list in leynos/grafana, so
a roll-up is excused in both places or in neither.

The checks are pure functions over a job's steps (`_telemetry.py`), so the
second half of this module hands them deliberately wrong arrangements and
requires each to be refused, whatever the workflows say today.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import copy
import typing as typ

import pytest
from _telemetry import (
    FINISH,
    INSTRUMENTED_WORKFLOWS,
    PIN,
    PINNED,
    REPORT,
    ROLL_UPS,
    START,
    STATUS,
    is_telemetry_action,
    roll_up_problems,
    sccache_problems,
    telemetry_problems,
    uses,
)
from _workflow_files import jobs
from _workflow_policy import Job


def _instrumented() -> tuple[Job, ...]:
    """Return every job of the instrumented workflows."""
    return tuple(j for j in jobs() if j.workflow in INSTRUMENTED_WORKFLOWS)


def _carrying() -> tuple[Job, ...]:
    """Return the instrumented jobs that must carry the telemetry pair."""
    return tuple(j for j in _instrumented() if (j.workflow, j.job_id) not in ROLL_UPS)


def _ids(candidates: tuple[Job, ...]) -> list[str]:
    """Return readable parameter identifiers for a job sequence."""
    return [str(j) for j in candidates]


def _setup_rust_jobs() -> tuple[Job, ...]:
    """Return the carrying jobs that call `setup-rust`, by action reference."""
    return tuple(
        j
        for j in _carrying()
        if any("/setup-rust@" in uses(s) for s in j.steps)
    )


def test_the_selectors_match_something() -> None:
    """Guard against a selector that silently matches nothing."""
    assert len(_carrying()) >= 15, f"only {len(_carrying())} carrying jobs found"
    found = {j.workflow for j in _instrumented()}
    assert found == set(INSTRUMENTED_WORKFLOWS), f"workflows found: {sorted(found)}"
    assert _setup_rust_jobs(), "no job calls setup-rust"


@pytest.mark.parametrize("job", _carrying(), ids=_ids(_carrying()))
def test_every_job_starts_and_reports_telemetry(job: Job) -> None:
    """The pair brackets the work, and the report runs even on failure."""
    assert telemetry_problems(job.steps) == [], f"{job} telemetry"


@pytest.mark.parametrize("job", _setup_rust_jobs(), ids=_ids(_setup_rust_jobs()))
def test_an_sccache_job_reports_through_the_action_and_passes_the_status(
    job: Job,
) -> None:
    """A job with `setup-rust` reports sccache through the action, not by hand."""
    assert sccache_problems(job.steps) == [], f"{job} sccache reporting"


@pytest.mark.parametrize("job", _carrying(), ids=_ids(_carrying()))
def test_the_old_sampler_is_gone(job: Job) -> None:
    """No job still calls the inline sampler that the actions replace."""
    assert "ci-resource-sampler" not in repr(job.body), f"{job} still calls it"


def test_every_telemetry_call_is_pinned_to_the_one_commit() -> None:
    """All three actions use the same full SHA, in every instrumented job."""
    seen: set[str] = set()
    for job in _instrumented():
        for step in job.steps:
            action = uses(step)
            if is_telemetry_action(step):
                assert PINNED.match(action), f"{job} uses an unpinned {action}"
                seen.add(action.rsplit("@", 1)[1])
    assert seen == {PIN}, f"telemetry actions pin {sorted(seen)}, not {PIN}"


@pytest.mark.parametrize(
    ("key", "reason"), sorted(ROLL_UPS.items()), ids=[str(k) for k in sorted(ROLL_UPS)]
)
def test_a_roll_up_exemption_names_a_real_roll_up(
    key: tuple[str, str], reason: str
) -> None:
    """An exemption is a `needs` roll-up that runs one step and measures nothing."""
    job = next(j for j in jobs() if (j.workflow, j.job_id) == key)
    assert reason.strip(), f"{job} has no reason"
    assert roll_up_problems(job.body) == [], f"{job} is not a roll-up"


def test_the_resource_sampler_script_is_gone() -> None:
    """The script the actions replace is deleted, so nothing can call it."""
    from pathlib import Path

    script = Path(__file__).parents[2] / "scripts" / "ci-resource-sampler.sh"
    assert not script.exists(), f"{script} still exists"


# --- the contract refuses wrong arrangements --------------------------------


def _good() -> list[dict[str, object]]:
    """Return a sound carrying job with `setup-rust`, as the workflows have them."""
    return [
        {"uses": "actions/checkout@v6"},
        {"name": "Start", "uses": START},
        {
            "name": "Setup Rust",
            "id": "setup-rust",
            "uses": f"leynos/shared-actions/.github/actions/setup-rust@{PIN}",
        },
        {"name": "Build", "run": "cargo build"},
        {"name": "Report sccache", "id": "sccache", "if": "always()", "uses": REPORT},
        {
            "name": "Report CI telemetry",
            "if": "always()",
            "uses": FINISH,
            "with": {"sccache-status": STATUS},
        },
        {"name": "Save", "uses": "actions/cache/save@v6"},
    ]


def _swap_start_and_finish(steps: list[dict[str, object]]) -> None:
    """Report before starting."""
    steps[1], steps[5] = steps[5], steps[1]


TELEMETRY_FAULTS: dict[str, typ.Callable[[list[dict[str, object]]], object]] = {
    "no-finish": lambda steps: steps.pop(5),
    "finish-not-always": lambda steps: steps[5].pop("if"),
    "report-before-start": _swap_start_and_finish,
    "work-before-start": lambda steps: steps.insert(1, {"name": "B", "run": "make"}),
    "work-after-report": lambda steps: steps.append({"name": "T", "run": "make test"}),
    "double-start": lambda steps: steps.insert(2, {"name": "S", "uses": START}),
}

SCCACHE_FAULTS: dict[str, typ.Callable[[list[dict[str, object]]], object]] = {
    "report-before-setup": lambda steps: steps.insert(2, steps.pop(4)),
    "report-moved-before-build": lambda steps: steps.insert(3, steps.pop(4)),
    "no-status": lambda steps: steps[5].pop("with"),
    "wrong-status": lambda steps: steps[5].update(
        {"with": {"sccache-status": "fallback"}}
    ),
    "report-not-always": lambda steps: steps[4].pop("if"),
    "report-wrong-id": lambda steps: steps[4].update(id="other"),
    "setup-wrong-id": lambda steps: steps[2].update(id="rust"),
    "by-hand": lambda steps: steps.insert(3, {"run": "sccache --show-stats"}),
}


@pytest.mark.parametrize("change", sorted(TELEMETRY_FAULTS))
def test_the_telemetry_check_refuses_each_wrong_arrangement(change: str) -> None:
    """Each deliberate fault is reported, so the contract cannot pass vacuously."""
    steps = copy.deepcopy(_good())
    TELEMETRY_FAULTS[change](steps)
    assert telemetry_problems(steps) != [], change


def test_the_sound_arrangement_has_no_problems() -> None:
    """The reference arrangement the mutations start from is itself accepted."""
    assert telemetry_problems(_good()) == [], "the reference telemetry is sound"
    assert sccache_problems(_good()) == [], "the reference sccache wiring is sound"


@pytest.mark.parametrize("change", sorted(SCCACHE_FAULTS))
def test_the_sccache_check_refuses_each_wrong_arrangement(change: str) -> None:
    """Moving the report, wiring the status wrongly or reading by hand is refused."""
    steps = copy.deepcopy(_good())
    SCCACHE_FAULTS[change](steps)
    assert sccache_problems(steps) != [], change


def test_a_job_without_setup_rust_has_no_sccache_requirements() -> None:
    """No `setup-rust` call, nothing to report."""
    assert sccache_problems([{"run": "make"}]) == [], "nothing to report"


@pytest.mark.parametrize(
    "body",
    [
        {"needs": ["a"], "steps": [{"run": "echo ${{ needs.a.result }}"}, {"run": "x"}]},
        {"needs": ["a"], "steps": [{"uses": "actions/checkout@v6"}]},
        {"needs": ["a"], "steps": [{"uses": FINISH}]},
        {"needs": ["a"], "steps": [{"uses": REPORT}]},
        {"needs": ["a"], "steps": [{"run": "true"}]},
        {"steps": [{"run": "echo ${{ needs.a.result }}"}]},
        {"needs": ["a"]},
    ],
    ids=[
        "two-steps",
        "an-action",
        "carries-finish",
        "carries-report",
        "does-not-aggregate",
        "no-needs",
        "no-steps",
    ],
)
def test_the_roll_up_check_refuses_a_job_that_is_not_a_roll_up(
    body: dict[str, object],
) -> None:
    """An exemption cannot hide a real job or a job that carries telemetry."""
    assert roll_up_problems(body) != [], f"{body} was accepted as a roll-up"


def test_a_real_roll_up_is_accepted() -> None:
    """One `run` step over `needs` is the exemption's shape."""
    body = {"needs": ["a"], "steps": [{"run": "echo ${{ needs.a.result }}"}]}
    assert roll_up_problems(body) == [], "one run over needs is a roll-up"

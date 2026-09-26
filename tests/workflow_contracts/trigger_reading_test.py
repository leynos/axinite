"""Unit tests for how a job's matrix legs are read, and how that fails.

`suite_duplication_test.py` expands the estate's matrices, which are all
readable, so the failure path is stated here against constructed jobs: a
matrix this cannot read must raise `MatrixReadError` naming the job, never
answer "no legs", which would exempt the job from every contract keyed on
what its legs run.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import pytest
from _trigger_reading import MatrixReadError, matrix_legs
from _workflow_policy import Job

#: An event-chosen leg list in the form the estate writes.
CONDITIONAL = (
    "${{ github.event_name == 'pull_request' && fromJSON('[{\"name\": \"pr\"}]') "
    "|| fromJSON('[{\"name\": \"push\"}]') }}"
)


def _job(matrix: object) -> Job:
    """Return a job whose `strategy.matrix` is the value given."""
    return Job("ci.yml", "build", {"strategy": {"matrix": matrix}})


@pytest.mark.parametrize(
    ("event", "expected"),
    [
        pytest.param("pull_request", ({"name": "pr"},), id="the-event-arm"),
        pytest.param("push", ({"name": "push"},), id="the-otherwise-arm"),
    ],
)
def test_an_event_chosen_leg_list_resolves_per_event(
    event: str, expected: tuple[dict[str, str], ...]
) -> None:
    """The positive half: each event reads its own arm."""
    assert matrix_legs(_job({"include": CONDITIONAL}), event) == expected


def test_a_job_without_a_matrix_is_one_empty_leg() -> None:
    """A caller can treat every job as a list of legs."""
    assert matrix_legs(Job("ci.yml", "build", {}), "push") == ({},)


@pytest.mark.parametrize(
    ("matrix", "reason"),
    [
        pytest.param(
            {"include": CONDITIONAL.replace('"pr"}]', '"pr"')},
            "not valid JSON",
            id="malformed-json-in-the-selected-arm",
        ),
        pytest.param(
            {"include": "${{ fromJSON(needs.plan.outputs.legs) }}"},
            "computes its legs in a form this cannot read",
            id="an-expression-of-another-form",
        ),
        pytest.param(
            {"include": {"name": "one"}}, "not a list", id="an-include-mapping"
        ),
        pytest.param(
            {"os": ["ubuntu-latest"]},
            "declares a matrix this helper cannot read",
            id="a-cross-product",
        ),
    ],
)
def test_an_unreadable_matrix_is_a_named_error(matrix: object, reason: str) -> None:
    """Each unreadable shape names the job and says what was wrong.

    `malformed-json-in-the-selected-arm` is the case that used to escape as a
    bare `JSONDecodeError`, naming neither the workflow nor the job.
    """
    with pytest.raises(MatrixReadError, match=reason) as raised:
        matrix_legs(_job(matrix), "pull_request")
    assert "ci.yml:build" in str(raised.value), (
        f"the error must name the job; it said {raised.value}"
    )

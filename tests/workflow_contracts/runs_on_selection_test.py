"""Unit tests for `selected_value`, which resolves a guarded scalar per event.

Split from `workflow_policy_helpers_test.py`. The reader answers what an
`${{ a && 'x' || 'y' }}` value selects on one event, and must answer `None`
for a shape it cannot read rather than guess.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import pytest
from _runs_on import selected_value
from _workflow_files import jobs_of, parse_workflow


@pytest.mark.parametrize(
    ("event", "expected"),
    [
        ("push", "skipped"),
        ("pull_request", "success"),
        ("schedule", "success"),
        ("workflow_dispatch", "success"),
    ],
)
def test_selected_value_resolves_a_guarded_scalar_per_event(
    event: str, expected: str
) -> None:
    """Both arms of a guarded value are read, not just the guarded one.

    A reader that answered the fallback for every event would agree with the
    truth on three of these four rows, so the push row is what discriminates.
    """
    declared = "${{ github.event_name == 'push' && 'skipped' || 'success' }}"
    assert selected_value(declared, event) == expected, (
        f"on {event!r}, {declared!r} selects {expected!r}; the reader answered "
        f"{selected_value(declared, event)!r}"
    )


@pytest.mark.parametrize(
    "declared",
    [
        pytest.param("success", id="plain-literal"),
        pytest.param(
            "${{ github.event_name == 'push' && 'skipped' }}", id="no-fallback"
        ),
        pytest.param(
            "${{ needs.tests.result == 'success' && 'yes' || 'no' }}",
            id="unrecognized-condition",
        ),
    ],
)
def test_selected_value_answers_none_for_a_shape_it_cannot_read(declared: str) -> None:
    """A shape the reader does not understand is opaque, never guessed.

    Answering the fallback for an unrecognized condition would let a contract
    report an expectation confidently and wrongly, which is worse than
    reporting that it could not read the value at all.
    """
    assert selected_value(declared, "push") is None, (
        f"{declared!r} is a shape the reader cannot read, so it must answer "
        f"None rather than guess; it answered "
        f"{selected_value(declared, 'push')!r}"
    )


def test_selected_value_reads_a_folded_scalar() -> None:
    """A folded YAML scalar arrives with its line breaks already joined.

    The estate writes these expressions across lines, so a reader that only
    handled the single-line form would answer ``None`` for every real one.
    """
    document = parse_workflow(
        """
on:
  push:
jobs:
  gate:
    runs-on: ubuntu-latest
    env:
      EXPECTED: >-
        ${{ github.event_name == 'push' && 'skipped'
        || 'success' }}
    steps:
      - run: 'true'
""",
        "folded.yml",
    )
    job = next(jobs_of("folded.yml", document))
    # `Job.body` is a `dict[str, object]`, so the nested reads have to narrow
    # before `selected_value` sees a `str`. Asserting each step also means a
    # fixture that stops declaring the expression names itself, rather than
    # raising `TypeError` from a subscript several frames away.
    env = job.body.get("env")
    assert isinstance(env, dict), "the fixture job declares no env mapping"
    declared = env.get("EXPECTED")
    assert isinstance(declared, str), "the fixture job declares no EXPECTED value"
    assert selected_value(declared, "push") == "skipped", (
        f"the folded expression {declared!r} selects 'skipped' on a push; the "
        f"reader answered {selected_value(declared, 'push')!r}"
    )
    assert selected_value(declared, "pull_request") == "success", (
        f"the folded expression {declared!r} takes its fallback on a pull "
        f"request; the reader answered "
        f"{selected_value(declared, 'pull_request')!r}"
    )

"""Contracts keeping the removed staging promotion process removed.

The promotion pipeline was deleted after failing every one of its 488 scheduled
runs on a secret this repository does not hold, while spending about £22 a month
on paid runners to do it. Its workflow went first, then the `claude-review.yml`
job that only a `staging-promotion` label could trigger, then the branch itself.

Nothing gates on any of that now, which is precisely why a fragment could come
back unnoticed: a workflow filtered on a branch that does not exist, or a job
guarded by a label nobody applies, simply never runs. It costs nothing, reports
nothing, and looks like working configuration to a reader.

These contracts read the parsed workflow rather than its text, so the history in
a comment survives and only a reference that could make a workflow *act* on
staging fails. `mutation-testing.yml` explains what the daily full-suite run
replaced, and should go on doing so.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import typing as typ

import pytest
from _workflow_policy import load, workflow_paths

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from pathlib import Path

#: The deleted branch.
BRANCH: typ.Final[str] = "staging"

#: The label that gated the deleted review job. Only the removed promotion job
#: applied it, so a condition naming it can never be true again.
LABEL: typ.Final[str] = "staging-promotion"

#: Workflow file the promotion pipeline lived in.
REMOVED_WORKFLOWS: typ.Final[tuple[str, ...]] = (
    "staging-ci.yml",
    "claude-review.yml",
)


def _values(document: object) -> typ.Iterator[str]:
    """Yield every string in a parsed workflow, keys included.

    Keys matter as much as values: a branch filter can put the branch name in
    either position depending on how the trigger is written.

    Walks with an explicit stack rather than recursion, which keeps the
    branching flat and cannot exhaust the interpreter stack on a deeply nested
    workflow.
    """
    pending: list[object] = [document]
    while pending:
        node = pending.pop()
        if isinstance(node, str):
            yield node
        elif isinstance(node, dict):
            pending.extend(node.keys())
            pending.extend(node.values())
        elif isinstance(node, list):
            pending.extend(node)


@pytest.mark.parametrize("name", REMOVED_WORKFLOWS)
def test_the_removed_workflows_stay_removed(name: str) -> None:
    """Neither the promotion pipeline nor its review job comes back.

    `claude-review.yml` is named here rather than left implicit because it read
    as working configuration: a valid workflow, on a valid trigger, guarded by a
    label that nothing applies. It had been skipped or cancelled in all 567 of
    its runs.
    """
    assert not any(path.name == name for path in workflow_paths()), (
        f"{name} was removed with the staging promotion process. Restoring it "
        "means restoring the pipeline deliberately, not reintroducing a "
        "workflow that cannot fire."
    )


@pytest.mark.parametrize(
    "path", workflow_paths(), ids=[p.name for p in workflow_paths()]
)
def test_no_workflow_acts_on_the_staging_branch(path: Path) -> None:
    """No workflow filters, checks out, or compares against a deleted branch.

    A filter on a branch that does not exist is not an error to GitHub; the
    workflow simply never matches. That is the failure worth catching, because
    nothing else reports it.
    """
    offenders = [value for value in _values(load(path)) if BRANCH in value]
    assert not offenders, (
        f"{path.name} still refers to the {BRANCH!r} branch in "
        f"{offenders!r}. That branch is deleted, so a workflow naming it "
        "cannot run and will not say so."
    )


@pytest.mark.parametrize(
    "path", workflow_paths(), ids=[p.name for p in workflow_paths()]
)
def test_no_workflow_waits_on_the_promotion_label(path: Path) -> None:
    """No job is guarded by a label only the removed pipeline applied."""
    offenders = [value for value in _values(load(path)) if LABEL in value]
    assert not offenders, (
        f"{path.name} still checks for the {LABEL!r} label in {offenders!r}. "
        "Only the removed promotion job applied it, so the condition can "
        "never be true."
    )

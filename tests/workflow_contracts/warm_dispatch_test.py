"""Contracts for the manual warm-cache dispatch.

Cache behaviour on `main` cannot be measured from a pull request. The
runner-migration exit evidence needs a run that restores what the merge
commit wrote and changes nothing, repeated so the second run's hit rate is
readable. That means `workflow_dispatch` on the workflows that carry the
developer-blocking Rust jobs.

A dispatch is only safe while it stays a reader. `github.ref` is
`refs/heads/main` for a dispatch against `main` just as it is for a push, so
a save step guarded on the ref alone would let a manual run take ownership of
a key from the job that is supposed to write it. Every save therefore has to
name the `push` event as well, and these contracts pin both halves together.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import pytest
from _workflow_policy import WORKFLOW_DIR, load

#: Workflows whose Rust jobs sit on the developer-blocking path, and whose
#: warm-cache behaviour on `main` is therefore part of the exit evidence.
#: `coverage.yml` and the scheduled workflows already declare a dispatch of
#: their own and are not re-asserted here.
WARM_RUN_WORKFLOWS: tuple[str, ...] = (
    "code_style.yml",
    "codescene-coverage.yml",
    "test.yml",
)


@pytest.mark.parametrize("workflow", WARM_RUN_WORKFLOWS)
def test_the_warm_run_workflows_accept_a_manual_dispatch(workflow: str) -> None:
    """Each developer-blocking workflow can be dispatched against a ref."""
    document = load(WORKFLOW_DIR / workflow)
    # PyYAML resolves an unquoted `on:` key to the boolean True, so a
    # workflow that drops the quotes would otherwise read as having no
    # triggers at all and pass every assertion below vacuously.
    triggers = document.get("on", document.get(True))
    assert isinstance(triggers, dict), (
        f"{workflow} must declare an on: mapping, not a shorthand"
    )
    assert "workflow_dispatch" in triggers, (
        f"{workflow} must accept workflow_dispatch; without it the warm-cache "
        "runs on main that the runner migration is measured by cannot be "
        "started at all"
    )
    # `gh workflow run --ref` and the Actions UI already choose the ref. An
    # input that named a branch would be a second, unvalidated way to say the
    # same thing, and the two could disagree.
    assert not triggers["workflow_dispatch"], (
        f"{workflow} must declare workflow_dispatch without inputs; the ref "
        "comes from the dispatch itself"
    )

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

import re
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


def _branch_expression_re(branch: str) -> re.Pattern[str]:
    """Return a pattern matching the forms that name `branch` as a branch.

    Three forms, each bounded so a longer branch name cannot match: fully
    qualified as `refs/heads/<branch>`, quoted as a comparison operand, or as
    the `@<branch>` suffix of an action reference.

    The bound is what makes the contract safe to keep. Without it
    `refs/heads/staging-preview` and `@staging-preview` both match `staging`,
    and a workflow with nothing to do with the deleted branch fails a mandatory
    gate. A contract that blocks legitimate work is as much a defect as one that
    misses a real reference, and the quoted forms already carry their own bound
    in the closing quote.
    """
    name = re.escape(branch)
    return re.compile(rf"(?:refs/heads/{name}|@{name})(?![\w./-])|'{name}'|\"{name}\"")


#: A checkout `ref` names a branch in either the bare or the qualified form, and
#: both check out the same thing. Comparing against the bare form alone let
#: `ref: refs/heads/staging` through, which is the gap this closes.
def _branch_ref_values(branch: str) -> frozenset[str]:
    """Return the values a checkout `ref` may use to name `branch`."""
    return frozenset({branch, f"refs/heads/{branch}"})


def _quoted(value: str) -> tuple[str, ...]:
    """Return the quoted spellings of an expression operand."""
    return (f"'{value}'", f'"{value}"')


def _as_strings(declared: object) -> tuple[str, ...]:
    """Return the strings a filter declares, in either form it may take.

    A branch filter accepts a single name or a list of them, and normalizing
    that here keeps the shape decision out of the caller.
    """
    if isinstance(declared, str):
        return (declared,)
    if isinstance(declared, list):
        return tuple(item for item in declared if isinstance(item, str))
    return ()


def _trigger_branches(document: dict[str, object]) -> typ.Iterator[str]:
    """Yield every branch named by a trigger's filters.

    PyYAML resolves an unquoted `on:` key to the boolean ``True``, so a workflow
    that drops the quotes would otherwise read as having no triggers.
    """
    triggers = document.get("on", document.get(True))
    if not isinstance(triggers, dict):
        return
    events = (event for event in triggers.values() if isinstance(event, dict))
    for event in events:
        for key in ("branches", "branches-ignore"):
            yield from _as_strings(event.get(key))


def _jobs(document: dict[str, object]) -> typ.Iterator[dict[str, object]]:
    """Yield each job body that is a mapping."""
    jobs = document.get("jobs")
    if not isinstance(jobs, dict):
        return
    yield from (body for body in jobs.values() if isinstance(body, dict))


def _steps(job: dict[str, object]) -> typ.Iterator[dict[str, object]]:
    """Yield each step mapping in a job."""
    steps = job.get("steps")
    if not isinstance(steps, list):
        return
    yield from (step for step in steps if isinstance(step, dict))


def _checkout_refs(document: dict[str, object]) -> typ.Iterator[str]:
    """Yield every explicit `ref` a step checks out."""
    for job in _jobs(document):
        for step in _steps(job):
            inputs = step.get("with")
            if isinstance(inputs, dict) and isinstance(inputs.get("ref"), str):
                yield typ.cast("str", inputs["ref"])


def _strings_at(mapping: dict[str, object], keys: tuple[str, ...]) -> typ.Iterator[str]:
    """Yield the string values a mapping holds at any of `keys`."""
    for key in keys:
        value = mapping.get(key)
        if isinstance(value, str):
            yield value


def _expressions(document: dict[str, object]) -> typ.Iterator[str]:
    """Yield every condition and script a branch name could be compared in."""
    for job in _jobs(document):
        yield from _strings_at(job, ("if", "uses"))
        for step in _steps(job):
            yield from _strings_at(step, ("if", "run", "uses"))


@pytest.mark.parametrize("name", REMOVED_WORKFLOWS, ids=list(REMOVED_WORKFLOWS))
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

    Only fields where a branch name acts are inspected, and inside expressions
    only the forms that name a branch. A job called `deploy-staging`, an
    `environment: staging`, or a URL containing the word are none of this
    contract's business.
    """
    document = load(path)
    refs = _branch_ref_values(BRANCH)
    pattern = _branch_expression_re(BRANCH)
    offenders = [value for value in _trigger_branches(document) if value in refs]
    offenders += [value for value in _checkout_refs(document) if value in refs]
    offenders += [
        expression
        for expression in _expressions(document)
        if pattern.search(expression)
    ]
    assert not offenders, (
        f"{path.name} still refers to the {BRANCH!r} branch in "
        f"{offenders!r}. That branch is deleted, so a workflow naming it "
        "cannot run and will not say so."
    )


@pytest.mark.parametrize(
    "path", workflow_paths(), ids=[p.name for p in workflow_paths()]
)
def test_no_workflow_waits_on_the_promotion_label(path: Path) -> None:
    """No job is guarded by a label only the removed pipeline applied.

    Matched as a quoted operand, so a job or environment whose name merely
    contains the words does not trip it.
    """
    document = load(path)
    quoted = _quoted(LABEL)
    offenders = [
        expression
        for expression in _expressions(document)
        if any(spelling in expression for spelling in quoted)
    ]
    assert not offenders, (
        f"{path.name} still checks for the {LABEL!r} label in {offenders!r}. "
        "Only the removed promotion job applied it, so the condition can "
        "never be true."
    )


class TestMatchingIsNarrow:
    """The contract must not fail a workflow that merely says "staging".

    A contract that blocks legitimate work is as much a defect as one that
    misses a real reference, and the first version of this file had exactly
    that fault: it compared every parsed string, so `deploy-staging`, an
    `environment: staging` and a URL all counted as branch references, and the
    `staging-promotion` label counted as one too.
    """

    @staticmethod
    def _document() -> dict[str, object]:
        """Return a workflow that names staging without referring to the branch."""
        return {
            "on": {"pull_request": {"branches": ["main"]}},
            "jobs": {
                "deploy-staging": {
                    "runs-on": "ubuntu-latest",
                    "environment": "staging",
                    "steps": [
                        {"run": "echo https://staging.example.com"},
                        {"uses": "actions/checkout@v6", "with": {"ref": "main"}},
                    ],
                }
            },
        }

    def test_a_descriptive_use_is_not_a_branch_reference(self) -> None:
        """A job name, an environment and a URL are none of this contract's business."""
        document = self._document()
        assert [
            v for v in _trigger_branches(document) if v in _branch_ref_values(BRANCH)
        ] == []
        refs = _branch_ref_values(BRANCH)
        assert [v for v in _checkout_refs(document) if v in refs] == []
        pattern = _branch_expression_re(BRANCH)
        assert [e for e in _expressions(document) if pattern.search(e)] == []

    def test_the_label_is_not_a_branch_reference(self) -> None:
        """`staging-promotion` contains the branch name and is not the branch.

        The first version of this contract reported the label as a branch
        reference, so a mutation adding the label failed both tests and the
        second failure looked like corroboration when it was noise.
        """
        assert not _branch_expression_re(BRANCH).search(f"'{LABEL}'")

    @pytest.mark.parametrize(
        "expression",
        [
            "github.ref == 'refs/heads/staging'",
            "github.base_ref == 'staging'",
            'github.head_ref == "staging"',
            "leynos/shared-actions/.github/workflows/x.yml@staging",
        ],
        ids=["qualified-ref", "single-quoted", "double-quoted", "action-ref"],
    )
    def test_a_real_reference_is_still_caught(self, expression: str) -> None:
        """Every form in which a branch name actually acts still matches."""
        assert _branch_expression_re(BRANCH).search(expression)

    @pytest.mark.parametrize(
        "expression",
        [
            "github.ref == 'refs/heads/staging-preview'",
            "leynos/shared-actions/.github/workflows/x.yml@staging-preview",
            "github.base_ref == 'staging-preview'",
        ],
        ids=["qualified-ref", "action-ref", "quoted"],
    )
    def test_a_longer_branch_name_is_not_a_match(self, expression: str) -> None:
        """A branch whose name merely starts with `staging` is someone else's.

        The unbounded version matched these, so a `staging-preview` branch could
        not be referred to anywhere without failing a mandatory gate.
        """
        assert not _branch_expression_re(BRANCH).search(expression)

    @pytest.mark.parametrize(
        "ref",
        ["staging", "refs/heads/staging"],
        ids=["bare", "qualified"],
    )
    def test_both_checkout_spellings_are_caught(self, ref: str) -> None:
        """A checkout names a branch bare or qualified, and both check it out.

        Comparing against the bare form alone let `ref: refs/heads/staging`
        through, so a workflow could check out the deleted branch while this
        contract passed.
        """
        document: dict[str, object] = {
            "jobs": {
                "build": {
                    "steps": [{"uses": "actions/checkout@v6", "with": {"ref": ref}}]
                }
            }
        }
        refs = _branch_ref_values(BRANCH)
        assert [value for value in _checkout_refs(document) if value in refs] == [ref]

    @pytest.mark.parametrize(
        "ref",
        ["main", "refs/heads/staging-preview", "staging-preview"],
        ids=["unrelated", "qualified-longer", "bare-longer"],
    )
    def test_an_unrelated_checkout_ref_is_left_alone(self, ref: str) -> None:
        """Only the deleted branch counts, not one that starts with its name."""
        document: dict[str, object] = {
            "jobs": {
                "build": {
                    "steps": [{"uses": "actions/checkout@v6", "with": {"ref": ref}}]
                }
            }
        }
        refs = _branch_ref_values(BRANCH)
        assert [value for value in _checkout_refs(document) if value in refs] == []

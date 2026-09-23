"""Contracts cancelling superseded pull-request runs.

Every push to a pull request starts a fresh run of each gate, and the run
already in flight is answering a question about a commit nobody will merge.
Left alone it holds a runner until it finishes, so the branch pays twice for
one answer. A concurrency group keyed on the pull request makes the newer run
cancel the older one.

Cancellation has to stay conditioned on the event. A literal
``cancel-in-progress: true`` would also cancel a push to `main`, a schedule,
and a dispatch, none of which has a successor waiting: the run that writes the
warm cache on `main` would be killed by the next merge, and the coverage
history would gain holes. The condition is therefore part of the contract, not
an implementation detail, and `test_cancellation_is_conditioned_on_the_event`
fails on the literal.

The group also has to distinguish one pull request from another. A group
derived from ``github.run_id`` is unique per run and so cancels nothing, while
a constant group would let one branch cancel another's gates.

Only `pull_request` is in scope. A `pull_request_target` workflow runs against
the base repository to carry a token, and the workflows that use it here label
and auto-merge rather than build; cancelling an auto-merge mid-flight is a
hazard with no minutes to win.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import typing as typ

import pytest
from _workflow_policy import WORKFLOW_DIR, load, workflow_paths

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Mapping
    from pathlib import Path

#: A parsed workflow document as PyYAML actually produces it. An unquoted `on:`
#: key resolves to the boolean ``True``, so a key is a string or that boolean;
#: annotating the document with string keys would make `document.get(True)` a
#: lookup the type says cannot succeed.
WorkflowDocument: typ.TypeAlias = "Mapping[str | bool, object]"

#: The exact `cancel-in-progress` expression every pull-request workflow
#: carries. Comparing against one string rather than searching for a substring
#: is what makes the literal `true` mutation fail: `true` is a YAML boolean and
#: never equals this.
CANCEL_EXPRESSION: str = "${{ github.event_name == 'pull_request' }}"

#: The trigger that puts a workflow in scope. `pull_request_target` is
#: deliberately absent; see the module docstring.
PULL_REQUEST: str = "pull_request"

#: Expressions that are unique to a single run. A group built from one of these
#: can never match another run, so it queues nothing and cancels nothing while
#: looking exactly like a concurrency control.
RUN_UNIQUE_EXPRESSIONS: tuple[str, ...] = (
    "github.run_id",
    "github.run_number",
    "github.run_attempt",
    "github.sha",
)

#: Expressions that differ between two pull requests. A group naming none of
#: them is shared by every branch, so one pull request's push would cancel
#: another's gates.
PER_PULL_REQUEST_EXPRESSIONS: tuple[str, ...] = (
    "github.event.pull_request.number",
    "github.head_ref",
    "github.ref",
)

#: Workflows known to start on `pull_request`. Discovery below is dynamic so a
#: new workflow is covered the day it lands, but a dynamic list that silently
#: empties turns every parametrized test into a vacuous pass. This names the
#: floor discovery must still reach.
KNOWN_PULL_REQUEST_WORKFLOWS: frozenset[str] = frozenset(
    {
        "code_style.yml",
        "codescene-coverage.yml",
        "e2e.yml",
        "regression-test-check.yml",
        "test.yml",
    }
)


def _document(path: Path) -> WorkflowDocument:
    """Read one workflow as the document PyYAML produces, `True` key included."""
    return typ.cast("WorkflowDocument", load(path))


# GitHub accepts three shapes for `on:`: a mapping of event to configuration,
# a list of event names, and a bare event name. All three are read. A reader
# that modelled only the mapping would drop a workflow written either other way
# out of discovery, and a contract over a filtered list reports nothing at all
# about a workflow it never sees. PyYAML also resolves an unquoted `on:` key to
# the boolean ``True``, so both spellings of the key are read.
#
# The result is empty when the workflow declares no `on:` key at all, in which
# case nothing can start it, and ``None`` when `on:` is present in a shape this
# reader does not model. An explicit `on: null` is present and unmodelled, not
# absent, and so is a collection holding anything but event names: dropping the
# `42` from `on: [42]` would leave an empty set that reads as "nothing starts
# this". Discovery cannot tell ``None`` apart from "not startable by a pull
# request", so `_unmodelled_workflows` reports it by name.
def _trigger_names(document: WorkflowDocument) -> frozenset[str] | None:
    """Return the event names under `on:`, or `None` for an unmodelled shape."""
    key = _trigger_key(document)
    if key is None:
        return frozenset()
    match document[key]:
        case dict() | list() as events:
            return _string_members(events)
        case str() as event:
            return frozenset({event})
        case _:
            return None


def _trigger_key(document: WorkflowDocument) -> str | bool | None:
    """Return whichever spelling of the `on` key is present, or `None`."""
    for key in ("on", True):
        if key in document:
            return key
    return None


def _string_members(
    declared: dict[object, object] | list[object],
) -> frozenset[str] | None:
    """Return a collection's event names, or `None` if any member is not one."""
    if not all(isinstance(event, str) for event in declared):
        return None
    return frozenset(typ.cast("str", event) for event in declared)


def _pull_request_workflows(directory: Path = WORKFLOW_DIR) -> list[Path]:
    """Return the workflows in a directory that a pull request can start."""
    return [
        path
        for path in workflow_paths(directory)
        if PULL_REQUEST in (_trigger_names(_document(path)) or frozenset())
    ]


def _unmodelled_workflows(directory: Path = WORKFLOW_DIR) -> list[str]:
    """Return the names of the workflows whose `on:` this reader cannot model."""
    return [
        path.name
        for path in workflow_paths(directory)
        if _trigger_names(_document(path)) is None
    ]


def _concurrency(path: Path) -> dict[str, object]:
    """Return a workflow's top-level concurrency mapping.

    Parameters
    ----------
    path
        Workflow file to read.

    Returns
    -------
    dict
        The concurrency mapping, empty when the workflow declares none or
        declares the shorthand string form, which cannot carry
        `cancel-in-progress` at all.
    """
    declared = _document(path).get("concurrency")
    return declared if isinstance(declared, dict) else {}


PULL_REQUEST_WORKFLOWS = _pull_request_workflows()
WORKFLOW_IDS = [path.name for path in PULL_REQUEST_WORKFLOWS]


def test_discovery_still_finds_the_known_pull_request_workflows() -> None:
    """Discovery reaches its floor, so the parametrized contracts are not empty.

    Every test below is parametrized over a list built by reading the workflow
    estate. If that read were to break, or the `on:` key were to change shape,
    the list would empty and each contract would report as passed having
    asserted nothing.
    """
    discovered = set(WORKFLOW_IDS)
    missing = sorted(KNOWN_PULL_REQUEST_WORKFLOWS - discovered)
    assert not missing, (
        f"these workflows start on pull_request but discovery missed them: "
        f"{', '.join(missing)}; the contracts below would pass without "
        "asserting anything about them"
    )


def test_every_workflow_declares_a_trigger_set_this_reader_models() -> None:
    """No workflow's `on:` defeats the reader that decides what is in scope.

    Discovery filters on the event names it can read, and a workflow whose
    `on:` the reader cannot model is dropped from that filter. Dropped
    silently it would take every contract below with it, each passing while
    saying nothing about that workflow. This is the half that makes the
    silence loud.
    """
    unreadable = _unmodelled_workflows()
    assert not unreadable, (
        f"these workflows declare an `on:` this reader does not model: "
        f"{', '.join(unreadable)}; each is dropped from discovery, so every "
        "contract below would pass without asserting anything about it"
    )


@pytest.mark.parametrize("workflow", PULL_REQUEST_WORKFLOWS, ids=WORKFLOW_IDS)
def test_every_pull_request_workflow_declares_a_concurrency_group(
    workflow: Path,
) -> None:
    """A workflow a pull request starts declares a concurrency group.

    Without one, every push to the branch leaves its predecessor running to
    completion on a paid runner.
    """
    group = _concurrency(workflow).get("group")
    assert isinstance(group, str) and group.strip(), (
        f"{workflow.name} starts on pull_request and must declare "
        "concurrency.group; without it a superseded run holds a runner until "
        "it finishes"
    )


@pytest.mark.parametrize("workflow", PULL_REQUEST_WORKFLOWS, ids=WORKFLOW_IDS)
def test_the_group_is_not_unique_to_one_run(workflow: Path) -> None:
    """The group is shared by successive runs of the same pull request.

    A group built from the run identifier or the commit SHA matches no other
    run, so it cancels nothing while reading as a concurrency control.
    """
    group = str(_concurrency(workflow).get("group", ""))
    offenders = [name for name in RUN_UNIQUE_EXPRESSIONS if name in group]
    assert not offenders, (
        f"{workflow.name} builds its concurrency group from "
        f"{', '.join(offenders)}, which is unique to one run; the group would "
        "never match a superseded run and would cancel nothing"
    )


@pytest.mark.parametrize("workflow", PULL_REQUEST_WORKFLOWS, ids=WORKFLOW_IDS)
def test_the_group_distinguishes_one_pull_request_from_another(
    workflow: Path,
) -> None:
    """The group varies with the pull request, so branches do not cancel each other.

    A constant group would put every open pull request in one queue, and the
    first push anywhere would cancel the gates running everywhere else.
    """
    group = str(_concurrency(workflow).get("group", ""))
    assert any(name in group for name in PER_PULL_REQUEST_EXPRESSIONS), (
        f"{workflow.name} must key its concurrency group on the pull request, "
        f"by naming one of {', '.join(PER_PULL_REQUEST_EXPRESSIONS)}; a group "
        "shared by every branch would cancel unrelated pull requests"
    )


@pytest.mark.parametrize("workflow", PULL_REQUEST_WORKFLOWS, ids=WORKFLOW_IDS)
def test_cancellation_is_conditioned_on_the_event(workflow: Path) -> None:
    """Cancellation applies to pull requests only, not to pushes or schedules.

    A literal `true` here reads as a stricter setting and is a regression: it
    would cancel the run on `main` that writes the warm cache and records
    coverage, which no later run repeats.
    """
    declared = _concurrency(workflow).get("cancel-in-progress")
    assert declared == CANCEL_EXPRESSION, (
        f"{workflow.name} must set cancel-in-progress to "
        f"{CANCEL_EXPRESSION!r}, not {declared!r}; a missing value leaves "
        "superseded runs in flight and a literal true also cancels pushes to "
        "main, schedules, and dispatches"
    )


@pytest.mark.parametrize(
    ("document", "expected"),
    [
        pytest.param(
            {"on": {"pull_request": {}, "push": None}},
            {"pull_request", "push"},
            id="mapping",
        ),
        pytest.param(
            {True: {"pull_request": {}}},
            {"pull_request"},
            id="mapping-under-the-true-key",
        ),
        pytest.param(
            {"on": ["pull_request", "push"]}, {"pull_request", "push"}, id="list"
        ),
        pytest.param(
            {True: ["pull_request"]}, {"pull_request"}, id="list-under-the-true-key"
        ),
        pytest.param({"on": "pull_request"}, {"pull_request"}, id="scalar"),
        pytest.param(
            {True: "pull_request"}, {"pull_request"}, id="scalar-under-the-true-key"
        ),
        pytest.param({"jobs": {}}, set(), id="absent"),
    ],
)
def test_every_shape_of_on_is_read_to_its_event_names(
    document: WorkflowDocument, expected: set[str]
) -> None:
    """Each shape GitHub accepts yields exactly the events it declares.

    The estate writes every `on:` as a mapping today, so the estate alone
    cannot tell this reader from a mapping-only one; these documents can.
    """
    found = _trigger_names(document)
    assert found == frozenset(expected), (
        f"{document!r} declares {sorted(expected)}, but the reader returned {found!r}"
    )


@pytest.mark.parametrize(
    "declared",
    [
        pytest.param(42, id="a-number"),
        pytest.param(True, id="a-boolean"),
        pytest.param(None, id="an-explicit-null"),
        pytest.param([42], id="a-list-holding-a-number"),
        pytest.param(["pull_request", 42], id="a-list-with-one-bad-member"),
        pytest.param({42: None}, id="a-mapping-with-a-number-key"),
    ],
)
def test_an_unmodelled_shape_is_none_rather_than_empty(declared: object) -> None:
    """A shape the reader cannot model is told apart from "no triggers".

    An empty set would read as "nothing starts this workflow", which is the
    passing case for every contract over the discovered list.
    """
    assert _trigger_names({"on": declared}) is None, (
        f"`on: {declared!r}` is not a shape GitHub accepts; the reader must "
        "return None so the workflow is reported, not an empty set"
    )


def test_discovery_keeps_every_shape_in_scope(tmp_path: Path) -> None:
    """A list or scalar `pull_request` trigger is still discovered.

    This is the case the estate cannot state: a mapping-only reader passes
    every contract over the repository's own workflows.
    """
    for name, trigger in (
        ("mapping.yml", "on:\n  pull_request:\n"),
        ("listed.yml", "on: [pull_request, push]\n"),
        ("scalar.yml", "on: pull_request\n"),
        ("push-only.yml", "on: push\n"),
    ):
        (tmp_path / name).write_text(f"{trigger}jobs: {{}}\n", encoding="utf-8")
    found = [path.name for path in _pull_request_workflows(tmp_path)]
    assert found == ["listed.yml", "mapping.yml", "scalar.yml"], (
        f"discovery should keep the three pull-request workflows whatever the "
        f"shape of their `on:`, and only those; it found {found}"
    )


def test_an_unmodelled_trigger_is_reported_by_name(tmp_path: Path) -> None:
    """A workflow whose `on:` cannot be read is named, not silently dropped."""
    for name, trigger in (
        ("odd.yml", "on: 42"),
        ("null.yml", "on: null"),
        ("listed-number.yml", "on: [42]"),
        ("number-key.yml", "on:\n  42: {}"),
    ):
        (tmp_path / name).write_text(f"{trigger}\njobs: {{}}\n", encoding="utf-8")
    (tmp_path / "fine.yml").write_text("on: pull_request\njobs: {}\n", encoding="utf-8")
    unmodelled = ["listed-number.yml", "null.yml", "number-key.yml", "odd.yml"]
    assert _unmodelled_workflows(tmp_path) == unmodelled, (
        "each workflow with an unmodelled `on:` must be reported by name, and "
        f"only those; got {_unmodelled_workflows(tmp_path)}"
    )

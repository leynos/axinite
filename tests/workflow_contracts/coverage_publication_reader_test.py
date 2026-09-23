"""Unit tests driving the coverage-publication readers with built workflows.

`coverage_publication_test.py` asserts the rule over the repository's own
workflows, which are correct, so it would pass with any reader's protection
deleted. Each case here states a workflow that breaks the rule in one way and
drives the same query the estate contract calls.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import textwrap

import pytest
from _coverage_publication import (
    local_callee,
    pull_request_faults,
    pull_request_surface,
    push_surface,
    pushes_to_main,
    upload_guard_faults,
)
from _strict_workflows import WorkflowReadError, Workflows, parse_strict, triggers

REF = "github.ref == 'refs/heads/main'"
AVAILABLE = "steps.codescene-token.outputs.available == 'true'"

#: A step running the uploader, as a workflow line.
UPLOADER_STEP = (
    "      - uses: leynos/shared-actions/.github/actions/upload-codescene-coverage@a\n"
)


def _estate(**sources: str) -> Workflows:
    """Parse workflows given as keyword arguments named for their files."""
    return {
        f"{name.replace('_', '-')}.yml": parse_strict(textwrap.dedent(text), name)
        for name, text in sources.items()
    }


def test_a_key_declared_twice_is_refused() -> None:
    """PyYAML would keep the second `runs-on` and hide the first."""
    text = "on: push\njobs:\n  build:\n    runs-on: a\n    runs-on: b\n"
    with pytest.raises(WorkflowReadError, match="duplicate key 'runs-on'"):
        parse_strict(text, "twice.yml")


@pytest.mark.parametrize(
    ("text", "expected"),
    [
        pytest.param("on: pull_request\n", {"pull_request"}, id="scalar"),
        pytest.param("on: [push, pull_request]\n", {"push", "pull_request"}, id="list"),
        pytest.param("on:\n  pull_request: {}\n", {"pull_request"}, id="mapping"),
        pytest.param("'on': [pull_request]\n", {"pull_request"}, id="quoted-key"),
        pytest.param("name: x\n", set(), id="absent"),
    ],
)
def test_every_trigger_form_is_read(text: str, expected: set[str]) -> None:
    """A list form must not stringify into one key, nor a scalar into none."""
    found = set(triggers(parse_strict(text, "t.yml")))
    assert found == expected, f"{text!r} declares {expected}; read {found}"


@pytest.mark.parametrize(
    "text",
    [
        pytest.param("on: null\n", id="explicit-null"),
        pytest.param("on: [42]\n", id="a-number-in-a-list"),
        pytest.param("on: push\n'on': pull_request\n", id="both-spellings"),
    ],
)
def test_an_unmodelled_trigger_is_refused(text: str) -> None:
    """Reading any of these as empty would drop the workflow from every clause."""
    with pytest.raises(WorkflowReadError):
        triggers(parse_strict(text, "t.yml"))


@pytest.mark.parametrize(
    ("uses", "expected"),
    [
        pytest.param("./.github/workflows/x.yml", "x.yml", id="dot-slash"),
        pytest.param("$/.github/workflows/x.yml", "x.yml", id="dollar-slash"),
        pytest.param("actions/checkout@v6", None, id="an-action"),
        pytest.param(
            "leynos/other/.github/workflows/x.yml@v1", None, id="another-repo"
        ),
    ],
)
def test_a_local_call_is_matched_by_shape(uses: str, expected: str | None) -> None:
    """Both spellings of a same-repository call resolve to the file."""
    assert local_callee(uses) == expected, f"{uses!r} should resolve to {expected!r}"


@pytest.mark.parametrize(
    "uses",
    [
        pytest.param("$/.github/workflows/x.yml@main", id="dollar-slash-with-a-ref"),
        pytest.param(
            "leynos/axinite/.github/workflows/x.yml@main", id="qualified-self"
        ),
    ],
)
def test_a_call_the_closure_cannot_read_is_refused(uses: str) -> None:
    """A self-call at a ref runs a file the closure cannot see."""
    with pytest.raises(WorkflowReadError):
        local_callee(uses)


def test_the_closure_follows_both_spellings_to_an_inherited_token() -> None:
    """The probe: a workflow_call-only callee curling CodeScene with the token.

    The pull-request workflow calls a middle workflow with `./`, which calls
    the callee with `$/` and `secrets: inherit`. Neither caller names the
    token or the host, so only a closure through both spellings sees it.
    """
    estate = _estate(
        gate="""
            on: pull_request
            jobs:
              call:
                uses: ./.github/workflows/middle.yml
        """,
        middle="""
            on: workflow_call
            jobs:
              call:
                uses: $/.github/workflows/leak.yml
                secrets: inherit
        """,
        leak="""
            on: workflow_call
            jobs:
              send:
                runs-on: ubuntu-latest
                steps:
                  - run: curl -H "$TOKEN" https://api.CodeScene.io/v2
                    env:
                      TOKEN: ${{ secrets.CS_ACCESS_TOKEN }}
        """,
    )
    assert pull_request_surface(estate) == {"gate.yml", "middle.yml", "leak.yml"}
    faults = "\n".join(pull_request_faults(estate))
    assert (
        "leak.yml is reachable from a pull request and mentions 'codescene.io'"
        in faults
    )
    assert (
        "leak.yml is reachable from a pull request and mentions 'cs_access_token'"
        in faults
    )
    assert "middle.yml:call forwards every secret" in faults


@pytest.mark.parametrize(
    "source",
    [
        pytest.param(
            "on: pull_request\ndefaults:\n  run:\n"
            "    shell: curl codescene.io; bash {0}\n",
            id="defaults-run-shell",
        ),
        pytest.param(
            "on:\n  pull_request: {}\n  workflow_call:\n    secrets:\n"
            "      CS_ACCESS_TOKEN: {}\n",
            id="workflow-call-secret-declaration",
        ),
        pytest.param(
            "on: merge_group\njobs:\n  x:\n    steps:\n" + UPLOADER_STEP,
            id="the-uploader-on-the-merge-queue",
        ),
        pytest.param(
            "on: pull_request_review\njobs:\n  x:\n    steps:\n"
            "      - run: cs-coverage check\n",
            id="the-cli-on-a-review-event",
        ),
    ],
)
def test_the_whole_document_is_read_for_the_host_and_the_token(source: str) -> None:
    """Keys, workflow-level defaults and every seed event, case-folded."""
    assert pull_request_faults(_estate(lane=source)), f"accepted {source!r}"


def test_a_workflow_run_chain_onto_a_pull_request_workflow_is_on_the_surface() -> None:
    """A workflow chained onto a pull-request workflow runs for that pull request."""
    estate = _estate(
        gate="name: Gate\non: pull_request\n",
        after="on:\n  workflow_run:\n    workflows: [Gate]\n",
    )
    assert pull_request_surface(estate) == {"gate.yml", "after.yml"}


def test_a_clean_surface_has_no_faults() -> None:
    """The narrow half: a lane that ratchets and publishes nothing passes."""
    estate = _estate(
        lane="on: pull_request\njobs:\n  x:\n    steps:\n      - run: make test\n",
        publisher=(
            "on:\n  push:\n    branches: [main]\njobs:\n  x:\n    steps:\n"
            + UPLOADER_STEP
        ),
    )
    assert pull_request_faults(estate) == []


@pytest.mark.parametrize(
    ("condition", "accepted"),
    [
        pytest.param(f"{AVAILABLE} && {REF}", True, id="the-required-guard"),
        pytest.param(
            f"matrix.name == 'x' && {AVAILABLE} && {REF}", True, id="narrowed"
        ),
        pytest.param(AVAILABLE, False, id="no-ref"),
        pytest.param(
            f"{AVAILABLE} && {REF} && github.actor != 'x' || "
            "github.event_name == 'workflow_dispatch'",
            False,
            id="an-or-hidden-in-an-extra-conjunct",
        ),
    ],
)
def test_the_upload_guard_is_split_on_and_and_refuses_or(
    condition: str, accepted: bool
) -> None:
    """The hidden `||` keeps every required conjunct whole.

    Only the explicit refusal catches it, which is why it is the case that
    proves the refusal.
    """
    faults = upload_guard_faults(condition, (AVAILABLE, REF))
    assert (not faults) is accepted, f"{condition!r}: {faults}"


@pytest.mark.parametrize(
    ("push", "reaches"),
    [
        pytest.param("push:\n    branches: [main]", True, id="main"),
        pytest.param("push:\n    branches: ['**']", True, id="every-branch"),
        pytest.param("push:\n    branches: ['**', '!main']", False, id="negated"),
        pytest.param("push:\n    branches-ignore: [main]", False, id="ignored"),
        pytest.param("push:\n    tags: ['v*']", False, id="tags-only"),
        pytest.param("push: {}", True, id="unfiltered"),
    ],
)
def test_a_push_filter_is_read_as_globs(push: str, reaches: bool) -> None:
    """A literal reading of `'**'` would miss a second baseline writer."""
    document = parse_strict(f"on:\n  {push}\n", "p.yml")
    assert pushes_to_main(document) is reaches, f"{push!r} should reach main: {reaches}"


def test_the_push_surface_follows_calls() -> None:
    """A push workflow calling a reusable one makes the callee a writer too."""
    estate = _estate(
        trunk="on:\n  push:\n    branches: [main]\njobs:\n  c:\n"
        "    uses: ./.github/workflows/called.yml\n",
        called="on: workflow_call\n",
    )
    assert push_surface(estate) == {"trunk.yml", "called.yml"}

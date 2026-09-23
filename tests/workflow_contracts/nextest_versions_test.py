"""Unit tests for reading which cargo-nextest each workflow installs.

`test_every_lane_installs_the_same_nextest` runs the reading over the
repository's workflows, which agree. A reading parametrized over input
that agrees discriminates nothing, so `installed_versions_in` is driven
here with workflow text written for each case, and `read_workflow_texts`
with a temporary tree.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import typing as typ

import pytest
from nextest_versions import UNDECLARED, installed_versions_in, read_workflow_texts

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from pathlib import Path

#: A step deferring to the variable, as the shared tool action is called.
DEFERRED_STEP = (
    "    steps:\n"
    "      - uses: taiki-e/install-action@v2\n"
    "        with:\n"
    "          tool: cargo-nextest@${{ env.CARGO_NEXTEST_VERSION }}\n"
)


@pytest.mark.parametrize(
    ("text", "expected"),
    [
        pytest.param(
            "jobs:\n  mutation:\n    with:\n      setup-commands: |\n"
            "        cargo binstall cargo-nextest@0.9.100\n",
            {"0.9.100"},
            id="a-literal-pin-in-setup-commands",
        ),
        pytest.param(
            "env:\n  CARGO_NEXTEST_VERSION: 0.9.100\n"
            "jobs:\n  test:\n" + DEFERRED_STEP,
            {"0.9.100"},
            id="deferred-to-the-workflow-env",
        ),
        pytest.param(
            "jobs:\n  test:\n    env:\n      CARGO_NEXTEST_VERSION: 0.9.101\n"
            + DEFERRED_STEP,
            {"0.9.101"},
            id="deferred-to-a-job-env",
        ),
        pytest.param(
            "jobs:\n  test:\n" + DEFERRED_STEP,
            {UNDECLARED},
            id="deferred-to-a-variable-nobody-declares",
        ),
        pytest.param(
            "jobs:\n  test:\n    steps:\n      - uses: taiki-e/install-action@v2\n"
            "        with:\n          tool: cargo-nextest@${{ inputs.nextest }}\n",
            {UNDECLARED},
            id="deferred-to-an-expression-the-reading-cannot-resolve",
        ),
        pytest.param(
            "jobs:\n  test:\n    steps:\n"
            "      - run: cargo binstall cargo-nextest@0.9.${{ matrix.patch }}\n",
            {UNDECLARED},
            id="a-literal-prefix-an-expression-completes",
        ),
        pytest.param(
            "env:\n  CARGO_NEXTEST_VERSION: 0.9.100\n"
            "jobs:\n  test:\n    steps:\n"
            "      - run: cargo binstall cargo-nextest@${{ matrix.nextest }}\n"
            "      - uses: taiki-e/install-action@v2\n"
            "        with:\n"
            "          tool: cargo-nextest@${{ env.CARGO_NEXTEST_VERSION }}\n",
            {"0.9.100", UNDECLARED},
            id="one-resolved-reference-and-one-unresolved",
        ),
        pytest.param(
            "env:\n  CARGO_NEXTEST_VERSION: 0.9.100\n"
            "jobs:\n  test:\n    env:\n      CARGO_NEXTEST_VERSION: 0.9.101\n"
            + DEFERRED_STEP,
            {"0.9.100", "0.9.101"},
            id="two-scopes-disagreeing",
        ),
    ],
)
def test_each_way_of_installing_the_runner_is_read(
    text: str, expected: set[str]
) -> None:
    """Every spelling of an install yields the versions it would install.

    The literal pin and the job-scope declaration are the two a reading
    confined to the workflow ``env`` missed; the undeclared deferral is
    the one that would otherwise read as installing nothing.
    """
    found = installed_versions_in({"ci.yml": text})
    assert found == {"ci.yml": expected}, (
        f"the workflow installs {sorted(expected)}, but the reading returned "
        f"{found}"
    )


def test_a_workflow_that_installs_no_runner_is_left_out() -> None:
    """Only installing workflows are reported, so agreement is over those.

    Reporting a non-installer with an empty set would add nothing to the
    version union, but it would also let the suite lane's presence be
    asserted over a workflow that installs nothing.
    """
    found = installed_versions_in({"lint.yml": "jobs:\n  lint:\n    steps: []\n"})
    assert found == {}, f"a workflow with no install was reported: {found}"


def test_the_workflow_texts_are_read_by_file_name(tmp_path: Path) -> None:
    """The acquisition returns each workflow's text under its file name."""
    (tmp_path / "b.yml").write_text("on: push\n", encoding="utf-8")
    (tmp_path / "a.yaml").write_text("on: pull_request\n", encoding="utf-8")
    (tmp_path / "notes.txt").write_text("not a workflow\n", encoding="utf-8")
    found = read_workflow_texts(tmp_path)
    assert found == {"a.yaml": "on: pull_request\n", "b.yml": "on: push\n"}, (
        f"the read should hold both workflows and nothing else; got {found}"
    )

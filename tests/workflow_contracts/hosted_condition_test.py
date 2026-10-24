"""Unit tests for the hosted arm's fork-or-Dependabot condition.

A pull request from a fork, or one Dependabot opened, takes the hosted arm of a
lane's `runs-on`. The condition contains its own `||`, so the chain reader must
split only on the separators outside parentheses, and it must recognize the
condition only as written: the fork field, then the author login, never
`github.actor`, which is whoever triggered the run.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import pytest
from _fork_lanes import paid_arms_before_the_fork
from _workflow_policy import (
    FORK_CONDITION,
    HOSTED_CONDITION,
    Job,
    conditional_runs_on_arms,
)

PAID = "ubicloud-standard-2"


def _chain(condition: str) -> str:
    """Return a folded lane `runs-on` carrying `condition` as its hosted arm."""
    return f"${{{{ {condition} && 'ubuntu-latest' || '{PAID}' }}}}"


def test_the_hosted_condition_is_one_arm_not_two() -> None:
    """The condition's inner `||` is not a chain separator."""
    arms = conditional_runs_on_arms(_chain(HOSTED_CONDITION))
    assert arms == ((HOSTED_CONDITION, "ubuntu-latest"), (None, PAID)), arms


@pytest.mark.parametrize(
    "condition",
    [
        HOSTED_CONDITION.replace("pull_request.user.login", "actor"),
        HOSTED_CONDITION.replace("||", "&&"),
        HOSTED_CONDITION.replace("== 'dependabot[bot]'", "!= 'dependabot[bot]'"),
    ],
    ids=["actor-for-author", "and-for-or", "not-equal"],
)
def test_a_lookalike_condition_leaves_the_chain_unreadable(condition: str) -> None:
    """The reader refuses a condition that is not exactly the hosted one.

    The variants are passed as written, so the refusal is the reader's and not
    an artefact of added parentheses.
    """
    assert conditional_runs_on_arms(_chain(condition)) is None, condition


def test_the_fork_field_alone_is_readable_but_not_the_hosted_condition() -> None:
    """A fork-only arm parses, yet fails the hosted-condition policy."""
    arms = conditional_runs_on_arms(_chain(FORK_CONDITION))
    assert arms == ((FORK_CONDITION, "ubuntu-latest"), (None, PAID)), arms
    assert arms[0][0] != HOSTED_CONDITION, "the fork-only arm met the hosted policy"


def test_no_arm_before_the_hosted_arm_may_be_paid() -> None:
    """A paid arm ahead of the hosted arm takes a Dependabot run first."""
    declared = (
        f"${{{{ github.event_name == 'pull_request' && '{PAID}' "
        f"|| {HOSTED_CONDITION} && 'ubuntu-latest' || '{PAID}' }}}}"
    )
    assert paid_arms_before_the_fork(declared) == (
        f"github.event_name == 'pull_request' && '{PAID}'",
    ), declared
    job = Job("ci.yml", "build", {"runs-on": declared})
    assert job.uses_ubicloud, f"{declared!r} should still read as a paid lane"

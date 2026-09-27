"""Unit tests for the readings the fork-fallback contracts assert with.

`fork_fallback_test.py` runs over the estate's own pull-request lanes, which
are all correct, so it cannot show that its readings refuse anything. The
cases here state the shapes each reading must refuse and the shapes it must
accept, against constructed jobs.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import pytest
from _fork_lanes import (
    is_a_bare_matrix_reference,
    opaque_runs_on_values,
    paid_arms_before_the_fork,
)
from _workflow_policy import UBICLOUD_LABEL_PREFIX, Job

#: A fork arm as the estate writes it.
FORK_ARM = "github.event.pull_request.head.repo.fork && 'ubuntu-latest'"


@pytest.mark.parametrize(
    ("runs_on", "expected"),
    [
        pytest.param("ubicloud-standard-2", (), id="a-plain-label"),
        pytest.param(
            f"${{{{ {FORK_ARM} || 'ubicloud-standard-2' }}}}", (), id="a-readable-chain"
        ),
        pytest.param(
            "${{ github.actor == 'x' && 'ubuntu-latest' || 'ubicloud-standard-2' }}",
            ("${{ github.actor == 'x' && 'ubuntu-latest' || 'ubicloud-standard-2' }}",),
            id="an-unreadable-scalar",
        ),
        pytest.param(["ubuntu-latest", "self-hosted"], (), id="a-list-of-labels"),
        pytest.param(
            [f"${{{{ {FORK_ARM} || 'ubicloud-standard-4' }}}}"],
            (f"${{{{ {FORK_ARM} || 'ubicloud-standard-4' }}}}",),
            id="a-readable-chain-inside-a-list",
        ),
        pytest.param(
            {"labels": ["${{ matrix.runner }}"]},
            ("${{ matrix.runner }}",),
            id="a-matrix-reference-inside-a-group-list",
        ),
    ],
)
def test_every_unresolved_expression_is_found(
    runs_on: object, expected: tuple[str, ...]
) -> None:
    """A list item is never resolved, however readable its chain.

    `a-readable-chain-inside-a-list` is the case the scalar-only reading
    missed: GitHub evaluates the item, `runner_labels` keeps it whole, and
    the job reads as asking for no Ubicloud runner at all.
    """
    job = Job("ci.yml", "build", {"runs-on": runs_on})
    assert opaque_runs_on_values(job) == expected


@pytest.mark.parametrize(
    ("first_arm", "expected"),
    [
        pytest.param(
            "github.event_name == 'schedule' && 'ubuntu-latest'", (), id="a-cron-arm"
        ),
        pytest.param(
            f"github.event_name == 'push' && '{UBICLOUD_LABEL_PREFIX}standard-8'",
            (),
            id="a-push-arm",
        ),
        pytest.param(
            f"github.event_name == 'pull_request' && '{UBICLOUD_LABEL_PREFIX}standard-4'",
            (
                f"github.event_name == 'pull_request' && "
                f"'{UBICLOUD_LABEL_PREFIX}standard-4'",
            ),
            id="a-paid-pull-request-arm",
        ),
        pytest.param(
            "github.event_name == 'pull_request' && 'ubuntu-latest'",
            (),
            id="a-hosted-pull-request-arm",
        ),
        pytest.param(
            "github.event.pull_request.head.repo.private && "
            f"'{UBICLOUD_LABEL_PREFIX}standard-4'",
            (
                "github.event.pull_request.head.repo.private && "
                f"'{UBICLOUD_LABEL_PREFIX}standard-4'",
            ),
            id="a-paid-sibling-field-arm",
        ),
    ],
)
def test_a_paid_arm_ahead_of_the_fork_arm_is_found(
    first_arm: str, expected: tuple[str, ...]
) -> None:
    """Only an arm a pull request can take, selecting Ubicloud, shadows the fork.

    `a-paid-pull-request-arm` is the review's case: both fork assertions
    find the field and its hosted label, and a fork still runs paid.
    `a-cron-arm` is the shape the estate uses and must stay valid.
    """
    declared = (
        f"${{{{ {first_arm} || {FORK_ARM} || '{UBICLOUD_LABEL_PREFIX}standard-4' }}}}"
    )
    assert paid_arms_before_the_fork(declared) == expected


def test_an_arm_after_the_fork_arm_is_not_counted() -> None:
    """Once the fork arm is reached, a fork has its runner."""
    declared = (
        f"${{{{ {FORK_ARM} || github.event_name == 'pull_request' && "
        f"'{UBICLOUD_LABEL_PREFIX}standard-4' || 'ubuntu-latest' }}}}"
    )
    assert paid_arms_before_the_fork(declared) == ()


@pytest.mark.parametrize(
    ("declared", "exempt"),
    [
        pytest.param("${{ matrix.runner }}", True, id="a-bare-reference"),
        pytest.param("${{ matrix.os }}", True, id="another-bare-reference"),
        pytest.param("${{  matrix.runner  }}", True, id="padded"),
        pytest.param(
            "${{ matrix.name == 'x' && 'ubuntu-latest' "
            f"|| '{UBICLOUD_LABEL_PREFIX}standard-2' }}}}",
            False,
            id="a-selection-wearing-a-matrix-key",
        ),
        pytest.param(
            f"${{{{ matrix.runner || '{UBICLOUD_LABEL_PREFIX}standard-2' }}}}",
            False,
            id="a-reference-with-a-fallback",
        ),
        pytest.param(
            "${{ github.actor == 'x' && 'ubuntu-latest' || 'matrix.runner' }}",
            False,
            id="the-key-named-inside-a-literal",
        ),
    ],
)
def test_only_a_bare_matrix_reference_is_exempt(declared: str, exempt: bool) -> None:
    """The exemption must not cover a selection that merely mentions a leg.

    A `runs-on` that is one matrix key resolves to whatever the leg names,
    and the sizing contract reads the label there, so the expression itself
    carries no placement decision and is rightly exempt.

    Testing for the substring `matrix.` exempted far more than that. The
    `a-selection-wearing-a-matrix-key` case is the one that cost something:
    it asks for a paid runner on every leg but one, the reader cannot split
    it, and a fork's pull request reaching such a leg is stranded exactly as
    this file exists to prevent.
    """
    assert is_a_bare_matrix_reference(declared) is exempt, (
        f"{declared!r} should read as exempt={exempt}"
    )


class TestChainReading:
    """The reader has to see every arm, or the fork contracts are vacuous."""

    @staticmethod
    def _job(declared: str) -> Job:
        """Return a job declaring one `runs-on` value."""
        return Job("fixture.yml", "fixture", {"runs-on": declared})

    _COMPOSED = (
        "${{ github.event_name == 'schedule' && 'ubuntu-latest' "
        "|| github.event.pull_request.head.repo.fork && 'ubuntu-latest' "
        "|| 'ubicloud-standard-2' }}"
    )

    def test_a_three_armed_chain_reports_every_label(self) -> None:
        """A label the parser cannot see is exempt from every contract."""
        job = self._job(self._COMPOSED)
        assert job.runner_labels == ("ubuntu-latest", "ubicloud-standard-2"), (
            f"a three-armed chain reports {job.runner_labels!r}; a label the "
            "reader drops is exempt from every placement contract"
        )
        assert job.uses_ubicloud, (
            "the chain names an Ubicloud label, so the job must answer that "
            "it uses one; otherwise the sizing contracts skip it"
        )

    @pytest.mark.parametrize(
        ("event", "expected"),
        [
            ("schedule", ("ubuntu-latest",)),
            ("pull_request", ("ubicloud-standard-2",)),
            ("push", ("ubicloud-standard-2",)),
        ],
        ids=["cron", "branch-pull-request", "push"],
    )
    def test_each_event_selects_its_own_arm(
        self, event: str, expected: tuple[str, ...]
    ) -> None:
        """A fork condition answers false, so a branch pull request pays.

        The contracts ask what a lane costs this repository. A fork's run
        costs nothing here, so reading the fork arm as the pull-request answer
        would report every one of these lanes as free and hide the shape they
        actually buy.
        """
        assert self._job(self._COMPOSED).labels_for_event(event) == expected, (
            f"{event!r} should select {expected!r}; reading the fork arm as the "
            "pull-request answer reports a paid lane as free"
        )

    @pytest.mark.parametrize(
        "declared",
        [
            "${{ github.event.pull_request.head.repo.fork && 'ubuntu-latest' }}",
            "${{ 'ubuntu-latest' || 'ubicloud-standard-2' }}",
            "${{ github.repository_owner == 'leynos' && 'a' || 'b' }}",
        ],
        ids=["no-fallback", "unguarded-first-arm", "unrecognized-condition"],
    )
    def test_an_unreadable_chain_stays_one_opaque_label(self, declared: str) -> None:
        """Fail towards opaque rather than towards a confident wrong answer.

        A chain with no fallback has no label to fall through to, an unguarded
        first arm makes every later arm unreachable, and a condition the
        helpers cannot evaluate would make `labels_for_event` invent an
        answer. Each stays one label, and the job is then visibly unplaced
        rather than silently misread.
        """
        job = self._job(declared)
        assert job.runner_labels == (declared,), (
            f"{declared!r} is not a shape the reader parses, so it must be "
            "reported whole rather than split into arms nobody checked"
        )

"""What a pull-request lane's `runs-on` gives a fork, read without the estate.

`fork_fallback_test.py` asserts the fork fallback on every lane a pull
request can dispatch; this module holds the readings it asserts with, so
`fork_lanes_test.py` can state each shape they must refuse against a
constructed job. The estate's own lanes are all correct, and a contract
parametrized over correct input discriminates nothing.

Two readings close holes the selector leaves. `opaque_runs_on_values` finds
an expression the placement reader cannot resolve, in the scalar form and
inside a list, where GitHub evaluates it but `Job.runner_labels` keeps it as
one label that carries no Ubicloud prefix. `paid_arms_before_the_fork` finds
an arm that hands a pull request a paid runner before the fork arm is
reached, which a fork's run takes as surely as a branch's.
"""

from __future__ import annotations

import re
import typing as typ

from _runs_on import EVENT_CONDITION_RE, conditional_runs_on_arms
from _workflow_policy import FORK_CONDITION, UBICLOUD_LABEL_PREFIX

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from _workflow_policy import Job

#: The trigger this rule is about. A `push`, a `schedule` and a
#: `workflow_dispatch` all run in this repository's own context, where an
#: Ubicloud runner is available.
FORKABLE_EVENT = "pull_request"

#: What a fork's pull request must be given instead.
HOSTED_LABEL = "ubuntu-latest"

#: A `runs-on` that is nothing but one matrix key: `${{ matrix.runner }}`.
#: Exempt from the opaque-expression refusal because the label it resolves to
#: is in the matrix rather than the expression, and the sizing contract reads
#: it there.
#:
#: The match is anchored on purpose. Testing whether the text merely contains
#: `matrix.` exempted every expression that mentioned a leg anywhere, which
#: includes a real selection the reader cannot split:
#: `${{ matrix.name == 'x' && 'ubuntu-latest' || 'ubicloud-standard-2' }}`
#: asks for a paid runner on every leg but one, and a fork's pull request
#: reaching such a leg is stranded exactly as the fork contract exists to
#: prevent.
BARE_MATRIX_REFERENCE_RE: typ.Final[re.Pattern[str]] = re.compile(
    r"^\$\{\{\s*matrix\.[A-Za-z0-9_-]+\s*\}\}$"
)


def is_a_bare_matrix_reference(declared: str) -> bool:
    """Report whether a `runs-on` value is one matrix key and nothing else."""
    return BARE_MATRIX_REFERENCE_RE.fullmatch(declared) is not None


def _declared_runs_on(job: Job) -> object:
    """Return a job's `runs-on`, unwrapping the `{group, labels}` form."""
    declared = job.body.get("runs-on")
    return declared.get("labels") if isinstance(declared, dict) else declared


def _folded(value: str) -> str:
    """Join a folded scalar's line breaks into single spaces."""
    return " ".join(value.split())


def runs_on_scalar(job: Job) -> str:
    """Return a job's scalar `runs-on`, folded; empty for any other form."""
    declared = _declared_runs_on(job)
    return _folded(declared) if isinstance(declared, str) else ""


def opaque_runs_on_values(job: Job) -> tuple[str, ...]:
    """Return every `runs-on` expression the placement reader cannot resolve.

    A scalar is opaque when it is an expression the chain reader cannot
    split. Every expression inside a list is opaque: GitHub evaluates it, but
    `Job.runner_labels` never splits a list item, so the item stays one label
    without the Ubicloud prefix and every contract selecting on the runner
    skips the job.

    Parameters
    ----------
    job
        The job whose `runs-on` is read.

    Returns
    -------
    tuple of str
        The opaque values, folded. A bare matrix reference is included; the
        caller decides whether to exempt it.
    """
    declared = _declared_runs_on(job)
    if isinstance(declared, str):
        folded = _folded(declared)
        is_opaque = folded.startswith("${{") and conditional_runs_on_arms(folded) is None
        return (folded,) if is_opaque else ()
    if isinstance(declared, list):
        labels = (_folded(label) for label in declared if isinstance(label, str))
        return tuple(label for label in labels if label.startswith("${{"))
    return ()


def paid_arms_before_the_fork(declared: str) -> tuple[str, ...]:
    """Return the paid arms a fork's pull request can reach before its own.

    The chain is read in order and the first arm whose condition holds
    wins, so an arm ahead of the fork arm that selects Ubicloud on a pull
    request takes a fork's run too. An arm keyed on another event, such as
    the cron arm, cannot, and neither can one selecting a hosted label. A
    head-repository condition other than the fork field is counted as able
    to hold, since nothing says a fork fails it.

    Parameters
    ----------
    declared
        A scalar `runs-on`, folded.

    Returns
    -------
    tuple of str
        Each offending arm as `<condition> && '<label>'`, in chain order.
        Empty when the value is not a chain.
    """
    shadowing: list[str] = []
    for condition, label in conditional_runs_on_arms(declared) or ():
        if condition == FORK_CONDITION:
            break
        if label.startswith(UBICLOUD_LABEL_PREFIX) and _holds_on_a_pull_request(
            condition
        ):
            shadowing.append(f"{condition} && '{label}'")
    return tuple(shadowing)


def _holds_on_a_pull_request(condition: str | None) -> bool:
    """Report whether an arm's condition can hold on a pull request."""
    match = EVENT_CONDITION_RE.match(condition or "")
    return match is None or match["event"] == FORKABLE_EVENT

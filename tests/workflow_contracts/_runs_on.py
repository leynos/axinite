"""Reading a `runs-on` that chooses its runner from the context.

A workflow that is both a developer gate and a cron, or that must hand a fork
a runner it can actually get, has nowhere but the label to put the
distinction. `runs-on` therefore carries an expression, and a contract that
read it as one opaque string would hide every paid runner request behind it.

The reading is deliberately narrow. An arm whose condition nobody has taught
these helpers to evaluate leaves the whole expression unparsed, because
splitting on a condition the reader does not understand would let it answer
confidently and wrongly. `fork_fallback_test.py` is what refuses an
unreadable expression in the estate, so the caution here costs nothing.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import re
import typing as typ

#: One arm of a `runs-on` that chooses its label from the context, as
#: `<condition> && 'ubuntu-latest'`. A workflow that is both a developer gate
#: and a cron, or that must hand a fork a runner it can actually get, has
#: nowhere but the label to put the distinction. Reading such a value as one
#: opaque label would hide the Ubicloud request from every placement contract,
#: so the arms are parsed.
RUNNER_ARM_RE: re.Pattern[str] = re.compile(
    r"^(?P<condition>.+?)\s*&&\s*'(?P<label>[^']+)'$"
)

#: A bare label, which can only be the last arm: the value everything else
#: falls through to.
RUNNER_FALLBACK_RE: re.Pattern[str] = re.compile(r"^'(?P<label>[^']+)'$")

#: The conditions an arm may test. Anything else leaves the whole expression
#: unparsed and therefore opaque, which is the safe direction: a shape the
#: helpers cannot read is reported as one label rather than silently split
#: into arms nobody checked.
EVENT_CONDITION_RE: re.Pattern[str] = re.compile(
    r"^github\.event_name\s*==\s*'(?P<event>[a-z_]+)'$"
)

#: A field of the pull request's head repository, `fork` above all. A pull
#: request from a fork cannot obtain an Ubicloud runner, so those lanes fall
#: back to a GitHub-hosted one. The pattern admits any field of that object so
#: that swapping `fork` for a sibling leaves the expression parseable and the
#: fork contract, not the parser, is what reports the swap.
HEAD_REPO_CONDITION_RE: re.Pattern[str] = re.compile(
    r"^github\.event\.pull_request\.head\.repo\.(?P<field>[a-z_]+)$"
)

def _expression_body(declared: str) -> str | None:
    """Return the inside of a `${{ ... }}` scalar, or None if it is not one.

    Parameters
    ----------
    declared
        The raw scalar. A folded YAML scalar arrives with its line breaks
        already joined into single spaces.

    Returns
    -------
    str or None
        The expression's body, stripped, or ``None`` for a plain label.
    """
    joined = " ".join(declared.split())
    if not (joined.startswith("${{") and joined.endswith("}}")):
        return None
    return joined[3:-2].strip()


def _parse_arm(part: str, *, last: bool) -> tuple[str | None, str] | None:
    """Return one arm of a `runs-on` chain as a condition and a label.

    Parameters
    ----------
    part
        One `||`-separated piece of the expression.
    last
        Whether this is the final piece, which is the only place a bare label
        may appear: an unguarded arm earlier in the chain would make every
        arm after it unreachable.

    Returns
    -------
    tuple, or None
        ``(condition, label)``, with ``None`` as the condition of the final
        fallback. ``None`` when the piece is not an arm these helpers read.
    """
    fallback = RUNNER_FALLBACK_RE.match(part)
    if fallback is not None:
        return (None, fallback["label"]) if last else None
    if last:
        return None
    arm = RUNNER_ARM_RE.match(part)
    if arm is None or not _recognized_condition(arm["condition"]):
        return None
    return arm["condition"], arm["label"]


def _runs_on_chain(declared: str) -> tuple[tuple[str | None, str], ...] | None:
    """Split a context-dependent `runs-on` into its arms.

    The value is a chain of guarded labels ending in an unguarded one, as
    `${{ a && 'x' || b && 'y' || 'z' }}`. GitHub binds `&&` tighter than `||`
    and yields the first truthy operand, so the arms are tried in order and
    the bare label is what everything falls through to.

    Parameters
    ----------
    declared
        The raw `runs-on` scalar.

    Returns
    -------
    tuple of tuple, or None
        One `(condition, label)` pair per arm, with ``None`` as the condition
        of the final fallback. ``None`` when the value is not this form, which
        includes a plain label, a matrix expression, a chain with no fallback,
        and any chain naming a condition these helpers do not recognize.
    """
    body = _expression_body(declared)
    if body is None:
        return None
    parts = [part.strip() for part in body.split("||")]
    if len(parts) < 2:
        return None
    arms = [
        _parse_arm(part, last=index == len(parts) - 1)
        for index, part in enumerate(parts)
    ]
    if any(arm is None for arm in arms):
        return None
    return typ.cast("tuple[tuple[str | None, str], ...]", tuple(arms))


def conditional_runs_on_arms(
    declared: str,
) -> tuple[tuple[str | None, str], ...] | None:
    """Split a context-dependent `runs-on` into its arms, or report it opaque.

    The public face of the chain reader. A contract that has to refuse a
    shape the reader cannot split needs to ask that question directly:
    `runner_labels` answers it by returning the raw text as one label, which
    is indistinguishable from a job that genuinely names a runner nobody
    recognizes.

    Parameters
    ----------
    declared
        The raw `runs-on` scalar, with any folded line breaks already joined.

    Returns
    -------
    tuple of tuple, or None
        One `(condition, label)` pair per arm, with ``None`` as the condition
        of the final fallback. ``None`` when the value is not a chain this
        reader splits, a plain label included.
    """
    return _runs_on_chain(declared)


def selected_value(declared: str, event: str) -> str | None:
    """Resolve a guarded `${{ a && 'x' || 'y' }}` scalar for one event.

    `runs-on` is not the only value a workflow keys on the event. An `env`
    entry that says what a gate must see from an upstream job has the same
    shape, and reading it as opaque text would let the gate's own expectation
    drift from the job it describes without any contract noticing.

    Parameters
    ----------
    declared
        The raw scalar. A folded YAML scalar arrives with its line breaks
        already joined into single spaces.
    event
        A `github.event_name` value, such as ``push``.

    Returns
    -------
    str or None
        The value this event selects, or ``None`` when the scalar is not a
        guarded chain these helpers read. A plain literal is not a chain, so
        it answers ``None`` too: a caller asking what an event selects wants
        to know that nothing was selected by the event at all.
    """
    chain = _runs_on_chain(declared)
    if chain is None:
        return None
    return next(
        (value for condition, value in chain if _arm_selected_by(condition, event)),
        None,
    )


def _recognized_condition(condition: str) -> bool:
    """Report whether an arm's condition is one these helpers can answer.

    An unrecognized condition leaves the whole expression opaque. That is
    deliberate: splitting on a condition nobody has taught the helpers to
    evaluate would let `labels_for_event` answer confidently and wrongly.
    """
    return (
        EVENT_CONDITION_RE.match(condition) is not None
        or HEAD_REPO_CONDITION_RE.match(condition) is not None
    )


def _arm_selected_by(condition: str | None, event: str) -> bool:
    """Report whether an arm's condition holds for an event.

    A head-repository condition is answered ``False``. The contracts ask what
    a lane costs, and a fork's pull request runs GitHub-hosted at no cost to
    this repository; answering ``True`` would report every such lane as free
    and hide the shape it actually buys for a branch pull request.
    """
    if condition is None:
        return True
    match = EVENT_CONDITION_RE.match(condition)
    if match is not None:
        return match["event"] == event
    return False

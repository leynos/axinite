"""Properties of the `runs-on` chain reader, over generated chains.

A conditional `runs-on` is a chain of guarded labels ending in an unguarded
one, `${{ a && 'x' || b && 'y' || 'z' }}`, and it takes any number of arms.
Every placement, sizing, sccache and fork contract in this directory asks a
`Job` which labels it requests and which one an event selects, so a reader
that drops an arm, reorders the chain, or answers confidently on a condition
it cannot evaluate would not fail: it would quietly narrow what the estate
appears to cost, and the contracts would pass over the difference.

The parametrized cases in `workflow_policy_helpers_test.py` pin the shapes the
estate uses today, which is one and two arms. These properties pin the rule
for the arm counts, label repeats, whitespace and condition mixtures nobody
has written yet, and each asserts both directions: what the reader must report
and what it must refuse to read.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import pytest
from _workflow_policy import FORK_CONDITION, Job
from hypothesis import HealthCheck, given, settings
from hypothesis import strategies as st

#: Hypothesis runs these against pure functions, but the suite shares a
#: machine with compiling CI jobs. A per-example deadline would turn host load
#: into a spurious failure, so only the example count is bounded.
PROPERTY = settings(
    deadline=None,
    max_examples=200,
    suppress_health_check=[HealthCheck.too_slow],
)

#: Events an arm may name. `runs_on_event` and the arm conditions both read a
#: `github.event_name` value, and the reader's pattern admits `[a-z_]+`.
EVENTS: tuple[str, ...] = (
    "pull_request",
    "push",
    "schedule",
    "workflow_call",
    "workflow_dispatch",
)

#: Labels an arm may select. Drawn from both sides of the placement rule so a
#: generated chain looks like one the estate could actually carry, and kept
#: free of the `'`, `|` and `&` characters the expression grammar uses.
LABELS: tuple[str, ...] = (
    "ubicloud-standard-2",
    "ubicloud-standard-4",
    "ubuntu-latest",
    "windows-latest",
    "macos-latest",
)

#: Conditions the reader is not taught to evaluate. Each is a plausible thing
#: to write in a `runs-on`, and each must leave the whole expression opaque
#: rather than being split into arms whose selection nobody can answer.
UNREADABLE_CONDITIONS: tuple[str, ...] = (
    "always()",
    "success()",
    "github.ref == 'refs/heads/main'",
    "github.repository == 'leynos/axinite'",
    "inputs.fast",
    "matrix.os == 'linux'",
)

events: st.SearchStrategy[str] = st.sampled_from(EVENTS)
labels: st.SearchStrategy[str] = st.sampled_from(LABELS)

#: One guarded arm: a condition the reader recognizes, and the label it picks.
#: Both recognized forms appear, because the fork fallback composes them and
#: the two answer differently to an event.
readable_arms: st.SearchStrategy[tuple[str, str]] = st.one_of(
    st.tuples(events.map(lambda e: f"github.event_name == '{e}'"), labels),
    st.tuples(st.just(FORK_CONDITION), labels),
)

#: One arm the reader must refuse: a condition it cannot evaluate, and the
#: label that condition would have selected.
unreadable_arms: st.SearchStrategy[tuple[str, str]] = st.tuples(
    st.sampled_from(UNREADABLE_CONDITIONS), labels
)


def _chain(arms: list[tuple[str, str]], fallback: str, *, joiner: str = " ") -> str:
    """Return the `runs-on` scalar a list of arms and a fallback spell.

    Parameters
    ----------
    arms
        The guarded arms, in the order they are tried.
    fallback
        The bare label everything falls through to.
    joiner
        What separates the tokens. The reader collapses whitespace, so a
        newline here is the folded YAML scalar a workflow actually carries.

    Returns
    -------
    str
        The scalar, as a workflow's `runs-on` value.
    """
    parts = [f"{condition} &&{joiner}'{label}'" for condition, label in arms]
    parts.append(f"'{fallback}'")
    body = f"{joiner}||{joiner}".join(parts)
    return "${{" + joiner + body + joiner + "}}"


def _job(declared: str) -> Job:
    """Return a job whose `runs-on` is the given scalar."""
    return Job(workflow="generated.yml", job_id="lane", body={"runs-on": declared})


def _selected_by(arms: list[tuple[str, str]], fallback: str, event: str) -> str:
    """Return the label an event should select, computed independently.

    The rule is restated here rather than borrowed from the reader, so the
    property compares two readings instead of comparing the reader with
    itself. An arm naming the event wins; a head-repository condition never
    does, because the contracts ask what a lane costs this repository and a
    fork's run costs nothing here; and everything else falls through.
    """
    for condition, label in arms:
        if condition == f"github.event_name == '{event}'":
            return label
    return fallback


@given(
    arms=st.lists(readable_arms, min_size=1, max_size=6),
    fallback=labels,
    joiner=st.sampled_from([" ", "\n", "  ", " \n  "]),
)
@PROPERTY
def test_every_label_in_the_chain_is_reported(
    arms: list[tuple[str, str]], fallback: str, joiner: str
) -> None:
    """A chain's labels are read back in order, whatever its length.

    This is the direction that protects the estate's cost: a reader that
    dropped an arm would hide an Ubicloud request from every placement and
    sizing contract, and those contracts would then pass.
    """
    declared = _chain(arms, fallback, joiner=joiner)
    expected = tuple(dict.fromkeys([label for _, label in arms] + [fallback]))
    assert _job(declared).runner_labels == expected, (
        f"{declared!r} should report {expected!r}; a label the reader drops "
        "is an Ubicloud request hidden from every placement contract"
    )


@given(
    arms=st.lists(readable_arms, min_size=1, max_size=6),
    fallback=labels,
    event=events,
)
@PROPERTY
def test_the_event_selects_the_first_arm_that_names_it(
    arms: list[tuple[str, str]], fallback: str, event: str
) -> None:
    """The first arm naming the event wins, and otherwise the fallback does.

    GitHub binds `&&` tighter than `||` and yields the first truthy operand,
    so a reader that took the last match, or the widest, would report a
    different runner from the one the lane gets.
    """
    declared = _chain(arms, fallback)
    expected = _selected_by(arms, fallback, event)
    assert _job(declared).labels_for_event(event) == (expected,), (
        f"on {event!r}, {declared!r} should select {expected!r}: the first "
        "arm naming the event wins, and otherwise the bare label does"
    )


@given(
    arms=st.lists(readable_arms, min_size=1, max_size=6),
    fallback=labels,
    event=events,
)
@PROPERTY
def test_the_selected_label_is_always_one_the_chain_declares(
    arms: list[tuple[str, str]], fallback: str, event: str
) -> None:
    """An event selects exactly one label, and never invents one."""
    declared = _chain(arms, fallback)
    job = _job(declared)
    selected = job.labels_for_event(event)
    assert len(selected) == 1, (
        f"{declared!r} selected {selected!r} on {event!r}; an event picks "
        "exactly one label"
    )
    assert selected[0] in job.runner_labels, (
        f"{selected[0]!r} is not among {job.runner_labels!r}; the selection "
        "must be a label the chain declares, never an invented one"
    )


@given(
    fork_arms=st.lists(
        st.tuples(st.just(FORK_CONDITION), labels), min_size=1, max_size=4
    ),
    fallback=labels,
    event=events,
)
@PROPERTY
def test_a_fork_arm_is_never_what_an_event_selects(
    fork_arms: list[tuple[str, str]], fallback: str, event: str
) -> None:
    """A chain guarded only on the fork field answers its fallback.

    The narrow direction of the rule above. These contracts ask what a lane
    costs this repository, and a fork's pull request runs GitHub-hosted at no
    cost here, so answering the fork arm would report the lane as free and
    hide the shape it buys for a branch pull request.
    """
    declared = _chain(fork_arms, fallback)
    assert _job(declared).labels_for_event(event) == (fallback,), (
        f"{declared!r} should fall through to {fallback!r} on {event!r}; a "
        "fork arm costs this repository nothing and must never be selected"
    )


@given(
    readable=st.lists(readable_arms, max_size=3),
    unreadable=unreadable_arms,
    fallback=labels,
    event=events,
)
@PROPERTY
def test_one_unreadable_condition_makes_the_whole_value_opaque(
    readable: list[tuple[str, str]],
    unreadable: tuple[str, str],
    fallback: str,
    event: str,
) -> None:
    """A condition the reader cannot evaluate stops it splitting the chain.

    This is the safe direction and it has to hold for the whole expression,
    not just the offending arm. Splitting on a condition nobody has taught the
    reader to answer would let `labels_for_event` report a runner confidently
    and wrongly; reporting the scalar as one opaque label instead makes the
    unreadable shape visible to whoever reads a contract's failure.
    """
    declared = _chain([*readable, unreadable], fallback)
    job = _job(declared)
    assert job.runner_labels == (declared,), (
        f"{declared!r} should read as one opaque label; splitting a chain the "
        "reader cannot evaluate lets it answer confidently and wrongly"
    )
    assert job.labels_for_event(event) == (declared,), (
        f"{declared!r} should answer itself on {event!r}; an unreadable chain "
        "has no arm anyone can select"
    )


@given(arms=st.lists(readable_arms, min_size=1, max_size=4), event=events)
@PROPERTY
def test_a_chain_with_no_fallback_is_opaque(
    arms: list[tuple[str, str]], event: str
) -> None:
    """Every arm guarded means the expression can yield nothing.

    GitHub would evaluate such a chain to `false` and the job would have no
    runner at all. Reading it as a set of arms would report labels the lane
    never gets, so it is refused outright.
    """
    parts = [f"{condition} && '{label}'" for condition, label in arms]
    declared = "${{ " + " || ".join(parts) + " }}"
    job = _job(declared)
    assert job.runner_labels == (declared,), (
        f"{declared!r} should read as one opaque label; splitting a chain the "
        "reader cannot evaluate lets it answer confidently and wrongly"
    )
    assert job.labels_for_event(event) == (declared,), (
        f"{declared!r} should answer itself on {event!r}; an unreadable chain "
        "has no arm anyone can select"
    )


@given(label=labels, event=events)
@PROPERTY
def test_a_plain_label_is_left_exactly_as_written(label: str, event: str) -> None:
    """A `runs-on` that is not an expression is one label for every event."""
    job = _job(label)
    assert job.runner_labels == (label,), (
        f"{label!r} is not an expression, so it is the one label declared"
    )
    assert job.labels_for_event(event) == (label,), (
        f"{label!r} does not depend on the event, so {event!r} selects it too"
    )


@pytest.mark.parametrize("arm_count", [1, 2, 3, 4, 5])
def test_the_generator_builds_the_chain_lengths_it_claims(arm_count: int) -> None:
    """Guard the properties above against a generator that builds nothing.

    Every property here is satisfied by a chain the reader refuses, so the
    builder's own output is checked: an `arm_count`-armed chain must read back
    as `arm_count` arms plus the fallback, which is what makes the arm count
    the thing under test rather than an unused parameter.
    """
    arms = [
        (f"github.event_name == '{EVENTS[i % len(EVENTS)]}'", LABELS[i])
        for i in range(arm_count)
    ]
    declared = _chain(arms, "ubuntu-latest")
    expected = dict.fromkeys([label for _, label in arms] + ["ubuntu-latest"])
    assert len(_job(declared).runner_labels) == len(expected), (
        f"a {arm_count}-armed chain read back as "
        f"{_job(declared).runner_labels!r}; if the builder does not produce "
        "the arm counts it claims, every property above is vacuous"
    )

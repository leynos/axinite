"""Contracts for who owns a CI cache and who is allowed to fill it.

A cache with two owners, or one that archives a `target` tree, writes
gigabytes that the next run mostly discards: the Ubicloud listing on
2026-09-03 showed 4.1 to 4.4 GB `v0-rust-*-tests` archives from exactly that
shape. The opposite failure is quieter and was live on this branch. Every
lane restored the Cargo registry key and nothing filled it, because the only
save step sat in a job whose guard refused the one event the step required,
so each lane downloaded the registry and the git index again and the only
evidence was the duration. Both are pinned here.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import typing as typ

import pytest
from _workflow_policy import (
    CACHE_ACTION_SHA,
    DIST_GENERATED,
    WORKFLOW_DIR,
    Job,
    cache_paths,
    is_cache_step,
    jobs,
    load,
    runs_on_event,
    triggers,
)

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Iterator

ALL_JOBS: tuple[Job, ...] = tuple(jobs())


def _ids(candidates: tuple[Job, ...]) -> list[str]:
    """Return readable parameter identifiers for a job sequence."""
    return [str(job) for job in candidates]


@pytest.mark.parametrize("job", ALL_JOBS, ids=_ids(ALL_JOBS))
def test_cache_action_is_pinned_to_the_reviewed_commit(job: Job) -> None:
    """Pin the cache action: Ubicloud's proxy intercepts this version."""
    if job.workflow == DIST_GENERATED:
        pytest.skip("dist owns release.yml's generated cache wiring")
    for step in job.steps:
        if not is_cache_step(step):
            continue
        uses = str(step.get("uses"))
        assert uses.endswith(f"@{CACHE_ACTION_SHA}"), (
            f"{job} uses {uses!r}; every cache step must pin "
            f"actions/cache at {CACHE_ACTION_SHA} (v6.1.0)"
        )


@pytest.mark.parametrize("job", ALL_JOBS, ids=_ids(ALL_JOBS))
def test_no_cache_step_archives_a_target_tree(job: Job) -> None:
    """Keep compiler output out of cache archives; sccache owns it."""
    for step in job.steps:
        if not is_cache_step(step):
            continue
        for path in cache_paths(step):
            assert not path.split("/")[0] == "target", (
                f"{job} archives {path!r}. A target tree duplicates sccache's "
                "ownership of compiler output and inflates the cache quota."
            )


@pytest.mark.parametrize("job", ALL_JOBS, ids=_ids(ALL_JOBS))
def test_each_cache_path_has_one_owner_within_a_job(job: Job) -> None:
    """Forbid two cache keys in one job from claiming the same path.

    A restore step and its matching save step share one key, so they are one
    owner. Two different keys over the same path are two owners, and the
    second write silently discards or duplicates the first.
    """
    owners: dict[str, set[str]] = {}
    for step in job.steps:
        if not is_cache_step(step):
            continue
        inputs = step.get("with")
        key = str(inputs.get("key", "")) if isinstance(inputs, dict) else ""
        for path in cache_paths(step):
            owners.setdefault(path, set()).add(key)
    for path, keys in sorted(owners.items()):
        assert len(keys) == 1, (
            f"{job} caches {path!r} under {len(keys)} different keys; each "
            "mutable path needs exactly one owner"
        )


#: The two `actions/cache` entry points a step can use to touch a key. The
#: combined `actions/cache@` form is neither: it restores and saves under one
#: step, which `test_each_cache_path_has_one_owner_within_a_job` allows and
#: the writer contracts below do not need to read.
RESTORE_ACTION = "actions/cache/restore@"
SAVE_ACTION = "actions/cache/save@"

#: The prefix every Cargo registry and Git index key shares. The rest of the
#: key is `runner.os`, `runner.arch`, `runner.environment` and the lockfile
#: hash, so this is what names the family rather than one member of it.
REGISTRY_KEY_PREFIX = "cargo-v1-"


def _cache_steps(action: str) -> Iterator[tuple[Job, dict[str, object]]]:
    """Yield every step in the estate that uses one cache entry point.

    Parameters
    ----------
    action
        A `uses` prefix, either `RESTORE_ACTION` or `SAVE_ACTION`.

    Yields
    ------
    tuple of Job and dict
        The job the step belongs to, and the step itself.
    """
    for job in ALL_JOBS:
        for step in job.steps:
            uses = step.get("uses")
            if isinstance(uses, str) and uses.startswith(action):
                yield job, step


def _step_key(step: dict[str, object]) -> str:
    """Return the key a cache step names, empty when it names none."""
    inputs = step.get("with")
    return str(inputs.get("key", "")) if isinstance(inputs, dict) else ""


def _registry_platforms(action: str, *, reachable: bool = False) -> set[str]:
    """Return the platforms whose registry key one cache action touches.

    Parameters
    ----------
    action
        A `uses` prefix, either `RESTORE_ACTION` or `SAVE_ACTION`.
    reachable
        When true, count only steps in a job a push can dispatch. A write
        from a job that refuses the push event never happens.

    Returns
    -------
    set of str
        The `runner.os` values, drawn from `Linux` and `Windows`.
    """
    return {
        _cache_platform(job)
        for job, step in _cache_steps(action)
        if REGISTRY_KEY_PREFIX in _step_key(step)
        and (not reachable or runs_on_event(job, "push"))
    }


def _pushes_to_main(workflow: str) -> bool:
    """Report whether a workflow is triggered by a push to `main`.

    Parameters
    ----------
    workflow
        A workflow file name, such as ``coverage.yml``.

    Returns
    -------
    bool
        True when the workflow declares a push trigger that includes `main`.
    """
    push = triggers(load(WORKFLOW_DIR / workflow)).get("push")
    if push is None:
        return False
    # A bare `push:`, or a mapping without `branches`, accepts every branch.
    # Only an explicit branch list can leave main out.
    branches = push.get("branches") if isinstance(push, dict) else None
    return branches is None or "main" in branches


def _cache_platform(job: Job) -> str:
    """Name the `runner.os` value a job's cache keys resolve to.

    Parameters
    ----------
    job
        The job whose runner labels are read.

    Returns
    -------
    str
        ``Windows`` when any label names Windows, otherwise ``Linux``.
    """
    labels = job.runner_labels
    return "Windows" if any("windows" in label for label in labels) else "Linux"


def _assert_the_write_is_restricted_to_a_push_to_main(job: Job, condition: str) -> None:
    """Assert a save step's own condition admits a push to `main` and no more.

    Parameters
    ----------
    job
        The job the save step belongs to, named in the failure messages.
    condition
        The step's `if` expression, with its whitespace collapsed.
    """
    assert "refs/heads/main" in condition, (
        f"{job} saves a cache without restricting the write to main; pull "
        "requests must restore and never save"
    )
    # The ref alone is not enough. `github.ref` reads `refs/heads/main` for a
    # manual dispatch against main just as it does for a push, so a guard on
    # the ref would let a warm-cache measurement run overwrite what the merge
    # wrote.
    assert "github.event_name == 'push'" in condition, (
        f"{job} saves a cache without naming the push event; a "
        "workflow_dispatch on main would satisfy a ref-only guard and take "
        "the key from its real writer"
    )


def _assert_the_write_can_actually_happen(job: Job) -> None:
    """Assert a save step's job is one a push to `main` can dispatch.

    A step condition naming the push event is worth nothing if the step can
    never see one. Standing `tests` down on a push left its save step guarded
    on the one event its own job had just refused, and every reader of that
    key went unwritten while the contract stayed green on the step alone.

    Parameters
    ----------
    job
        The job the save step belongs to.
    """
    assert _pushes_to_main(job.workflow), (
        f"{job} saves the registry cache, but {job.workflow} is not triggered "
        "by a push to main, so the write never runs"
    )
    assert runs_on_event(job, "push"), (
        f"{job} saves the registry cache on a push, but its own condition "
        "refuses that event: the step is unreachable and the key has no writer"
    )


def test_the_cargo_registry_cache_has_exactly_one_writer_per_platform() -> None:
    """Keep one save step per key family so no two jobs race to publish."""
    writers: list[str] = []
    for job, step in _cache_steps(SAVE_ACTION):
        _assert_the_write_is_restricted_to_a_push_to_main(
            job, " ".join(str(step.get("if", "")).split())
        )
        _assert_the_write_can_actually_happen(job)
        writers.append(str(job))
    # One Linux writer and one Windows writer. The Linux write sits in
    # `coverage.yml` because that is the job which still runs on a push.
    assert sorted(writers) == ["coverage.yml:coverage", "test.yml:windows-build"], (
        f"unexpected set of cache writers: {sorted(writers)}"
    )


def test_every_restored_cargo_registry_key_has_a_writer() -> None:
    """Every platform that restores the registry key needs one to fill it.

    A restore-only estate is silent. Each lane simply downloads the registry
    and the git index again, and the only evidence is the duration.
    """
    restorers = _registry_platforms(RESTORE_ACTION)
    written = _registry_platforms(SAVE_ACTION, reachable=True)
    assert restorers, "no job restores the cargo registry cache at all"
    assert restorers <= written, (
        "these platforms restore the cargo registry cache with no reachable "
        f"writer: {sorted(restorers - written)}"
    )

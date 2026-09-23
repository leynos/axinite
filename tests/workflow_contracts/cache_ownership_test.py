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
from pathlib import PurePosixPath, PureWindowsPath

import pytest
from _cache_conditions import save_condition_faults
from _estate import estate_jobs, read_workflow
from _workflow_policy import (
    CACHE_ACTION_SHA,
    DIST_GENERATED,
    WORKFLOW_DIR,
    Job,
    cache_paths,
    is_cache_step,
    runs_on_event,
    triggers,
)

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Iterator

#: Every job in the estate, read through `_sources`. Named rather than
#: called: `pytest_generate_tests` in `conftest.py` calls it during
#: collection and parametrizes each test's `job` argument with the result,
#: turning a `SourceError` into a failure of that test naming the file.
JOB_SELECTOR = estate_jobs


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


def _path_components(path: str) -> set[str]:
    """Return every component a cache path names, under either separator.

    The estate caches on Windows runners as well as Linux ones, and
    `PurePosixPath` reads a backslash as an ordinary character: it sees
    `tools-src\\github\\target` as a single component that merely ends in
    `target`. Reading the same text under both flavours means one archive of
    a build tree cannot hide behind the separator it was written with.

    Parameters
    ----------
    path
        One line of a cache step's `path` input.

    Returns
    -------
    set of str
        Every component, under POSIX and under Windows separator rules.
    """
    return set(PurePosixPath(path).parts) | set(PureWindowsPath(path).parts)


def test_no_cache_step_archives_a_target_tree(job: Job) -> None:
    """Keep compiler output out of cache archives; sccache owns it."""
    for step in job.steps:
        if not is_cache_step(step):
            continue
        for path in cache_paths(step):
            # Every component, not just the first: `tools-src/github/target`
            # is as much a build tree as `target` is, and a check on the
            # leading component alone would wave the nested one through.
            assert "target" not in _path_components(path), (
                f"{job} archives {path!r}. A target tree duplicates sccache's "
                "ownership of compiler output and inflates the cache quota."
            )


@pytest.mark.parametrize(
    "path",
    [
        pytest.param("target", id="bare"),
        pytest.param("tools-src/github/target", id="nested-posix"),
        pytest.param("tools-src\\github\\target", id="nested-windows"),
        pytest.param("target/debug", id="inside-posix"),
        pytest.param("target\\debug", id="inside-windows"),
    ],
)
def test_a_build_tree_is_seen_under_either_separator(path: str) -> None:
    """The reader must find `target` however the path was written.

    A Windows-separated path is the case `PurePosixPath` alone gets wrong: it
    reads the whole string as one component, so the assertion above passed on
    exactly the archive it exists to refuse.
    """
    assert "target" in _path_components(path)


@pytest.mark.parametrize(
    "path",
    [
        pytest.param("~/.cargo/registry", id="registry"),
        pytest.param("~/.cargo/git", id="git-index"),
        pytest.param("targeted", id="a-longer-name"),
        pytest.param("~\\.cargo\\registry", id="registry-windows"),
    ],
)
def test_a_path_that_is_not_a_build_tree_is_left_alone(path: str) -> None:
    """An ordinary cache path must not read as a build tree.

    The narrow direction. A reader that answered `target` for everything
    would satisfy the cases above and condemn every cache in the estate, and
    the contract would still read as though it discriminated.
    """
    assert "target" not in _path_components(path)


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
    for job in estate_jobs():
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


def _push_reaches_main(declared: dict[str, object]) -> bool:
    """Report whether a parsed `on:` mapping is triggered by a push to `main`.

    Split from `_pushes_to_main` so the reading is a pure function of the
    mapping. The three shapes it has to tell apart cannot be produced by the
    estate on demand, and two of them look identical to `dict.get`.

    Parameters
    ----------
    declared
        A workflow's parsed `on:` mapping.

    Returns
    -------
    bool
        True when a push to `main` runs the workflow.
    """
    # Membership, not the value. PyYAML reads a valueless `push:` as `None`,
    # which is exactly what a missing key returns, so asking for the value
    # cannot tell a workflow triggered on every branch from one that is not
    # triggered by a push at all. The first is the broadest possible trigger
    # and the second is the narrowest, and reading them as the same thing
    # would refuse a writer sitting in a workflow that runs on every push.
    if "push" not in declared:
        return False
    push = declared["push"]
    # A bare `push:`, or a mapping without `branches`, accepts every branch.
    # Only an explicit branch list can leave main out.
    branches = push.get("branches") if isinstance(push, dict) else None
    return branches is None or "main" in branches


def _pushes_to_main(workflow: str) -> bool:
    """Report whether a workflow file is triggered by a push to `main`.

    Parameters
    ----------
    workflow
        A workflow file name, such as ``coverage.yml``.

    Returns
    -------
    bool
        True when the workflow declares a push trigger that includes `main`.
    """
    return _push_reaches_main(triggers(read_workflow(WORKFLOW_DIR / workflow)))


@pytest.mark.parametrize(
    ("declared", "expected"),
    [
        pytest.param({}, False, id="no-triggers-at-all"),
        pytest.param({"pull_request": None}, False, id="pull-request-only"),
        pytest.param({"push": None}, True, id="a-bare-push-accepts-every-branch"),
        pytest.param({"push": {}}, True, id="a-push-without-branches"),
        pytest.param({"push": {"branches": ["main"]}}, True, id="main-named"),
        pytest.param(
            {"push": {"branches": ["main", "release"]}}, True, id="main-among-others"
        ),
        pytest.param({"push": {"branches": ["release"]}}, False, id="main-left-out"),
    ],
)
def test_a_bare_push_is_not_a_missing_push(
    declared: dict[str, object], *, expected: bool
) -> None:
    """Tell a trigger on every branch from no trigger at all.

    PyYAML reads a valueless `push:` as `None`, which is what a missing key
    also yields, so a reader that asks for the value collapses the broadest
    possible trigger into the narrowest. The pair that matters is
    `a-bare-push-accepts-every-branch` against `pull-request-only`: both
    return `None` from `.get("push")` and they mean opposite things.
    """
    assert _push_reaches_main(declared) is expected, (
        f"{declared!r} should read as reaches-main={expected}"
    )


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


def _restore_ids_in(job: Job) -> frozenset[str]:
    """Return the IDs the job's registry restore steps declare."""
    return frozenset(
        {
            str(step["id"])
            for step in job.steps
            if isinstance(step.get("uses"), str)
            and str(step["uses"]).startswith(RESTORE_ACTION)
            and isinstance(step.get("id"), str)
        }
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
        # The reading is in `_cache_conditions.py`, which takes the
        # expression apart rather than searching it, and
        # `cache_condition_test.py` proves it by mutating each conjunct.
        faults = save_condition_faults(str(step.get("if", "")), _restore_ids_in(job))
        assert not faults, f"{job}'s cache save condition is wrong: " + "; ".join(
            faults
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

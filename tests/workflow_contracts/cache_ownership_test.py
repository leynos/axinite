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
from _cache_policy import (
    CACHE_ACTION_SHA,
    COMBINED_ACTION,
    REGISTRY_KEY_PREFIX,
    RESTORE_ACTION,
    SAVE_ACTION,
    cache_paths,
    cache_steps,
    is_cache_step,
    registry_entries,
    step_key,
    writer_job_faults,
)
from _estate import estate_jobs, jobs_across
from _trigger_reading import triggers
from _workflow_policy import DIST_GENERATED, Job

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from _estate import Estate

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


def _restore_ids_in(job: Job) -> frozenset[str]:
    """Return the IDs the job's registry restore steps declare."""
    return frozenset(
        {
            str(step["id"])
            for _, step in cache_steps([job], RESTORE_ACTION)
            if isinstance(step.get("id"), str)
        }
    )


def _save_step_faults(estate: Estate, job: Job, step: dict[object, object]) -> list[str]:
    """Return what is wrong with one save step and the job holding it.

    The condition reading is in `_cache_conditions.py`, which takes the
    expression apart rather than searching it; the job reading is
    `writer_job_faults`. `cache_condition_test.py` and `cache_policy_test.py`
    prove each by mutation.
    """
    return save_condition_faults(
        str(step.get("if", "")), _restore_ids_in(job)
    ) + writer_job_faults(job, triggers(dict(estate[job.workflow])))


def test_the_cargo_registry_cache_has_exactly_one_writer_per_platform(
    estate: Estate,
) -> None:
    """Keep one save step per key family so no two jobs race to publish."""
    jobs = tuple(jobs_across(estate))
    writers: list[str] = []
    for job, step in cache_steps(jobs, SAVE_ACTION):
        faults = _save_step_faults(estate, job, step)
        assert not faults, f"{job}'s cache save is wrong: " + "; ".join(faults)
        writers.append(str(job))
    # One Linux writer and one Windows writer. The Linux write sits in
    # `coverage.yml` because that is the job which still runs on a push.
    assert sorted(writers) == ["coverage.yml:coverage", "test.yml:windows-build"], (
        f"unexpected set of cache writers: {sorted(writers)}"
    )


def test_no_combined_cache_action_writes_the_registry_key(estate: Estate) -> None:
    """Refuse the combined action for the registry family outright.

    It saves its key at the end of every successful job that missed, from any
    event and any leg, so it is a second writer the guarded save steps above
    cannot see.
    """
    combined = [
        str(job)
        for job, step in cache_steps(jobs_across(estate), COMBINED_ACTION)
        if REGISTRY_KEY_PREFIX in step_key(step)
    ]
    assert not combined, (
        f"{combined} use {COMBINED_ACTION} for a {REGISTRY_KEY_PREFIX} key, which "
        "saves it after every miss; restore it and leave the write to the "
        "guarded save step"
    )


def test_every_restored_cargo_registry_key_has_a_writer(estate: Estate) -> None:
    """Every registry cache a job restores needs a reachable writer to fill it.

    A restore-only estate is silent. Each lane simply downloads the registry
    and the git index again, and the only evidence is the duration. Matched
    on platform, key and paths together: GitHub matches a cache on its key and
    on a version derived from the paths, so a writer agreeing on the key
    prefix alone fills a cache nobody restores.
    """
    jobs = tuple(jobs_across(estate))
    restored = registry_entries(jobs, RESTORE_ACTION)
    reachable = [
        job
        for job, _ in cache_steps(jobs, SAVE_ACTION)
        if not writer_job_faults(job, triggers(dict(estate[job.workflow])))
    ]
    written = registry_entries(reachable, SAVE_ACTION)
    assert restored, "no job restores the cargo registry cache at all"
    assert restored <= written, (
        "these registry caches are restored with no reachable writer saving "
        f"the same key and paths: {sorted(restored - written)}"
    )

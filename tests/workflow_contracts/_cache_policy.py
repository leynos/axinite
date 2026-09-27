"""What the Actions cache steps in the estate touch, and who writes each key.

Split from `_workflow_policy.py`, which reads jobs and runners; this module
reads cache steps. Everything here is a pure function of parsed jobs and
triggers, so `cache_policy_test.py` can state each case without a workflow
file, and `cache_ownership_test.py` applies it to the estate.

The writer questions are the ones a substring reading got wrong. A cache key
is written only if some save step names it, that step's job runs on a push
to `main`, and the step archives the same paths the readers restore: GitHub
matches a cache on its key and on a version derived from the paths, so a
writer that keeps the key prefix but archives something else fills a cache
nobody restores. The combined `actions/cache@` action is a writer too: it
saves its key after every successful job that missed, which is why the
registry family refuses it.
"""

from __future__ import annotations

import re
import typing as typ

from _push_filters import push_reaches_branch
from _trigger_reading import runs_on_event

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Iterable, Iterator, Mapping

    from _workflow_policy import Job

#: The reviewed pin for the Actions cache, v6.1.0. Ubicloud's transparent
#: cache proxy is confirmed to intercept this version's traffic.
CACHE_ACTION_SHA = "55cc8345863c7cc4c66a329aec7e433d2d1c52a9"

#: The three `actions/cache` entry points. The combined action restores at
#: the start of a job and saves at the end; the other two do one each.
COMBINED_ACTION = "actions/cache@"
RESTORE_ACTION = "actions/cache/restore@"
SAVE_ACTION = "actions/cache/save@"
CACHE_ACTION_PREFIXES = (COMBINED_ACTION, RESTORE_ACTION, SAVE_ACTION)

#: The prefix every Cargo registry and Git index key shares. The rest of the
#: key is `runner.os`, `runner.arch`, `runner.environment` and the lockfile
#: hash, so this is what names the family rather than one member of it.
REGISTRY_KEY_PREFIX = "cargo-v1-"

#: A job condition naming the ref. The save step's own condition already
#: holds the write to `main`; a job-level ref guard is one more constraint
#: that can refuse `main`, and this reading does not follow it.
_REF_GUARD_RE: typ.Final[re.Pattern[str]] = re.compile(
    r"\bgithub\.(?:ref|ref_name|ref_type|head_ref|base_ref)\b"
)


class RegistryEntry(typ.NamedTuple):
    """One platform's registry cache, as a step names it.

    Attributes
    ----------
    platform
        The `runner.os` value the key resolves to.
    key
        The key expression, as written.
    paths
        The archived paths, which GitHub folds into the cache version.
    """

    platform: str
    key: str
    paths: frozenset[str]


def cache_paths(step: Mapping[object, object]) -> list[str]:
    """Return the paths a cache step declares.

    Parameters
    ----------
    step
        One step mapping, normally an `actions/cache` invocation.

    Returns
    -------
    list of str
        One entry per non-empty line of the step's `path` input, stripped of
        surrounding whitespace. Empty when the step declares no paths.
    """
    inputs = step.get("with")
    if not isinstance(inputs, dict):
        return []
    declared = inputs.get("path")
    if not isinstance(declared, str):
        return []
    return [line.strip() for line in declared.splitlines() if line.strip()]


def is_cache_step(step: Mapping[object, object]) -> bool:
    """Report whether a step invokes the Actions cache.

    Parameters
    ----------
    step
        One step mapping.

    Returns
    -------
    bool
        True for the combined action and for its `restore` and `save`
        sub-actions alike.
    """
    uses = step.get("uses")
    return isinstance(uses, str) and uses.startswith(CACHE_ACTION_PREFIXES)


def cache_steps(
    jobs: Iterable[Job], action: str
) -> Iterator[tuple[Job, dict[object, object]]]:
    """Yield every step, with its job, that uses one cache entry point.

    Parameters
    ----------
    jobs
        The jobs to search.
    action
        A `uses` prefix: `COMBINED_ACTION`, `RESTORE_ACTION` or
        `SAVE_ACTION`. The three do not overlap.

    Yields
    ------
    tuple of Job and dict
        The job the step belongs to, and the step itself.
    """
    for job in jobs:
        for step in job.steps:
            uses = step.get("uses")
            if isinstance(uses, str) and uses.startswith(action):
                yield job, step


def step_key(step: Mapping[object, object]) -> str:
    """Return the key a cache step names, empty when it names none."""
    inputs = step.get("with")
    return str(inputs.get("key", "")) if isinstance(inputs, dict) else ""


def cache_platform(job: Job) -> str:
    """Name the `runner.os` value a job's cache keys resolve to.

    Returns ``Windows`` when any runner label names Windows, otherwise
    ``Linux``.
    """
    labels = job.runner_labels
    return "Windows" if any("windows" in label for label in labels) else "Linux"


def registry_entries(jobs: Iterable[Job], action: str) -> set[RegistryEntry]:
    """Return each registry cache one cache entry point touches.

    Parameters
    ----------
    jobs
        The jobs to search.
    action
        A `uses` prefix, as for `cache_steps`.

    Returns
    -------
    set of RegistryEntry
        One entry per distinct platform, key and path set among the steps
        whose key is in the registry family.
    """
    return {
        RegistryEntry(cache_platform(job), step_key(step), frozenset(cache_paths(step)))
        for job, step in cache_steps(jobs, action)
        if REGISTRY_KEY_PREFIX in step_key(step)
    }


def writer_job_faults(job: Job, declared_on: Mapping[object, object]) -> list[str]:
    """Return why a save step's job cannot write on a push to `main`.

    A step condition naming the push event is worth nothing if the step can
    never see one. Standing `tests` down on a push once left its save step
    guarded on the one event its own job had just refused, and every reader
    of that key went unwritten while a contract on the step alone stayed
    green.

    Parameters
    ----------
    job
        The job holding the save step.
    declared_on
        Its workflow's parsed `on:` mapping.

    Returns
    -------
    list of str
        One sentence per fault; empty when a push to `main` runs the job.
    """
    faults: list[str] = []
    if not push_reaches_branch(declared_on, "main"):
        faults.append(
            f"{job.workflow} is not triggered by a push to main, so the write "
            "never runs"
        )
    if not runs_on_event(job, "push"):
        faults.append("its own condition refuses the push event")
    if _REF_GUARD_RE.search(str(job.body.get("if", ""))):
        faults.append(
            "its own condition constrains the ref, which this reading does "
            "not follow; the save step's condition already holds the write "
            "to main"
        )
    return faults

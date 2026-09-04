"""Contracts keeping scheduled work off paid runners.

A developer waiting on a gate is the only thing an Ubicloud runner is bought
for. Cron work has nobody waiting, so it belongs on GitHub-hosted runners,
where the estate's public repositories pay nothing for it.

The rule is easy to satisfy by accident and easy to break invisibly, because a
scheduled workflow need not declare a runner at all. Axinite's `staging-ci.yml`
put every job it owned on `ubuntu-latest` and still spent about £22 a month on
`ubicloud-standard-8`, because two of its jobs were `uses:` callers into
`test.yml` and `e2e.yml`, whose jobs are Ubicloud by design for the developer
path. Reading only the calling workflow finds nothing wrong. These contracts
therefore follow a local reusable-workflow call into the workflow it names.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import typing as typ

import pytest
from _workflow_policy import (
    UBICLOUD_LABEL_PREFIX,
    WORKFLOW_DIR,
    Job,
    jobs_of,
    load,
    workflow_paths,
)

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from pathlib import Path

#: Prefix identifying a `uses:` that names another workflow in this repository.
#: A call into a shared repository cannot be resolved from here, and its runner
#: choice belongs to the repository that owns it.
LOCAL_CALL_PREFIX = "./.github/workflows/"


def _triggers(document: dict[str, object]) -> dict[str, object]:
    """Return a workflow's `on:` mapping.

    PyYAML resolves an unquoted `on:` key to the boolean ``True``, so a
    workflow that omits the quotes would otherwise read as having no triggers
    and pass every assertion vacuously.
    """
    declared = document.get("on", document.get(True))
    return declared if isinstance(declared, dict) else {}


def _scheduled_workflows() -> list[Path]:
    """Return every workflow that declares a `schedule` trigger."""
    return [path for path in workflow_paths() if "schedule" in _triggers(load(path))]


def _called_workflow(job: Job) -> str | None:
    """Return the local workflow file a job calls, if it calls one."""
    uses = job.body.get("uses")
    if not isinstance(uses, str) or not uses.startswith(LOCAL_CALL_PREFIX):
        return None
    return uses[len(LOCAL_CALL_PREFIX) :].split("@")[0]


def _ubicloud_labels_on_schedule(job: Job) -> tuple[str, ...]:
    """Return the Ubicloud labels a job requests when a schedule triggers it."""
    return tuple(
        label
        for label in job.labels_for_event("schedule")
        if label.startswith(UBICLOUD_LABEL_PREFIX)
    )


def test_some_workflow_is_scheduled() -> None:
    """Guard against a trigger reader that silently matches nothing.

    Without this, deleting `schedule` from every workflow, or breaking
    `_triggers`, would make each contract below pass by having no cases.
    """
    assert _scheduled_workflows(), "expected at least one scheduled workflow"


@pytest.mark.parametrize(
    "path", _scheduled_workflows(), ids=[p.name for p in _scheduled_workflows()]
)
def test_a_scheduled_workflow_owns_no_ubicloud_job(path: Path) -> None:
    """Keep a scheduled workflow's own jobs off paid runners."""
    for job in jobs_of(path.name, load(path)):
        requested = _ubicloud_labels_on_schedule(job)
        assert not requested, (
            f"{job} runs on a schedule and requests {', '.join(requested)}. "
            "Nobody is waiting on cron output, so it belongs on a "
            "GitHub-hosted runner."
        )


@pytest.mark.parametrize(
    "path", _scheduled_workflows(), ids=[p.name for p in _scheduled_workflows()]
)
def test_a_scheduled_workflow_calls_no_ubicloud_job(path: Path) -> None:
    """Follow a local reusable-workflow call and apply the same rule.

    This is the assertion that would have caught Axinite's staging suite. Its
    calling jobs declared no runner at all, so every label in the file read as
    GitHub-hosted while the work itself ran on `ubicloud-standard-8`.
    """
    for job in jobs_of(path.name, load(path)):
        called = _called_workflow(job)
        if called is None:
            continue
        target = WORKFLOW_DIR / called
        assert target.is_file(), f"{job} calls {called!r}, which does not exist"
        for inner in jobs_of(target.name, load(target)):
            requested = _ubicloud_labels_on_schedule(inner)
            assert not requested, (
                f"{job} is reached by a schedule and calls {inner}, which "
                f"requests {', '.join(requested)}. The calling workflow's own "
                "labels do not show this; the cost is real regardless."
            )

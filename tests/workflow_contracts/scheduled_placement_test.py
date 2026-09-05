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
from pathlib import PurePosixPath

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
    from collections.abc import Callable, Iterator
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
    """Return the local workflow file a job calls, if it calls one.

    A `uses:` reference is a POSIX path with an optional `@ref` suffix, so it
    is parsed as one rather than sliced. `PurePosixPath` is deliberate: the
    separator in a workflow reference is a forward slash on every platform,
    including the Windows runners that read these same files.
    """
    uses = job.body.get("uses")
    if not isinstance(uses, str) or not uses.startswith(LOCAL_CALL_PREFIX):
        return None
    reference = PurePosixPath(uses.partition("@")[0])
    return reference.name


def _reachable_jobs(
    path: Path,
    directory: Path | None = None,
    seen: set[str] | None = None,
    follow: Callable[[Job], bool] | None = None,
) -> Iterator[Job]:
    """Yield every job a workflow can reach, following local calls.

    A caller can call a caller. Checking only one hop would let a nested
    workflow keep an unexamined Ubicloud job, which is the same blind spot one
    level down that this module exists to close.

    Parameters
    ----------
    path
        Workflow file to walk.
    directory
        Directory in which to resolve a local call. Defaults to the
        repository's workflow directory; the parameter exists so a test can
        build a call chain in a temporary tree.
    seen
        File names already walked, so a cycle terminates rather than recursing
        forever. A workflow calling itself is invalid to GitHub, but a
        contract that hangs on bad input is worse than one that reports it.
    follow
        Decides whether a calling job's target is walked at all. A caller that
        cannot run dispatches nothing, so its target's jobs are never reached;
        filtering them after the walk would be too late, because an
        unconditional job in that target would be reported against a call that
        never happens.

    Yields
    ------
    Job
        Every job in the workflow and in every local workflow it reaches
        through a caller that `follow` accepts.
    """
    directory = WORKFLOW_DIR if directory is None else directory
    seen = set() if seen is None else seen
    if path.name in seen:
        return
    seen.add(path.name)
    for job in jobs_of(path.name, load(path)):
        yield job
        called = _called_workflow(job)
        if called is None:
            continue
        if follow is not None and not follow(job):
            continue
        target = directory / called
        assert target.is_file(), (
            f"{job} calls {called!r}, which is not a workflow in this repository"
        )
        yield from _reachable_jobs(target, directory, seen, follow)


def _runs_on_schedule(job: Job) -> bool:
    """Report whether a job can run when a schedule triggered the workflow.

    A guard such as ``github.event_name == 'push' || (github.event_name ==
    'pull_request' && ...)`` names the events the job accepts, and a scheduled
    call is not among them, so the job costs nothing in that context and
    reporting it would be a false violation.

    The reading is deliberately narrow: a condition that mentions
    `github.event_name` at all, and never mentions `schedule`, cannot run on a
    schedule. Any other condition is treated as runnable, so an expression this
    cannot follow errs towards reporting a cost rather than hiding one.
    """
    condition = " ".join(str(job.body.get("if", "")).split())
    if "github.event_name" not in condition:
        return True
    return "schedule" in condition


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
    """Follow local reusable-workflow calls and apply the same rule.

    This is the assertion that would have caught Axinite's staging suite. Its
    calling jobs declared no runner at all, so every label in the file read as
    GitHub-hosted while the work itself ran on `ubicloud-standard-8`.

    The walk is transitive, because a caller can call a caller, and it skips
    any job whose own guard excludes a scheduled event: such a job is not
    dispatched in this context and costs nothing, so reporting it would make
    the contract impossible to satisfy for reasons that are not real.
    """
    for job in _reachable_jobs(path, follow=_runs_on_schedule):
        if job.workflow == path.name:
            continue
        if not _runs_on_schedule(job):
            continue
        requested = _ubicloud_labels_on_schedule(job)
        assert not requested, (
            f"{path.name} is scheduled and reaches {job}, which requests "
            f"{', '.join(requested)} and has no guard excluding a scheduled "
            "call. The calling workflow's own labels do not show this; the "
            "cost is real regardless."
        )


def _write(directory: Path, name: str, body: str) -> Path:
    """Write a workflow fixture and return its path."""
    path = directory / name
    path.write_text(body, encoding="utf-8")
    return path


class TestReachableJobs:
    """The walk has to survive nesting, cycles and event guards."""

    def test_it_follows_a_call_through_two_hops(self, tmp_path: Path) -> None:
        """A caller can call a caller, and the cost is real at any depth."""
        _write(
            tmp_path,
            "outer.yml",
            "jobs:\n  call:\n    uses: ./.github/workflows/middle.yml\n",
        )
        _write(
            tmp_path,
            "middle.yml",
            "jobs:\n  call:\n    uses: ./.github/workflows/inner.yml@main\n",
        )
        _write(
            tmp_path,
            "inner.yml",
            "jobs:\n  build:\n    runs-on: ubicloud-standard-8\n",
        )
        found = list(_reachable_jobs(tmp_path / "outer.yml", tmp_path))
        assert [str(job) for job in found] == [
            "outer.yml:call",
            "middle.yml:call",
            "inner.yml:build",
        ]
        deepest = found[-1]
        assert _ubicloud_labels_on_schedule(deepest) == ("ubicloud-standard-8",)

    def test_it_does_not_follow_a_caller_a_schedule_cannot_dispatch(
        self, tmp_path: Path
    ) -> None:
        """A call that never happens cannot cost anything.

        Filtering the yielded jobs instead of the call would report the
        target's unconditional Ubicloud job against a caller that a schedule
        never dispatches.
        """
        _write(
            tmp_path,
            "outer.yml",
            "jobs:\n"
            "  call:\n"
            "    if: github.event_name == 'push'\n"
            "    uses: ./.github/workflows/inner.yml\n",
        )
        _write(
            tmp_path,
            "inner.yml",
            "jobs:\n  build:\n    runs-on: ubicloud-standard-8\n",
        )
        walked = list(
            _reachable_jobs(tmp_path / "outer.yml", tmp_path, follow=_runs_on_schedule)
        )
        assert [str(job) for job in walked] == ["outer.yml:call"]
        # Without the guard the same walk reaches the paid job, which is the
        # report this change exists to prevent.
        unguarded = list(_reachable_jobs(tmp_path / "outer.yml", tmp_path))
        assert [str(job) for job in unguarded] == [
            "outer.yml:call",
            "inner.yml:build",
        ]

    def test_a_cycle_terminates(self, tmp_path: Path) -> None:
        """A malformed pair of workflows must fail the run, not hang it."""
        _write(
            tmp_path,
            "a.yml",
            "jobs:\n  call:\n    uses: ./.github/workflows/b.yml\n",
        )
        _write(
            tmp_path,
            "b.yml",
            "jobs:\n  call:\n    uses: ./.github/workflows/a.yml\n",
        )
        found = [str(job) for job in _reachable_jobs(tmp_path / "a.yml", tmp_path)]
        assert found == ["a.yml:call", "b.yml:call"]


class TestRunsOnSchedule:
    """A job a schedule cannot dispatch costs nothing and is not a violation."""

    @pytest.mark.parametrize(
        ("condition", "runnable"),
        [
            (None, True),
            ("always()", True),
            ("github.event_name == 'schedule'", True),
            ("github.event_name == 'push' || github.event_name == 'schedule'", True),
            ("github.event_name == 'push'", False),
            (
                "github.event_name == 'push' || (github.event_name == "
                "'pull_request' && github.base_ref != 'staging')",
                False,
            ),
        ],
        ids=[
            "no-condition",
            "always",
            "schedule-only",
            "schedule-among-others",
            "push-only",
            "push-or-pull-request",
        ],
    )
    def test_it_reads_the_event_guard(
        self, condition: str | None, *, runnable: bool
    ) -> None:
        """Only a guard that names events and omits schedule excludes one.

        Anything this cannot follow is treated as runnable, so an unfamiliar
        expression errs towards reporting a cost rather than hiding one.
        """
        body: dict[str, object] = {"runs-on": "ubicloud-standard-8"}
        if condition is not None:
            body["if"] = condition
        assert _runs_on_schedule(Job("test.yml", "example", body)) is runnable

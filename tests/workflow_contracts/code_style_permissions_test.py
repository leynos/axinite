"""Contract tests for the code-style workflow's token scopes.

Without a workflow-level ``permissions`` block every job inherits the
repository or organization default ``GITHUB_TOKEN`` scopes, which may be
write-capable. These jobs read the checkout and write nothing, and they
run third-party actions and repository commands, so the token they are
handed is worth pinning.

The block is asserted rather than merely written, because its absence is
invisible: a workflow with no ``permissions`` key behaves exactly like
one with a permissive default, and the diff that deletes it looks like
housekeeping. The assertion is by equality at both scopes, so widening
the grant fails as loudly as deleting it.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import typing as typ
from pathlib import Path

from _workflow_files import declared_jobs, load
from _workflow_policy import WORKFLOW_DIR

WORKFLOW_PATH: typ.Final[Path] = WORKFLOW_DIR / "code_style.yml"

#: The only scope these jobs need. Read by equality: a grant this
#: workflow does not need is the finding, and a subset test would pass
#: over one.
REQUIRED_PERMISSIONS: typ.Final[dict[str, str]] = {"contents": "read"}


def test_the_workflow_grants_only_read_access_to_contents() -> None:
    """Assert the workflow-level block exists and is exactly least privilege.

    The failure this guards is the block being absent, which is not a
    smaller grant but an unbounded one: the jobs then take whatever the
    repository or organization default is.
    """
    workflow = load(WORKFLOW_PATH)
    permissions = workflow.get("permissions")
    assert permissions == REQUIRED_PERMISSIONS, (
        f"{WORKFLOW_PATH.name} must declare permissions "
        f"{REQUIRED_PERMISSIONS} at workflow scope, got {permissions!r}; "
        f"with no block each job inherits the repository or organization "
        f"default GITHUB_TOKEN scopes, which may be write-capable"
    )


def test_no_job_widens_the_workflow_grant() -> None:
    """Assert no job declares scopes of its own.

    A job-level block replaces the workflow-level one rather than
    narrowing it, so a job that declares its own takes whatever it asks
    for and the workflow-level assertion above says nothing about it.
    """
    jobs = declared_jobs(WORKFLOW_PATH)
    declaring = sorted(
        name
        for name, job in jobs.items()
        if isinstance(job, dict) and "permissions" in job
    )
    assert not declaring, (
        f"these {WORKFLOW_PATH.name} jobs declare their own permissions, "
        f"which replaces the workflow-level grant rather than narrowing it: "
        f"{declaring}"
    )


def test_the_sweep_finds_the_jobs_it_is_about() -> None:
    """Assert the job reading is not empty and names a job it must find.

    The assertion above is satisfied by a reading that returns no jobs,
    so the reading itself is pinned.
    """
    jobs = declared_jobs(WORKFLOW_PATH)
    assert jobs, f"no job was read from {WORKFLOW_PATH.name}"
    assert "clippy" in jobs, (
        f"the reading missed {WORKFLOW_PATH.name}'s clippy job, so the "
        f"permissions assertion above passed over a job that runs a "
        f"third-party action against the checkout"
    )

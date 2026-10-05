"""Contract for the token that `cargo binstall` steps are handed.

`cargo binstall` resolves releases through api.github.com. Anonymous requests
share a per-runner-IP rate limit, so an unlucky run gets a 403, waits 120
seconds and compiles from source, and a warm cache hides it. Each such step
therefore receives the workflow token, at step scope only.

Handing a token to a step is only as safe as the token. The repository default
for `GITHUB_TOKEN` is write-capable, so a workflow with no `permissions` block
widens exactly what this change introduces. Three properties are asserted, each
on its own so that removing one cannot hide behind another:

- every `cargo binstall` step in the three workflows carries
  `GITHUB_TOKEN: ${{ github.token }}`, and none carries it at job or workflow
  scope, where it would reach steps that do not need it;
- `test.yml`, which had no `permissions` block, is pinned to `contents: read`
  and no job of its own widens it;
- the one workflow that calls `test.yml` grants at least that, because a
  reusable workflow cannot raise the token its caller gives it.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import re
import typing as typ

import pytest
from _workflow_files import declared_jobs, load
from _workflow_policy import WORKFLOW_DIR

WORKFLOWS: typ.Final[tuple[str, ...]] = (
    "test.yml",
    "coverage.yml",
    "codescene-coverage.yml",
)
TOKEN_KEY: typ.Final[str] = "GITHUB_TOKEN"
TOKEN_VALUE: typ.Final[str] = "${{ github.token }}"
#: A `cargo binstall <crate>` invocation, not a mention or a version probe.
BINSTALL: typ.Final[re.Pattern[str]] = re.compile(
    r"(?:^|[;&|(]|\bthen\b|\bdo\b)\s*cargo\s+binstall\s+(?!-V\b|--version\b)\S",
    re.MULTILINE,
)
READ_ONLY: typ.Final[dict[str, str]] = {"contents": "read"}


def _binstall_steps(name: str) -> list[tuple[str, dict[str, typ.Any]]]:
    """Return (job id, step) for every `cargo binstall` step in a workflow."""
    jobs = declared_jobs(WORKFLOW_DIR / name)
    return [
        (job_id, step)
        for job_id, job in jobs.items()
        if isinstance(job, dict)
        for step in job.get("steps", [])
        if isinstance(step, dict) and BINSTALL.search(str(step.get("run") or ""))
    ]


@pytest.mark.parametrize("name", WORKFLOWS)
def test_the_workflow_still_runs_cargo_binstall(name: str) -> None:
    """The token assertions below have steps to judge.

    A guard over a filtered list passes when the list is empty, so deleting the
    install steps would leave every assertion green.
    """
    assert _binstall_steps(name), f"{name} must run cargo binstall"


@pytest.mark.parametrize("name", WORKFLOWS)
def test_every_binstall_step_carries_the_workflow_token(name: str) -> None:
    """Each `cargo binstall` step gets the workflow token at step scope."""
    for job_id, step in _binstall_steps(name):
        env = step.get("env") or {}
        assert env.get(TOKEN_KEY) == TOKEN_VALUE, (
            f"{name}:{job_id}: step {step.get('name')!r} runs cargo binstall "
            f"with {TOKEN_KEY}={env.get(TOKEN_KEY)!r}; it must be "
            f"{TOKEN_VALUE!r}, or an unlucky run waits 120 s on a 403 and "
            "compiles from source"
        )


@pytest.mark.parametrize("name", WORKFLOWS)
def test_the_token_is_never_set_above_step_scope(name: str) -> None:
    """No workflow or job declares the token, so it reaches only its steps."""
    document = load(WORKFLOW_DIR / name)
    workflow_env = document.get("env") or {}
    assert TOKEN_KEY not in workflow_env, f"{name} sets {TOKEN_KEY} workflow-wide"
    for job_id, job in declared_jobs(WORKFLOW_DIR / name).items():
        job_env = (job.get("env") or {}) if isinstance(job, dict) else {}
        assert TOKEN_KEY not in job_env, f"{name}:{job_id} sets {TOKEN_KEY} job-wide"


def test_test_yml_pins_its_token_to_read_access() -> None:
    """`test.yml` declares exactly `contents: read` at workflow scope.

    Asserted by equality: a grant this workflow does not need is the finding,
    and a subset test would pass over one. With no block each job takes the
    repository default, which is write-capable.
    """
    permissions = load(WORKFLOW_DIR / "test.yml").get("permissions")
    assert permissions == READ_ONLY, (
        f"test.yml must declare permissions {READ_ONLY} at workflow scope, got "
        f"{permissions!r}; with no block each job inherits the repository "
        "default GITHUB_TOKEN scopes, which are write-capable"
    )


def test_no_test_yml_job_widens_the_grant() -> None:
    """No job of `test.yml` declares scopes of its own.

    A job-level block replaces the workflow-level one rather than narrowing
    it, so the workflow-level assertion says nothing about a job that has one.
    """
    declaring = sorted(
        job_id
        for job_id, job in declared_jobs(WORKFLOW_DIR / "test.yml").items()
        if isinstance(job, dict) and "permissions" in job
    )
    assert not declaring, f"test.yml jobs declaring their own permissions: {declaring}"


def test_the_caller_of_test_yml_grants_what_it_needs() -> None:
    """Each job that calls `test.yml` grants at least `contents: read`.

    The caller workflow defaults to no scopes, and a reusable workflow cannot
    raise the token it is given, so a caller that grants nothing would fail to
    start once `test.yml` asks for read access.
    """
    callers = {
        job_id: job
        for job_id, job in declared_jobs(WORKFLOW_DIR / "mutation-testing.yml").items()
        if isinstance(job, dict) and job.get("uses") == "./.github/workflows/test.yml"
    }
    assert callers, "mutation-testing.yml must still call test.yml"
    for job_id, job in callers.items():
        granted = job.get("permissions") or {}
        if granted in ("read-all", "write-all"):
            # GitHub's scalar forms; both include `contents: read`.
            continue
        assert isinstance(granted, dict), (
            f"mutation-testing.yml:{job_id} has unsupported permissions "
            f"{granted!r}; use a mapping or read-all/write-all"
        )
        assert granted.get("contents") in ("read", "write"), (
            f"mutation-testing.yml:{job_id} calls test.yml but grants "
            f"{granted!r}; test.yml needs contents: read"
        )

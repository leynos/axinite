"""Contract tests for the CI runner-selection policy.

Axinite splits its CI estate across two runner pools. Compile-bound jobs stay
on the paid `ubicloud-standard-8` pool, where the extra vCPUs pay for
themselves against a full workspace build. Glue jobs — labelling scripts, the
regression-test check, the Claude review, and the scheduled dependency audit —
run on GitHub-hosted `ubuntu-latest`, which is free for this public repository
and fast enough for single-threaded script and API-bound work.

That split is a policy, not an accident, so it is pinned here. The tests fail
when a job's runner drifts, when a new job appears without a recorded runner,
and when a job placed on the free pool starts compiling the workspace (which
would make the free pool the wrong choice for it).

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import re
import typing as typ
from pathlib import Path

import pytest
import yaml

WORKFLOW_DIR = Path(__file__).resolve().parents[2] / ".github" / "workflows"

UBICLOUD = "ubicloud-standard-8"
GITHUB_HOSTED_LINUX = "ubuntu-latest"

#: Every job in the repository, with the runner it is expected to request.
#: A job that calls a reusable workflow declares no runner of its own and is
#: recorded as ``None``. Adding or moving a job must be a deliberate edit here.
RUNNER_POLICY: typ.Final[dict[str, dict[str, str | None]]] = {
    "audit.yml": {"audit": GITHUB_HOSTED_LINUX},
    "claude-review.yml": {"review": GITHUB_HOSTED_LINUX},
    "code_style.yml": {
        "format": UBICLOUD,
        "clippy": UBICLOUD,
        "clippy-windows": "windows-latest",
        "code-style": UBICLOUD,
    },
    "codescene-coverage.yml": {"coverage-check": UBICLOUD},
    "coverage.yml": {
        "coverage": UBICLOUD,
        "e2e-coverage": UBICLOUD,
        "coverage-gate": UBICLOUD,
    },
    "dependabot-automerge.yml": {"automerge": None},
    "e2e.yml": {"build": UBICLOUD, "test": UBICLOUD, "e2e": UBICLOUD},
    "mutation-testing.yml": {"mutation": None},
    "pr-label-classify.yml": {"classify": GITHUB_HOSTED_LINUX},
    "pr-label-scope.yml": {"scope": GITHUB_HOSTED_LINUX},
    "regression-test-check.yml": {"regression-test": GITHUB_HOSTED_LINUX},
    "release-plz.yml": {"release-plz-release": UBICLOUD, "release-plz-pr": UBICLOUD},
    "release.yml": {
        "plan": "ubuntu-22.04",
        "build-local-artifacts": "${{ matrix.runner }}",
        "build-global-artifacts": "ubuntu-22.04",
        "build-wasm-extensions": "ubuntu-22.04",
        "host": "ubuntu-22.04",
        "update-registry-checksums": "ubuntu-22.04",
        "announce": "ubuntu-22.04",
    },
    "staging-ci.yml": {
        "check-changes": UBICLOUD,
        "tests": None,
        "e2e": None,
        "create-promotion-pr": UBICLOUD,
        "gate": UBICLOUD,
        "update-tag": UBICLOUD,
        "report": UBICLOUD,
    },
    "test.yml": {
        "audit": UBICLOUD,
        "tests": UBICLOUD,
        "telegram-tests": UBICLOUD,
        "windows-build": "windows-latest",
        "wasm-wit-compat": UBICLOUD,
        "docker-build": UBICLOUD,
        "version-check": UBICLOUD,
        "run-tests": UBICLOUD,
    },
}

#: The jobs migrated off the paid pool. These must stay free-runner eligible:
#: no workspace compilation, no Rust build cache.
FREE_RUNNER_JOBS: typ.Final[tuple[tuple[str, str], ...]] = (
    ("audit.yml", "audit"),
    ("claude-review.yml", "review"),
    ("pr-label-classify.yml", "classify"),
    ("pr-label-scope.yml", "scope"),
    ("regression-test-check.yml", "regression-test"),
)

#: Workflows whose jobs compile the workspace and therefore stay on Ubicloud.
COMPILE_BOUND_WORKFLOWS: typ.Final[tuple[str, ...]] = (
    "code_style.yml",
    "codescene-coverage.yml",
    "coverage.yml",
    "e2e.yml",
    "staging-ci.yml",
    "test.yml",
)

#: Commands that compile the workspace. `make audit` and `cargo audit` only
#: read the lockfile, so `audit` is deliberately absent from this pattern.
COMPILE_COMMAND_RE: typ.Final[re.Pattern[str]] = re.compile(
    r"""
    \bcargo(?:\s+\+\S+)?\s+
        (?:build|test|nextest|check|clippy|bench|component|llvm-cov|mutants|doc)\b
  | \bmake\s+
        (?:all|build[\w-]*|test[\w-]*|lint[\w-]*|check-fmt|typecheck|install)\b
  | \./scripts/build-wasm-extensions\.sh
    """,
    re.VERBOSE,
)

#: Actions that only make sense for a compiling job.
COMPILE_ACTION_PREFIXES: typ.Final[tuple[str, ...]] = (
    "Swatinem/rust-cache",
    "taiki-e/install-action@cargo-llvm-cov",
    "taiki-e/install-action@cargo-nextest",
)

SHA_RE: typ.Final[re.Pattern[str]] = re.compile(r"^[0-9a-f]{40}$")


def _load(name: str) -> dict[str, object]:
    """Parse a workflow file into a mapping."""
    workflow = yaml.safe_load((WORKFLOW_DIR / name).read_text(encoding="utf-8"))
    assert isinstance(workflow, dict), f"{name} must parse as a mapping"
    return workflow


def _jobs(name: str) -> dict[str, dict[str, object]]:
    """Return a workflow's job mappings, keyed by job identifier."""
    jobs = _load(name).get("jobs")
    assert isinstance(jobs, dict), f"{name} must declare a jobs mapping"
    assert all(isinstance(job, dict) for job in jobs.values()), (
        f"every job in {name} must be a mapping"
    )
    return typ.cast("dict[str, dict[str, object]]", jobs)


def _job(name: str, job_id: str) -> dict[str, object]:
    """Return a single job mapping."""
    jobs = _jobs(name)
    assert job_id in jobs, f"{name} must declare a {job_id!r} job"
    return jobs[job_id]


def _triggers(workflow: dict[str, object]) -> object:
    """Return a workflow's ``on:`` block.

    PyYAML resolves an unquoted ``on`` key to the boolean ``True`` under the
    YAML 1.1 rules it implements, so both spellings must be accepted.
    """
    if "on" in workflow:
        return workflow["on"]
    return workflow.get(True)


def _steps(job: dict[str, object]) -> list[dict[str, object]]:
    """Return a job's ordered step mappings."""
    steps = job.get("steps")
    assert isinstance(steps, list), "the job must declare steps"
    assert all(isinstance(step, dict) for step in steps), (
        "every step must be a mapping"
    )
    return [step for step in steps if isinstance(step, dict)]


def _step_identities(job: dict[str, object]) -> list[str]:
    """Return each step's name, falling back to the action it invokes."""
    return [str(step.get("name", step.get("uses"))) for step in _steps(job)]


def _workflow_files() -> list[str]:
    """Return every workflow file name, sorted."""
    return sorted(path.name for path in WORKFLOW_DIR.glob("*.yml"))


def test_workflow_inventory_matches_the_recorded_policy() -> None:
    """Every workflow file is accounted for in the runner policy."""
    assert _workflow_files() == sorted(RUNNER_POLICY), (
        "a workflow was added or removed without updating RUNNER_POLICY"
    )


@pytest.mark.parametrize("name", sorted(RUNNER_POLICY))
def test_each_workflow_requests_its_recorded_runners(name: str) -> None:
    """Each job requests exactly the runner the policy records for it."""
    actual = {job_id: job.get("runs-on") for job_id, job in _jobs(name).items()}
    assert actual == RUNNER_POLICY[name], (
        f"{name} runner assignments drifted from the recorded policy; "
        "update RUNNER_POLICY only when the change is deliberate"
    )


@pytest.mark.parametrize("name", sorted(RUNNER_POLICY))
def test_jobs_without_a_runner_call_a_pinned_reusable_workflow(name: str) -> None:
    """A job may omit ``runs-on`` only when it delegates to another workflow.

    A local caller inherits the callee's runners, which the policy already
    covers. A remote caller must be pinned to a commit SHA; the value itself
    is Dependabot's to bump, so only its shape is asserted.
    """
    for job_id, job in _jobs(name).items():
        if job.get("runs-on") is not None:
            continue
        uses = job.get("uses")
        assert isinstance(uses, str), (
            f"{name}:{job_id} declares no runner, so it must call a reusable "
            "workflow via `uses`"
        )
        if uses.startswith("./"):
            callee = uses.removeprefix("./.github/workflows/")
            assert callee in RUNNER_POLICY, (
                f"{name}:{job_id} calls {uses!r}, which is not in RUNNER_POLICY"
            )
            continue
        assert SHA_RE.match(uses.split("@")[-1]), (
            f"{name}:{job_id} must pin its reusable workflow to a full commit SHA"
        )


@pytest.mark.parametrize(("name", "job_id"), FREE_RUNNER_JOBS)
def test_migrated_jobs_use_the_github_hosted_runner(name: str, job_id: str) -> None:
    """The migrated glue jobs run on the free GitHub-hosted pool."""
    assert _job(name, job_id).get("runs-on") == GITHUB_HOSTED_LINUX, (
        f"{name}:{job_id} must stay on {GITHUB_HOSTED_LINUX}; it does no "
        "workspace compilation, so the paid pool buys nothing"
    )


@pytest.mark.parametrize(("name", "job_id"), FREE_RUNNER_JOBS)
def test_migrated_jobs_do_not_compile_the_workspace(name: str, job_id: str) -> None:
    """No free-runner job runs a compile command or a Rust build cache."""
    for step in _steps(_job(name, job_id)):
        command = step.get("run")
        if isinstance(command, str):
            match = COMPILE_COMMAND_RE.search(command)
            assert match is None, (
                f"{name}:{job_id} runs {match.group(0)!r}, which compiles the "
                f"workspace; move the job back to {UBICLOUD} or drop the step"
            )
        uses = str(step.get("uses", ""))
        assert not uses.startswith(COMPILE_ACTION_PREFIXES), (
            f"{name}:{job_id} uses {uses!r}, which only pays off for a "
            f"compiling job; move the job back to {UBICLOUD}"
        )


@pytest.mark.parametrize("name", COMPILE_BOUND_WORKFLOWS)
def test_compile_bound_workflows_stay_off_the_free_linux_pool(name: str) -> None:
    """Compile-bound Linux jobs keep the paid runner."""
    for job_id, job in _jobs(name).items():
        runner = job.get("runs-on")
        assert runner != GITHUB_HOSTED_LINUX, (
            f"{name}:{job_id} compiles the workspace, so it must not move to "
            f"{GITHUB_HOSTED_LINUX}"
        )


def test_audit_job_reads_the_lockfile_without_building() -> None:
    """The scheduled audit installs a prebuilt binary and audits only."""
    workflow = _load("audit.yml")
    assert _triggers(workflow) == {
        "schedule": [{"cron": "33 7 * * 1"}],
        "workflow_dispatch": None,
    }, "the audit must remain a weekly scheduled run with a manual trigger"
    assert workflow.get("permissions") == {"contents": "read"}, (
        "the audit needs only read access to contents"
    )

    job = _job("audit.yml", "audit")
    assert _step_identities(job) == [
        "Checkout repository",
        "Install Rust",
        "Install cargo-audit",
        "Audit dependencies",
    ], "the audit job's steps must stay unchanged by the runner move"

    installer = _steps(job)[2]
    assert str(installer.get("uses", "")).startswith("taiki-e/install-action@"), (
        "cargo-audit must arrive as a prebuilt binary, not a source build"
    )
    assert installer.get("with") == {"tool": "cargo-audit"}, (
        "the installer must fetch cargo-audit"
    )
    assert _steps(job)[3].get("run") == "make audit", (
        "the audit step must read the lockfile via `make audit`"
    )


def test_claude_review_stays_label_gated_and_build_free() -> None:
    """The review job keeps its label guard and its no-build instruction."""
    workflow = _load("claude-review.yml")
    assert _triggers(workflow) == {"pull_request": {"types": ["labeled"]}}, (
        "the review must trigger only when a pull request is labelled"
    )
    assert workflow.get("permissions") == {
        "contents": "read",
        "pull-requests": "write",
        "issues": "write",
        "id-token": "write",
    }, "the review's permissions must survive the runner move"

    job = _job("claude-review.yml", "review")
    assert job.get("if") == (
        "contains(github.event.pull_request.labels.*.name, 'staging-promotion')"
    ), "the review must stay gated on the staging-promotion label"

    action = _steps(job)[1]
    assert str(action.get("uses", "")).startswith("anthropics/claude-code-action@"), (
        "the review must run through the Claude Code action"
    )
    prompt = action.get("with", {}).get("prompt")
    assert isinstance(prompt, str), "the review action must supply a prompt"
    assert "Do NOT check build signal or attempt to build/test the code" in prompt, (
        "the review prompt must keep instructing the agent not to build, since "
        "the free runner is not provisioned for a workspace compile"
    )


def test_label_workflows_run_scripts_without_a_toolchain() -> None:
    """Both labelling workflows remain checkout-plus-script jobs."""
    classify = _load("pr-label-classify.yml")
    scope = _load("pr-label-scope.yml")
    expected_trigger = {
        "pull_request_target": {"types": ["opened", "synchronize", "reopened"]}
    }
    assert _triggers(classify) == expected_trigger, (
        "classify must keep its pull_request_target trigger"
    )
    assert _triggers(scope) == expected_trigger, (
        "scope must keep its pull_request_target trigger"
    )
    assert classify.get("permissions") == {
        "contents": "read",
        "pull-requests": "write",
        "issues": "read",
    }, "classify needs issues read access for the contributor-count search"
    assert scope.get("permissions") == {
        "contents": "read",
        "pull-requests": "write",
    }, "scope needs only pull-request write access"

    classify_job = _job("pr-label-classify.yml", "classify")
    assert _step_identities(classify_job) == ["Checkout base branch", "Classify PR"], (
        "classify must stay a checkout plus a single script step"
    )
    assert _steps(classify_job)[1].get("run") == "bash .github/scripts/pr-labeler.sh", (
        "classify must invoke the labeller script directly"
    )

    scope_job = _job("pr-label-scope.yml", "scope")
    scope_steps = _steps(scope_job)
    assert len(scope_steps) == 1, "scope must remain a single labeller step"
    assert str(scope_steps[0].get("uses", "")).startswith("actions/labeler@"), (
        "scope must delegate to the upstream labelling action"
    )
    assert scope_steps[0].get("with") == {
        "configuration-path": ".github/labeler.yml",
        "sync-labels": False,
    }, "scope must stay additive against the checked-in labeller configuration"


def test_regression_check_is_a_git_and_grep_job() -> None:
    """The regression check needs full history but no toolchain."""
    workflow = _load("regression-test-check.yml")
    assert _triggers(workflow) == {"pull_request": None}, (
        "the regression check must run on every pull request"
    )

    job = _job("regression-test-check.yml", "regression-test")
    assert _step_identities(job) == [
        "Checkout repository",
        "Check for regression tests",
    ], "the regression check must stay a checkout plus a single script step"

    checkout = _steps(job)[0]
    assert str(checkout.get("uses", "")).startswith("actions/checkout@")
    assert checkout.get("with") == {"fetch-depth": 0}, (
        "the check diffs against the base ref, so it needs full history"
    )

    script = _steps(job)[1].get("run")
    assert isinstance(script, str), "the check must declare a script"
    assert "git diff" in script and "git log" in script, (
        "the check must remain a git-history inspection, not a build"
    )

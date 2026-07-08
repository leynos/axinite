"""Contract tests for the mutation-testing caller workflow.

The executable logic lives in the ``leynos/shared-actions`` reusable
workflow, which carries its own unit and integration tests; axinite's
caller is declarative configuration. These tests parse the caller with
PyYAML and pin the contract it must uphold, so drift (repointing the pin
at a branch, widening permissions, or losing the root-crate scoping that
keeps the vendored grammers-crypto patch crate and the out-of-workspace
tools-src/ and channels-src/ crates out of mutation) fails CI on the
pull request rather than surfacing in a scheduled or manual run.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

from pathlib import Path

import pytest
import yaml

WORKFLOW_PATH = (
    Path(__file__).resolve().parents[2] / ".github" / "workflows" / "mutation-testing.yml"
)

pytestmark = pytest.mark.skipif(
    not WORKFLOW_PATH.exists(),
    reason="workflow file not present in this working copy (e.g. "
    "inside a mutation-testing sandbox that does not copy .github/)",
)

#: The pinned commit of leynos/shared-actions carrying the
#: setup-commands input, the python-version fail-fast guard, and
#: step-level timeout artefact preservation. Bump the caller and this
#: test together.
PINNED_SHA = "859416a90eb3987b46a57682c5d6b8964ad3f0a6"

EXPECTED_USES = (
    "leynos/shared-actions/.github/workflows/mutation-cargo.yml@" + PINNED_SHA
)

#: Test-support scaffolding compiled into non-test builds; mutants
#: there survive as noise. src/tools/builder/testing.rs is production
#: code and must NOT appear here.
SCAFFOLDING_EXCLUDES = (
    "src/testing/**",
    "src/testing_wasm.rs",
    "src/skills/test_support.rs",
    "src/channels/web/test_helpers.rs",
)

#: Commands the setup block must run so mutants see the same
#: environment as CI's test job (test.yml).
REQUIRED_SETUP_FRAGMENTS = (
    "apt-get install -y clang mold",
    "rustup target add wasm32-wasip2",
    "cargo binstall --no-confirm cargo-component cargo-nextest",
    "make build-github-tool-wasm",
    "./scripts/build-wasm-extensions.sh --channels",
)


def _load() -> dict[str, object]:
    """Parse the workflow file."""
    return yaml.safe_load(WORKFLOW_PATH.read_text(encoding="utf-8"))


def _triggers(workflow: dict[str, object]) -> dict[str, object]:
    """Return the ``on:`` mapping (PyYAML parses a bare key as True)."""
    triggers = workflow.get("on", workflow.get(True))
    assert isinstance(triggers, dict), "the workflow must declare an on: mapping"
    return triggers


def _mutation_job(workflow: dict[str, object]) -> dict[str, object]:
    """Return the single calling job."""
    jobs = workflow.get("jobs")
    assert isinstance(jobs, dict), "the workflow must declare a jobs mapping"
    assert jobs, "the workflow must declare at least one job"
    assert list(jobs) == ["mutation"], (
        f"expected a single job named 'mutation', found {sorted(jobs)}"
    )
    return jobs["mutation"]


def test_uses_reference_is_pinned_to_the_documented_sha() -> None:
    """The job must call the shared workflow at the exact documented SHA."""
    uses = _mutation_job(_load()).get("uses")
    assert uses is not None, "jobs.mutation.uses is missing"
    path, _, ref = uses.partition("@")
    assert path == "leynos/shared-actions/.github/workflows/mutation-cargo.yml", (
        f"jobs.mutation.uses must reference mutation-cargo.yml, got {path!r}"
    )
    assert len(ref) == 40, (
        f"jobs.mutation.uses must pin a full 40-character commit SHA, "
        f"not a branch or tag: {ref!r}"
    )
    assert all(c in "0123456789abcdef" for c in ref), (
        f"jobs.mutation.uses must pin a lowercase hex commit SHA, "
        f"not a branch or tag: {ref!r}"
    )
    assert uses == EXPECTED_USES, (
        f"jobs.mutation.uses pins {ref!r}; this test documents {PINNED_SHA!r} — "
        "bump the test and the workflow together"
    )


def test_job_permissions_are_exactly_least_privilege() -> None:
    """The job grants contents: read and id-token: write, nothing broader."""
    permissions = _mutation_job(_load()).get("permissions")
    assert permissions == {"contents": "read", "id-token": "write"}, (
        "jobs.mutation.permissions must be exactly "
        f"{{'contents': 'read', 'id-token': 'write'}}, got {permissions!r}"
    )


def test_workflow_default_permissions_are_empty() -> None:
    """The workflow-level default token scope is empty."""
    workflow = _load()
    assert workflow.get("permissions") == {}, (
        f"top-level permissions must be an empty mapping, got "
        f"{workflow.get('permissions')!r}"
    )


def test_concurrency_serializes_per_ref_without_cancelling() -> None:
    """Runs queue per ref instead of cancelling one another."""
    concurrency = _load().get("concurrency")
    assert isinstance(concurrency, dict), "the workflow must declare concurrency"
    assert concurrency.get("group") == "mutation-testing-${{ github.ref }}", (
        f"concurrency.group must key on the triggering ref, got "
        f"{concurrency.get('group')!r}"
    )
    assert concurrency.get("cancel-in-progress") is False, (
        f"concurrency.cancel-in-progress must be false, got "
        f"{concurrency.get('cancel-in-progress')!r}"
    )


def test_triggers_keep_schedule_and_plain_dispatch() -> None:
    """The daily schedule stays; dispatch has no legacy branch input."""
    triggers = _triggers(_load())
    schedule = triggers.get("schedule")
    assert schedule == [{"cron": "50 10 * * *"}], (
        f"on.schedule must be the daily 10:50 UTC cron, got {schedule!r}"
    )
    assert "workflow_dispatch" in triggers, "on.workflow_dispatch is missing"
    dispatch = triggers.get("workflow_dispatch") or {}
    inputs = dispatch.get("inputs") or {}
    assert "branch" not in inputs, (
        "on.workflow_dispatch must not declare a branch input; the Actions "
        "run-workflow control selects the ref"
    )


def test_with_block_carries_the_caller_configuration() -> None:
    """The caller passes exactly the axinite-specific configuration."""
    with_block = _mutation_job(_load()).get("with")
    assert isinstance(with_block, dict), "jobs.mutation.with is missing"
    assert set(with_block) == {
        "paths",
        "exclude-globs",
        "extra-args",
        "shard-count",
        "setup-commands",
    }, (
        "jobs.mutation.with must set exactly paths, exclude-globs, "
        f"extra-args, shard-count and setup-commands, got {sorted(with_block)}"
    )
    assert with_block.get("paths") == "src/", (
        "with.paths must scope mutation to the root crate only, keeping "
        "the out-of-workspace tools-src/ and channels-src/ crates (and the "
        "vendored grammers-crypto patch) out of scope, got "
        f"{with_block.get('paths')!r}"
    )
    excludes = with_block.get("exclude-globs")
    assert isinstance(excludes, str), "with.exclude-globs is missing"
    assert sorted(g.strip() for g in excludes.split(",")) == sorted(
        SCAFFOLDING_EXCLUDES
    ), (
        f"with.exclude-globs must cover exactly {SCAFFOLDING_EXCLUDES}, "
        f"got {excludes!r}"
    )
    assert with_block.get("extra-args") == (
        "--features test-helpers --test-workspace=true "
        '--test-tool=nextest -- -E "not binary(schema_helpers_ui)"'
    ), (
        "with.extra-args must mirror the CI default test leg "
        "(--features test-helpers), run workspace tests against each "
        "mutant, use nextest as the test tool (the boot-screen snapshot "
        "test needs process-per-test isolation, issue #237), and drop "
        "the slow schema_helpers_ui trybuild binary from per-mutant "
        f"runs, got {with_block.get('extra-args')!r}"
    )
    assert with_block.get("shard-count") == 8, (
        f"with.shard-count must be 8 (heavy wasmtime/libsql build; keep "
        f"dispatch legs inside the step timeout), got "
        f"{with_block.get('shard-count')!r}"
    )
    setup = with_block.get("setup-commands")
    assert isinstance(setup, str), "with.setup-commands is missing"
    for fragment in REQUIRED_SETUP_FRAGMENTS:
        assert fragment in setup, (
            f"with.setup-commands must run {fragment!r} to mirror the CI "
            f"test environment, got {setup!r}"
        )

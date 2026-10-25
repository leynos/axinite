"""Contracts for how the workflows provision and exclude the build standard.

The standard is only as good as the jobs that install what it needs and the
jobs that opt out of it. These contracts derive both from what a job runs:

* every Linux job that compiles installs the pinned build tools first, with an
  unconditional, hard-failing step, whether it compiles through Cargo directly,
  through a Make target whose evaluated recipe reaches Cargo, through the WASM
  builder, through the coverage action, or through a reusable workflow's
  ``setup-commands``;
* coverage jobs use step-level ``RUSTFLAGS: ''``, which keeps the parallel
  frontend and Linux linker flag out of those builds;
* release jobs exclude it with ``RUSTFLAGS``;
* the Whitaker job installs the rolling suite through the shared action without
  pinning it, then runs ``make lint-whitaker``; no Whitaker-related workflow
  step assigns ``RUSTFLAGS``.

The Make half is evaluated rather than assumed. Each judge is also run against
mutated copies of the real workflows, and must reject every one.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import copy
import subprocess
import typing as typ

import pytest
import yaml

from _build_standard_make import dry_run, parse_invocations
from _build_standard_judge import judge_all
from _build_standard_workflow_mutations import TEXT_MUTATIONS, WORKFLOW_MUTATIONS
from _build_standard_workflows import (
    INSTALL_TARGET,
    compiles,
    make_targets_in,
    runs_on_linux,
    steps_of,
)
from _estate import read_workflows
from _workflow_policy import WORKFLOW_DIR

COMPILE_VERBS = {"build", "check", "clippy", "test", "nextest"}


@pytest.fixture(scope="module")
def documents() -> dict[str, dict[str, typ.Any]]:
    """Return every workflow, parsed, including the dist-generated one."""
    return {
        name: dict(document) for name, document in read_workflows(WORKFLOW_DIR).items()
    }


@pytest.fixture(scope="module")
def texts() -> dict[str, str]:
    """Return every workflow's source text."""
    return {
        path.name: path.read_text(encoding="utf-8")
        for path in sorted(WORKFLOW_DIR.glob("*.y*ml"))
    }


@pytest.fixture(scope="module")
def compiling_targets(documents: dict[str, dict[str, typ.Any]]) -> set[str]:
    """Return the Make targets workflows name whose recipes reach Cargo."""
    reached: set[str] = set()
    for target in make_targets_in(documents):
        try:
            lines = dry_run(target)
        except subprocess.CalledProcessError:
            continue
        invocations, _ = parse_invocations(lines)
        if any(invocation.verb in COMPILE_VERBS for invocation in invocations):
            reached.add(target)
    return reached


def _linux_compiling_jobs(
    documents: dict[str, dict[str, typ.Any]], targets: set[str]
) -> list[str]:
    """Return `workflow:job` for each Linux job with a compiling step."""
    found: list[str] = []
    for name, document in documents.items():
        for job_id, job in (document.get("jobs") or {}).items():
            if runs_on_linux(job) and any(
                compiles(step, targets) for step in steps_of(job)
            ):
                found.append(f"{name}:{job_id}")
    return found


def test_the_repository_conforms(
    documents: dict[str, dict[str, typ.Any]],
    texts: dict[str, str],
    compiling_targets: set[str],
) -> None:
    """No workflow violates the provisioning, exclusion or Whitaker contracts."""
    problems = judge_all(documents, compiling_targets, texts=texts)
    assert problems == []


def test_make_reachability_was_derived_not_assumed(compiling_targets: set[str]) -> None:
    """The Make half of the compile test saw the targets suites run through."""
    expected = {
        "test-workspace",
        "test-github-tool",
        "test-libsql-transport",
        "lint-libsql-transport",
        "build-github-tool-wasm",
    }
    assert expected <= compiling_targets, f"missing {expected - compiling_targets}"
    assert INSTALL_TARGET not in compiling_targets, (
        "installing the tools does not compile"
    )
    assert "check-fmt" not in compiling_targets


def test_the_set_of_judged_jobs_is_not_empty(
    documents: dict[str, dict[str, typ.Any]], compiling_targets: set[str]
) -> None:
    """A reader that found no compiling Linux job would pass vacuously."""
    judged = _linux_compiling_jobs(documents, compiling_targets)
    assert len(judged) >= 12, (
        f"expected the estate's compiling Linux jobs, found {judged}"
    )
    for expected in (
        "test.yml:tests",
        "test.yml:github-tool-tests",
        "coverage.yml:coverage",
        "e2e.yml:build",
        "mutation-testing.yml:mutation",
    ):
        assert expected in judged, (
            f"{expected} was not recognized as compiling on Linux"
        )


def test_runner_forms_are_all_read(documents: dict[str, dict[str, typ.Any]]) -> None:
    """Scalar, list, mapping and chained `runs-on` are all placed on Linux."""
    scalar = {"runs-on": "ubuntu-latest"}
    chain = {
        "runs-on": "${{ github.event_name == 'schedule' && 'ubuntu-latest' || 'ubicloud-standard-4' }}"
    }
    listed = {"runs-on": ["self-hosted", "linux"]}
    mapped = {"runs-on": {"group": "g", "labels": ["ubicloud-standard-2"]}}
    caller = {"uses": "org/repo/.github/workflows/w.yml@" + "0" * 40}
    windows = {"runs-on": "windows-latest"}
    macos = {"runs-on": "macos-14"}
    assert all(runs_on_linux(job) for job in (scalar, chain, listed, mapped, caller))
    assert not any(runs_on_linux(job) for job in (windows, macos))


@pytest.mark.parametrize(
    "name", sorted(WORKFLOW_MUTATIONS), ids=sorted(WORKFLOW_MUTATIONS)
)
def test_each_structural_mutation_is_detected(
    name: str,
    documents: dict[str, dict[str, typ.Any]],
    texts: dict[str, str],
    compiling_targets: set[str],
) -> None:
    """Breaking provisioning, exclusion or Whitaker wiring one way is reported."""
    mutate, fragment = WORKFLOW_MUTATIONS[name]
    mutated = copy.deepcopy(documents)
    mutate(mutated)
    problems = judge_all(mutated, compiling_targets, texts=texts)
    assert any(fragment in problem for problem in problems), (
        f"the mutation {name!r} was not reported as {fragment!r}: {problems}"
    )


@pytest.mark.parametrize("name", sorted(TEXT_MUTATIONS), ids=sorted(TEXT_MUTATIONS))
def test_each_retired_mechanism_is_detected(
    name: str,
    documents: dict[str, dict[str, typ.Any]],
    texts: dict[str, str],
    compiling_targets: set[str],
) -> None:
    """A mechanism the standard replaced may not come back."""
    mutated_texts = dict(texts)
    mutated_texts["test.yml"] += TEXT_MUTATIONS[name]
    assert judge_all(documents, compiling_targets, texts=mutated_texts)


def test_the_mutations_executed() -> None:
    """Prove the parametrized cases exist, so none can be discovered away."""
    assert len(WORKFLOW_MUTATIONS) == 19
    assert len(TEXT_MUTATIONS) == 4


def test_workflows_parse_to_the_documents_judged(
    documents: dict[str, dict[str, typ.Any]],
) -> None:
    """The documents the judges read are the YAML on disk, not a stale copy."""
    on_disk = yaml.safe_load((WORKFLOW_DIR / "test.yml").read_text(encoding="utf-8"))
    assert set(on_disk["jobs"]) == set(documents["test.yml"]["jobs"])

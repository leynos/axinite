"""Contracts for the scripts that run Cargo outside Make and the workflows.

`scripts/coverage.sh` is a coverage route and `scripts/build-all.sh` and
`scripts/build-wasm-extensions.sh` build release artefacts; none may take the
development standard in `.cargo/config.toml`. The judge is also run against
mutated copies of the real scripts and must reject each.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import pytest

from _build_standard import REPOSITORY_ROOT
from _build_standard_scripts import all_script_violations, script_violations
from contract_sources import is_regular_file, matching_entries, read_source

SCRIPTS = REPOSITORY_ROOT / "scripts"
JUDGED = ("coverage.sh", "build-all.sh", "build-wasm-extensions.sh")


@pytest.fixture(scope="module")
def sources() -> dict[str, str]:
    """Return every shell script's source, keyed by file name."""
    return {
        path.name: read_source(path)
        for path in matching_entries(SCRIPTS, "*.sh")
        if is_regular_file(path)
    }


def test_the_repository_scripts_conform(sources: dict[str, str]) -> None:
    """No script runs a release or coverage build under the standard."""
    problems = all_script_violations(sources)
    assert problems == []


def test_the_scripts_that_matter_were_read(sources: dict[str, str]) -> None:
    """The judge is not vacuous: each route-bearing script is present and bites."""
    for name in JUDGED:
        assert name in sources, f"{name} was not found under scripts/"
    assert "llvm-cov" in sources["coverage.sh"]
    assert "--release" in sources["build-all.sh"]
    assert "--release" in sources["build-wasm-extensions.sh"]


MUTATIONS = {
    "coverage-without-rustflags": (
        "coverage.sh",
        lambda t: t.replace('export RUSTFLAGS="${RUSTFLAGS-}"\n', "", 1),
        "RUSTFLAGS first",
    ),
    "release-build-inherits": (
        "build-all.sh",
        lambda t: t.replace('RUSTFLAGS="${RUSTFLAGS-}" cargo build', "cargo build", 1),
        "release build inherits",
    ),
    "wasm-release-inherits": (
        "build-wasm-extensions.sh",
        lambda t: t.replace(
            'RUSTFLAGS="${RUSTFLAGS-}" cargo component', "cargo component", 1
        ),
        "release build inherits",
    ),
}


@pytest.mark.parametrize("name", sorted(MUTATIONS), ids=sorted(MUTATIONS))
def test_each_mutation_is_detected(name: str, sources: dict[str, str]) -> None:
    """Breaking a script's exclusion one way at a time must be reported."""
    script, edit, fragment = MUTATIONS[name]
    changed = edit(sources[script])
    assert changed != sources[script], "the mutation did not apply"
    problems = script_violations(script, changed)
    assert any(fragment in problem for problem in problems), problems


def test_the_mutations_executed() -> None:
    """Prove the parametrized cases exist, so none can be discovered away."""
    assert len(MUTATIONS) == 3

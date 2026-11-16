"""Contracts for the fail-closed repair-claims Kani gate (#178).

The gate must never report success for a blocked, partial, or lockfile-
mutating proof run. These tests inject a recorded runner, so no verifier is
needed. Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import importlib.util
import subprocess
import sys
from pathlib import Path
from types import ModuleType

import pytest

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
GATE = REPOSITORY_ROOT / "scripts" / "repair_claims_kani.py"


def _load_gate() -> ModuleType:
    """Load the gate script as a module for direct boundary tests."""
    spec = importlib.util.spec_from_file_location("repair_claims_kani_under_test", GATE)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


GATE_MODULE = _load_gate()


def _passing_stdout(names: tuple[str, ...]) -> str:
    """Return Kani terse output in which every named harness succeeds."""
    lines = [
        f"Checking harness claim_registry::verification::{name}..." for name in names
    ]
    lines.append(
        f"Complete - {len(names)} successfully verified harnesses, "
        f"0 failures, {len(names)} total."
    )
    return "\n".join(lines) + "\n"


class _Runner:
    """Record prover actions and return scripted results."""

    def __init__(
        self,
        results: dict[str, subprocess.CompletedProcess[str]],
        mutate: Path | None = None,
    ) -> None:
        """Script one result per action; optionally rewrite a lockfile."""
        self.results = results
        self.mutate = mutate
        self.actions: list[str] = []

    def __call__(self, action: str, capture: bool) -> subprocess.CompletedProcess[str]:
        """Record *action* and return its scripted result."""
        del capture
        self.actions.append(action)
        if self.mutate is not None and action == GATE_MODULE.LIFECYCLE_ACTION:
            self.mutate.write_text("mutated\n", encoding="utf-8")
        return self.results[action]


def _completed(returncode: int, stdout: str = "") -> subprocess.CompletedProcess[str]:
    """Build a finished process result."""
    return subprocess.CompletedProcess([], returncode, stdout, "")


@pytest.fixture
def proof_dir(tmp_path: Path) -> Path:
    """Provide a proof package directory holding a lockfile."""
    (tmp_path / "Cargo.lock").write_text("version = 4\n", encoding="utf-8")
    return tmp_path


def test_blocked_compatibility_probe_fails_before_lifecycle(proof_dir: Path) -> None:
    """A failed probe is the gate's status, and no lifecycle run follows."""
    runner = _Runner({GATE_MODULE.COMPATIBILITY_ACTION: _completed(2)})

    assert GATE_MODULE.run_gate(proof_dir, runner) == 2
    assert runner.actions == [GATE_MODULE.COMPATIBILITY_ACTION]


def test_complete_lifecycle_inventory_passes(proof_dir: Path) -> None:
    """Every required harness with an exact success summary passes."""
    runner = _Runner(
        {
            GATE_MODULE.COMPATIBILITY_ACTION: _completed(0),
            GATE_MODULE.LIFECYCLE_ACTION: _completed(
                0, _passing_stdout(GATE_MODULE.EXPECTED_HARNESSES)
            ),
        }
    )

    assert GATE_MODULE.run_gate(proof_dir, runner) == 0
    assert runner.actions == [
        GATE_MODULE.COMPATIBILITY_ACTION,
        GATE_MODULE.LIFECYCLE_ACTION,
    ]


def test_missing_harness_fails_even_when_kani_succeeds(proof_dir: Path) -> None:
    """A successful exit with an incomplete inventory is still a failure."""
    partial = GATE_MODULE.EXPECTED_HARNESSES[:-1]
    runner = _Runner(
        {
            GATE_MODULE.COMPATIBILITY_ACTION: _completed(0),
            GATE_MODULE.LIFECYCLE_ACTION: _completed(0, _passing_stdout(partial)),
        }
    )

    assert GATE_MODULE.run_gate(proof_dir, runner) == 1


def test_lifecycle_failure_status_is_preserved(proof_dir: Path) -> None:
    """A Kani failure propagates its own exit status."""
    runner = _Runner(
        {
            GATE_MODULE.COMPATIBILITY_ACTION: _completed(0),
            GATE_MODULE.LIFECYCLE_ACTION: _completed(3),
        }
    )

    assert GATE_MODULE.run_gate(proof_dir, runner) == 3


def test_changed_lockfile_refuses_a_passing_result(proof_dir: Path) -> None:
    """A proof run that rewrites the isolated lockfile is not accepted."""
    runner = _Runner(
        {
            GATE_MODULE.COMPATIBILITY_ACTION: _completed(0),
            GATE_MODULE.LIFECYCLE_ACTION: _completed(
                0, _passing_stdout(GATE_MODULE.EXPECTED_HARNESSES)
            ),
        },
        mutate=proof_dir / "Cargo.lock",
    )

    assert GATE_MODULE.run_gate(proof_dir, runner) == 1


def test_inventory_faults_reject_an_extra_failure() -> None:
    """A summary that counts a failure is rejected though all names ran."""
    names = GATE_MODULE.EXPECTED_HARNESSES
    stdout = _passing_stdout(names).replace("0 failures", "1 failures")

    faults = GATE_MODULE.inventory_faults(stdout, names)

    assert len(faults) == 1
    assert "refusing an incomplete result" in faults[0]

"""Fail closed on the repair-claims package's required Kani proof inventory.

The gate runs two prover actions through ``scripts/prover_cli.py``. The
standard-library compatibility probe runs first, so a verifier limitation is
reported as a blocked gate rather than as a lifecycle counterexample. The
lifecycle run must then report every required harness and an exact success
summary; a missing harness, an extra failure, or a changed lockfile fails the
gate even when Kani itself exits successfully. Run via
``make kani-repair-claims``.
"""

from __future__ import annotations

import os
import subprocess
import sys
from collections.abc import Callable, Sequence
from pathlib import Path

REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
PROVER_CLI = REPOSITORY_ROOT / "scripts" / "prover_cli.py"
COMPATIBILITY_ACTION = "kani-repair-claims-compatibility"
LIFECYCLE_ACTION = "kani-repair-claims-lifecycle"
HARNESS_PREFIX = "claim_registry::verification::"
EXPECTED_HARNESSES = (
    "same_tool_is_refused_while_held",
    "distinct_tools_coexist",
    "dropped_claim_can_be_reclaimed",
    "bounded_operation_sequences",
)

#: Runs one prover action and returns its exit status, stdout and stderr.
Runner = Callable[[str, bool], subprocess.CompletedProcess[str]]


def _run_action(action: str, capture: bool) -> subprocess.CompletedProcess[str]:
    """Run one prover CLI action with the caller's environment.

    Parameters
    ----------
    action : str
        Prover CLI action to run.
    capture : bool
        Whether to capture standard output and error for inspection.

    Returns
    -------
    subprocess.CompletedProcess[str]
        The finished process.
    """
    return subprocess.run(
        [sys.executable, str(PROVER_CLI), action],
        check=False,
        capture_output=capture,
        text=True,
    )


def _summary(count: int) -> str:
    """Return Kani's exact all-success summary line for ``count`` harnesses."""
    return (
        f"Complete - {count} successfully verified harnesses, "
        f"0 failures, {count} total."
    )


def inventory_faults(stdout: str, expected: Sequence[str]) -> list[str]:
    """Return one message per missing harness or a non-exact success summary.

    Parameters
    ----------
    stdout : str
        Kani's standard output for the lifecycle run.
    expected : Sequence[str]
        Unqualified names of the harnesses that must all succeed.

    Returns
    -------
    list[str]
        Fault messages; empty when every expected harness ran and the summary
        reports exactly that many successes and no failures.
    """
    faults = [
        f"missing required lifecycle proof result: {name}"
        for name in expected
        if f"Checking harness {HARNESS_PREFIX}{name}..." not in stdout
    ]
    if _summary(len(expected)) not in stdout:
        faults.append(
            f"expected exactly {len(expected)} successful lifecycle proofs "
            "and no failures; refusing an incomplete result"
        )
    return faults


def run_gate(proof_dir: Path, runner: Runner = _run_action) -> int:
    """Run the compatibility probe, then the required lifecycle proofs.

    Parameters
    ----------
    proof_dir : Path
        The repair-claims proof package directory.
    runner : Runner
        Runs one prover action; injected so tests need no verifier.

    Returns
    -------
    int
        Zero only when the probe passed, every required harness succeeded,
        and the package lockfile is unchanged; otherwise a non-zero status.
    """
    lockfile = proof_dir / "Cargo.lock"
    original_lock = lockfile.read_bytes()
    probe = runner(COMPATIBILITY_ACTION, False)
    if probe.returncode:
        print(
            "repair-claims: lifecycle verification is blocked by the "
            "standard-library compatibility probe",
            file=sys.stderr,
        )
        return probe.returncode
    lifecycle = runner(LIFECYCLE_ACTION, True)
    print(lifecycle.stdout, end="")
    print(lifecycle.stderr, end="", file=sys.stderr)
    if lockfile.read_bytes() != original_lock:
        print(
            "repair-claims: Kani changed the isolated lockfile; "
            "refusing the proof result",
            file=sys.stderr,
        )
        return 1
    if lifecycle.returncode:
        return lifecycle.returncode
    faults = inventory_faults(lifecycle.stdout, EXPECTED_HARNESSES)
    for fault in faults:
        print(f"repair-claims: {fault}", file=sys.stderr)
    return 1 if faults else 0


def main() -> int:
    """Run the gate against the configured repair-claims proof package."""
    proof_dir = os.environ.get("REPAIR_CLAIMS_PROOF_DIR", "")
    if not proof_dir:
        print(
            "repair-claims: REPAIR_CLAIMS_PROOF_DIR must be configured", file=sys.stderr
        )
        return 1
    try:
        return run_gate(Path(proof_dir))
    except OSError as error:
        print(f"repair-claims: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())

"""Shared process helpers for real Kani and Verus integration tests."""

from __future__ import annotations

import os
import subprocess
from collections.abc import Sequence
from pathlib import Path

import pytest

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
PROVER_TARGET_DIR = REPOSITORY_ROOT / "target" / "prover-tools"
PROVER_TIMEOUT_SECONDS = 1_200


def _make_environment(target_name: str, **overrides: str) -> dict[str, str]:
    """Keep prover builds inside the worktree and incremental builds disabled."""
    environment = os.environ.copy()
    environment.update(
        {
            "CARGO_INCREMENTAL": "0",
            "CARGO_TARGET_DIR": str(PROVER_TARGET_DIR / target_name),
        }
    )
    environment.update(overrides)
    return environment


def _run_make(
    target: str,
    target_name: str,
    **overrides: str,
) -> subprocess.CompletedProcess[str]:
    """Run one Make target, failing clearly when a real proof times out."""
    try:
        return subprocess.run(
            ["make", "--no-print-directory", target],
            cwd=REPOSITORY_ROOT,
            env=_make_environment(target_name, **overrides),
            check=False,
            capture_output=True,
            text=True,
            timeout=PROVER_TIMEOUT_SECONDS,
        )
    except subprocess.TimeoutExpired as error:
        pytest.fail(
            f"{target} exceeded {PROVER_TIMEOUT_SECONDS} seconds: {error}",
            pytrace=False,
        )


def _run_cli_with_closed_readers(
    command: Sequence[str],
    target_name: str,
    **overrides: str,
) -> int:
    """Return the prover CLI's status after closing both readers early."""
    process = subprocess.Popen(
        command,
        cwd=REPOSITORY_ROOT,
        env=_make_environment(target_name, **overrides),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    assert process.stdout is not None
    assert process.stderr is not None
    process.stdout.close()
    process.stderr.close()
    try:
        return process.wait(timeout=PROVER_TIMEOUT_SECONDS)
    except subprocess.TimeoutExpired as error:
        process.kill()
        process.wait()
        pytest.fail(
            f"{command[0]} exceeded {PROVER_TIMEOUT_SECONDS} seconds: {error}",
            pytrace=False,
        )

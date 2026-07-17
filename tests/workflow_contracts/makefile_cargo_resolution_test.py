"""Behavioural contracts for Cargo executable resolution in the Makefile.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import os
import subprocess
from pathlib import Path

import pytest

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]


@pytest.mark.parametrize(
    ("cargo_override", "expected_command"),
    [("", None), ("/caller/cargo", "/caller/cargo")],
    ids=("empty-override", "caller-override"),
)
def test_check_fmt_resolves_cargo_override(
    tmp_path: Path,
    cargo_override: str,
    expected_command: str | None,
) -> None:
    """Resolve an empty override from PATH and preserve a non-empty one."""
    fake_bin = tmp_path / "bin"
    fake_bin.mkdir()
    path_cargo = fake_bin / "cargo"
    path_cargo.touch(mode=0o755)

    environment = os.environ.copy()
    environment.update(
        {
            "CARGO": cargo_override,
            "PATH": f"{fake_bin}{os.pathsep}{environment['PATH']}",
        }
    )
    result = subprocess.run(
        ["make", "--no-print-directory", "-n", "check-fmt"],
        cwd=REPOSITORY_ROOT,
        env=environment,
        check=True,
        capture_output=True,
        text=True,
    )

    resolved_command = expected_command or str(path_cargo)
    assert result.stdout.splitlines() == [
        f"{resolved_command} fmt --all -- --check",
        f"{resolved_command} fmt --manifest-path tools-src/github/Cargo.toml --all -- --check",
    ]

"""Behavioural contracts for Cargo executable resolution in the Makefile.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import os
import shutil
import subprocess
from pathlib import Path

import pytest

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]


@pytest.mark.parametrize(
    (
        "resolution_case",
        "cargo_override",
        "has_path_cargo",
        "has_home_cargo",
        "expected_location",
    ),
    [
        ("path-resolution", "", True, False, "path"),
        ("path-precedence", "", True, True, "path"),
        ("home-fallback", "", False, True, "home"),
        ("caller-override", "/caller/cargo", False, False, "override"),
    ],
    ids=(
        "path-resolution",
        "path-precedence",
        "home-fallback",
        "caller-override",
    ),
)
def test_check_fmt_resolves_cargo_override(
    tmp_path: Path,
    resolution_case: str,
    cargo_override: str,
    has_path_cargo: bool,
    has_home_cargo: bool,
    expected_location: str,
) -> None:
    """Resolve an empty override and preserve a non-empty one."""
    fake_bin = tmp_path / "bin"
    fake_bin.mkdir()
    path_cargo = fake_bin / "cargo"
    fake_home = tmp_path / "home"
    home_cargo = fake_home / ".cargo" / "bin" / "cargo"

    if has_path_cargo:
        path_cargo.touch(mode=0o755)
    if has_home_cargo:
        home_cargo.parent.mkdir(parents=True)
        home_cargo.touch(mode=0o755)

    expected_command = {
        "path": str(path_cargo),
        "home": str(home_cargo),
        "override": cargo_override,
    }[expected_location]
    make_executable = shutil.which("make")
    assert make_executable is not None, "make must be available to run this contract"

    environment = os.environ.copy()
    environment.update(
        {
            "CARGO": cargo_override,
            "HOME": str(fake_home),
            "PATH": str(fake_bin),
        }
    )
    result = subprocess.run(
        [make_executable, "--no-print-directory", "-n", "check-fmt"],
        cwd=REPOSITORY_ROOT,
        env=environment,
        check=True,
        capture_output=True,
        text=True,
    )

    emitted_commands = result.stdout.splitlines()
    expected_commands = [
        f"{expected_command} fmt --all -- --check",
        f"{expected_command} fmt --manifest-path tools-src/github/Cargo.toml --all -- --check",
    ]
    assert emitted_commands == expected_commands, (
        f"{resolution_case} emitted unexpected commands: {emitted_commands!r}"
    )

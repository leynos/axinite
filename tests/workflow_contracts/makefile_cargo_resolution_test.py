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
    ("resolution_case", "cargo_override", "cargo_location"),
    [
        ("path-resolution", "", "path"),
        ("home-fallback", "", "home"),
        ("caller-override", "/caller/cargo", "override"),
    ],
    ids=("path-resolution", "home-fallback", "caller-override"),
)
def test_check_fmt_resolves_cargo_override(
    tmp_path: Path,
    resolution_case: str,
    cargo_override: str,
    cargo_location: str,
) -> None:
    """Resolve an empty override and preserve a non-empty one."""
    fake_bin = tmp_path / "bin"
    fake_bin.mkdir()
    path_cargo = fake_bin / "cargo"
    fake_home = tmp_path / "home"
    home_cargo = fake_home / ".cargo" / "bin" / "cargo"

    if cargo_location == "path":
        path_cargo.touch(mode=0o755)
    elif cargo_location == "home":
        home_cargo.parent.mkdir(parents=True)
        home_cargo.touch(mode=0o755)

    expected_command = {
        "path": str(path_cargo),
        "home": str(home_cargo),
        "override": cargo_override,
    }[cargo_location]
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

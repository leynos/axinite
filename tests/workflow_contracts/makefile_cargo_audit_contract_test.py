"""Behavioural contracts for Cargo Audit command resolution."""

from __future__ import annotations

import os
import subprocess
from dataclasses import dataclass
from pathlib import Path

import pytest

from _makefile_test_support import shell_quote

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]


@dataclass(frozen=True)
class CargoAuditCase:
    """Immutable inputs and expected executable for one audit case.

    Parameters
    ----------
    cargo_override : str or None
        Optional value supplied through the ``CARGO`` environment variable.
    has_path_cargo : bool
        Whether the test provides Cargo on ``PATH``.
    has_home_cargo : bool
        Whether the test provides Cargo under ``$HOME/.cargo/bin``.
    expected_location : str
        Expected source of the executable: ``path``, ``home``, or ``override``.

    Returns
    -------
    CargoAuditCase
        Immutable case describing the environment and expected resolution.
    """

    cargo_override: str | None
    has_path_cargo: bool
    has_home_cargo: bool
    expected_location: str


@pytest.mark.parametrize(
    "case",
    [
        CargoAuditCase(
            cargo_override=None,
            has_path_cargo=True,
            has_home_cargo=False,
            expected_location="path",
        ),
        CargoAuditCase(
            cargo_override="",
            has_path_cargo=True,
            has_home_cargo=False,
            expected_location="path",
        ),
        CargoAuditCase(
            cargo_override="",
            has_path_cargo=False,
            has_home_cargo=True,
            expected_location="home",
        ),
        CargoAuditCase(
            cargo_override="/caller/cargo",
            has_path_cargo=False,
            has_home_cargo=False,
            expected_location="override",
        ),
    ],
    ids=("unset", "empty", "home-fallback", "caller-override"),
)
def test_audit_uses_resolved_cargo_command(
    tmp_path: Path,
    case: CargoAuditCase,
    make_executable: str,
    utility_bin: Path,
) -> None:
    """Check the resolved Cargo executable passed to each audit subprocess.

    Parameters
    ----------
    tmp_path : Path
        Temporary directory for fake Cargo executables and the home directory.
    case : CargoAuditCase
        Audit resolution inputs and the expected executable location.
    make_executable : str
        Absolute path to the Make executable provided by the shared fixture.
    utility_bin : Path
        Directory containing required utilities and no Cargo executable.

    Returns
    -------
    None
        Asserts that the dry-run audit command uses the expected executable.
    """
    fake_bin = tmp_path / "bin"
    fake_bin.mkdir()
    path_cargo = fake_bin / "cargo"
    fake_home = tmp_path / "home"
    home_cargo = fake_home / ".cargo" / "bin" / "cargo"

    if case.has_path_cargo:
        path_cargo.touch(mode=0o755)
    if case.has_home_cargo:
        home_cargo.parent.mkdir(parents=True)
        home_cargo.touch(mode=0o755)

    expected_cargo = {
        "path": str(path_cargo),
        "home": str(home_cargo),
        "override": case.cargo_override,
    }[case.expected_location]

    environment = os.environ.copy()
    environment.update(
        {
            "HOME": str(fake_home),
            "PATH": os.pathsep.join((str(fake_bin), str(utility_bin))),
        }
    )
    environment.pop("CARGO_AUDIT", None)
    environment.pop("CARGO_AUDIT_SUBCOMMAND", None)
    if case.cargo_override is None:
        environment.pop("CARGO", None)
    else:
        environment["CARGO"] = case.cargo_override

    result = subprocess.run(
        [make_executable, "--no-print-directory", "-n", "audit"],
        cwd=REPOSITORY_ROOT,
        env=environment,
        check=True,
        capture_output=True,
        text=True,
    )

    composed_command = f"{shell_quote(expected_cargo)} 'audit'"
    emitted_audit_command = f"sh {shell_quote(composed_command)} {{}} +"
    assert emitted_audit_command in result.stdout, (
        f"{case!r} emitted unexpected audit command: {result.stdout!r}"
    )


@pytest.mark.parametrize(
    "audit_override",
    ("/opt/tools/cargo-audit", "cargo +stable audit"),
    ids=("absolute-command", "cargo-toolchain-command"),
)
def test_audit_preserves_full_command_override(
    tmp_path: Path,
    audit_override: str,
    make_executable: str,
    utility_bin: Path,
) -> None:
    """Check CARGO_AUDIT remains a full command rather than one Cargo argument.

    Parameters
    ----------
    tmp_path : Path
        Temporary directory for the isolated Cargo resolution environment.
    audit_override : str
        Full command supplied through ``CARGO_AUDIT``.
    make_executable : str
        Absolute path to the Make executable provided by the shared fixture.
    utility_bin : Path
        Directory containing required utilities and no Cargo executable.

    Returns
    -------
    None
        Asserts that the command is passed intact and is not prefixed by Cargo.
    """
    environment = os.environ.copy()
    environment.update(
        {
            "CARGO": "/resolved/cargo-must-not-be-used",
            "CARGO_AUDIT": audit_override,
            "HOME": str(tmp_path / "home"),
            "PATH": str(utility_bin),
        }
    )
    environment.pop("CARGO_AUDIT_SUBCOMMAND", None)
    result = subprocess.run(
        [make_executable, "--no-print-directory", "-n", "audit"],
        cwd=REPOSITORY_ROOT,
        env=environment,
        check=True,
        capture_output=True,
        text=True,
    )

    command_argument = shell_quote(audit_override)
    assert f"sh {command_argument} {{}} +" in result.stdout, (
        f"CARGO_AUDIT={audit_override!r} was not emitted as one full command: "
        f"{result.stdout!r}"
    )
    assert shell_quote("/resolved/cargo-must-not-be-used") not in result.stdout


def test_audit_executes_multiword_command_override(
    tmp_path: Path,
    make_executable: str,
    utility_bin: Path,
) -> None:
    """Check that a multiword caller command reaches the audit executable.

    Parameters
    ----------
    tmp_path : Path
        Temporary directory for fake tools and the audit argument log.
    make_executable : str
        Absolute path to the Make executable provided by the shared fixture.
    utility_bin : Path
        Directory containing audit utilities and no Cargo executable.

    Returns
    -------
    None
        Asserts that the fake Cargo tool receives the toolchain and audit words.
    """
    fake_bin = tmp_path / "bin"
    fake_bin.mkdir()
    cargo_path = fake_bin / "cargo"
    cargo_path.write_text('#!/bin/sh\nprintf \'%s\\n\' "$*" >> "$AUDIT_ARGUMENT_LOG"\n')
    cargo_path.chmod(0o755)
    argument_log = tmp_path / "audit-arguments"

    environment = os.environ.copy()
    environment.update(
        {
            "AUDIT_ARGUMENT_LOG": str(argument_log),
            "CARGO": "",
            "CARGO_AUDIT": "cargo +stable audit",
            "HOME": str(tmp_path / "home"),
            "PATH": os.pathsep.join((str(fake_bin), str(utility_bin))),
        }
    )
    environment.pop("CARGO_AUDIT_SUBCOMMAND", None)
    subprocess.run(
        [make_executable, "--no-print-directory", "audit"],
        cwd=REPOSITORY_ROOT,
        env=environment,
        check=True,
        capture_output=True,
        text=True,
    )

    commands = argument_log.read_text().splitlines()
    assert commands, "audit should invoke the fake Cargo executable"
    assert all(command.startswith("+stable audit --ignore ") for command in commands), (
        f"multiword CARGO_AUDIT override emitted unexpected arguments: {commands!r}"
    )

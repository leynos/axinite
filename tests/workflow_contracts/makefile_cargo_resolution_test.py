"""Behavioural contracts for Cargo executable resolution in the Makefile.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import os
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path

import pytest
from hypothesis import HealthCheck, given, settings
from hypothesis import strategies as st

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]


@dataclass(frozen=True)
class CargoResolutionCase:
    """Inputs and expected source for one Cargo resolution scenario."""

    cargo_override: str
    has_path_cargo: bool
    has_home_cargo: bool
    expected_location: str


@pytest.mark.parametrize(
    "case",
    [
        CargoResolutionCase(
            cargo_override="",
            has_path_cargo=True,
            has_home_cargo=False,
            expected_location="path",
        ),
        CargoResolutionCase(
            cargo_override="   ",
            has_path_cargo=True,
            has_home_cargo=False,
            expected_location="path",
        ),
        CargoResolutionCase(
            cargo_override="",
            has_path_cargo=True,
            has_home_cargo=True,
            expected_location="path",
        ),
        CargoResolutionCase(
            cargo_override="",
            has_path_cargo=False,
            has_home_cargo=True,
            expected_location="home",
        ),
        CargoResolutionCase(
            cargo_override="/caller/cargo",
            has_path_cargo=False,
            has_home_cargo=False,
            expected_location="override",
        ),
    ],
    ids=(
        "path-resolution",
        "whitespace-resolution",
        "path-precedence",
        "home-fallback",
        "caller-override",
    ),
)
def test_check_fmt_resolves_cargo_override(
    tmp_path: Path,
    case: CargoResolutionCase,
) -> None:
    """Resolve empty or whitespace-only overrides and preserve non-empty ones."""
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

    expected_command = {
        "path": str(path_cargo),
        "home": str(home_cargo),
        "override": case.cargo_override,
    }[case.expected_location]
    make_executable = shutil.which("make")
    assert make_executable is not None, "make must be available to run this contract"

    environment = os.environ.copy()
    environment.update(
        {
            "CARGO": case.cargo_override,
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
    quoted_command = shell_quote(expected_command)
    expected_commands = [
        f"{quoted_command} fmt --all -- --check",
        f"{quoted_command} fmt --manifest-path tools-src/github/Cargo.toml --all -- --check",
    ]
    assert emitted_commands == expected_commands, (
        f"{case!r} emitted unexpected commands: {emitted_commands!r}"
    )


@settings(
    max_examples=16,
    deadline=None,
    suppress_health_check=[HealthCheck.function_scoped_fixture],
)
@given(
    uses_resolved_path=st.booleans(),
    whitespace_override=st.text(alphabet=" \t", max_size=8),
    path_suffix=st.text(alphabet="abcXYZ0123 $;()'", min_size=1, max_size=12),
)
def test_check_fmt_shell_quotes_generated_cargo_paths(
    tmp_path: Path,
    uses_resolved_path: bool,
    whitespace_override: str,
    path_suffix: str,
) -> None:
    """Quote generated paths and resolve whitespace-only Cargo overrides."""
    fake_bin = tmp_path / "generated-bin"
    fake_bin.mkdir(exist_ok=True)
    path_cargo = fake_bin / "cargo"
    path_cargo.touch(exist_ok=True, mode=0o755)
    override_cargo = tmp_path / f"cargo{path_suffix}"
    override_cargo.touch(exist_ok=True, mode=0o755)
    cargo_override = whitespace_override if uses_resolved_path else str(override_cargo)
    expected_cargo = str(path_cargo) if uses_resolved_path else cargo_override
    make_executable = shutil.which("make")
    assert make_executable is not None, "make must be available to run this contract"

    environment = os.environ.copy()
    environment.update(
        {
            "CARGO": cargo_override,
            "HOME": str(tmp_path / "home"),
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

    quoted_cargo = shell_quote(expected_cargo)
    assert result.stdout.splitlines() == [
        f"{quoted_cargo} fmt --all -- --check",
        f"{quoted_cargo} fmt --manifest-path tools-src/github/Cargo.toml --all -- --check",
    ], f"CARGO={cargo_override!r} emitted unexpected commands: {result.stdout!r}"


def shell_quote(value: str) -> str:
    """Return the POSIX shell representation emitted by the Makefile."""
    return "'" + value.replace("'", "'\"'\"'") + "'"


@pytest.mark.parametrize(
    ("nextest_override", "expected_nextest"),
    [
        (None, "{cargo} nextest"),
        ("/caller/nextest", "/caller/nextest"),
    ],
    ids=("resolved-cargo", "caller-override"),
)
def test_test_target_uses_expected_nextest_command(
    tmp_path: Path,
    nextest_override: str | None,
    expected_nextest: str,
) -> None:
    """Use resolved Cargo for nextest unless a caller overrides NEXTEST."""
    fake_bin = tmp_path / "bin"
    fake_bin.mkdir()
    path_cargo = fake_bin / "cargo"
    path_cargo.touch(mode=0o755)
    make_executable = shutil.which("make")
    assert make_executable is not None, "make must be available to run this contract"

    environment = os.environ.copy()
    environment.update({"CARGO": "   ", "HOME": str(tmp_path / "home"), "PATH": str(fake_bin)})
    if nextest_override is None:
        environment.pop("NEXTEST", None)
    else:
        environment["NEXTEST"] = nextest_override

    result = subprocess.run(
        [make_executable, "--no-print-directory", "-n", "test"],
        cwd=REPOSITORY_ROOT,
        env=environment,
        check=True,
        capture_output=True,
        text=True,
    )

    expected_command = expected_nextest.format(cargo=shell_quote(str(path_cargo)))
    emitted_commands = result.stdout.splitlines()
    assert (
        f"{expected_command} run --workspace --features test-helpers --profile default"
        in emitted_commands
    ), (
        f"NEXTEST override {nextest_override!r} emitted unexpected commands: "
        f"{emitted_commands!r}"
    )


def test_test_matrix_uses_quoted_cargo_command(tmp_path: Path) -> None:
    """Use the resolved Cargo command for the final test-matrix invocation."""
    fake_bin = tmp_path / "bin"
    fake_bin.mkdir()
    path_cargo = fake_bin / "cargo"
    path_cargo.touch(mode=0o755)
    make_executable = shutil.which("make")
    assert make_executable is not None, "make must be available to run this contract"

    environment = os.environ.copy()
    environment.update({"CARGO": "", "HOME": str(tmp_path / "home"), "PATH": str(fake_bin)})
    result = subprocess.run(
        [make_executable, "--no-print-directory", "-n", "test-matrix"],
        cwd=REPOSITORY_ROOT,
        env=environment,
        check=True,
        capture_output=True,
        text=True,
    )

    emitted_commands = result.stdout.splitlines()
    expected_command = (
        f"{shell_quote(str(path_cargo))} test --manifest-path "
        "tools-src/github/Cargo.toml -- --nocapture"
    )
    assert expected_command in emitted_commands, (
        f"test-matrix emitted unexpected commands: {emitted_commands!r}"
    )


@dataclass(frozen=True)
class CargoAuditCase:
    """Inputs and expected executable for one audit-resolution scenario."""

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
) -> None:
    """Pass the resolved Cargo executable to each audit subprocess."""
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
    make_executable = shutil.which("make")
    assert make_executable is not None, "make must be available to run this contract"

    environment = os.environ.copy()
    environment.update({"HOME": str(fake_home), "PATH": str(fake_bin)})
    environment.pop("CARGO_AUDIT", None)
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

    emitted_audit_command = f"sh {shell_quote(expected_cargo)} 'audit' {{}} +"
    assert emitted_audit_command in result.stdout, (
        f"{case!r} emitted unexpected audit command: {result.stdout!r}"
    )


@pytest.mark.parametrize("make_target", ("check-fmt", "test-matrix", "audit"))
@pytest.mark.parametrize("resolution_source", ("home", "path"))
def test_targets_escape_resolved_cargo_paths(
    tmp_path: Path,
    resolution_source: str,
    make_target: str,
) -> None:
    """Execute metacharacter paths without evaluating their shell syntax."""
    marker = tmp_path / "injected"
    marker_relative = os.path.relpath(marker, REPOSITORY_ROOT)
    unsafe_root = tmp_path / f"cargo$literal; printf injected > {marker_relative}; #"
    fake_bin = unsafe_root if resolution_source == "path" else tmp_path / "bin"
    fake_home = unsafe_root if resolution_source == "home" else tmp_path / "home"
    cargo_path = (
        fake_bin / "cargo"
        if resolution_source == "path"
        else fake_home / ".cargo" / "bin" / "cargo"
    )
    cargo_path.parent.mkdir(parents=True)
    cargo_path.write_text("#!/bin/sh\nexit 0\n")
    cargo_path.chmod(0o755)
    make_executable = shutil.which("make")
    assert make_executable is not None, "make must be available to run this contract"

    environment = os.environ.copy()
    environment.pop("NEXTEST", None)
    environment.pop("CARGO_AUDIT", None)
    environment.update(
        {
            "CARGO": "",
            "HOME": str(fake_home),
            "PATH": os.pathsep.join((str(fake_bin), "/usr/bin", "/bin")),
        }
    )
    subprocess.run(
        [make_executable, "--no-print-directory", make_target],
        cwd=REPOSITORY_ROOT,
        env=environment,
        check=True,
    )

    assert not marker.exists(), (
        f"{make_target} evaluated {resolution_source} path shell syntax"
    )

"""Behavioural contracts for Cargo executable resolution in the Makefile.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import os
import subprocess
from dataclasses import dataclass
from pathlib import Path

import pytest
from hypothesis import HealthCheck, example, given, settings
from hypothesis import strategies as st

from _makefile_test_support import shell_quote

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]


@dataclass(frozen=True)
class CargoResolutionCase:
    """Immutable inputs and expected location for a Cargo resolution case.

    Parameters
    ----------
    cargo_override : str
        Value supplied through the ``CARGO`` environment variable.
    has_path_cargo : bool
        Whether the test provides Cargo on ``PATH``.
    has_home_cargo : bool
        Whether the test provides Cargo under ``$HOME/.cargo/bin``.
    expected_location : str
        Expected source of the resolved executable: ``path``, ``home``, or
        ``override``.

    Returns
    -------
    CargoResolutionCase
        Immutable case describing the environment and expected resolution.
    """

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
    make_executable: str,
    utility_bin: Path,
) -> None:
    """Check Cargo resolution for empty, whitespace-only, and explicit overrides.

    Parameters
    ----------
    tmp_path : Path
        Temporary directory for the fake Cargo executables and home directory.
    case : CargoResolutionCase
        Resolution inputs and the expected executable location.
    make_executable : str
        Absolute path to the Make executable provided by the shared fixture.
    utility_bin : Path
        Directory containing required utilities and no Cargo executable.

    Returns
    -------
    None
        Asserts that ``make -n check-fmt`` emits the expected commands.
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

    expected_command = {
        "path": str(path_cargo),
        "home": str(home_cargo),
        "override": case.cargo_override,
    }[case.expected_location]
    environment = os.environ.copy()
    environment.update(
        {
            "CARGO": case.cargo_override,
            "HOME": str(fake_home),
            "PATH": os.pathsep.join((str(fake_bin), str(utility_bin))),
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

    quoted_command = shell_quote(expected_command)
    emitted_commands = [
        line
        for line in result.stdout.splitlines()
        if line.startswith(f"{quoted_command} ")
    ]
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
@example(
    uses_resolved_path=False,
    whitespace_override="",
    path_suffix="$tool",
)
@example(
    uses_resolved_path=False,
    whitespace_override="",
    path_suffix="$(",
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
    make_executable: str,
    utility_bin: Path,
) -> None:
    """Check shell quoting for generated paths and whitespace-only overrides.

    Parameters
    ----------
    tmp_path : Path
        Temporary directory for generated Cargo executables.
    uses_resolved_path : bool
        Whether to test a path resolved from ``PATH`` instead of an override.
    whitespace_override : str
        Empty or whitespace-only ``CARGO`` value used for path resolution.
    path_suffix : str
        Generated suffix, including shell metacharacters, for an override path.
    make_executable : str
        Absolute path to the Make executable provided by the shared fixture.
    utility_bin : Path
        Directory containing required utilities and no Cargo executable.

    Returns
    -------
    None
        Asserts that ``make -n check-fmt`` emits the expected quoted commands.
    """
    fake_bin = tmp_path / "generated-bin"
    fake_bin.mkdir(exist_ok=True)
    path_cargo = fake_bin / "cargo"
    path_cargo.touch(exist_ok=True, mode=0o755)
    override_cargo = tmp_path / f"cargo{path_suffix}"
    override_cargo.touch(exist_ok=True, mode=0o755)
    cargo_override = whitespace_override if uses_resolved_path else str(override_cargo)
    expected_cargo = str(path_cargo) if uses_resolved_path else cargo_override
    environment = os.environ.copy()
    environment.update(
        {
            "CARGO": cargo_override,
            "HOME": str(tmp_path / "home"),
            "PATH": os.pathsep.join((str(fake_bin), str(utility_bin))),
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
    emitted_commands = [
        line
        for line in result.stdout.splitlines()
        if line.startswith(f"{quoted_cargo} ")
    ]
    assert emitted_commands == [
        f"{quoted_cargo} fmt --all -- --check",
        f"{quoted_cargo} fmt --manifest-path tools-src/github/Cargo.toml --all -- --check",
    ], f"CARGO={cargo_override!r} emitted unexpected commands: {emitted_commands!r}"

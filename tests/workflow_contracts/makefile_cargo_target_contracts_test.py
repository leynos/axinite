"""Behavioural contracts for Make targets that invoke Cargo."""

from __future__ import annotations

import os
import subprocess
from pathlib import Path

import pytest

from _makefile_test_support import _MakeTestContext, shell_quote

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]


@pytest.mark.parametrize(
    ("nextest_override", "expected_nextest"),
    [
        (None, "{cargo} nextest"),
        ("/caller/nextest", "/caller/nextest"),
    ],
    ids=("resolved-cargo", "caller-override"),
)
def test_test_target_uses_expected_nextest_command(
    nextest_override: str | None,
    expected_nextest: str,
    make_context: _MakeTestContext,
) -> None:
    """Check default and caller-overridden nextest commands on the test target.

    Parameters
    ----------
    nextest_override : str or None
        Optional caller-provided ``NEXTEST`` command.
    expected_nextest : str
        Expected command template, with ``{cargo}`` for the resolved path.
    make_context : _MakeTestContext
        Temporary directory and executables required for the Make invocation.

    Returns
    -------
    None
        Asserts that the expected nextest command is emitted by ``make -n``.
    """
    tmp_path = make_context.tmp_path
    make_executable = make_context.make_executable
    utility_bin = make_context.utility_bin
    fake_bin = tmp_path / "bin"
    fake_bin.mkdir()
    path_cargo = fake_bin / "cargo"
    path_cargo.touch(mode=0o755)

    environment = os.environ.copy()
    environment.update(
        {
            "CARGO": "   ",
            "HOME": str(tmp_path / "home"),
            "PATH": os.pathsep.join((str(fake_bin), str(utility_bin))),
        }
    )
    environment.pop("CARGO_AUDIT", None)
    environment.pop("CARGO_AUDIT_SUBCOMMAND", None)
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


@pytest.mark.parametrize(
    ("make_target", "cargo_arguments"),
    [
        ("install", ("install --path .",)),
        (
            "build-github-tool-wasm",
            (
                "build --manifest-path tools-src/github/Cargo.toml --release "
                "--target wasm32-wasip2",
            ),
        ),
        ("fmt", ("fmt --all", "fmt --manifest-path tools-src/github/Cargo.toml --all")),
        (
            "check-fmt",
            (
                "fmt --all -- --check",
                "fmt --manifest-path tools-src/github/Cargo.toml --all -- --check",
            ),
        ),
        (
            "typecheck",
            (
                "check --all --benches --tests --examples --features test-helpers",
                "check --all --benches --tests --examples --no-default-features "
                "--features libsql-test-helpers",
                "check --all --benches --tests --examples --all-features "
                "--features test-helpers",
                "check --manifest-path tools-src/github/Cargo.toml --tests",
            ),
        ),
        (
            "lint-clippy",
            (
                "clippy --all --benches --tests --examples --features test-helpers "
                "-- -D warnings",
                "clippy --all --benches --tests --examples --no-default-features "
                "--features libsql-test-helpers -- -D warnings",
                "clippy --all --benches --tests --examples --all-features "
                "--features test-helpers -- -D warnings",
                "clippy --manifest-path tools-src/github/Cargo.toml --tests "
                "-- -D warnings",
            ),
        ),
        (
            "test",
            (
                "build --manifest-path tools-src/github/Cargo.toml --release "
                "--target wasm32-wasip2",
                "test --manifest-path tools-src/github/Cargo.toml",
            ),
        ),
        (
            "test-cargo",
            (
                "build --manifest-path tools-src/github/Cargo.toml --release "
                "--target wasm32-wasip2",
                "test --features test-helpers",
                "test --manifest-path tools-src/github/Cargo.toml",
            ),
        ),
        (
            "test-matrix",
            (
                "build --manifest-path tools-src/github/Cargo.toml --release "
                "--target wasm32-wasip2",
                "test --manifest-path tools-src/github/Cargo.toml -- --nocapture",
            ),
        ),
        (
            "test-matrix-cargo",
            (
                "build --manifest-path tools-src/github/Cargo.toml --release "
                "--target wasm32-wasip2",
                "test --features test-helpers -- --nocapture",
                "test --no-default-features --features libsql-test-helpers -- --nocapture",
                "test --features postgres,libsql-test-helpers,html-to-markdown -- --nocapture",
                "test --manifest-path tools-src/github/Cargo.toml -- --nocapture",
            ),
        ),
        ("clean", ("clean", "clean --manifest-path tools-src/github/Cargo.toml")),
    ],
    ids=(
        "install",
        "build-github-tool-wasm",
        "fmt",
        "check-fmt",
        "typecheck",
        "lint-clippy",
        "test",
        "test-cargo",
        "test-matrix",
        "test-matrix-cargo",
        "clean",
    ),
)
def test_targets_use_resolved_quoted_cargo_command(
    make_target: str,
    cargo_arguments: tuple[str, ...],
    make_context: _MakeTestContext,
) -> None:
    """Check every direct Cargo recipe uses the resolved executable as one word.

    Parameters
    ----------
    make_target : str
        Make target whose Cargo recipe is being checked.
    cargo_arguments : tuple[str, ...]
        Expected Cargo argument strings, in emitted recipe order.
    make_context : _MakeTestContext
        Temporary directory and executables required for the Make invocation.

    Returns
    -------
    None
        Asserts that each direct Cargo invocation uses the quoted path.
    """
    tmp_path = make_context.tmp_path
    make_executable = make_context.make_executable
    utility_bin = make_context.utility_bin
    cargo_path = tmp_path / "cargo with spaces" / "cargo"
    cargo_path.parent.mkdir()
    cargo_path.touch(mode=0o755)

    environment = os.environ.copy()
    environment.update(
        {
            "CARGO": str(cargo_path),
            "HOME": str(tmp_path / "home"),
            "NEXTEST": "/caller/nextest",
            "PATH": str(utility_bin),
        }
    )
    environment.pop("CARGO_AUDIT", None)
    environment.pop("CARGO_AUDIT_SUBCOMMAND", None)
    result = subprocess.run(
        [make_executable, "--no-print-directory", "-n", make_target],
        cwd=REPOSITORY_ROOT,
        env=environment,
        check=True,
        capture_output=True,
        text=True,
    )

    quoted_cargo = shell_quote(str(cargo_path))
    emitted_cargo_commands = [
        line
        for line in result.stdout.splitlines()
        if line.startswith(f"{quoted_cargo} ")
    ]
    expected_cargo_commands = [
        f"{quoted_cargo} {arguments}" for arguments in cargo_arguments
    ]
    assert emitted_cargo_commands == expected_cargo_commands, (
        f"{make_target} emitted unexpected Cargo commands: {emitted_cargo_commands!r}"
    )


@pytest.mark.parametrize("make_target", ("check-fmt", "test-matrix", "audit"))
@pytest.mark.parametrize("resolution_source", ("home", "path"))
def test_targets_escape_resolved_cargo_paths(
    resolution_source: str,
    make_target: str,
    make_context: _MakeTestContext,
) -> None:
    """Check that Cargo paths with shell metacharacters execute literally.

    Parameters
    ----------
    resolution_source : str
        Resolution location under test: ``home`` or ``path``.
    make_target : str
        Make target to execute with the metacharacter-containing Cargo path.
    make_context : _MakeTestContext
        Temporary directory and executables required for the Make invocation.

    Returns
    -------
    None
        Asserts that the target succeeds without creating the injection marker.
    """
    tmp_path = make_context.tmp_path
    make_executable = make_context.make_executable
    utility_bin = make_context.utility_bin
    marker = tmp_path / "injected"
    marker_relative = marker.relative_to(REPOSITORY_ROOT, walk_up=True)
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

    environment = os.environ.copy()
    environment.pop("NEXTEST", None)
    environment.pop("CARGO_AUDIT", None)
    environment.pop("CARGO_AUDIT_SUBCOMMAND", None)
    environment.update(
        {
            "CARGO": "",
            "HOME": str(fake_home),
            "PATH": os.pathsep.join((str(fake_bin), str(utility_bin))),
        }
    )
    subprocess.run(
        [make_executable, "--no-print-directory", make_target],
        cwd=REPOSITORY_ROOT,
        env=environment,
        check=True,
        capture_output=True,
        text=True,
    )

    assert not marker.exists(), (
        f"{make_target} evaluated {resolution_source} path shell syntax"
    )

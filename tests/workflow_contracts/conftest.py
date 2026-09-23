"""Shared fixtures for Make workflow contracts."""

from __future__ import annotations

import shutil
import sys
from pathlib import Path

import pytest

from _makefile_test_support import _MakeTestContext


@pytest.fixture(scope="session")
def make_executable() -> str:
    """Return the Make executable or fail with a useful setup diagnostic."""
    executable = shutil.which("make")
    if executable is None:
        pytest.fail("make must be available to run these workflow contracts")
    return executable


@pytest.fixture
def utility_bin(tmp_path: Path) -> Path:
    """Provide a PATH directory with required utilities but no Cargo binary."""
    utility_bin = tmp_path / "utilities"
    utility_bin.mkdir()
    for name in ("dirname", "find", "git", "make", "mdtablefix", "sh"):
        executable = shutil.which(name)
        if executable is None:
            pytest.fail(f"{name} must be available to run workflow contracts")
        (utility_bin / name).symlink_to(executable)
    python_executable = Path(sys.base_prefix) / "bin" / "python3"
    if not python_executable.is_file():
        pytest.fail(f"stable Python executable not found at {python_executable}")
    (utility_bin / "python3").symlink_to(python_executable)
    return utility_bin


@pytest.fixture
def make_context(
    tmp_path: Path,
    make_executable: str,
    utility_bin: Path,
) -> _MakeTestContext:
    """Group the per-test directory and Make support executables."""
    return _MakeTestContext(
        tmp_path=tmp_path,
        make_executable=make_executable,
        utility_bin=utility_bin,
    )

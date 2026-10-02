"""Shared fixture and subprocess helpers for nextest boundary contracts."""

import os
import shutil
import subprocess
import typing as typ
from collections.abc import Sequence
from pathlib import Path

import pytest
from _workflow_policy import REPOSITORY_ROOT

FIXTURE_CRATE = (
    REPOSITORY_ROOT
    / "tests"
    / "workflow_contracts"
    / "fixtures"
    / "nextest_boundary"
)
SUBPROCESS_TIMEOUT_SECONDS = 600.0


class Fixture(typ.NamedTuple):
    """Where a copied nextest fixture crate lives and where it builds."""

    crate: Path
    target: Path


def _tool_is_present(command: str) -> bool:
    """Return whether a command resolves on `PATH`."""
    return shutil.which(command) is not None


def _run(
    arguments: Sequence[str],
    crate: Path,
    target: Path,
    environment_overrides: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    """Run a Cargo command in the copied crate and capture its output.

    The fixture source is copied outside the repository because Cargo finds
    `.cargo/config.toml` in parent directories. Its build artefacts stay in
    the repository's ignored `target` directory and use the shared Cargo
    package cache.
    """
    environment = dict(os.environ)
    environment["CARGO_TARGET_DIR"] = str(target)
    environment["CARGO_BUILD_BUILD_DIR"] = str(target / "build")
    environment.update(environment_overrides or {})
    return subprocess.run(  # noqa: S603 - fixed argument vector, no shell
        list(arguments),
        cwd=crate,
        env=environment,
        capture_output=True,
        text=True,
        timeout=SUBPROCESS_TIMEOUT_SECONDS,
        check=False,
    )


@pytest.fixture(scope="module")
def fixture_crate(tmp_path_factory: pytest.TempPathFactory) -> Fixture:
    """Return the fixture crate copied outside the repository tree."""
    root = tmp_path_factory.mktemp("nextest-boundary")
    crate = root / "crate"
    shutil.copytree(FIXTURE_CRATE, crate)
    target = REPOSITORY_ROOT / "target" / "workflow-contracts" / "nextest-boundary"
    return Fixture(crate=crate, target=target)


@pytest.fixture(scope="module", autouse=True)
def _require_the_toolchain() -> None:
    """Fail rather than skip when the required Cargo tools are missing."""
    missing = [
        tool for tool in ("cargo", "cargo-nextest") if not _tool_is_present(tool)
    ]
    assert not missing, (
        f"this contract runs nextest itself and needs {missing} on PATH; "
        "`make test` requires them too"
    )
